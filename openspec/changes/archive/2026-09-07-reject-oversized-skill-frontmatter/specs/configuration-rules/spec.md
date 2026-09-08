## ADDED Requirements

### Requirement: Oversized skill frontmatter is not downgraded to plain content
技能发现 SHALL 拒绝已识别 opening fence 后超过 frontmatter 读取预算的文件，不得将截断的元数据当成无 frontmatter 普通技能。

#### Scenario: Restrictions occur after oversized metadata
- **WHEN** 已开始的 frontmatter 超限，限制字段位于未读部分
- **THEN** 读取返回错误，发现跳过技能，不以默认限制值加载。

#### Scenario: Long plain body has no frontmatter
- **WHEN** 长正文没有 opening fence
- **THEN** 保持普通技能发现及有界描述预览。
