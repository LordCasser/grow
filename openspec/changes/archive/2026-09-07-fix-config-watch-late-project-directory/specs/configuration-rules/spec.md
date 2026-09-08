## ADDED Requirements

### Requirement: Project config watches survive directory creation
已注册项目的配置监听 SHALL 在 .grow 晚于启动创建或被删除后重建时继续观察 config.toml；补挂 SHALL 不依赖配置内容变化或 MCP reload 去重结果，监听范围保持非递归。

#### Scenario: 首次创建项目配置目录
- **WHEN** 注册 cwd 时没有 .grow，随后创建目录和配置文件
- **THEN** 首次配置和后续写入均能触发配置事件。

#### Scenario: 删除并重建目录
- **WHEN** 已监听 .grow 被删除并重建
- **THEN** 新目录配置更新继续可观察。

#### Scenario: 项目取消注册
- **WHEN** cwd 已 unwatch 后创建或重建 .grow
- **THEN** 不为该 cwd 重新挂载监听。
