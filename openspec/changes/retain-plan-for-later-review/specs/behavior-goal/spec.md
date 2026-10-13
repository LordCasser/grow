## ADDED Requirements

### Requirement: Retained plans remain under the existing session owner
会话 SHALL 在既有 SessionActor/Behavior owner 下保留一个当前计划候选的不可变引用，并区分候选内容、Plan 执行阶段及 UI 展开状态。保留 SHALL NOT 批准、拒绝、放弃或执行计划，不新增 workflow owner；approval_pending 不得被无区别复用为所有保留文档的状态。

#### Scenario: Defer an unapproved plan
- **WHEN** 用户明确选择保留稍后审阅
- **THEN** 先确认持久候选引用，再释放当前交互占用，候选仍未批准且不会自动实施。

#### Scenario: Defer resolves a live or resumed waiter
- **WHEN** 保留决定已持久接纳，原交互为 live plan_control 或 idle 恢复审批
- **THEN** 清除 approval_pending，不产生 Execute/Revise handoff；live 路径仅结算一次延后结果并沿 EndTurn 结束，idle 路径只结束 waiter，不重新采样、重弹审批或恢复旧版本实施。

#### Scenario: Leave Plan with a retained candidate
- **WHEN** 用户按既有规则切换到其他 Behavior
- **THEN** 保留候选仍可只读发现，但不能自动抢占新 owner；再次实施需重新接纳。

#### Scenario: Select another Behavior then reload
- **WHEN** 有保留候选的会话切换到 Normal/Goal/Workflow 后保存并恢复
- **THEN** 候选按独立于 Plan runtime 的生命周期通过同一 Control snapshot 校验；不携带伪造的活跃 Plan phase，也不放宽其他跨 runtime 非法字段校验。

实施入口：`crates/codegen/shell/src/session/behavior.rs::PlanRuntime/select_behavior/finish_plan` 和 SessionActor；保留资格是新增契约，四阶段不是新增能力。

### Requirement: Later plan approval binds the reviewed submission
重新审阅后的批准 SHALL 绑定确切候选提交、artifact revision/hash 和稳定决定身份，在后端冻结快照校验成功且决定持久接纳后才进入既有执行路径。旧决定不得应用于后续提交或另一个计划周期。

#### Scenario: Candidate changes during review
- **WHEN** 用户审阅 v1 时当前候选变成 v2，随后提交 v1 的批准
- **THEN** 返回失效/冲突并要求审阅 v2，不批准 v2 或自动执行旧候选。

#### Scenario: Same text is submitted in a new plan cycle
- **WHEN** 退出 Plan 后重新提交相同正文且旧窗口回答迟到
- **THEN** 新旧提交身份可区分，旧批准不生效，新决定也不被历史 handoff 错误去重。

#### Scenario: Candidate replacement races an unresolved decision
- **WHEN** 旧候选的批准/放弃仍在接纳或 ACK 未确认，新提交尝试替换候选
- **THEN** 在同一 owner 边界先核对原决定，不覆盖其身份或未决证据；已执行/放弃的提交不能再次直接获批，重新执行需要新提交与重审。

实施入口：`behavior.rs::pending_plan_approval_snapshot/approve_submitted_plan_if_with_feedback`、`actor/tool/mod.rs::resume_plan_approval`。

### Requirement: Retained plan execution preserves existing arbitration and revision phases
批准保留计划 SHALL 复用当前 live ResampleStep 或 idle PlanHandoff，不建立第二条 turn admission；Goal、Rhai Workflow、用户 FIFO hold 和工具权限仍按各自 owner 生效。未批准计划修订 SHALL 回到 Drafting 后 submit，执行中修订 SHALL 经 Amending 和新版本重审。

#### Scenario: Approval without an active turn
- **WHEN** 合法候选在无活跃 turn 时获批
- **THEN** 通过原 durable handoff 和空闲仲裁最多启动一次实施，不把已有恢复能力重复实现。

#### Scenario: Existing work prevents immediate execution
- **WHEN** 当前前台任务、队首 hold 或准入故障阻止执行
- **THEN** 返回真实等待/拒绝原因，不绕过仲裁或激活 Goal。

#### Scenario: Revision changes implementation requirements
- **WHEN** 用户反馈改变计划要求
- **THEN** 形成新 artifact 后重新审批，不继承旧版本批准或暗改被批准正文。

#### Scenario: Approval comments and revision intent differ
- **WHEN** 用户提交批准说明或明确选择要求修改
- **THEN** 前者不改变冻结 artifact，后者进入 Drafting/Amending 修订路径；不依靠模型猜测评论语义来绕过重新审批。

实施入口：`actor/tool/preparation.rs`、`actor/notification_drain.rs`；既有执行与授权边界继续适用。
