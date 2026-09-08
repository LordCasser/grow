## ADDED Requirements

### Requirement: Tmux reload guidance distinguishes reattachment
诊断与修复提示 SHALL 要求显式重新加载配置以应用到运行中的tmux服务器，不将客户端detach/reattach描述为配置重载的替代方式。Grow SHALL 保持不自动执行重载。

#### Scenario: Persistent tmux fix completed
- **WHEN** 显示tmux修复预览、结果或诊断建议
- **THEN** 指向source-file重载，必要的客户端重连作为重载后的独立步骤。

#### Scenario: Path cannot be safely shown as a command
- **WHEN** 文件路径不能安全显示为shell命令
- **THEN** 提示手动加载变更文件，不宣称重新连接即可激活配置。
