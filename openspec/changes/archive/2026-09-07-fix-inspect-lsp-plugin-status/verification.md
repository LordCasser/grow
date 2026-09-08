## 原始失败

`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib lsp_inspection_distinguishes_disabled_and_untrusted_plugins --quiet`：0 passed / 1 failed；同名声明仅剩 1 项，预期活动来源、禁用声明及未信任声明共 3 项。

## 修复后验证

- `cargo test --locked --offline -p shell --lib inspect::tests --quiet`：10 passed / 0 failed，0.03s。
- `cargo test --locked --offline -p tools --lib implementations::lsp:: --quiet`：98 passed / 0 failed / 2 ignored，9.80s。忽略项依赖真实 TypeScript 和 Roslyn 服务，不计为通过。
- Rust 命令使用 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216`。两个 Rust 文件格式检查、git diff --check 与 OpenSpec 严格校验通过。

新 inspector 回归由真实 PluginRegistry 的 enabled/disabled 列表驱动，交换文件式/inline 配置，验证活动来源保留、禁用和未信任各自的 JSON 标记，并保留此前项目同名覆盖测试。新增插件解析回归证明文件先于 inline、前一文件优先，以及来源名称和路径保持正确。

终端输出沿用 disabled_tag 和已有 untrusted 标记，读取同一条目；未声称做真实终端视觉测试。测试未修改用户插件启用状态，也未安装真实服务或调用模型。

开发者说明在 `crates/codegen/shell/README.md`；backlog 对应启用状态条目关闭。LSP 列表是配置诊断，不是运行进程清单。
