# Verification

基线 `8ab486a6`；父预审并整合专属 patch 后，在 main 共享 target、Rust 1.93.1、`--locked --offline`、`RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2` 下验证。

- `cargo +1.93.1 test --locked --offline -p workspace --lib file_state -- --test-threads=1`：实际 **41 passed / 0 failed，exit 0**，task `01a1af28-af7d-7bb0-b96b-77989fdd190f`。覆盖成功/确认不存在/错误/UTF-8 失败/活 sender 永不返回的同轮矩阵、公共5秒预算、重试清旧、merge最后触及轮、serde/lazy往返。该组合命令的envrc独立失败不作为本项失败或通过证据。
- `cargo +1.93.1 test --locked --offline -p shell --lib -- subagent_usage_fold_tests rewind_unknown_snapshot_tests subagent_terminal_commit_tests --test-threads=1`：编译成功，25项中24通过；本项三条 `rewind_unknown_snapshot_tests` 全通过，独立child deadline测试失败另项处理。
- 父随后直接运行该 Cargo 生成的同一默认测试产物 `target/debug/deps/shell-65d26e53a07f48a9 subagent_usage_fold_tests rewind_unknown_snapshot_tests --test-threads=1`，task `01a1af36-8063-78b2-8ef4-5fa93c8446f2`：**21 passed / 0 failed，exit 0**，其中本项3条全部通过。日志 `/tmp/grow-backlog-freeze-rewind-green.log`。

Shell三条实际调用 `handle_rewind`，分别验证最新触及轮unknown不使用旧after/不判clean、force恢复原内容、ConversationOnly合并后仍unknown；测试内含确认删除仍clean的对照。Workspace测试证明未知在持久表示与lazy恢复保持，capture返回后不启动后台写点；底层读可能在外部完成，未宣称中止它。

父核对生产diff与设计：未知沿既有schema缺条目表达，`content=None`仅确认不存在，merge清旧、consumer最后触及点和force原语义一致。开发说明同步链接主契约。

限制：未在原实现上单独执行新公共路径watchdog，故不声称已有old-red；旧无界await由已核源码支持，当前行为由上述实际测试验证。Windows现场未取得，不把本机源码回归当其根因或现场恢复证据。读取观察预算不承诺强行终止锁等待、同步I/O或底层远端操作。
