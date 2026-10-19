## ADDED Requirements

### Requirement: Goal continuation respects explicit readiness

Goal continuation SHALL 在既有身份/预算/控制 gate 下检查当前工作等待意图。所有当前可推进工作均明确等待时，内层准入与外层 idle 重排 SHALL 都保持事件等待，不产生重复模型调用或忙循环。存在其他可推进步骤时 MAY 继续。等待 SHALL NOT 自动变成 Goal paused/blocked，也不得累计 blocked 的连续 turn 审计。

#### Scenario: BG-01 Active Goal has only waiting work
- **WHEN** Goal Active，当前所有可推进工作都等待真实依赖，且没有新输入
- **THEN** 不采样、不持续重新调度 idle；相关依赖变化后重新核对准入，等待时间不算一次 blocked turn。

#### Scenario: BG-02 Another step can proceed
- **WHEN** 一个步骤等待 child，但同一 Goal 还有可推进的独立步骤
- **THEN** 可继续处理该步骤，不因某处有 child 在运行就冻结整个 Goal；没有显式计划时继续原全目标审计逻辑。

#### Scenario: BG-03 User control wins over readiness
- **WHEN** 依赖变为就绪时已有用户输入、Stop、挂起、权限/用户等待或预算关闭
- **THEN** 按原控制与输入优先级处理并重新核对；就绪事件不能自动回答问题、授予权限或越过关闭的执行 gate。

#### Scenario: BG-09 Update arrives while Goal or request admission is closed
- **WHEN** Goal Paused 或预算关闭，或 child 正等待真实用户/权限请求，此时父项变化和依赖结果到达
- **THEN** 只保存交付与处理责任，无新采样或自动批准；恢复/回答/授权仍由原控制与请求 owner 接纳，TODO 通知不替代它们。

证据与实施入口：`crates/codegen/shell/src/session/actor/goal.rs::{schedule_goal_on_idle,drive_goal_on_idle,start_goal_internal_turn}`、`idle_arbitration.rs`。

### Requirement: Goal completion audits explicit obligations and the whole objective

Goal complete SHALL 保留原目标整体完成审计，并检查当前范围内显式必需交付、有效等待、影响这些交付的未处理要求变化及必要收尾。未闭合时 SHALL 返回可操作的引用与原因。普通局部 TODO 或无关后代步骤 SHALL NOT 自动成为硬门槛，全部勾选 SHALL NOT 自动完成 Goal。有理由地解除某项义务 SHALL NOT 改写原目标已经达成的判断。

#### Scenario: BG-04 Checked TODOs hide unfinished mandatory work
- **WHEN** 父 TODO 都显示 completed，但必需 child 尚未交付、验收或处理影响交付的要求变化
- **THEN** complete 被拒绝并指出未闭合义务，直到接受或明确处置；旧清单状态不能代替全目标审计。

#### Scenario: BG-05 Unrelated local TODO is still open
- **WHEN** 原目标与必需交付已通过审计，某 child 留有不属于当前必需交付的局部探索项
- **THEN** 不仅因该复选框未勾选就阻止完成；仍活跃的执行和真实收尾责任按现有 owner 规则处置，不能隐藏副作用。

#### Scenario: BG-08 Three-level delegation preserves local acceptance and root authority
- **WHEN** root Goal 的必需交付由普通 Task child 承担，child 又委派 grandchild 并作局部验收
- **THEN** child 不获得 Goal mutation authority；root 依据 child 的结果及局部交付摘要作自己的验收，检查 root 必需义务和原目标，不要求 root 消费 grandchild 第二份回执或勾完所有后代 TODO。

证据与实施入口：`crates/codegen/shell/src/session/actor/goal.rs::{render_goal_continuation_from,handle_goal_command}`、`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs`。

### Requirement: Local planning never substitutes for Goal or Plan authority

TODO 变化、重规划及验收 SHALL NOT 自动创建 Goal、更新 Goal definition revision、恢复暂停 Goal 或修改已批准 Plan artifact。真正 Goal 定义变更、Plan 越界和 Workflow 执行 SHALL 继续遵守原 authority 与审批协议；弱 TODO 版本不能替代 Goal revision fence。

#### Scenario: BG-06 TODO update preserves the active Goal identity
- **WHEN** 活动 Goal 内父 TODO 修改并触发 child 重规划
- **THEN** 原 Goal identity/revision、预算归属和 child admission 保持原含义，不因 TODO 版本改变失效；真正旧 Goal revision 的迟到结果仍遵守原失效 owner 规则。

#### Scenario: BG-07 Plan refinement exceeds approved scope
- **WHEN** 局部计划调整只是执行细化，或实际超出了已批准 Plan 的工作范围
- **THEN** 前者不生成新的审批 artifact，后者遵循原重新审批路径；不能以 TODO 调整名义绕过审批或 Goal/Plan 互斥。

证据与实施入口：`crates/codegen/shell/src/session/actor/{goal.rs,behavior.rs}`、`crates/codegen/shell/src/session/goal_tracker.rs`。
