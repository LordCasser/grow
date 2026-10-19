## ADDED Requirements

### Requirement: Explicit skill loading shares one runtime operation
用户斜杠技能调用与 Agent 工具调用 SHALL 使用同一技能加载操作，复用唯一运行时目录和正文准备规则。显式用户调用 SHALL 由 Runtime 派发，不依赖前置模型决定。触发来源 SHALL 由 Runtime 持有，不接受调用参数伪造。

#### Scenario: User and model load the same skill
- **WHEN** 用户斜杠命令与 Agent 分别请求同一有效来源和参数
- **THEN** 两者使用相同加载语义和结果结构；用户路径无需前置任务模型决定是否加载，来源分别标记为用户与模型，现有权限辅助调用仍按原管线执行。

#### Scenario: Model attempts to claim user invocation
- **WHEN** 模型试图通过加载参数声明用户来源或权限批准
- **THEN** Runtime 不接受该声明，仍按模型调用资格和权限处理。

#### Scenario: Read and initialization remain distinct
- **WHEN** Agent 通过文件工具审阅 SKILL.md，或初始化阶段处理 Agent 的 skills 声明
- **THEN** 文件读取不构成显式加载，初始化保持预加载角色；两者都不伪造用户斜杠加载记录。

证据入口：`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` 的 SkillManager、`tools/src/implementations/skills/skill.rs` 的内容准备、`shell/src/session/slash_commands.rs`、`agent/src/prompt/skills.rs`（后面三项均位于 `crates/codegen/`）。本条是新增契约。

### Requirement: Skill load eligibility follows its actual caller
加载 SHALL 复核已选条目的 enabled、条件可见性和调用资格。用户入口 SHALL 遵守 user_invocable；模型入口 SHALL 遵守 disable_model_invocation；不得将一个入口的过滤结果冒充另一个入口的可调用目录。

#### Scenario: User-only skill
- **WHEN** 技能允许用户调用且禁止模型自主调用
- **THEN** 用户显式调用可进入加载门控，模型调用被明确拒绝。

#### Scenario: Model-only skill
- **WHEN** 技能禁止用户调用但允许模型调用，且满足其余门控
- **THEN** 模型可加载，用户入口不把它作为可调用斜杠技能。

#### Scenario: Catalog changes before execution
- **WHEN** 已接纳引用的来源在消费前禁用、不可见或失去调用资格
- **THEN** 加载失败，不使用旧目录可见性绕过当前门控。

证据入口：`crates/codegen/shell/src/session/slash_commands.rs` 的 EffectiveCommandCatalog、`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` 的模型列表过滤。
