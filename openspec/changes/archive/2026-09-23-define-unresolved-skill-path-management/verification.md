## 验证结果

- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test --locked --offline -p shell --lib extensions::skills::tests -- --test-threads=1`：24/24 通过。新增用例覆盖缺失路径中间组件后的 `..` 不被词法折叠、符号链接删除后旧别名不误删保存目标、无效 cwd 返回 `invalid_params`。
- 首次编译暴露 `read_dir` 返回值未消费的 warning；改为读取首项以确证可读后，重新编译并运行 `extensions::skills::tests::skill_request_rejects_unavailable_cwd -- --exact --test-threads=1`：1/1 通过，仅余既有 macOS linker unwind 表 warning。
- 请求处理代码在 add/remove 两条分支均先 `validate_skill_request_cwd`，通过后才调用 `cli_config::update_config`；无匹配移除由 `remove_skill_path` 返回 false，响应不再声称已删除。`rustfmt --edition 2024`、`git diff --check` 与 `openspec validate --all --strict --no-interactive` 均通过。
- 归档更新 `configuration-rules` 两项要求后，全量严格校验 19/19、归档校验 421/421 通过；`git diff --check` 通过。

## 身份边界

缺失路径保留锚定的表达式；后续请求按当时文件系统重新解析。符号链接删除后没有可靠的历史目标证据，旧别名不删除已保存的规范目标；使用添加响应中的规范路径可明确移除。该语义不承诺文件系统路径在两次请求之间不会被外部替换。
