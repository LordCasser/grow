## ADDED Requirements

### Requirement: Standalone doctor checks the selected shell target
独立doctor报告的SSH已配置判定 SHALL 复用修复规划使用的shell配置目录解析。

#### Scenario: Custom target configured
- **WHEN** 自定义zsh/fish配置目标中已包含有效托管alias
- **THEN** 报告按该目标判定，不因HOME默认文件缺失而重复报错。

#### Scenario: Only the inactive default target is configured
- **WHEN** 显式配置目录指向的文件未配置，但HOME默认文件有alias
- **THEN** 不把默认文件状态冒充实际目标的已配置状态。
