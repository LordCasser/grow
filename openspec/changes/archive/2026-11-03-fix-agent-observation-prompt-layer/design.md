## Evidence and decision

builder.rs 使用 prompt_context.render() 生成稳定 head，另以 finalized ToolBridge 生成 render_role。primary.md 的 tools.by_kind 条件在前一入口永远为假；render_with_renderer 测试并非该生产组合。复用 extensions/session.md 的既有动态层承载工具名，不让稳定 head 读取 mutable tool state。

观察提示适用于具有对应能力的主/子 Agent：缺少某工具时省略该工具的指令；所有工具都缺失时不提示不存在的入口。Full composition 仍保留 session extensions。协调职责和 child 的能力边界留在各自 audience 中。

同时修正 Task 描述中“keeps ... tool state”的失真表述。已归档的 resume/权限契约要求重建当前 runtime，不能复制 prior permits；这里只对齐说明，不改变恢复实现。

## Verification

先通过生产 PromptContext::render / render_role_with_renderer 组合证明旧行为遗漏提示，再覆盖 default/renamed/absent 工具、两个 audience 与 Full/Extend。既有稳定 head、角色替换和 builder 测试继续验证跨层不变量。这里只验证 prompt 渲染与装配，不声称模型行为评测通过。
