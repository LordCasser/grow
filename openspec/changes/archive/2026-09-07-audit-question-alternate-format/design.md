## Scope
对全部仓库检索 use_id_keyed_format/useIdKeyedFormat/format_id_keyed_accepted_tool_result，核对 schema/serde 属性、工具调用分支、formatter 与 Pager response 构造。

## Decision
ID formatter 及内部开关作为待删除候选，不据此删除 Question/QuestionOption.id（它们可经 serde 接收，需另查用途），不改变正常问答格式。
