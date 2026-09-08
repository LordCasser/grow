# development-workflow Specification

## Purpose
定义本项目从此次迁移起采用的 OpenSpec SDD 开发流程。明确当前规范、开发过程和开发者文档的职责，约束变更前的场景设计、最小非行为变更与无关架构债务处理；这是新增治理契约。

## Requirements

### Requirement: Specification authority
项目开发 SHALL 将 openspec/specs 作为已归档行为契约，将 openspec/changes 作为变更过程记录，将 docs 作为开发者阅读说明。

#### Scenario: 文档冲突
- **WHEN** docs、历史审计与当前 spec 描述不一致
- **THEN** 先核对代码证据，在 change 中记录差异并修正规范或实现；不把历史计划直接当成已实现事实。

证据：`AGENTS.md` — `OpenSpec`。

### Requirement: Spec first implementation
改变行为、接口、状态、安全或持久化契约的开发 SHALL 先形成 proposal、delta specs、design 和 tasks，再实施与验证。

#### Scenario: 开始实现行为变化
- **WHEN** 已识别受影响 capability 与调用方
- **THEN** 先写可验证 WHEN/THEN 场景，完成实现和相关验证后归档更新主规范。

证据：`AGENTS.md` — `OpenSpec`。

### Requirement: Minimal nonbehavior change
不改变行为的纯文档、工具维护或纯重构 SHALL 保持最小 change，并在 .openspec.yaml 显式设置 skip_specs: true；不得为通过校验虚构产品需求。

#### Scenario: 修正文档错字
- **WHEN** 确认现有契约不变
- **THEN** 简述理由、影响与验证，不增加无意义 delta；仍保留可追溯 change 记录。

证据：`AGENTS.md` — `skip_specs`。

### Requirement: Separate architecture debt
审计发现的无关架构债务 SHALL 单独登记，不扩入当前实现；长期计划不自动构成开发授权。

#### Scenario: 审计发现额外问题
- **WHEN** 问题与当前 change 验收无关
- **THEN** 登记到 OpenSpec backlog，待明确启动后单独建立 change。

证据：`AGENTS.md` — `backlog`。
