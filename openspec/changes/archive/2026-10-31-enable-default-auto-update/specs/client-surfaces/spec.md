## ADDED Requirements

### Requirement: Automatic updates default to enabled and announce availability
在既有自动更新准入条件满足且版本缓存需要刷新时，自动更新入口 SHALL 将未配置的 `[cli].auto_update` 视为 true，检查并按现有版本策略执行更新。显式 false SHALL 关闭这些入口的检查与下载。TUI 检测到可更新目标后 SHALL 提示目标版本与重启操作；完整模式使用即时 toast 并保留欢迎页提示，minimal 使用既有 scrollback notice。

#### Scenario: No preference configured
- **WHEN** auto_update 未配置且通过既有准入条件，缓存需要刷新
- **THEN** 自动更新入口继续检查；发现需更新目标时按现有路径自动下载并交付更新结果。

#### Scenario: Explicit opt out
- **WHEN** auto_update 显式配置为 false
- **THEN** 自动更新入口不检查版本、不启动下载，也不生成更新可用提示。

#### Scenario: Update detected after entering a full TUI session
- **WHEN** 完整 TUI 的后台检查交付更新可用结果
- **THEN** 显示包含目标版本及重启提示的即时 toast，并保留 pending update 供欢迎页使用。

#### Scenario: Minimal update notice
- **WHEN** minimal TUI 的后台检查交付更新可用结果
- **THEN** 在当前会话 scrollback 显示目标版本及重启提示。

#### Scenario: No actionable update
- **WHEN** 缓存仍新鲜、没有可更新目标、检查失败或版本策略不允许更新
- **THEN** 不生成更新可用提示，沿用既有缓存和失败处理。
