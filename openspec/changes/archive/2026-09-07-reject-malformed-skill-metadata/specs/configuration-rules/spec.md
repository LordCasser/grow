## ADDED Requirements

### Requirement: Malformed skill metadata is not replaced with defaults
技能发现 SHALL 跳过已开始但未闭合的 frontmatter 以及 YAML 语法错误，不得用默认元数据替换损坏的声明。

#### Scenario: Incomplete or invalid header
- **WHEN** frontmatter 有 opening fence 但未闭合，或闭合内容存在 YAML 语法错误
- **THEN** 发现跳过该文件，不生成默认无限制技能。

#### Scenario: Plain Markdown skill
- **WHEN** 内容没有 frontmatter opening fence
- **THEN** 继续允许按目录名和正文描述回退发现。
