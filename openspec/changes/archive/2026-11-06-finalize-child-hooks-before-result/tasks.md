# Tasks: finalize-child-hooks-before-result

## 1. SDD 文档

- [x] 1.1 核对当前实现与契约（handle_request 收尾、run_loop Shutdown/finish_graceful_shutdown、hook_dispatch Triggered、timeline 封闭保护、teardown 最终排空、取消/panic/冷恢复路径），形成 proposal/design/delta。
- [x] 1.2 `openspec validate finalize-child-hooks-before-result --strict --no-interactive` 通过（输出：`Change 'finalize-child-hooks-before-result' is valid`）。

## 2. 实现

- [x] 2.1 `session/commands.rs`：新增内部 `CommitSubagentResult` 终态提交变体（`SubagentResultEvent` + 绝对 deadline + typed ACK oneshot）。
- [x] 2.2 `session/actor/run_loop.rs`：新命令臂（runtime_suspend → `Err(Cancelled)`；latch/`begin_graceful_shutdown` 失败 → `Err(Cancelled)` + 既有 fatal teardown；成功 → `GracefulShutdown { terminal_commit }`）；`GracefulShutdown` 增加 `terminal_commit` 字段，既有构造填 `None`。
- [x] 2.3 `session/actor/teardown.rs`：最小拆出 `drain_final_persistence_owners`（幂等），`final_session_persistence_flush` 复用之，普通路径语义不变。
- [x] 2.4 `session/actor/run_loop.rs::finish_graceful_shutdown`：permission audit fail-stop 检查通过后、`final_session_persistence_flush` 前（父方 review 修正位置）：先 `drain_final_persistence_owners`（失败 → 不提交、ACK `Err(Cancelled)`），再按 deadline 提交 `SubagentResult`，ACK 精确 `TimelineRangeRef` 或 typed 失败；audit 失败沿用既有 fail-stop，不 ACK 成功。
- [x] 2.5 `agent/subagent/{mod.rs,handle_request.rs}`：`record_child_result_before` 改为“准备事件 + 交接 actor + durable ACK”（`handed_off` 在命令送达 mailbox 时即置位，deadline 分支不会误发普通 `Shutdown`）；send 失败/ACK drop → `AcknowledgementLost`（Unknown）；交接成功后不再补发普通 `Shutdown`，未交接时保留 Shutdown（含 deadline fallback 分支）；父 `Ended`/receipt/usage fold/worktree 逻辑不变。
- [x] 2.6 自查：`timeline.rs` validator 未改；turn settlement、usage fold 测试、rewind、file_state、sampler 未触碰；普通 `Shutdown` 路径与 Hook payload `reason` 值不变；rustfmt（--edition 2024）对全部触碰文件 clean。Cargo 编译/测试受 main freeze 限制未运行，静态核对了新命令变体的全部 match 位点（run_loop 主 match 新增臂；sideband/goal/planning mailbox、notification_bridge、tests fixture 均为非穷举或带 `_ =>`）。

## 3. 测试（Tier 3；Cargo 窗口由父协调，未运行前保持未勾选并标注）

- [x] 3.1 老顺序 red：真实 child 公共生命周期（复用 subagent 隔离测试入口）完成带 SessionEnd Hook 的任务，断言 Hook occurrence 恰一次、`SubagentResult` 恰一次且为最后一条事件、父 `Ended` 精确引用、无 `session-end hook lifecycle was not durable`。测试已写：`crates/codegen/shell/tests/test_subagent_session_end_order.rs`（`#[ignore]`，需预构建二进制）；red 使用旧 CLI `f0f461e54`，实际失败于缺 SessionEnd occurrence；当前 CLI 同测试1 passed。详见 verification.md 的完整旧红新绿日志。
- [x] 3.2 actor 级：向 subagent actor 发 `CommitSubagentResult` 并等待退出，验证 SessionEnd Hook 事实恰一次在 Result 前、Result 为最后一条、ACK ref 与持久事件一致；无 Hook 配置的对照。测试已写：`crates/codegen/shell/src/session/actor/tests/subagent_terminal_commit_tests.rs`（默认测试集；空 handler plan 即“无 Hook 配置”对照，Triggered 仍先写）。真实运行一轮（父窗口，25 项）：seed 修正生效，正常/Cancelled/丢 ACK 三测通过；expiry 测试失败于 poisoned writer 后 `timeline_events()` 返回 `None`，已按父设计重构：fixture 改既有 `create_test_actor_ex_with_physical_projection`，本地 stub 先记录成功 `TimelineDurablyAndAck` 进共享 Vec 再 ACK，失败路径以持久记录为证据（含 seed 阳性对照），不再 unwrap 内存查询。最终重构后的五项 actor 均实际通过（与七项 SessionAdmin 组合12 passed / 0 failed）。
- [x] 3.3 取消：真实runner以background child进入受控provider后由父kill并wait，Cancelled Result为最后事实，SessionEnd Triggered/Completed在前，父Ended唯一并精确引用；新CLI上实际通过，见 `/tmp/grow-backlog-terminal-e2e-integrated.log`。
- [x] 3.4 失败面：最终五项actor实际通过，含Hook永久写错、typed deadline与ACK接收者丢失；无合法Result时不向父交付终态，未知ACK保留原identity。既有248项subagent组覆盖cold拒绝live writer、interrupted provenance、receipt幂等与usage先于terminal；本项冷恢复路径未改、不重放Hook。日志见verification.md。
- [x] 3.5 普通 `Shutdown` 回归：普通非leader headless在新CLI上执行Hook、stdin envelope与reason=shutdown均通过。补验发现的Activity owner竞态由独立observe-session-owners-through-process-exit修复，未放宽本项断言。

## 4. 收尾

- [x] 4.1 rustfmt 自文件（8 个触碰文件 `--edition 2024` clean）；生成包含 tracked/untracked 的 git diff patch 到 /tmp 交父（不 commit、不 apply 主 workspace），patch 仅显式列本项文件。
- [x] 4.2 汇报实现、接口/owner决策、真实旧红新绿、取消/普通退出与平台限制，父核实际diff及原始证据后验收。
