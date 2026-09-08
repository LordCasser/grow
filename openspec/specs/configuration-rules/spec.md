# configuration-rules Specification

## Purpose
定义个人配置、项目配置和 Agent 规则的发现边界。覆盖个人配置的固定来源与解析错误、项目内配置查找，以及规则文件的去重和排序；不从发现行为推导额外的信任授权。

## Requirements

### Requirement: User config authority
个人配置 SHALL 从 GROW_HOME 下的 config.toml 加载，缺失文件按空配置处理，语法错误明确返回错误。

#### Scenario: 配置语法错误
- **WHEN** 个人 config.toml 无法解析
- **THEN** 报告 TOML 行列信息，不静默改用 cwd 的同名文件。

证据：`crates/codegen/config/src/loader.rs` — `load_from_disk`。

### Requirement: Project scoped config discovery
项目配置 SHALL 通过工作区配置发现逻辑查找仓库内 .grow/config.toml。

#### Scenario: 打开项目目录
- **WHEN** find_project_configs 运行于工作目录
- **THEN** 返回该仓库发现范围内的配置路径。具体 trust 与覆盖次序由加载方决定。

证据：`crates/codegen/workspace/src/project_config.rs` — `find_project_configs`。

### Requirement: Agent rule discovery
Agent 规则 SHALL 发现 AGENTS.md 与规则目录中的 Markdown 文件，并去重规范路径。

#### Scenario: 加载目录规则
- **WHEN** 多个发现根指向同一规范路径
- **THEN** 规则发现去重，目录内 Markdown 规则按文件名排序。

证据：`crates/codegen/agent/src/prompt/agents_md.rs` — `find_rules_files`。
