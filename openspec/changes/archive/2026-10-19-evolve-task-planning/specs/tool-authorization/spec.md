## ADDED Requirements

### Requirement: Planning commands validate local ownership and exact relationships

计划写入、task 关联、等待及重规划确认 SHALL 在 runtime 依据当前 session/工作范围和已验证委派关系检查。模型提供的 owner、任意他人步骤、非接收方处理确认或缺失结果身份 SHALL NOT 获得授权。合法内部重规划通知只能由已提交父项变化产生，不作为可任意调用的跨 session 写入口。

#### Scenario: TA-01 Forged plan owner or relationship
- **WHEN** 模型尝试修改另一个 session 的 TODO，关联 sibling 的步骤或不属于当前范围的历史 ID
- **THEN** 精确调用被拒绝，不改任何计划、不产生通知，也不改变 task 的实际 owner。

#### Scenario: TA-02 Handling acknowledgment has no receipt authority
- **WHEN** child 确认未消费的变化、另一个 child 的请求，或父级验收引用不存在的结果
- **THEN** runtime 拒绝确认并保留原责任；自然语言声称“已完成”不弥补缺失证据。

证据与实施入口：`crates/codegen/tools/src/implementations/grow_build/task/{types.rs,coordinator.rs}`、`crates/codegen/shell/src/session/actor/notification_drain.rs`、现有 `GoalMutationAuthority` 模式。

### Requirement: Parent replanning guidance grants no additional execution rights

自动重规划指导 SHALL 保留 agent 来源，仅在合法直接委派关系和允许的执行状态处理，不授予文件/进程权限、用户同意或一次性 permit。普通 parent message 的 Write 投影与精确参数校验 SHALL 保留；内部通知不成为绕过这些规则的公共 Send。Workflow child 的既有通信限制 SHALL 保留。

#### Scenario: TA-03 Replanning asks for an unauthorized action
- **WHEN** 父项新要求涉及 child 无权执行的文件写入或命令
- **THEN** child 可重规划或报告限制，但执行仍经过原权限 Gate；父项变化不能作为人类批准。

#### Scenario: TA-04 Inactive or Workflow target is addressed
- **WHEN** 普通消息尝试发送到终态 child 或受现有通信限制的 Workflow child，或者重规划目标暂处挂起
- **THEN** 普通消息按原规则拒绝或仅核对既有回执；挂起目标的自动重规划保留来源责任等待合法恢复，不扩大普通消息准入。

证据与实施入口：`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs::{interaction_route,send_to_child_route}`、`crates/codegen/shell/src/session/actor/notification_drain.rs::receive_agent_message`、既有 “Receipt-bound replies grant only communication authority”。
