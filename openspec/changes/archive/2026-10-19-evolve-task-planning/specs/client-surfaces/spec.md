## ADDED Requirements

### Requirement: Plan presentation preserves actual progress and waiting

客户端 SHALL 区分计划业务状态、执行活动、等待依据、结果交付及验收。turn 结束 SHALL NOT 把未完成的 in_progress 显示成 completed。Grow 投影 SHALL 携带 session/工作范围/步骤与执行关联的稳定身份；仅支持粗粒度 ACP 的客户端 SHALL 保守呈现，不靠显示完成消除 spinner。

#### Scenario: CS-01 Turn ends with unfinished TODOs
- **WHEN** 模型回合结束但计划项仍未完成，或工作已结构化等待 child/命令/用户
- **THEN** 页面显示真实未完成/等待状态，等待有可查看依据；刷新、headless 和恢复不出现先完成后回退的假状态。

#### Scenario: CS-02 Generic ACP cannot express waiting
- **WHEN** 客户端只能表示 pending/in_progress/completed
- **THEN** 未完成等待保留保守的 in_progress 及支持的说明，不映射为 completed；Grow 客户端通过稳定引用显示完整状态。

证据与实施入口：`crates/codegen/shell/src/session/actor/turn/settlement.rs::emit_turn_end_plan_cleanup`、`crates/codegen/shell/src/session/acp_conversion.rs::acp_plan_update`、`crates/codegen/pager/src/{app/acp_handler,headless/reducer}`。

### Requirement: Linked plans and unapplied changes are inspectable

用户 SHALL 能从父项查看关联执行及 child 自己的计划，区分待重规划、已处理、已交付待验收、要求变更待复核和终态未应用。父子列表 SHALL 保持各自所有权，不以子项数生成假总进度。查看、恢复展示和 replay SHALL 只投影已有事实，不触发模型工作。

#### Scenario: CS-03 Parent changes a step with active and completed children
- **WHEN** 同一父项关联一个活跃 child 和一个已结束 child 后被修改
- **THEN** 活跃 child 显示待检查/处理结果，已结束 child 显示变更未应用且原结果仍可查看；父级可明确决定复用或另委派。

#### Scenario: CS-04 Inspect a nested plan during replay
- **WHEN** 用户展开 grandchild 或播放导出的会话树
- **THEN** 只显示已交付历史中的本地计划/关联/等待，无模型调用、通知消费或旧任务复活；不同 session 的同名 step 不合并。

证据与实施入口：`crates/codegen/pager/src/app/agent_view`、`crates/codegen/pager/src/headless/reducer`、现有 session/child 状态核对与 transcript replay 投影。
