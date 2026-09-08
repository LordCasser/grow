## ADDED Requirements

### Requirement: Doctor surfaces share configured SSH status
CLI与TUI doctor报告 SHALL 使用同一当前shell配置目标判定SSH托管alias状态。

#### Scenario: Local managed alias exists
- **WHEN** 本地当前shell目标含有效托管SSH alias
- **THEN** 两种报告均移除重复的SSH配置建议，不把持久化状态当作实时路由探测。

#### Scenario: Target not configured or session remote
- **WHEN** 当前目标未配置，或报告来自SSH/VS Code Remote
- **THEN** 不用其他目标或远端shell配置隐藏本地SSH配置建议。
