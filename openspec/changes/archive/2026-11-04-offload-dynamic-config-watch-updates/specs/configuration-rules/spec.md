## MODIFIED Requirements

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

A config directory notification SHALL reuse the existing directory-identity reconciliation; an unchanged directory entity SHALL NOT cause an unconditional native unwatch/watch cycle. A replacement SHALL still attach the new entity only while an applicable source owner remains.

#### Scenario: Ordinary directory notification
- **WHEN** an event names an already watched .grow directory whose filesystem identity is unchanged
- **THEN** the watcher retains that native registration and subsequent config writes remain observable without restarting the watch stream.

On macOS, config observation SHALL use non-recursive directory metadata polling and config.toml-only content comparison at the existing debounce interval so adding/removing project watches does not recreate an FSEvents stream and discard intervening configuration changes. Other watcher purposes and platforms SHALL retain their current event sources. Polling SHALL NOT recursively traverse project contents.

#### Scenario: Configuration changes while another project interest is registered
- **WHEN** macOS config observation is registering or releasing another project's watch interest while an owned config changes
- **THEN** that config's final metadata/content change remains observable without resetting its event source; the current project/global reload scope is preserved.

#### Scenario: Same-timestamp atomic config replacement
- **WHEN** an owned config.toml is replaced with different bytes but the same modification time and length
- **THEN** macOS observation detects its changed content without hashing unrelated project files.
