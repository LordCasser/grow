## Evidence
Pager build_accepted_response 发送 selected_labels；format_id_keyed_accepted_tool_result 用 options.iter().find(label) 映射 ID。不同描述或 ID 不解决 label 歧义。

## Decision
在现有 question text 校验循环内新增每题局部 HashSet，完全相同的 label 返回 invalid_arguments，发生在 channel 请求之前。保留大小写与空白差异，不隐式规范化。

## Validation
同题重复 label 无论 ID 格式与否都被拒绝且无 coordinator 请求；跨题同名合法，复用现有正常 round-trip。
