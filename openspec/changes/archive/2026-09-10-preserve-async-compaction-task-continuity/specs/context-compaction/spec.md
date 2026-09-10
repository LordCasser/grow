## ADDED Requirements

### Requirement: Compaction summaries describe only replaced history
压缩摘要的模型可见载体 SHALL 明确摘要只描述被替换的历史，摘要之后保留的消息是更新的任务和执行进度。旧摘要中的完成、等待用户或下一步建议 SHALL NOT 被载体描述为当前回合的权威状态；压缩本身 SHALL NOT 改变用户授权范围。

#### Scenario: Old completed work precedes a newer active task
- **WHEN** 被压缩片段中的任务已完成，但保留 tail 包含更新的用户任务和工具结果
- **THEN** 模型请求保留该 tail 的顺序，并包含明确以较新消息判断当前任务的摘要范围说明。

验证入口：`crates/codegen/shell/src/session/actor/compaction.rs` 的摘要载体组装；`crates/codegen/shell/src/session/actor/tests/compaction_pre_prune_tests.rs` 的异步场景组。

### Requirement: Async compaction hands off to an admitted next step
前台在闭合 Step 边界成功发布异步压缩，并获准开始同一普通 turn 的后继 Step 时，SHALL 在未替换 tail 之后、下一 Step 请求之前持久化恰好一个 synthetic `AutoContinue` 提示，说明压缩不等于任务完成并要求继续最新授权的未完成工作。该提示 SHALL NOT 成为真实用户输入、权限证据或新的 turn，也 SHALL NOT 单独触发额外采样。

#### Scenario: Continue after a tool result
- **WHEN** 工具 Step 已结束、异步压缩已提交且当前 owner 允许下一 Step
- **THEN** 下一请求包含旧片段摘要、保留的工具事实和其后的续接提示；重复构造请求不重复追加提示。

#### Scenario: No next step is admitted
- **WHEN** 压缩仅生成尚未提交、失败或取消，或前台已完成/失去 owner/被控制终止
- **THEN** 不追加该续接提示，也不为了压缩打开新 Step 或 turn。

#### Scenario: Ordinary final response after handoff
- **WHEN** 带有续接提示的请求收到完整、合法且无工具调用的最终回复
- **THEN** 回合仍按正常完成路径结束，不将该回复改判为流失败或无条件重新采样。

#### Scenario: Background result survives into a new turn
- **WHEN** 后台摘要在新 turn 的首个 Step 前提交
- **THEN** 新输入自身承担任务接纳，不追加旧回合的续接提示；摘要范围说明仍保留。

#### Scenario: Continuation persistence fails
- **WHEN** 续接提示未取得持久化确认
- **THEN** 不开始下一 Step 的 provider 请求，按现有错误路径收尾。

验证入口：`crates/codegen/shell/src/session/actor/turn/mod.rs` 的 between-step 准入；`async_compaction_between_steps_continues_the_latest_task_once`、异步取消/失败/跨回合场景；ChatState durable append 故障测试。
