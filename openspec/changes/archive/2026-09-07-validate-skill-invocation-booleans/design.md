## Decision
复用原 parse_boolean_frontmatter 并返回 Result，参数携带字段名和缺省值。类型错误沿既有解析失败路径跳过技能。保留字符串 true/false；null 是显式错误而非缺省。

## Verification
错误值矩阵覆盖两个字段，合法布尔/字符串与缺省测试保留。旧 yes/数字转 false 场景更新为拒绝，避免以修改测试隐藏输入。
