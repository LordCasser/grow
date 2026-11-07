# Proposal: observe-session-owners-through-process-exit

## Why

`AgentActivity::flush_all_sessions`（`crates/codegen/shell/src/agent/activity.rs`）把「命令入口已关闭」当作「Session actor/persistence 已退役」：entry 的存活与 flush 完成条件都是 `!cmd_tx.is_closed()`。但 `begin_graceful_shutdown`（`crates/codegen/shell/src/session/actor/run_loop.rs:517`）在 `finish_graceful_shutdown` 的 SessionEnd 生命周期与最终 flush **之前**就执行 `cmd_rx.close()`，此后命令通道对所有发送方呈现 closed，而 actor 线程与其 persistence task 仍在收尾。结果：进程退出（in-process `/exit`、one-shot quit；leader auto-update 同理）时 `flush_all_sessions` 立即认为 actor 已退出并返回，`LocalSet` drop 截断承载 persistence worker，真实的 ordinary `--no-leader` e2e 出现 ACK 丢失、SessionEnd Hook marker 缺失。归档规范「Drain the complete session writer incarnation」要求 actor thread 与 persistence task 双终结；当前 flush 观察口径违反该契约。

## What Changes

- `AgentActivity` 的 session entry 在注册时携带所属 `SessionThread`（`SessionThread` 已 `Clone + Arc` 共享所有权，clone 不转移 join 责任；现有 map/reaper 保留原 owner 与 join）。
- entry 存活（`is_live`）与 `flush_all_sessions` 的完成观察改为 `SessionThread::is_finished()`（同时观测 actor 线程与 persistence task，必要时 `request_persistence_stop`），不再使用 `cmd_tx.is_closed()`。
- 生产注册入口 `register_session(id, handle, session_thread)` 必传真实 thread；生产路径不得回退到 closed-channel 判定。测试可用 `cfg(test)` 辅助生成 owner。
- 其余语义不变：flush 仍按命令通道身份去重识别同 session 不同 incarnation、每 50ms 重 snapshot、晚注册补 `Shutdown`、单一总 grace（10s）到期记日志后退出，不无限等待；不推迟 `cmd_rx.close()`、不放宽 Timeline 屏障。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `session-timeline`：进程退出 flush 必须观察到完整 writer incarnation（actor thread + persistence）退役（delta 见 `specs/session-timeline/spec.md`）。

## Impact

- 代码：`crates/codegen/shell/src/agent/activity.rs`（entry 结构、liveness、flush 完成条件与测试 seam）、`crates/codegen/shell/src/agent/mvp_agent/agent_ops.rs`（唯一生产 caller 在注册时传入 `SessionThread` clone）、`crates/codegen/shell/tests/test_session_end_hook_e2e.rs`（诊断日志 tempdir 隔离并在失败消息附日志）。
- 不改 child run loop/teardown、`SessionThread` 生产、`SessionHandle`、ACP、其他文件；不建新 registry 或退出 flag。

## Non-Goals

- 不重做整个 shutdown 系统，不推迟 `cmd_rx.close()`，不放宽 SessionEnd/Timeline 屏障。
- 不取消 grace 上限或引入无限等待；`SessionEnd` hook timeout 与 flush grace 的对齐维持既有已知缺口记录。
