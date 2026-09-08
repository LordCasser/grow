## 验证记录

- 旧实现：`CARGO_BUILD_JOBS=2 cargo test --locked --offline -p update --lib test_cleanup_old_downloads_keeps_future_dated_versioned_binary --quiet`，0 passed / 1 failed；mtime 设置到未来一小时的候选被误删。
- 修复后：`CARGO_BUILD_JOBS=2 cargo test --locked --offline -p update --lib --quiet -- --test-threads=2`，132 passed / 0 failed，20.15s。
- 同一临时目录夹具同时证明未来时间候选保留、明确过期项删除、N-1 与当前版本保留；既有新文件、临时文件、不可解析版本与符号链接回归保持通过。
- `rustfmt --edition 2024 --check crates/codegen/update/src/auto_update.rs`、`git diff --check`、OpenSpec 全量严格检查通过。

未知 metadata 分支由 Option 判定直接保持 false（不满足 stale），本次真实文件测试覆盖时钟回拨，不声称已注入所有文件系统错误或消除检查→删除的竞争。没有触及用户下载目录或已安装程序。

开发者说明已更新 `crates/codegen/update/README.md`。多平台上一版本按文件计数的语义问题登记 `openspec/backlog.md`，未混入本修复。
