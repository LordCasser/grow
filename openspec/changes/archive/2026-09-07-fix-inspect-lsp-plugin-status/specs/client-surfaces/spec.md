## MODIFIED Requirements

### Requirement: LSP inspection preserves permitted fallback visibility
LSP 配置诊断 SHALL 使用报告的项目信任结果展示允许来源合并结果，并在项目未信任时另列被禁用的项目定义。同名允许项 SHALL 不被禁用项目项遮蔽。

#### Scenario: 未信任项目与插件同名
- **WHEN** 项目定义与允许插件来源同名且项目未信任
- **THEN** JSON 和终端条目包含允许来源及明确标记 untrusted 的项目来源，允许项在同名禁用项之前。

#### Scenario: 项目已信任
- **WHEN** 项目已获信任且存在同名覆盖
- **THEN** 诊断显示项目覆盖结果，不额外重复列出已启用的低优先级同名配置；禁用插件定义以禁用标记单列。

证据入口：`crates/codegen/shell/src/inspect/mod.rs` — `list_lsp_servers`、`LspServerEntry`。此视图不宣称列出正在运行的服务器。

## ADDED Requirements

### Requirement: LSP inspection distinguishes plugin activation
LSP 诊断 SHALL 从插件注册表取得启用与信任状态；只有已启用且已信任插件参与允许来源合并，其他插件的声明作为独立诊断项展示。禁用和未信任 SHALL 分别以 disabled、untrusted 表达，终端与 JSON 使用同一状态。

#### Scenario: 禁用插件与活动插件同名
- **WHEN** 已信任但禁用插件与已启用插件定义同名服务器
- **THEN** 允许来源仍来自活动插件，禁用声明另列并标记 disabled，不遮蔽允许来源。

#### Scenario: 插件不受信任
- **WHEN** 插件已启用但未受信任
- **THEN** 其声明仅以 untrusted 诊断项展示，不进入允许来源合并。

证据入口：`crates/codegen/shell/src/inspect/mod.rs`、`crates/codegen/tools/src/implementations/lsp/config.rs`。展示不启动 LSP 进程。
