## 验证
- 旧实现：`cargo test --locked --offline -p update --lib test_update_version_checks --quiet`，2 项失败；超时和任务取消后直接子进程仍存活。夹具在断言前清理遗留进程。
- 修复后：`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p update --lib --quiet -- --test-threads=2`，128 passed，0 failed，20.11s。
- 覆盖 probe/smoke 的超时、取消及正常版本输出；真实 Unix 子进程检查在 macOS 执行。Windows 未运行此 Unix 夹具；生产使用 Tokio kill_on_drop 通用接口，不承诺任意后代进程树清理。
- 开发者说明：`crates/codegen/update/README.md`。
