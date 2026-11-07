## MODIFIED Requirements

### Requirement: Drain the complete session writer incarnation
Session lifecycle drain SHALL include termination of the persistence task and release of its storage writer ownership as well as termination of the dedicated actor thread. A flush acknowledgement alone SHALL NOT establish writer termination. 进程退出前的会话 flush（`AgentActivity::flush_all_sessions`）SHALL 以完整 incarnation（actor 线程与 persistence task 双终结，即 `SessionThread::is_finished`）作为每个会话的完成条件；命令通道入口关闭（`cmd_rx.close()` / `cmd_tx.is_closed()`）SHALL NOT 被解释为该会话 actor 或 persistence 已退役。flush SHALL 保留既有 quiesce 语义：按命令通道身份区分同 session 的不同 incarnation、周期性重 snapshot 并对晚注册会话补发 `Shutdown`、单一总 grace 到期记日志后退出而不无限等待。生产注册路径 SHALL 携带真实 `SessionThread`，不得在生产回退到 closed-channel 判定。

Storage writer ownership SHALL release its OS lock when its final legitimate shared owner retires or explicitly relinquishes that lease. A descriptor inherited by an unrelated Unix subprocess SHALL NOT prolong that ownership; a still-live shared storage owner SHALL continue excluding replacement writers.

#### Scenario: Actor exits before persistence owner
- **WHEN** the actor thread has terminated but its persistence task still owns the writer lease
- **THEN** close/replacement drain remains pending and does not admit a replacement writer.

#### Scenario: Complete writer exit
- **WHEN** both the actor thread and the persistence owner have terminated
- **THEN** lifecycle drain may finish and a subsequent cold load can acquire the writer lease.

#### Scenario: Drain timeout
- **WHEN** the persistence owner does not terminate before the existing drain deadline
- **THEN** drain reports the outstanding shutdown and preserves the old incarnation's ownership record for a later retry.

#### Scenario: Retained handle after actor exit
- **WHEN** an external resource retains a persistence sender after the actor thread exits
- **THEN** lifecycle drain explicitly closes persistence admission, drains already accepted messages, and waits for owner exit without requiring that handle to be dropped.

#### Scenario: Command entry closed while owners still run

- **WHEN** 会话 actor 已执行 `cmd_rx.close()`（graceful shutdown 早期）但其 actor 线程或 persistence task 仍在执行 SessionEnd 生命周期与最终 flush，进程退出调用 `flush_all_sessions`
- **THEN** flush 对该会话保持 pending，不因命令入口关闭而提前返回；进程退出路径不得在 owner 退役前截断其收尾。

#### Scenario: Actor thread finished but persistence task alive

- **WHEN** 某 entry 的 actor 线程已终结但其 persistence task 仍在运行
- **THEN** `flush_all_sessions` 不返回，并可通过既有 `request_persistence_stop` 促使 persistence 收敛；仅在两者都终结后该会话完成。

#### Scenario: Grace expiry and late registration unchanged

- **WHEN** grace 到期仍有 incarnation 未退役，或 flush 期间新注册/同 id 重生新 incarnation
- **THEN** 到期时记 warn 日志后退出（不无限等待）；新 incarnation 按通道身份单独收到 `Shutdown` 并被独立等待，语义与既有 quiesce 循环一致。

证据与实施入口：`crates/codegen/shell/src/agent/activity.rs`（entry 携带 `SessionThread`、`is_live`/flush 完成条件）、`crates/codegen/shell/src/agent/mvp_agent/agent_ops.rs`（生产注册传入 thread clone）、`crates/codegen/shell/src/session/actor/run_loop.rs::begin_graceful_shutdown`（`cmd_rx.close()` 早于 `finish_graceful_shutdown` 的顺序证据）、`crates/codegen/shell/src/session/actor/spawn.rs::SessionThread::is_finished`（双 owner 观察口径）、`crates/codegen/shell/tests/test_session_end_hook_e2e.rs`（ordinary `--no-leader` 真实链路回归）。

#### Scenario: Unrelated child retains an inherited descriptor
- **WHEN** an unrelated Unix subprocess still has an inherited writer-lock descriptor after the final legitimate storage owner retires or explicitly releases the lease
- **THEN** a replacement owner can acquire the lease without waiting for that subprocess to exec or exit, and closing its stale inherited descriptor does not release the replacement owner's lock.

#### Scenario: Shared storage owner remains live
- **WHEN** one adapter is dropped while another clone still retains the shared writer ownership
- **THEN** replacement acquisition remains refused until the final legitimate owner retires or explicitly releases that lease.
