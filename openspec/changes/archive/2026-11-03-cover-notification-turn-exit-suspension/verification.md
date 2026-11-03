## Regression

命令：`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p pager --lib notification_turn_viewer_still_suspends_on_double_ctrl_d -- --test-threads=1`。

- 当前实现首次：1 passed、0 failed、0 ignored。
- 临时仅在 `begin_exit_suspension` 恢复旧的 non-viewer guard：同一测试失败，0 passed、1 failed；断言得到 `[UnregisterActiveSession { session_id: SessionId("test-session") }, Quit]`，预期是 `SuspendForExit`。Viewer 标志由生产 ACP handler 推导，未在测试中手动设为 true；退出由真实 AppView 按键入口确认。
- 移除实验 guard 后最终重跑：1 passed、0 failed、0 ignored、7568 filtered out，0.06s。最终源文件没有实验 guard。
- 测试还核对匹配的挂起成功 ACK 到达后才产生 Quit，且没有 CancelTurn。本次未重跑真实进程树 PTY；前轮已安装程序的该验证及其覆盖限制见前序事故记录。

## Checks and review

- `rustfmt --edition 2024 --check crates/codegen/pager/src/app/root/dispatch/tests/router.rs` 和 `git diff --check`：通过。
- 归档前 `openspec validate --all --strict --no-interactive`：18 passed、0 failed。前轮并行 change 的未完成状态已由其自身工作处理，本轮没有修改它。
- 归档后全量 strict：17 passed、0 failed；archive strict：801 passed、0 failed。
- 编译保留既有 Atomic::fetch_update deprecated warnings 及 linker unwind table warning，未扩大本轮修改。
- 对抗性核对：宿主 prompt 不在 self-originated 集合，viewer=true 不能推出 runtime 无工作；恢复旧 guard 在 ACK 前直接 Quit，足以捕获本次错误边界。UI 状态测试未冒充真实 actor 的取消/持久化验收，事故旧内存状态不可直接观测的限制仍保留。
