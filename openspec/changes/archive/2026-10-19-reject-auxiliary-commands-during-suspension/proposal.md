## Why

用户会话在双 Ctrl+D 挂起后延迟响应，最终在 `run_loop.rs` 的 AISuggest 活动准入断言 panic，分发版本以 SIGABRT 终止。durable 记录只有 SuspendRequested 和 CommandsSettled，没有 checkpoint。当前候选仍保留同一断言：挂起关闭 session_activities，但邮箱仍为结算服务，迟到的有限辅助命令不能假定准入开放。

## What Changes

在有效挂起请求被接纳时关闭顶层辅助活动准入；所有经现有 activity tracker 派发的有限邮箱工作正常处理准入拒绝，不 panic、不产生新模型请求。best-effort 补全明确返回 None，其他请求沿既有关闭响应路径收尾。活动 permit 须在相关命令开始异步处理/修改前获取。保留原共享退出 deadline、未确认反馈和后续 owner 排空；恢复、观察与用量结算继续服务。

## Impact

修改 Shell 命令循环的生命周期门控及相关回归，加入 late-suggestion 的真实 mailbox/挂起验证。不改 Ctrl+C/Ctrl+D 语义，不实施 MCP 功能、策略或 SDK 变更，不重放或修改用户会话。图片参数误用的独立验证由 audit-image-read-parameter-failures 承载。
