## ADDED Requirements

### Requirement: Input diagnostics dumps are independent private files
每次 input recorder 导出 SHALL 创建独立文件，即使时间戳相同也不覆盖已有快照；Unix 从创建起 SHALL 使用 0600 权限。

#### Scenario: Repeated dump within one second
- **WHEN** 同一秒导出两份不同诊断快照
- **THEN** 返回两个不同路径，每份保持对应完整正文。

#### Scenario: Partial write or directory failure
- **WHEN** 写入、同步或创建目录失败
- **THEN** 返回错误，提交前新临时文件清理，已有快照保持不变。
