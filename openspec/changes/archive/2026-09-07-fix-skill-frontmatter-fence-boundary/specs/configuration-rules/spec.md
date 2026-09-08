## ADDED Requirements

### Requirement: Skill frontmatter uses complete delimiter lines
技能元数据解析和正文提取 SHALL 仅将去除行两侧空白后恰为 --- 的行作为分隔符，保持与流式读取一致。

#### Scenario: 前缀伪分隔符
- **WHEN** 开始或结束位置只有 ---suffix 而无完整分隔行
- **THEN** 不解析为完整 frontmatter，不丢弃正文。

#### Scenario: 换行与末尾
- **WHEN** 分隔行使用 LF、CRLF 或关闭分隔行位于 EOF
- **THEN** 元数据与正文边界正确，真正关闭行之后的正文保留。
