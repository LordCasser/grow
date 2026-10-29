## 现状

`TaskOutputToolInput` 已定义正超时阻塞、缺失/零仅快照，多 ID 是 wait-all。`TaskOutputTool` 通过 coordinator waiter 等待，Shell 对可中断等待监听 steer；任务继续由原 owner 管理。Pager 已将正超时输出等待呈现为 `Waiting(TaskOutput)`，不需要新增任务或 UI 状态。

`primary.md` 没有点明独立工作结束后的调用路径。`build_task_description` 已持有实际结果工具名。`format_subagent_started_background` 当前硬编码 `task_ids`/`timeout_ms`；除了 task 启动，它还用于树恢复，因此示例必须覆盖两处调用。

## 设计

1. primary 指引使用与工具命名无关的语义，明确原 ID、正超时、仅等待下一步所需依赖，以及等待返回不等于完成/验收。
2. spawn 说明通过已有 `background_retrieval_tool` 引用实际入口。
3. 共享后台回执接收既有参数映射和当前 max-wait；使用 serde_json 生成示例，数值取当前上限。映射缺失的参数沿现有 remap 语义使用 canonical 名称。上限为零时不生成伪装成阻塞等待的零超时示例。
4. task 与恢复调用方从现有 TemplateRenderer 读取参数映射，不新增 ToolContext 字段或持久事实。前台调用被转入后台时保留既有状态说明，补充原 ID 等待指引。
5. 用户指南说明调用与边界，行为链接 local-coordination 契约。

## 范围和验证

按 audit.md 的反例审查，测试真实参数重命名后的 JSON 可解析性与边界值；运行相关既有工具说明、输出等待和 prompt 验证。主运行时与 View 没有修改；不为纯提示词新增镜像字符串测试，也不把静态验证描述为模型实际遵循保证。长于单次上限的无限等待保持已有 PlanWait 的独立语义，本次不扩展。
