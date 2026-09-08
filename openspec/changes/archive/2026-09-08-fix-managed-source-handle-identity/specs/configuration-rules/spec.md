## ADDED Requirements

### Requirement: Managed source fields share a file handle
托管配置源状态的内容、身份与权限 SHALL 来自同一个已打开文件对象，不将路径预检元数据与另一个文件的内容合并。

#### Scenario: Path replaced after opening
- **WHEN** 源文件打开后其路径被另一个文件替换
- **THEN** 已打开源的读取仍使用该句柄的内容、权限和身份，后续路径读取使用新的对象；不混合两者。

#### Scenario: Opened object validation
- **WHEN** 已打开对象不是普通文件或长度超过上限
- **THEN** 在收集内容前拒绝，实际读取仍受既有字节预算限制。
