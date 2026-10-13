## ADDED Requirements

### Requirement: Agent tree status reconciliation preserves freshness and binding

Pager SHALL 对根和嵌套子代理执行有界状态核对，包括尚无当前 prompt identity 的 child lifecycle。每个目标最多一个实际在途查询，整体并发及重试速率有界。查询结果 SHALL 核对 session、适用的 child/prompt identity、当前视图绑定及运行实例；查询失败不得刷新最后确认存活或工作进展的时间。页面 SHALL 明确展示过期或无法确认的观察，不能仅因原状态是 Running 就持续声称正在等待模型响应。

#### Scenario: Nested child loses its completion notification
- **WHEN** 嵌套子页面只有 spawn 展示，完成通知丢失或其当前 prompt identity 不可得
- **THEN** 失活检查、打开页面或重连按 child identity 发起有界核对，并展示已确认状态或状态未知。

#### Scenario: Repeated failed probes
- **WHEN** 多次查询失败且没有新的权威观察
- **THEN** 最后确认时间保持不变，页面显示持续的观察缺口，重试按有界退避执行，不无限堆积请求。

#### Scenario: Reply belongs to an earlier binding or incarnation
- **WHEN** 原查询回复到达时，页面已重新绑定或目标执行实例已换代
- **THEN** 原回复不改变新目标状态、不清除新目标查询标记，也不关闭新 turn。

#### Scenario: Observation reports progress without terminal evidence
- **WHEN** 查询确认执行仍活跃或正在结算，但没有该目标的持久终态
- **THEN** 页面显示对应阶段，不因耗时长自动杀死子任务或伪造完成。

证据与实施入口：`crates/codegen/pager/src/app/root/dispatch/turn.rs::poll_stalled_prompt_submissions`、`task_result.rs::PromptStatusResolved`、`crates/codegen/pager/src/app/root/effects/mod.rs`、`crates/codegen/pager/src/app/agent_view/session.rs`。

## MODIFIED Requirements

### Requirement: Cancelling turns reconcile against authoritative prompt state

Pager SHALL 在发送取消后用短时、单请求在途且有总期限的状态核对覆盖 `TurnCancelling`。只有匹配目标的持久 terminal 才确认任务结束。Unknown、不可达或超时 SHALL 结束界面上的阻塞等待并展示取消未确认，保留目标身份和取消意图；用户可编辑草稿及继续核对，但不得据此推进下一条执行或发布取消成功。时间本身 SHALL NOT 伪造成功终态。

#### Scenario: Cancel targets a vanished session
- **WHEN** Pager 已进入 `TurnCancelling`，但 shell 不再拥有运行 handle，因而不会发送取消终态
- **THEN** Pager 在短窗口后核对 runtime 与持久状态；没有 terminal 证据时显示取消未确认并允许编辑输入，不把 handle 缺失当作 session 消失或自动派发队列。

#### Scenario: Cancel is still being processed
- **WHEN** 权威查询仍返回 running 或 settling
- **THEN** Pager 保留取消意图，显示相应阶段，安排下一次有界核对，且同一目标同时最多一个实际查询在途。

#### Scenario: Cancel terminal notification was missed
- **WHEN** 权威查询返回同一 session、prompt 与实例对应的 terminal 状态
- **THEN** Pager 通过既有 single-finalizer 路径结束原 turn，不重复添加终态或重复推进队列。

#### Scenario: Cancel status query times out
- **WHEN** 状态查询在期限内没有完成
- **THEN** Pager 显示取消尚未确认并保持可交互，不宣称底层执行或写入已停止，也不因本次超时启动替代执行。

证据入口：`crates/codegen/pager/src/app/root/dispatch/turn.rs`、`task_result.rs` 及其 tests。本 change 收紧既有 unknown/error 退出 cancelling 的含义。
