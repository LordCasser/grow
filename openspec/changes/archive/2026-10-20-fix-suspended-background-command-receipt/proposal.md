## Why

Grow 会话 `01a1574e-6c7f-7921-a8be-fbfeafcc0a2a` 的退出挂起已持久记录 `SuspendRequested` 和 `CommandsSettled`，但未写入 checkpoint，Pager 显示退出未确认。代码核对发现，后台命令结算生成的 `TaskCompleted` 通知使用 `Opaque` source version；Timeline 仅接受此类终态通知的正数 `Ordinal` version，因此 receipt 被拒绝，挂起无法越过命令结算。

## What Changes

- 后台命令挂起时，完成通知使用与常规任务完成相同的终态版本身份。
- 使用现有 Timeline 通知和命令结算回归，核对终态身份与重试边界。
- 在开发者诊断说明中记录该边界。

## Capabilities

### Modified Capabilities

- `session-timeline`：后台命令结算后的终态通知必须满足 Timeline 的 source/version 契约，挂起方能继续确认 checkpoint。

## Impact

仅涉及 Grow 会话挂起的后台命令完成通知。既有会话数据不改写；未确认的挂起继续使用原 operation 身份重试。
