## ADDED Requirements

### Requirement: LSP inspection preserves permitted fallback visibility
LSP 配置诊断 SHALL 使用报告的项目信任结果展示允许来源合并结果，并在项目未信任时另列被禁用的项目定义。同名允许项 SHALL 不被禁用项目项遮蔽。

#### Scenario: 未信任项目与插件同名
- **WHEN** 项目定义与允许插件来源同名且项目未信任
- **THEN** JSON 和终端条目包含允许来源及明确标记 untrusted 的项目来源，允许项在同名禁用项之前。

#### Scenario: 项目已信任
- **WHEN** 项目已获信任且存在同名覆盖
- **THEN** 诊断显示项目覆盖结果，不额外重复列出低优先级同名配置。

证据入口：`crates/codegen/shell/src/inspect/mod.rs` — `list_lsp_servers`、`LspServerEntry`。此视图不宣称列出正在运行的服务器。
