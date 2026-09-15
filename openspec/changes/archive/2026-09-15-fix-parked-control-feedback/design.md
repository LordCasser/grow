## Context

`AgentSession::live_status` 汇合 Shell 权威控制投影及临时反馈；`AgentView` 已传给 `render_turn_status`。渲染器只在 Idle 状态优先显示该字符串；parked 前台仍为 TurnRunning，却提前进入后台任务提示分支，永远到不了后面的控制 suffix。

## Goals / Non-Goals

让等待期间已接纳的模型、effort、Agent 或 Behavior 切换继续可见。保持模型在安全 Step 边界应用；不将 parked 视为真正 Idle，不终止或重启子 Agent，不新增本地伪造的成功或重复通知。

## Decisions

- 把现有 Idle 实时反馈渲染分支同时用于 parked。反馈优先于静态后台任务提示；结束后走原提示分支。复用宽度裁剪、样式和动效，不增加状态字段或新的通知协议。
- 使用真实 buffer 的渲染断言防止仅验证内存中有 pending 而遗漏屏幕；配合模型派发、Shell 控制通知与 Step 边界测试核对整条路径。

## Risks / Trade-offs

- 待处理反馈显示期间状态行暂不提供任务提示点击区 → 与现有 Idle 控制反馈行为一致，反馈结束即恢复；任务面板入口仍在。
- 截图无法证明每次请求的瞬时通知都到达客户端 → 日志仅作为接收证据，测试验证派发、通知消费与最终渲染，不声称已远程修改运行中会话。
