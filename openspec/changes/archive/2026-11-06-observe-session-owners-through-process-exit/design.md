# Design: observe-session-owners-through-process-exit

## 问题与口径

- 缺陷根因：`SessionActivityEntry::is_live` 与 `flush_all_sessions` 的完成判定都是 `!cmd_tx.is_closed()`，而 `begin_graceful_shutdown`（`run_loop.rs:517`）在 SessionEnd 生命周期与最终 flush 之前就 `cmd_rx.close()`，使观察口径在收尾最关键的窗口内恒为「已退出」。
- 修复口径：采用既有 `SessionThread::is_finished`（`crates/codegen/shell/src/session/actor/spawn.rs`）：同时观察 actor 线程与 persistence task，actor 结束而 persistence 未结束时会 `request_persistence_stop` 并保持未完成。该口径与归档规范「Drain the complete session writer incarnation」一致。

## 结构决策

- `SessionActivityEntry` 新增 `session_thread: Option<SessionThread>`。
  - `Some`：liveness/flush 完成条件为 `!thread.is_finished()`（含 persistence 观察）。生产路径唯一形态。
  - `None`：仅 `#[cfg(test)]` 的 `register_for_test` 原始通道 seam（保留 app.rs / leader server 测试的纯通道语义），生产注册签名 `register_session(id, handle, session_thread: SessionThread)` 强制 `Some`，不存在生产 closed-channel 回退。
- `SessionThread` 已 `Clone + Arc`；clone 交给 activity 只用于观察 `is_finished`，join/清理责任仍由 `MvpAgent::session_threads` map 与既有 reaper 持有，不新增 registry、退出 flag 或状态机。
- 同 session 不同 incarnation：仍按 `cmd_tx.same_channel` 去重补发；等待阶段按各自 thread `is_finished` 独立观察。旧 incarnation 的 thread 在 map 中被替换时，entry 仍持有 Arc，直到其 `is_finished`。
- flush 循环结构不变：50ms 重 snapshot、晚注册补 `Shutdown`、单一总 grace 到期 warn 后退出。

## 验证设计（Tier 2，真实 exit observer）

- `rx 已 close 但 thread 未终结`：受控 barrier——真实 `std` 线程持有 entry，接收端已 drop；flush 在释放前必须保持 pending（用 spawn 的 flush 任务 `is_finished` 断言），释放后返回。
- `actor 线程已终结但 persistence 未终结`：用真实 `SessionThread::with_persistence` 挂一个真实阻塞的 tokio 任务作为 exit observer（不 fake zero 延迟）；释放后确认返回。
- 既有 quiesce 语义回归（grace 共享、incarnation 重用、晚注册、挂起放弃）在 `activity.rs` 内以携带真实 thread 的 seam 重写保留。
- 生产注册路径：`agent_ops.rs` 唯一 caller 传入 `session_thread.clone()`；编译即覆盖（无其他生产 caller）。
- Tier 3：ordinary CLI `--no-leader` 真实链路由父在整合重 build 后运行 `test_session_end_hook_e2e`（本项把该 fixture 的诊断日志从硬编码 `/tmp` 收回 tempdir 并在失败消息附日志）。

## 明确不做

- 不改 child run loop/teardown、`SessionThread` 生产、`SessionHandle`、ACP、Timeline 屏障；不推迟 `cmd_rx.close()`；不扩 shutdown 系统。
