## 验证记录

- 旧实现：`CARGO_BUILD_JOBS=2 cargo test --locked --offline -p update --lib test_cleanup_old_downloads_retains_whole_version_groups --quiet`，0 passed / 1 failed。当前 0.1.141 时，最高其他版本 0.1.140 的 linux-x86_64 产物被删除。
- 修复后：`CARGO_BUILD_JOBS=2 cargo test --locked --offline -p update --lib --quiet -- --test-threads=2`，133 passed / 0 failed，20.16s。
- 新回归使用真实临时目录，覆盖 grow/grow-pager 两个前缀、正常升级/回退、正反创建顺序，以及当前/最高其他版本的全部平台产物保留和更旧产物删除。
- 既有未来时间戳、新文件、未知布局、版本前缀、预发布版本、符号链接等回归保持通过。
- `rustfmt --edition 2024 --check crates/codegen/update/src/auto_update.rs`、`git diff --check` 和全量 OpenSpec 严格校验通过。

开发者解释更新至 `crates/codegen/update/README.md`，backlog 对应条目已转为完成并链接本归档。测试未操作用户下载目录和已安装程序；没有宣称解决每个平台分别保留不同版本或所有旧进程二进制存活问题。
