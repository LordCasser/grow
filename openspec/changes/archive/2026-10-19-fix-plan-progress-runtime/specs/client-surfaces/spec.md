## ADDED Requirements

### Requirement: Active Plan has an inspectable top status entry

Plan Behavior 活跃时，Pager SHALL 在既有顶部状态栏展示当前 Plan 阶段入口；入口 SHALL 独立于 TODO 是否存在及前端是否缓存 artifact 正文。用户点击 SHALL 沿既有审批视图或 session 绑定的只读 artifact 查询，在计划详情中查看完整正文，不批准计划、不启动执行。Plan 模式中的 TODO task、进度徽标及任务面板 SHALL 独立保留，不被计划详情替换。非活跃 Plan 的 retained/配置展示继续要求有效预览来源。

#### Scenario: Executing Plan without cached body or TODO
- **WHEN** session 已投影为 Plan Executing，但前端没有审批视图、正文缓存或 TODO
- **THEN** 顶部仍显示 Plan Executing，点击会查询该 session 的 canonical artifact 并沿既有版本校验展示。

#### Scenario: Drafting has no submitted artifact
- **WHEN** Plan Drafting 尚未提交正文，用户查看顶部入口
- **THEN** 顶部显示 Drafting；点击后若后端尚无 artifact，沿原查询路径说明尚无计划，不伪造内容或发起模型工作。

#### Scenario: Plan phase changes or exits
- **WHEN** Plan 的权威投影在 Drafting、AwaitingApproval、Executing、Amending 间改变，随后退出
- **THEN** 顶部随投影更新阶段；退出后依原 retained/配置与有效缓存条件显示，不残留活跃阶段。

#### Scenario: Plan details and TODO progress coexist
- **WHEN** Plan 活跃且存在已提交的执行 TODO
- **THEN** 顶部 Plan 入口打开完整计划详情，TODO 入口仍打开任务列表；查看计划及退出 Plan 不更改 TODO 状态。

实施入口：`crates/codegen/pager/src/app/agent_view/{plan.rs,render.rs,mouse.rs}`、`app/root/dispatch/modes.rs`。
