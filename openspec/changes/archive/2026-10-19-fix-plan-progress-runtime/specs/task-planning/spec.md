## ADDED Requirements

### Requirement: Agent replacement preserves session planning access

同一 session 更换 Agent 后，仍允许的 TODO 工具 SHALL 继续访问该 session 的当前局部计划，并通过原 session owner 提交更新。替换 SHALL 保留已提交步骤身份与状态，不产生新工作范围，不从历史文本重建；不能因更换工具运行环境导致 Planning runtime unavailable。计划读取失败 SHALL 阻止未准备完成的 Agent 替换。

#### Scenario: Switch Agent before the first TODO
- **WHEN** session 切换到提供 TODO 工具的 Agent，随后在已接纳工作中首次提交 TODO
- **THEN** 工具通过原 session owner 成功提交，并产生对应 ACP plan 投影。

#### Scenario: Switch Agent with an existing plan
- **WHEN** session 已有包含完成及未完成步骤的局部计划并切换 Agent
- **THEN** 新 Agent 读取同一 scope、步骤身份及状态，后续合并更新保留未修改步骤；替换本身不提交新计划。

实施入口：`crates/codegen/shell/src/session/actor/{planning.rs,model_switch.rs,spawn.rs}`。
