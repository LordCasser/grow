## 原始失败证据

- `cargo test --locked --offline -p chat-state --lib multi_context_control_can_rebuild_after_replay --quiet`：旧实现 0 passed / 1 failed，完整替换报 `IncompleteShadowSet`，与用户 resume 报错对应。
- `cargo test --locked --offline -p shell --lib idle_controls_refresh_behavior_picker --quiet`：旧 Shell 0 passed / 1 failed；实际 foreground 已 Idle，最后 ACP AvailableCommandsUpdate 仍包含全部 Behavior unavailable 和截图中的 Stop the active foreground work 原因。

## 修复后验证

命令使用 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216`。

- `cargo test --locked --offline -p chat-state --lib --quiet`：464 passed，0 failed，13.53s。覆盖 durable initial context、第二次 replay、控制上下文即时身份、step/turn 分批激活和同层 supersession；校验仍拒绝不完整 shadow set。
- `cargo test --locked --offline -p shell --lib session::actor::model_switch::tests --quiet`：24 passed，0 failed，0.70s。新增回归通过实际 Agent 和持有 catalog authority 的 model 控制，在没有用户消息的会话检查 ACP 最终可用性，并分别连续选择 Goal → Workflow → Normal。
- `cargo test --locked --offline -p shell --lib session::actor::session_mode:: --quiet`：6 passed，0 failed，0.09s。
- 四个修改 Rust 文件 `rustfmt --edition 2024 --check` 与 `git diff --check` 通过。

## 边界

测试使用真实 Shell / ChatState actor 和 Timeline 重放，未读取或修改用户原会话文件，未发起 provider 请求。没有替换用户 PATH 中的已安装二进制。macOS linker 报已有大体积 unwind table 警告，测试正常完成。

开发者说明同步到 `docs/architecture/behavior-state-overview.md` 与 `docs/architecture/agent-core-timeline.md`，链接相应主规范。

`CARGO_BUILD_JOBS=2 cargo build --locked --offline -p cli --bin grow --quiet` 成功，产物 `target/debug/grow`；最终全量严格规范校验通过，归档后再次校验主规范和 archive。
