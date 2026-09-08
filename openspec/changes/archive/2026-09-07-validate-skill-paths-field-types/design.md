## Decision
coerce_path_list/parse_skill_paths 返回 Result，列表逐项要求字符串，不使用 filter_map 丢弃元素。解析错误沿既有 YamlError 路径传递并跳过发现。保留 absent/null/空列表/空字符串与 ** 的既有 None 结果。

## Limits
本轮不更改 glob 语法验证或其他元数据字段转换。
