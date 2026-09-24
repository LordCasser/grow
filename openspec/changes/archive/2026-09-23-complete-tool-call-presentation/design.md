## Boundary

`ToolInput`/`ToolOutput` 是封闭枚举。开始事件由 `send_tool_call_start` 生成，保留 `raw_input` 和已注册工具的 `grow/tool` 元数据；终态由 `acp_tool_update` 生成，Pager 按原 tool-call ID 原位更新。当前两个 wildcard 把遗漏隐藏在编译之外。

LSP 标题使用 `LspOperation` 的稳定英文表示及可选文件路径。动态工具没有静态输入类型，使用实际 `wire_name` 而不从不可信参数中猜标题。`ContextRecall` 与动态结果均把模型可见正文交给 Pager 既有通用输出展示，并保留 `raw_output` 与 Completed 终态。本 change 不重新解释动态结果的应用层错误语义。

## Verification

通过真实 `send_tool_call_start` 覆盖 LSP 与动态输入的非泛称标题和原始输入；直接验证 `acp_tool_update` 对 ContextRecall/动态结果的终态、ID 与 raw output。运行 Shell 定向测试、格式检查和 OpenSpec strict。
