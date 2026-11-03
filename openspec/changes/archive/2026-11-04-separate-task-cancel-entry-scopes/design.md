# Design

Introduce one internal cancellation helper with an explicit entry scope. It snapshots the available resource handles under the shared resource lock, then performs cancellation outside the lock. The terminal-only scope requires and calls `Terminal` only. The mixed scope accepts either a `Terminal` or a `SubagentBackendResource`; when both exist, a terminal result other than `NotFound` is final, while `NotFound` allows the existing backend cancellation path. With only a subagent backend, mixed cancellation calls that backend directly.

Keep `SubagentBackend::cancel` as the authorization and idempotency owner. Map its actual cancellation, already-finished, not-found, unauthorized and observation-error results without manufacturing success. Keep tool output conversion and completion receipts on the existing callers; change no tool names, kind, authorization taxonomy or terminal lifecycle code.

The shell-level verification drives `KillTaskTool` through a real `SessionActor` turn and reads its persisted Timeline `ToolResult`. A session-bound `ChannelBackend` event owner records the child transition before acknowledging cancellation, then returns the terminal status for a repeated request and authorization failure for a foreign ID. This verifies the actor's durable receipt boundary alongside the tools-level entry-scope tests.

NotFound 文案不声称全会话无 child：terminal-only 从未查询 child 清单，mixed 的单ID拒绝也不证明全清单为空。仅报告本入口未找到该目标及已知 terminal IDs；在相应单测保留该边界。
