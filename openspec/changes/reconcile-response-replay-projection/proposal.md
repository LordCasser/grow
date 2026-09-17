# Change: Reconcile admitted response replay projections

## Why

Timeline 已经以 `{request_id, attempt}` 持久接纳 canonical assistant response，但 `updates.jsonl` 仍只在随后到达的瞬态 `SamplingAttempt::Accepted` 上把内存 candidate 写成历史。进程在两者之间停止，或 Accepted flush 写入失败时，模型 Surface 保留 response，而 session/load、子任务视图和导出只能从 `updates.jsonl` 看到缺失的 UI 历史。后续工具结果、继续采样或回复因此可能没有用户可见的因果前件。

## What Changes

- 为 identity-bearing Timeline response 定义一个独立、可校验、可重建的 accepted replay projection；它是 UI cache record，不是第二份 response authority，也不冒充瞬态 SamplingAttempt lifecycle。
- Shell 从已接纳 Timeline response 生成确定性的 ACP text/reasoning projection，并在发布 Accepted、继续 truncation/pause-turn、执行工具或结束 Turn 前等待该 projection durable ACK。
- persistence actor 在同一 FIFO 边界丢弃 exact attempt 的 provisional candidate cache、保留交织的独立更新，并幂等持久化单条 response projection record；写入确认不明时按 identity、Timeline event 和 digest 核对，冲突 fail closed。
- session replay 在建立 snapshot/cursor cutoff 前，以 Timeline 为权威补齐缺失 projection；writer 可用时修复 cache，只读 replay 在内存中合并。普通 session/load、子任务 replay 与导出共享同一 projection 解析/重建规则。
- quarantine、fallback-only response、discarded earlier attempt、rewind 与 legacy identity-less response 使用显式失败/忽略语义，不通过文本或位置猜测绑定。

## Capabilities

### Modified Capabilities

- `session-timeline`: response durable admission 之后新增 replay projection gate；projection failure 不能跨越到 Accepted、provider recovery、continuation 或工具副作用。
- `client-surfaces`: accepted history 从 Timeline-derived projection 恢复；`updates.jsonl` 保持可重建 cache，所有 production replay 入口去重且保持因果顺序。

## Impact

- 主要实现：`crates/codegen/chat-state/src/timeline.rs` 的 validated admitted-response query；`crates/codegen/shell/src/session/actor/*`、`persistence.rs`、`storage/*`、`agent/mvp_agent/*` 的 projection/gate/replay；相关 pager/export caller 只切换到统一 replay 语义，不新增 UI 状态权威。
- 需要 persistence fault injection、cold/resident session/load、cursor、direct child replay、fallback、quarantine、tool ordering、discarded attempt 和 rewind 回归。
- 不改变 provider retry 分类、Timeline response payload、native continuation 持久化或工具执行 authority。
