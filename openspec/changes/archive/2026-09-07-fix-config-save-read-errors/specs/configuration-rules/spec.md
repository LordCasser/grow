## ADDED Requirements

### Requirement: Config writes stop on existing-file read errors
配置保存 SHALL 仅在目标不存在时使用空配置；其他读取失败 SHALL 返回错误且不得替换原文件。读改写操作 SHALL 在初始加载失败时停止，不执行修改闭包。

#### Scenario: Existing config is invalid UTF-8
- **WHEN** 设置保存无法将已有文件读取为 UTF-8
- **THEN** 返回错误，原文件字节保持不变。

#### Scenario: Config does not exist
- **WHEN** 设置保存目标文件不存在
- **THEN** 正常创建有效 TOML 配置。
