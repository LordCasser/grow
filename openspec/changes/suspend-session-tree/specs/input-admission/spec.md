## ADDED Requirements

### Requirement: Suspension preserves admitted input ownership

挂起 SHALL 保留已接纳输入的稳定 identity、payload、FIFO/steer/followup 路由与消费状态，不因关闭运行实例追加 SessionClosing dismissal。恢复 SHALL 先续接原前台工作，再按现有仲裁处理未消费输入；不会重跑已完成的 input admission Hook 或重复生成用户消息。绑定客户端的临时编辑 hold 随断连按原规则释放，未提交草稿 SHALL NOT 被当作已接纳输入。

#### Scenario: Queued input survives suspension
- **WHEN** 活跃 turn 挂起时有两个已接纳的排队输入
- **THEN** 两者保留原顺序、内容和身份，恢复后不重复 Submitted/Consumed，也不越过原前台工作直接启动。

#### Scenario: Input was consumed before exit
- **WHEN** 原 prompt 已进入模型上下文后发生挂起
- **THEN** 恢复沿原续跑位置继续，不把该 prompt 当作新的用户提交。

#### Scenario: A targeted input waits on the suspended turn
- **WHEN** steer/followup 已接纳并绑定被挂起的 turn，但尚未消费
- **THEN** 保留原绑定并按该 turn 的恢复状态处理，不因进程退出套用目标已死亡的重路由。

#### Scenario: Queue edit holder disconnects
- **WHEN** 挂起退出时客户端仍持有临时编辑 hold
- **THEN** 释放临时 hold 并保留已经接纳的原输入；尚未提交的编辑不替换 durable payload，树仍保持挂起，不能因 hold 释放提前恢复执行。

目标入口：`crates/codegen/shell/src/session/actor/input_admission.rs`、`run_loop.rs::reject_queued_inputs_for_shutdown`、`crates/codegen/chat-state/src/timeline.rs`。
