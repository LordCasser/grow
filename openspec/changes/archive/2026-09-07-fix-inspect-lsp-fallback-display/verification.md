## 验证记录

- 旧视图回归：`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib lsp_inspection_shows_fallback_and_untrusted_project_definition --quiet`，0 passed / 1 failed；同名项实际 1 条，预期允许来源与禁用项目定义共 2 条。
- 修复后：`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib inspect::tests --quiet`，9 passed / 0 failed，0.05s。
- 新测试使用临时项目/插件 JSON，验证未信任时同名允许项在前、项目项 untrusted、项目独有项可见，已信任时项目覆盖且无多余低优先级条目。JSON 序列化验证保留相同行数和禁用标记。
- 终端 `print_columns` 直接遍历同一 `lsp_servers` 列表，并展示 command、source 与 untrusted 标记；此次以代码路径核对该呈现，没有声称完成真实终端视觉回归。
- 单文件 rustfmt --check、git diff --check 和 OpenSpec 全量严格检查通过。Shell 既有大体积 unwind table linker 警告不影响运行。

无需启动 LSP 或模型，不写用户配置；复用报告已解析的 project_trusted，而不是在子列表中二次取不同快照。插件启用状态不在本次显示契约中，列表是来源诊断而非运行进程清单。

解释已更新 `crates/codegen/shell/README.md`，backlog 的同名来源展示条目关闭。
