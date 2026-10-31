## Evidence

`capture_pending_plan_decision` 保存 pending 后立即发 `QueryPlanDecisionStatus`。后端明确允许查询越过决定接纳而返回 unconfirmed；前端仅显示 toast，不会在接纳后自动闭合。`handle_plan_approval` 替换旧审批视图，却不清理旧 pending，新视图的输入与提示继续被它阻塞。

## Decision

只有 `accept_plan_decision` 的 Control durable ACK 成功后，才把本次 immutable receipt 放进既有 CurrentModeUpdate 的 `grow/planDecisionReceipt`。不从尚未确认的全局 snapshot 自动推导回执。Pager 核对 session/binding、submission、decision、action、feedback，复用状态查询已有关闭操作。无回执的 phase update 不作为决定确认。

新权威审批如果替换了提交，则释放旧 pending 的交互占用，不宣称旧决定成功。批注保留现有 artifact binding；旧确认到达后不能影响新的审批。相同提交重发不得解除其未确认决定。

## Verification

覆盖首次查询返回 unconfirmed 后收到精确回执、失败 ACK 不发送回执、错误 payload/session/binding 拒绝、旧回执晚于新审批、相同提交重发与连续两轮修订。
