## Why
算法S2：live search worker对同一个session每次500ms合并更新后仍完整decode/fold历史。当前debug合成2048轮4.9MB时decode约386ms，fold/extract约16ms；body hash只省SQLite写，不省这些工作。

## What Changes
仅worker保留最近一次成功投影的一个已验证Timeline和物理完整前缀checkpoint；新pinned reader完整hash旧committed prefix，相同才重用旧fold并只decode suffix。任何key变化、短截或hash不符都由同一handle全量重建；任何失败丢弃候选，不发布部分document。bootstrap保持全量。复用同一物理record scanner，不引入另一parser、mtime信任、全session缓存表或增量正文规则。

## Impact
内部计算复用，最终内容/错误/快照/SQLite持久化行为不变，skip_specs。保持旧物理解码先于lifecycle fold的错误顺序。Tier2通过真实pinned readers和SQLite upsert验证，旧full collector作差分；记录单entry内存保留及命中收益/冷miss成本，独立对抗性review后归档。
