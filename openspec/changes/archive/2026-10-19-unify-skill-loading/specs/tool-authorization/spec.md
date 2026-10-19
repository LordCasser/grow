## MODIFIED Requirements

### Requirement: Legacy unregistered Skill IO is removed
ToolInput 和 ToolOutput SHALL NOT 暴露未注册的旧 Skill 变体。技能显式加载 SHALL 使用已注册的 load_skill 能力及其真实调用语义；已注册工具、动态 ToolPack、技能发现与初始化预加载 SHALL 保持可用。

#### Scenario: Built-in skill capability
- **WHEN** 构造具有技能加载能力的内建工具集
- **THEN** 模型可通过已注册 load_skill 加载目录中的合格技能，不依赖旧 Skill IO 或运行时用户消息正文展开。

证据入口：`crates/codegen/tools/src/implementations/skills/`、工具注册与 `crates/codegen/shell/src/session/actor/tool/preparation.rs`；旧 IO 的移除保持，新增注册操作不代表恢复旧无效变体。

## ADDED Requirements

### Requirement: Forced skill loading preserves execution gates
用户强制触发 SHALL 只保证 Runtime 派发，加载仍 MUST 遵守当前工具能力上限、权限、适用文件访问边界、Hook 和取消规则。技能元数据 SHALL 不额外授予权限。Hook SHALL 复用现有允许、拒绝、观察及失败策略，不因技能加载新增参数或结果正文改写能力。

#### Scenario: User load is denied
- **WHEN** 用户显式加载被权限或 PreToolUse 拒绝
- **THEN** 返回可见失败，拒绝前尚未读取的正文不被读取，依赖输入不执行。

#### Scenario: Child cannot use loader
- **WHEN** 子 Agent 的能力上限不允许加载操作
- **THEN** 用户或模型触发均不能通过用户消息展开或另一个读取入口代为执行加载。

#### Scenario: Metadata does not authorize subsequent tools
- **WHEN** 成功加载的技能声明 allowed-tools 或要求执行其他工具
- **THEN** 声明保持描述作用，后续工具仍接受正常权限判定。

#### Scenario: Hook failure and observation preserve existing policy
- **WHEN** 加载的门控 Hook 失败、取消，或工具终态触发 PostToolUse / PostToolUseFailure
- **THEN** 门控按现有 on_failure 和取消策略结算，Post Hook 在对应工具终态确认后观察；不改写调用参数或正文，已结算或结果不明的 Hook 不因恢复重跑。

证据入口：`crates/codegen/shell/src/session/actor/tool/preparation.rs` 的工具门控与 Hook、`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` 的运行时文件访问，以及主规范现有 capability ceiling 契约。
