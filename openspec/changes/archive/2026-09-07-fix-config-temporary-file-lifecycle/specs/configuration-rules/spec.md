## ADDED Requirements

### Requirement: Config temporary files preserve privacy and clean up
配置原子写入 SHALL 使用同目录独占临时文件，失败时清理未提交文件。Unix 新配置 SHALL 默认仅所有者读写；已有目标权限 SHALL 在写内容前应用，权限读取或设置失败 SHALL 返回错误。

#### Scenario: New Unix config
- **WHEN** 创建新的配置文件
- **THEN** 文件权限为 0600 或更严格。

#### Scenario: Existing private config update
- **WHEN** 覆盖已有 0600 配置
- **THEN** 临时文件及最终文件不扩大权限。

#### Scenario: Replacement fails
- **WHEN** 临时文件不能替换目标
- **THEN** 返回错误，目标不变且临时文件被清理。
