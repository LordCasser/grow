# 验证记录

- 已核对 `openspec/README.md`、client-surfaces 正式规范、inventory 归档中的 local-diagnostics 草稿、既有 `preserve-unowned-debug-link-temporaries` 变更、`debug_log.rs`、`appender.rs` 及调用/测试边界。
- 静态确认：per-session 路由仅在 `install_firehose` 的 PerSession 分支安装后调用一次 `sweep_old_logs`；无周期任务或字节配额。一个 RoutingLayer 对每个 distinct session key 只保留一个 writer/guard；guard 存于 appender 进程级 Vec，`flush` 清空 Vec，WorkerGuard drop 会 flush 并 join。Concurrent first-write 测试用子进程、16 个并发写者证明同 sink 只留一个 worker；已有跨进程回归证明持有协作锁的旧文件可保留，释放后可清理。
- 本 change 新增测试：8 个 distinct session 各写一条确定长度的行，断言 guard 数增加 8、flush 后归零，flush 完成后文件总字节数等于所有输入行字节数；Unix 下对不持协作锁但仍由另一 FD 打开的旧文件做 unlink 对照。
- `openspec validate --all --strict --no-interactive`：20 项通过；`rustfmt --edition 2024 --check crates/codegen/diagnostics/src/debug_log.rs` 与本 change 的 `git diff --check` 通过。
- `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p diagnostics --lib debug_log::tests`：26 passed, 0 failed, 0 ignored；覆盖新增增长/释放及非协作 writer 对照。
