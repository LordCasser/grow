# 验证记录

`handle_new_file_creation` 的读取错误现在只接受明确的 NotFound 作为缺失；PermissionDenied、未知 kind 和 NotADirectory 都在写入前返回 `InvalidInput`。错误消息保留目标与原始读取错误。代码中的 FileWritten 发布位于写入成功之后，因此这些错误路径也不会发布写入通知。

- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p tools --lib implementations::grow_build::search_replace::tests:: -- --test-threads=4`：66 passed，0 failed。新增故障夹具从工具入口注入 PermissionDenied 和未知 kind，断言 write 次数为 0、既有文件不变；已有缺失文件、空文件、非空文件和路径组件非目录回归同批通过。
- 初次定向运行中，旧 `create_file_under_file_path` 断言仍期待写入阶段的“already exists”错误，现行行为更早报告读取阶段的 NotADirectory；按新契约更新断言后 66/66 通过。该用例结果仍为 InvalidInput，不会写入。
- `rustfmt --check --edition 2024`（修改文件）、change strict、全量 OpenSpec strict 16/16、`git diff --check`：均通过。
- 构建复用仓库既有 `target/`，未清理共享编译产物。当前文件系统可用空间约 23 GiB；本次没有运行全 workspace 测试。

未解决：读取确认后至 `write_file` 的并发替换、路径别名和跨文件系统原子提交，仍留在 backlog。该测试不能证明版本提交或对抗性写者安全。

使用 `openspec archive reject-search-replace-read-errors --yes` 合入 `tool-authorization` 主规范。归档后全量 strict 15/15、归档 371/371 通过，`git diff --check` 通过。
