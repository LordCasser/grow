## Why

自动 Memory Flush 当前按固定周期运行，没有要求用户真正空闲或前台 turn 已结束；失败和锁冲突会提前推进历史长度水位，导致同一段历史无法在下一次 idle interval 重试。零秒配置还会形成持续唤醒。

## What Changes

- Memory Flush 仅在用户活动停止达到配置时长、前台及 session 活动均空闲后运行；用户活动和前台忙转闲会重新开始 idle interval。
- 仅成功 checkpoint 推进已有历史长度水位；provider、snapshot 或 flush lock 失败保留重试资格。
- `idle_timeout_secs = 0` 禁用自动 flush。

## Impact

- Affected capability: `memory-search`.
- Affected implementation: Shell session actor run loop and startup timeout configuration.
- Manual flush and pre-compaction flush semantics remain unchanged.
