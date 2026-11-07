# Proposal: finalize-child-hooks-before-result

## Why

runner 在 child 执行返回后的收尾中，先通过共享 ChatStateHandle 直接把 canonical `SubagentResult` 写入 child Timeline（`crates/codegen/shell/src/agent/subagent/handle_request.rs::record_child_result_before`），之后才向 child 发送 `SessionCommand::Shutdown`。child SessionActor 的 `finish_graceful_shutdown` 收到 Shutdown 后无条件派发 SessionEnd Hook 生命周期，其第一笔 Timeline 写入（`Hook::Triggered`，即使 handler plan 为空也先写）被 `crates/codegen/chat-state/src/timeline.rs::validate_header` 的 `subagent_result_recorded` 封闭保护拒绝（`SubagentTimelineEnded`）。

因此在 child Result 已提交的正常路径，SessionEnd 必然落入 `session-end hook lifecycle was not durable` fail-stop 分支：child 正常收尾未完成，Goal close 与最终 persistence frontier 被扣留。隔离 child 日志已出现该错误；归档调查 `2026-11-06-investigate-windows-subagent-result-stall` 亲核控制流确认这是确定的顺序错误，与平台无关。

## What Changes

- child SessionActor 成为自身最终 canonical Result 的唯一提交 owner。runner 准备好完整 `SubagentResultEvent`（含 planning_summary 与 output artifact 引用）后，通过新的内部 `SessionCommand` 终态提交变体把事件与绝对 deadline 交给 child actor；该命令与 `Shutdown` 一样立即 latch Graceful termination 并执行 `begin_graceful_shutdown`，同时把待提交终态带入 `finish_graceful_shutdown`。
- 在 `finish_graceful_shutdown` 内，全部正常 producer（SessionEnd Hook 生命周期、memory/dream、辅助 owner drain、Goal 用量结算与 checkpoint）完成、所有仍可 append 的 owner 退役之后、最终 persistence frontier 之前，actor 用自身 ChatStateHandle 按 deadline 提交该 Result 作为 child Timeline 的最后一条正常事实，并以精确 `TimelineRangeRef` 或 typed `TimelineWriteError` ACK。
- runner 不再直接写 child Result。ACK 成功后父 `Ended` 与 completion receipt 继续严格引用同一精确 ref，各提交一次。取消或 actor 已退役时不等待 ACK、不直接写、不重放 Hook：结果按原 identity 保持 pending，交由现有 `ensure_recovered_child_result` 冷恢复补齐。
- 普通（非 child 终态提交）`Shutdown` 行为不变；`Timeline` 封闭 validator 不放宽。
- runner 保留 output artifact、最终 usage/outcome 与父链接责任；child actor 只拥有最后一次 canonical append。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `session-timeline`：新增 child 终态结果与其自身 session-end 生命周期的持久顺序契约（delta 见 `specs/session-timeline/spec.md`）。

## Impact

- 代码：`crates/codegen/shell/src/session/commands.rs`（内部终态提交命令）、`session/actor/run_loop.rs`（命令臂与 `finish_graceful_shutdown` 提交点）、`session/actor/teardown.rs`（最小拆出最终 append owner 排空段，普通路径语义不变）、`agent/subagent/{handle_request.rs,mod.rs}`（runner 交接与 ACK 映射）及 child 终态相关测试。
- 不改公共工具/API、`Timeline` validator、turn settlement、usage fold 测试、rewind 与 file_state/sampler（属其他 owner）。
- Hook payload 的 `reason` 保持既有 `shutdown` 值，不新增脚本可见枚举。

## Non-Goals

- 不诊断或修复 Windows 现场 IO；不处理 usage freeze 的 Outstanding 等待（另一 coder）、after-snapshot 语义、finalization 重试政策与 pending 自动收敛。
- 不引入全局 finalization manager、第二 writer 或新 journal。
- 不放宽 `subagent_result_recorded` 封闭保护，不跳过或吞掉 SessionEnd，不重放未知 Hook。
