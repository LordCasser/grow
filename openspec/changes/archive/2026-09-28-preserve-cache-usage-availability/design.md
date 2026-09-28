## Context

`TokenUsage` 用 `u32` 表示缓存读写；Chat 的 `map_or(0)`、Messages start usage 的 serde default 和 Responses 的固定 write=0 会丢掉字段是否存在。`UsageTotals`、Sideband 的双向转换、子 Agent 最终账单及 UI 继续累加这些 0。现有总量未知处理和逐 attempt 结算已经存在，不能用一个新的全局 incomplete 位同时表示总量与分类未知。

源码入口：`sampling-types/src/{types,messages,conversation}.rs`，`sampler/src/stream/{chat_completions,responses,messages}.rs`，`chat-state/src/usage.rs`，`shell/src/session/actor/{sideband,updates}.rs`，`session/goal_tracker.rs`，`pager/src/app/status_blocks.rs`。路径均相对于 `crates/codegen/`。

## Goals / Non-Goals

提供可靠的全量 token、缓存读写和覆盖范围。继续使用当前 attempt 身份、durable settlement、Goal、session 和 child 账本，不增加第二套计费事实。

本项不调整任何请求缓存参数、不推断官方价格、不承诺旧格式的完整兼容；已缺失的证据不被迁移为确定的 0。

## Decisions

### 1. 缺失在协议边界保留

在现有 usage 类型上使用可选缓存计数或等价的显式可用性表示。读取是否已知与写入是否已知彼此独立。wire 类型必须先保留字段 presence，再转换；不得到 UI 才猜测 0 的含义。

| 协议 | full input | read / write |
| --- | --- | --- |
| Chat Completions | 可信 `prompt_tokens` | 标准 nested cached count；兼容的 DeepSeek 顶层 hit/miss 以单独 fixtures 核对 |
| Responses | 可信 `input_tokens` | `input_tokens_details.cached_tokens` 与实际返回的 `cache_write_tokens`；核对锁定 SDK 是否保留字段 |
| Messages | uncached input + read + write，或协议明确提供的可信总量 | start 与 delta 保留 presence；delta 缺失沿用 start，显式 0 覆盖旧值 |

初始支持范围就是以上已知字段。Chat 的标准 nested read 存在时优先采用；只有缺失时才使用已验证兼容字段。若同一响应两种 read 计数冲突，则 read 为未知并保留现有诊断证据，不能任选一个或求和。不能根据 endpoint 字符串猜 provider。没有 write 字段时保留未知；只有适用协议明确规定缺失等价于零，才能按该有据可查的约定归零。

Responses 写入字段来自本轮已查阅的 [官方 prompt caching 文档](https://developers.openai.com/api/docs/guides/prompt-caching)；实现时再次核对实际 route/锁定 SDK，并用 wire fixture 证明解析链没有丢弃字段。

### 2. 完整总量与缓存分类独立

full input/output 都可信时，用原有已知 attempt 路径结算，即使 read/write 未报告。Goal 总预算仍为 full input + output，reasoning 不重复加入 output；不能因为 cache 明细未知而停止总 token 预算。

Messages 若只给 uncached input，且组成 full input 所必需的缓存分量缺失、又无协议证据可补零，就不能把 uncached input 伪装为全量输入；沿现有总用量不完整路径处理。完整响应不会因为统计字段不足重采样。总量不完整仍遵守现有精确预算关闭准入规则。

缓存桶越界或别名冲突时，仅把受影响分类标为不可用；若 full totals 独立可信就保留它们。已进入持久账本的格式/身份冲突仍按既有 fail-closed 规则处理。写入量不进入命中分子，费用只沿用 provider 实报，不据本 change 合成价格。

### 3. 聚合保存分子、分母和可用性

扩展现有 `UsageTotals`，保留已知 read/write 累计、read-known 样本的 full input 累计以及各桶未知调用标记/计数。选择最少能无损 fold 的字段，不存每个请求的重复正文或另建逐请求统计库。

对给定报表窗口：

```text
hit_rate = Σ read / Σ full_input（仅 read 和 full_input 均有效的 attempts）
coverage = Σ full_input（上述样本） / Σ full_input（所有总输入已知的 attempts）
```

分母为零显示 N/A。存在总量未知 attempt 时，只能称覆盖“已记录输入”，不能称全部消费覆盖率。完整总量但无缓存信息时，总 token 保持精确；缓存累计标为已知部分。read 已知但 write 未知仍可计算命中率，write 另标可用性。

例如 A 输入 100、read=80，B 输入 900、read 缺失：总输入 1,000，已知样本命中率 80%，输入覆盖率 10%；不能显示总体命中率 8% 或无标记的 80%。B 若明确 read=0，则覆盖 100%、命中率 8%。

### 4. 沿全部既有结算链传递

主 attempt、失败/重试、Sideband attempt、child final bill 和 Goal settlement 都保留同一 availability。幂等 payload 比较包括这些事实；相同身份冲突不能默默覆盖。恢复、resume segment、provider/model 分组和 child fold 只加计数，不平均百分比。

新增结构化字段同步覆盖 ACP/headless。normal 状态栏在 read 覆盖不完整或总账本不完整时显示 cache N/A，详细 `/usage` 展示已知样本比例和覆盖率；避免紧凑栏让部分数据看成完整值。

Goal 的 cache-miss 沿用“未从缓存读取的输入”口径，包含 cache write：对单个 full input 和 read 均已知的 attempt，可由两者之差得到，即使 write 或独立 miss 字段缺失。read 未知的 attempt，其输入分类全部保留未知。聚合时不能把所有 attempt 的 `full_input` 减去部分样本的 `known_read`，就将余量全称为已知 cache miss；Goal 详情单列这些未分类输入。

### 5. 文档与契约同步

修改 `/usage`、normal status、headless 的既有契约，以及 Goal 分类中的总量/分类不完整边界。主规范仍保持当前状态，到实现通过验证并归档后才更新。实现期间开发者说明直接链接 change delta，并在归档时换成主规范链接。

## Risks / Trade-offs

- 只改 mapper 会在 Sideband/child/restore 重新丢字段：验收必须贯通整条消费链，不能先发布失真的中间形态。
- 把分类缺失当成 total incomplete 会误停 Goal；把所有缺失当零又会放行未知总消费：用成对场景验证两种方向。
- SDK 可能在反序列化时丢掉未知字段：升级或最小调整现有协议边界须由 wire fixture 支持，不在 UI 推断。
- 修改公共 usage 形状：一次同步生产者和消费者，不另做双写、兼容分支或后台历史重算。

## Validation

必需 fixtures：缺字段/显式 0/正值、read/write 独立缺失、别名冲突、非法桶、Messages start/delta 覆盖及缺分量、Responses write 非零。贯通 rejected→retry、无 Goal、精确 Goal、Sideband、child final bill、冷恢复和 resident reconnect。核对上述 100/900 输入例子的详情、normal、headless 与恢复结果。

使用各受影响 crate 的定向测试，最终按 `docs/development.md` 的受影响 crate 入口执行综合验证。实际命令和结果在实施时写 `verification.md`，不以 OpenSpec 格式通过代替运行验证。
