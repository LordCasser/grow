## Why
技能流式读取按完整 --- 行判断分隔符，parse_skill_frontmatter 与 extract_skill_body 却使用前缀匹配；---suffix 等正文可被误当 frontmatter 边界，导致内容截断及解析结果不一致。

## What Changes
元数据解析和正文提取共享完整分隔行扫描，维持现有前后空白容忍。覆盖 LF/CRLF 和 EOF 关闭分隔符，不改变 YAML 字段恢复策略。

## Capabilities
### Modified Capabilities
- configuration-rules: 技能 frontmatter 分隔行一致性。

## Impact
skills/skill.rs 与 discovery.rs 的共享边界查找，不增加新解析器依赖。
