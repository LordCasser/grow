# Bound the usage-freeze Outstanding ACK wait by the original deadline

## Why

Windows subagent-result-stall 调查（`archive/2026-11-06-investigate-windows-subagent-result-stall/investigation.md` 发现一）确认了一个期限覆盖缺口：`turn/mod.rs` 的 drain 先计算 120 秒绝对 deadline，但每轮先 `await outstanding_reply_for_prompt`，而 `settlement.rs` 中该查询在 send 之后直接 `rx.await`，没有任何期限。接收方把请求排队或持有 `respond_to` 却不回复时，deadline 分支不可达：prompt waiter 不返回，`execution_returned` 与其后的 30 秒 finalization 都不会开始。turn 错误路径的 `error_path_usage_fallback` 在 actor loop 上发起同一查询，同样是无界等待。Coordinator 正常 Outstanding 处理是同步回复，因此这是一次有界等待修复，不是生命周期重设计。

## What Changes

- freeze 路径的每一次 Outstanding 用量观察（含排队与 ACK 等待）都被 freeze 的单一绝对 deadline 约束，不按轮刷新；到期映射到既有查询失败语义：fail-closed，prompt report 与双账本标记 incomplete。
- `error_path_usage_fallback` 沿取消路径已有的 bounded observation 约定（短固定窗口）收敛，到期等同查询失败，报告可能 under-count。
- 剩余预算为零的轮次允许消费即时 ready 的观察，否则按现语义 fail-closed（fatal-terminal 零预算调用方保守成立）。
- 不终止 child、不记零用量、不新建 ledger、不做全局 RPC 改造；迟到 fold 仍沿既有归属路径落在 session ledger。

## Impact

仅 Shell：`session/actor/turn/{mod.rs,settlement.rs}` 与 usage-fold 测试。取消路径既有 250ms 有界查询、Coordinator、admission 调用方与账本语义不变。新增一条 session-timeline 契约。对应 backlog 非延期项；child SessionEnd 顺序、文件快照等待等同级事项不在本项范围。
