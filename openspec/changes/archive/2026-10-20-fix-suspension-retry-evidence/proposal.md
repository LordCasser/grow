## Why

挂起审计确认两个 P1：正常后台完成回执与挂起结算共用终态身份但正文不同；首次挂起释放人工等待 owner 后失败，重试从空内存 registry 将原等待错误结算为 OutcomeUnknown。

## What Changes

- 挂起复用已接纳（包括已消费）的完成回执，缺少回执时才提交结算正文。
- 从 Timeline 的 Interaction 和当前挂起 operation 边界恢复等待证据，跨失败重试保留原调用和 attempt。
- 增加挂起失败、同 operation 重试与恢复的跨阶段回归，并覆盖已回答等待不能复活的反例。

## Capabilities

### Modified Capabilities

- `session-timeline`：挂起重试复用终态回执，并保全人工等待。

## Impact

局限于 SessionActor 挂起路径和回归测试；不修改通知一致性规则，不新增持久事件或放宽无 checkpoint 的冷恢复。
