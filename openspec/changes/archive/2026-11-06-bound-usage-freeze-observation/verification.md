# Verification

## Old red（回归先于实现在未修复源上失败）

- 回归测试 `freeze_held_outstanding_ack_fails_closed_at_deadline`（responder 接收 Outstanding 并持有 `respond_to` 不回复；freeze 预算 120ms；测试内 2s 外层守卫使旧行为失败而非挂死）先写入，未改任何生产代码。
- 命令（pinned PATH 下 `cargo --version` = 1.93.1，`RUST_MIN_STACK=16777216`，`CARGO_BUILD_JOBS=2`）：
  `cargo test -p shell --lib --locked --offline freeze_held_outstanding_ack_fails_closed_at_deadline`
- 结果：`FAILED ... panicked ... held ACK must end at the freeze deadline: Elapsed(())`；`test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 4692 filtered out; finished in 2.14s`，cargo exit 101。证明旧实现的 ACK 等待越过 120ms 预算且不会自行结束。
- 首次运行未加 `--lib`，被既有 fixture 编译失败阻塞：`crates/codegen/shell/tests/test_grow_session_update.rs:269` 引用 `SessionUpdate::ResponseReplayProjection`，该 variant 仅在 `#[cfg(any(test, feature = "test-support"))]` 下存在，integration target 以默认 feature 编译不过。这是 HEAD `8ab486a6` 既有状态，与本次改动无关，未修复（父另项处理）。该次命令的 `| tail; echo EXIT=$?` 管道掩盖了 cargo 退出码，后续改为 `--lib` + 日志重定向 + 真实 exit code。

## Green（实现后）

- `cargo test -p shell --lib --locked --offline subagent_usage_fold_tests -- --test-threads=1`
  → `running 17 tests`；`test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 4676 filtered out; finished in 0.47s`；exit 0。
  覆盖：新增 held-ACK 回归 + 既有空集合、前台 live 超时（fail-closed 双账本）、background-only、sticky-only、mid-drain fold、error-path fallback、`usage_drain_outcome_policy_matches_freeze_and_cancel`（freeze/cancel 共享策略）、apply-miss、归属/迟到落账矩阵，断言未修改。
- `cargo test -p shell --lib --locked --offline goal_degradation_tests -- --test-threads=1`
  → `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 4692 filtered out`；exit 0（settlement.rs 既有测试）。
- 取消路径未触碰：`tasks_cancel.rs` 仍以自身 250ms 界包裹原 `outstanding_reply_for_prompt`（helper 签名与行为不变）；`tasks_cancel.rs` 自身测试为 task-slot 机制测试，不经该查询。
- fmt：`rustfmt +1.93.1 --edition 2024 --check`（仅本次三个触碰文件）exit 0；未运行 workspace fmt（其他 agent 在不同文件工作）。

## 最终源 SHA256

- `crates/codegen/shell/src/session/actor/turn/mod.rs`：`0d00130efb005f85c08440b2b806653341f1303c71664ab4a1e83fc606d1a524`
- `crates/codegen/shell/src/session/actor/turn/settlement.rs`：`41e8d92bfd141e9aeff0eca9df7cefbb2f71fdcf8aabfbad38c038c31e83b861`
- `crates/codegen/shell/src/session/actor/tests/subagent_usage_fold_tests.rs`：`d61de7ed7105013245773ba6539b52f5e8cce2287d8d2de9a0f22e60e2933657`

## 边界与未执行项

- 未跑：全量回归、clippy、integration test targets（上述既有编译阻塞）、Windows 现场复现。真实 Coordinator 正常路径未注入实验；回归以持有 sender 的 responder 模拟“接收但不回复”的观察形态，与调查报告的反例控制流一致。
- 零 `max_wait`（admission.rs:1279 fatal-terminal 调用方）：即时 ready 的观察可能被消费（如无 subagent channel 时内部查询立即返回 `Some(Default::default())`），也可能按现语义 fail-closed；不写单一必然行为，两态均在 delta 允许的"即时 ready 或 fail-closed"范围内。
- 迟到 fold 仍沿既有归属路径落 session ledger（本改动未触碰该路径；由既有归属矩阵覆盖）。

## Review 修订（第二轮，父 review 后窄修）

父亲核 diff/SHA 与 old-red + 17 + 1 green 后提出三项窄修，本轮完成，**受影响测试重跑待 Cargo 窗口释放后由父执行**：

1. drain 的 live 回复分支后固定 `sleep(POLL)` 可在仅剩几毫秒预算时越过原 deadline → 改为 `sleep_until(min(now + POLL, deadline))`，clamp 到原绝对期限。
2. bounded helper 由 `timeout(remaining, …)` 改为 `timeout_at(deadline.into(), …)`，直接表达原绝对期限而非每轮换算剩余窗口。
3. doc/design/verification 中零预算单一行为的必然断言（含早先"timeout_at 先检查 deadline、过期绝不 poll 内部 future"的表述）已去除，保留 delta 的"即时 ready 或 fail-closed"两可语义；不据此再调查或改业务代码。

新增回归 `freeze_late_reply_fails_closed_at_deadline`（responder 每次都在 400ms 后才回复，预算 120ms，外层守卫 300ms——宽松量级区分期限收敛与旧无界等待，不做 ms 级墙钟断言；旧实现会在迟到回复时刻 ~400ms 才退出并撞守卫）。held-ACK old-red 证据仍有效：它针对的是基线源的无界等待缺陷，本轮修订未削弱该回归。`error_path_usage_fallback`、冻结/取消既有 outcome、integration fixture（另行任务）均未触碰。goal_degradation_tests 无新改动，沿用上轮 1 passed 证据。

受影响面：`subagent_usage_fold_tests` 由17增至18项。父最终在task `01a1af32-b962-7762-9d18-f996c8801dd3` 编译整合版本并运行25项组合回归，用量18项全通过；唯一失败属独立child deadline测试。随后直接运行该 Cargo 生成的同一默认测试产物：`target/debug/deps/shell-65d26e53a07f48a9 subagent_usage_fold_tests rewind_unknown_snapshot_tests --test-threads=1`，task `01a1af36-8063-78b2-8ef4-5fa93c8446f2`，**21 passed / 0 failed，exit 0**，含本项18项和独立rewind3项。完整日志 `/tmp/grow-backlog-freeze-rewind-green.log`。最终18项包含held-ACK与late-reply两个期限反例，旧17项行为断言保留。

父审查及开发说明已完成。以上是本机源码行为验证，未取得Windows现场，不能据此声明其运行实例已恢复。

修订后文件 SHA256（rustfmt 之后，供父验收核对）：

- `crates/codegen/shell/src/session/actor/turn/mod.rs`：`d3a478114c68db22693a81ba294d947964c7b3cb46da2961b6a7cfe1c17179c8`
- `crates/codegen/shell/src/session/actor/turn/settlement.rs`：`2f33a0ee3cf14d9b21b81e1aa2aa9c734dd10fd5cb1f70becab1aadb7fd50e61`
- `crates/codegen/shell/src/session/actor/tests/subagent_usage_fold_tests.rs`：`64f90b8601a36c2ff897372ac75ef7975a1b30507d13dc5f842fcd2abcc04e06`
