## ADDED Requirements

### Requirement: Doctor SSH fixes honor shell config directories
SSH自动修复 SHALL 使用请求中可见的ZDOTDIR选择zsh配置，使用非空XDG_CONFIG_HOME选择fish配置；预览、写入和后置检查 SHALL 使用同一目标。未设置覆盖时保持默认。

#### Scenario: Custom shell config directory
- **WHEN** 当前shell为zsh且设置安全绝对ZDOTDIR，或为fish且设置安全绝对非空XDG_CONFIG_HOME
- **THEN** 分别使用该目录下.zshrc或fish/config.fish，不写入HOME默认路径。

#### Scenario: Unsafe relevant override
- **WHEN** 相关覆盖不满足现有安全绝对目录约束
- **THEN** 规划明确拒绝，不悄悄改写默认文件。

#### Scenario: Default or irrelevant override
- **WHEN** 覆盖未设置、fish的XDG为空，或变量不适用于当前shell
- **THEN** 按当前shell默认路径或其相关覆盖规划，无关变量不改变目标。
