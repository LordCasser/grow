## ADDED Requirements

### Requirement: Skill frontmatter reading bounds source consumption
read_frontmatter_only SHALL 在底层限定读取至 MAX_FRONTMATTER_BYTES 加一个探测字节，超限行不加入 metadata；UTF-8 在探测边界被截断 SHALL 不作为真实编码错误报告。

#### Scenario: 超长单行
- **WHEN** frontmatter 候选包含大于上限的单行
- **THEN** 读取量不超过上限加一，不保留超限行。

#### Scenario: 边界截断多字节字符
- **WHEN** 合法 UTF-8 长行在读取上限处被截断
- **THEN** 按超限结束，而非返回编码错误。
