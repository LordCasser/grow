## Decision
read_frontmatter_only 到 EOF 时 found_opening 仍为真则 InvalidData。parse_skill_files 对 YamlError 记录警告并跳过，移除该错误分支中推测默认 metadata 的逻辑。NoFrontmatter 分支不变。

## Limits
不将字段级宽松转换（非标量 description、布尔解析等）一并重构。本轮针对不完整边界和 YAML 语法错误；独立 extract_skill_body 仍保留未闭合内容。

## Additional finding
回归发现 parse_skill_frontmatter 在 YAML 错误后还有全字段重新引号化和标量恢复，导致外层 YamlError 不触发；本轮移除生产修复尝试，直接传播 YAML 错误。四个旧恢复场景保留为拒绝加载回归，闲置 helper 作为 R9 留待用户确认删除。
