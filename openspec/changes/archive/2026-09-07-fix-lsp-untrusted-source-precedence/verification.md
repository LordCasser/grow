## 失败证据

`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p tools --lib untrusted_project_does_not_shadow_plugin_servers --quiet` 在旧实现 0 passed / 1 failed；同名项目配置被过滤后，期望的 plugin-file 配置实际为 None。

## 修复验证

- `cargo test --locked --offline -p tools --lib implementations::lsp::config::tests --quiet`：4 passed。
- `cargo test --locked --offline -p tools --lib implementations::lsp:: --quiet`：97 passed / 0 failed / 2 ignored，9.74s。忽略项为真实 TypeScript language server 与 Roslyn E2E，不计为通过。
- `cargo test --locked --offline -p shell --lib lsp --quiet`：2 passed，覆盖子 Agent LSP 继承/缺省；Shell 与 Workspace 的改动调用点同时编译通过。
- `cargo test --locked --offline -p shell --lib agent::folder_trust::tests --quiet -- --test-threads=2`：6 passed，0.02s。
- Rust 命令使用 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216`；四个修改 Rust 文件 rustfmt 检查和 git diff --check 通过。

新回归通过真实临时 JSON 验证 file/inline 插件同名 fallback、已信任项目覆盖、无冲突插件保留和项目独有项被排除。用户来源仍在项目合并前加载，include_project=false 不进入覆盖循环，因此保留原 user 值；没有为了测试改变全局 GROW_HOME 或写入真实用户配置。未发起模型请求，未安装语言服务器。

## 审计交付

`crates/codegen/shell/README.md` 已解释执行合并和 inspect 诊断边界。R3 已加入 `tmp-feature-removal-candidates.md`，等待用户确认，未删除。开关入口证据在 audit-notes.md。

inspect 仍显示声明的项目优先来源并标记 untrusted，尚未展示同名允许来源 fallback；该诊断视图问题已单独记入 backlog，不改变此次执行端契约。
