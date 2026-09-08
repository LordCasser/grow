## ADDED Requirements

### Requirement: Untrusted project LSP cannot shadow permitted sources
LSP 执行配置合并 SHALL 在项目来源参与同名覆盖之前应用项目信任许可。未获准的项目 LSP 配置 SHALL 不遮蔽用户或已允许插件的同名配置。

#### Scenario: 未信任项目同名覆盖
- **WHEN** 未信任项目与用户或已允许插件定义同名 LSP server
- **THEN** 合并保留允许来源的配置，项目命令不进入可执行服务器集合。

#### Scenario: 信任项目覆盖
- **WHEN** 已信任项目与其他来源定义同名 LSP server
- **THEN** 项目配置继续覆盖低优先级来源，其他无冲突允许来源保留。

证据入口：`tools/src/implementations/lsp/config.rs` 的 sourced loader、`workspace/src/handle.rs` 和 `shell/src/agent/mvp_agent/agent_ops.rs`（均在 `crates/codegen/`）。inspect 只展示配置并标记禁用，不授权执行。
