## ADDED Requirements

### Requirement: Interactive session export resolves paths against session cwd
交互式 /export 的相对文件路径 SHALL 在展开 ~ 后按活动会话 cwd 解析，与会话路径补全的目录基准一致。

#### Scenario: Session cwd differs from process cwd
- **WHEN** 用户提供相对导出路径且会话目录不同于进程启动目录
- **THEN** 文件写入会话目录下对应位置。

#### Scenario: Absolute export path
- **WHEN** 展开后的导出目标是绝对路径
- **THEN** 保留该绝对目标，不附加会话目录。
