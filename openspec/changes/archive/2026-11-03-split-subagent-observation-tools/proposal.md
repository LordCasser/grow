# 分离子任务枚举、详情和等待

## Why

混合get_task_output同时承担command与subagent的发现、状态、结果、等待，且空ID数组有特殊语义。用户明确要求拆分职责并剥离command部分，不保留旧混合入口兼容。

## What Changes

新增list_subagent(scope=active|all)、get_subagent_status(subagent_ids)、wait_subagent(subagent_ids,timeout_ms)。list仅返回身份、描述、状态和时间等基本信息，不含工具调用、最终正文。详情返回可确认的工具调用次数、轮次、用量、状态、持续时间，不读取答案正文。等待与最终结果交付由独立入口负责（已完成任务立即返回），保留真实期限、取消和完成通知交接。

command独立使用get_terminal_command_output；三个subagent工具不访问Terminal。同步修改preset、ToolKind/requirements、提示词与通知、等待识别/Pager展示、测试和开发说明，删除旧混合工具注册与空数组发现协议。
