## ADDED Requirements

### Requirement: Assistant copy file uses session cwd
交互式 /copy 的显式相对文件路径 SHALL 在展开 ~ 后按活动会话 cwd 解析，绝对路径保持原目标。

#### Scenario: Relative copy destination
- **WHEN** 会话目录不同于进程目录且提供相对复制文件路径
- **THEN** 文件写入会话目录，内容为选定 assistant 消息。

#### Scenario: Existing copy privacy
- **WHEN** 指定相对或绝对目标文件
- **THEN** 继续使用复制文件的私有权限策略，不因目录修复改用导出的权限继承规则。
