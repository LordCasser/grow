# 验证记录

## 红绿回归

- **修复前红测：**
  `cargo test --locked -p sampler --test test_actor owned_shutdown_waits_for_evidence_and_usage_acknowledgments -- --nocapture`
  失败于 `shutdown must wait for blocked evidence acknowledgment`。旧 actor 调用 `JoinSet::shutdown()`，在 response evidence ACK 阻塞时直接 abort request task 并提前完成关闭。
- **修复后绿测：** 同一回归通过。测试先等待 request/response 两次 evidence sink 调用已进入，在 response ACK 释放前确认 shutdown 仍等待；随后等待 usage sink 已进入，在 usage ACK 释放前再次确认 shutdown 仍等待；最后断言 evidence 恰好两次、usage 恰好一次且 request 完成收尾。

## 最终状态定向验证

- `CARGO_BUILD_JOBS=2 cargo test --locked -p sampler --test test_actor owned_shutdown_waits_for_evidence_and_usage_acknowledgments -- --nocapture`：1 passed，22 filtered out。
- `CARGO_BUILD_JOBS=2 cargo test --locked -p sampler --lib actor::owner_tests -- --nocapture`：2 passed，覆盖 hung owner forced abort 与 panicked owner 终态观察。
- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib session::actor::teardown::sampler_shutdown_tests::shutdown_sampler_joins_drainer_and_breaks_session_cycle -- --nocapture`：1 passed，证明 sampler event drainer 在 owner 后退出并释放 Session 引用。
- `rustfmt --check --edition 2024 crates/codegen/sampler/src/actor/mod.rs crates/codegen/sampler/tests/test_actor.rs`：通过。
- `git diff --check`：通过。
- `openspec validate --all --strict --no-interactive`：19 passed，0 failed。
- `cargo fmt --all -- --check`：未通过；只报告仓库其他 crate 已有格式漂移，本 change 没有修改这些文件。受影响 Rust 文件已由上面的定向 `rustfmt --check` 证明干净。

## 磁盘回收

全部 Rust 验证完成后执行 `cargo clean`：

- 清理前：文件系统剩余 106 GiB，`target/` 为 25 GiB。
- `cargo clean`：删除 120,914 个文件，Cargo 报告回收 26.1 GiB。
- 清理后：文件系统剩余 118 GiB，`target/` 已移除。

清理后未再次运行 Rust 构建，避免立即重新生成产物；OpenSpec 文档更新不依赖 Cargo 构建。

## 覆盖边界

本回归覆盖正常 actor shutdown 对阻塞 evidence/usage ACK 的协作等待，并保留现有 owner deadline forced-abort 测试。没有另建 provider 永久卡死的集成场景；现有 bounded owner timeout 仍是唯一强制终止边界。未运行真实 Provider 或进程 kill-point 实验。
