# session-timeline Delta: finalize-child-hooks-before-result

## ADDED Requirements

### Requirement: Child terminal result is the last fact of its own session lifecycle

child 会话的 canonical `SubagentResult` SHALL 由该 child 自身的 SessionActor 提交：在已封闭新准入、foreground/控制/辅助工作退役、SessionEnd 生命周期（Hook 事实、memory/dream、Goal 结算与 checkpoint）与全部晚位正常 producer（含 memory summary 发送与 Hook UI 投影）完成、且所有仍可 append 的 owner 已排空之后，最终 persistence frontier 之前，作为该 child Timeline 的最后一条正常事实写入。runner SHALL 通过内部终态提交请求把完整事件（含 planning_summary 与 output artifact 引用）与绝对 deadline 交给 child actor，并只在 durable 完成后收到精确 `TimelineRangeRef`；失败 SHALL 返回 typed Timeline 写失败，不伪造成功。runner SHALL 保留 output artifact、最终 usage/outcome 与父链接责任；父 `Ended` 与 completion receipt SHALL 引用 ACK 返回的同一结果且各只提交一次。`Timeline` 对已记录 subagent result 的封闭保护 SHALL NOT 被放宽，SessionEnd SHALL NOT 被跳过或吞掉。

#### Scenario: Normal completion with a SessionEnd hook

- **WHEN** 配置了 SessionEnd Hook 的 child 正常完成执行且 runner 收到执行返回
- **THEN** child Timeline 中 SessionEnd Hook occurrence（Triggered 与其 Completed 聚合）恰好一次且位于 `SubagentResult` 之前，`SubagentResult` 恰好一次且是该 Timeline 最后一条事件，父 `Ended` 恰好一次并精确引用该结果。

#### Scenario: No SessionEnd handlers are configured

- **WHEN** child 未配置任何 SessionEnd handler
- **THEN** 空 plan 的 Hook 事实仍在 Result 之前记录，Result 仍为最后一条 canonical 事实；收尾不因 handlers 为空而被跳过、降级或改由 runner 直接写。

#### Scenario: SessionEnd lifecycle is not durable

- **WHEN** child 的 SessionEnd Hook 生命周期未达 durable（例如 Hook 事实写入失败）
- **THEN** child 不提交 Result，runner 不直接写 child Timeline，父不写 `Ended` 与 completion receipt，finalization 按原 identity 保持 pending；后续恢复只读取既有 Hook 事实，不重新执行 Hook 或 provider。

#### Scenario: Child actor is already retired

- **WHEN** 取消或早期错误路径已使 child actor 退役（命令通道关闭或已 latch 终止），runner 无法送达终态提交请求
- **THEN** runner 不等待已退出 actor 的 ACK、不直接写 child Result、不重放 Hook，结果按原 identity 保持 pending，由既有冷恢复（exact 身份、owner lease、`ensure_recovered_child_result`）补齐 Result；收尾 deadline 不因该等待续期。

#### Scenario: Result commit acknowledgement is lost or the deadline elapses

- **WHEN** 终态提交请求已送达但 durable ACK 丢失，或收尾预算 deadline 到期
- **THEN** 不二次提交 Result、不写父 `Ended` 与 receipt；恢复按原 identity 精确核对已写入事实，仅在证明未写入后 exact 重放一次，绝不追加第二份 Result 或第二份父终态。

#### Scenario: Committed result write fails

- **WHEN** child actor 侧 Result 写入返回 typed 失败（含 deadline、持久化失败或 append owner 未能在期限内排空）
- **THEN** ACK 携带该 typed 失败，runner 记录 ChildResult 阶段失败并保持 finalization pending，不把失败伪装成成功或取消；普通 shutdown 的最终 frontier 扣留行为保持。

#### Scenario: Cancelled child still ends through its own actor

- **WHEN** child 的执行被取消但其 SessionActor 仍存活
- **THEN** 取消结果的提交沿用同一终态顺序（SessionEnd 生命周期在前、取消 `SubagentResult` 为最后事实），父 `Ended` 引用同一取消结果；runner 的普通取消与子任务拆除行为不变。

#### Scenario: Ordinary shutdown without a terminal commit is unchanged

- **WHEN** 未携带终态提交的普通 `Shutdown` 触发（非 subagent 会话、早期失败路径或显式关闭）
- **THEN** 其 SessionEnd 生命周期、最终 persistence frontier 与既有行为完全一致，不产生 Result 提交或 ACK。

证据与实施入口：`crates/codegen/shell/src/agent/subagent/handle_request.rs`（runner 终态交接）、`crates/codegen/shell/src/agent/subagent/mod.rs::record_child_result_before`（事件准备与交接）、`crates/codegen/shell/src/session/commands.rs`（内部终态提交命令）、`crates/codegen/shell/src/session/actor/run_loop.rs::finish_graceful_shutdown`、`crates/codegen/shell/src/session/actor/teardown.rs::final_session_persistence_flush`（append owner 排空段）、`crates/codegen/chat-state/src/timeline.rs::validate_header`（封闭保护不放宽）；既有 `ensure_recovered_child_result` 冷恢复与 `test_session_end_hook_e2e` / subagent 隔离测试为对照证据。
