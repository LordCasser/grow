## ADDED Requirements

### Requirement: Current Behavior restores independently of display history

成功挂起 SHALL 保留最后已确认的 Behavior 及其有效 Plan 状态。冷恢复或 resident reconnect SHALL 从当前控制 owner 向客户端提供已确认的 Behavior、Plan phase 和 completed-plan 状态，不依赖任何历史页包含模式切换记录。旧截点或旧绑定的展示数据 SHALL NOT 覆盖当前控制；恢复本身 SHALL NOT 伪造一次选择、审批、Goal restart 或 Workflow 执行。

#### Scenario: A selected Behavior lies before the recent page
- **WHEN** 用户选择非 Normal Behavior，产生超过最近页范围的历史，Ctrl+D 成功后在新进程 resume
- **THEN** 当前控制、后续模型行为和用户界面恢复原 Behavior 及有效 Plan phase，即使最近页尚未返回或没有任何模式记录。

#### Scenario: A newer live selection precedes load completion
- **WHEN** 同一 actor 的较新 Behavior 更新先于旧 load 快照或旧历史页到达
- **THEN** 客户端保持较新状态，不因恢复展示回退模式或结算另一项本地控制意图。

#### Scenario: A Plan phase transition waits for persistence
- **WHEN** Submit、Amend 或审批决定的候选阶段正在等待持久确认，或写入失败
- **THEN** 当前控制查询和实时通知仍只反映上一个已确认阶段与版本；成功确认后两者同步推进，失败不暴露候选阶段。

#### Scenario: Reconnect observes a resident owner
- **WHEN** 用户重新连接仍存活的 actor
- **THEN** 当前 Behavior 来自该 actor，而非用磁盘或已加载正文重新推导；原 Turn 与控制身份不改变。

证据入口：`crates/codegen/shell/src/session/actor/session_mode.rs`、`actor/spawn.rs`、`agent/mvp_agent/acp_agent.rs`；`crates/codegen/pager/src/app/acp_handler/session_notification.rs`。

### Requirement: Planning tools serve the approval-bound plan

Plan 处于 Drafting、AwaitingApproval 或 Amending 时，行为提示 SHALL 明确每次工具调用仅为形成、验证或修订待审批计划服务。工具权限批准 SHALL NOT 被解释为计划批准。模型 SHALL NOT 直接实施所规划的产品变更，也不得借终端脚本、编辑、MCP、子 Agent、其它会话或 Workflow 绕过计划审批；委派调研 SHALL 携带相同限制。必要的有限实验 SHALL 使用可回收范围并保护已有工作，不能以实验名义交付未批准实现。

#### Scenario: Research needs terminal execution
- **WHEN** 形成计划需要检查依赖、环境、构建信息或运行有限实验
- **THEN** 模型可沿普通权限调用所需工具，产出调研证据与计划，不把结果扩展成未批准的产品实现。

#### Scenario: A tool or child is allowed but the plan is unapproved
- **WHEN** shell、MCP、编辑工具或子 Agent 具备执行/写入权限，而 Plan 尚未获批
- **THEN** 行为护栏仍禁止直接或间接实施；普通授权不改变 Plan phase，不构成审批替代。

#### Scenario: Approval or amendment is pending
- **WHEN** 计划已冻结等待决定，或新范围仍处于 Amending
- **THEN** 模型不得自行实施该未批准范围、修改冻结计划或以新调查替代决定；沿现有审批/修订协议进入可执行阶段。

证据入口与目标落点：`crates/codegen/shell/prompts/behaviors/plan/base.md`、`crates/codegen/shell/src/session/behavior.rs::plan_phase_model_context` 与原 PlanControl 决定路径。
