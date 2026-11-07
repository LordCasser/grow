# Tasks: observe-session-owners-through-process-exit

## 1. SDD

- [x] proposal / delta（session-timeline）/ design 建立。

## 2. 实现

- [x] `activity.rs`：`SessionActivityEntry` 携带 `Option<SessionThread>`；`is_live` 与 flush 完成条件改为 thread `is_finished`（`Some` 时），生产注册签名强制真实 thread。
- [x] `agent_ops.rs`：唯一生产 caller 注册时传入 `session_thread.clone()`（map/reaper 保留原 owner）。

## 3. 验证

- [x] Tier 2（activity 单测，真实 exit observer）：rx 已 close 但 thread 未终结 → flush 保持 pending，释放后返回；actor 线程已终结但 persistence 未终结 → flush 不返回（`with_persistence` + 真实阻塞任务），释放后返回。
- [x] 既有 quiesce 回归保留：grace 共享、incarnation 通道重用、晚注册补 Shutdown、grace 放弃、busy 语义。
- [x] e2e fixture：诊断日志收回 tempdir，失败消息附读日志；marker/stdin/event/reason 断言保留（父在整合重 build 后跑 Tier 3）。
- [x] `rustfmt --edition 2024` 仅本项文件；最终主工作树统一96项通过，新CLI普通退出及child正常/取消三条e2e均通过；独立owner review未发现可行动缺陷。具体命令与边界见verification.md。

## 4. 验证记录

- `cargo +1.93.1 test -p shell --lib agent::activity`：14 passed（含 3 个新增真实 incarnation 口径测试）。
- `cargo +1.93.1 test -p shell --lib auto_update_flushes`：1 passed；`--lib leader::server`：138 passed（共享 `register_for_test` seam 的下游测试不受影响）。
- `cargo +1.93.1 test -p shell --test test_session_end_hook_e2e`：编译通过（1 ignored，Tier 3 需真实 binary，父整合重 build 后运行）。
- `rustfmt --edition 2024 --check` 三个改动文件通过。
- `openspec validate --all --strict --no-interactive`：20 passed, 0 failed。
