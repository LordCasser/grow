## MODIFIED Requirements

### Requirement: Explicit skill loads reuse visible Skill interactions
主窗口 SHALL 使用现有 Skill 行的折叠、展开和正文查看交互呈现显式加载，以结构化身份和实际状态驱动，不以路径或模型文字推断成功。用户与模型来源 SHALL 沿同一 ACP 工具行投影与回放管线关联真实加载身份。行 SHALL 显示技能名称、来源及加载中、已加载、失败或取消结果；准备完成但未发布 SHALL 保持加载中，提交未知 SHALL 明确可见，Loaded SHALL 晚于权威发布。终态 SHALL 更新原行。

#### Scenario: Forced load becomes visible
- **WHEN** 用户技能输入开始实际加载并随后成功提交
- **THEN** 主窗口显示对应名称的加载中行，提交确认后原行变为已加载，不需要额外模型文字确认。

#### Scenario: Failure and cancellation
- **WHEN** 加载被拒绝、正文读取失败或明确取消
- **THEN** 原 Skill 行显示真实结果和可展开原因，不显示成功标记；尚未派发的引用不冒充已加载。

#### Scenario: Multiple skills collapse
- **WHEN** 多个显式 Skill 行出现且用户折叠正文
- **THEN** 每行仍保留名称和结果，显式 Skill 不参加匿名数量合并；无需新分组器或面板。

#### Scenario: Collapsed rows distinguish resources of the same skill
- **WHEN** 同一技能先加载主说明，随后显式加载包内资源，用户在实时或历史回放中查看折叠行
- **THEN** 资源行 SHALL 在标题中显示调用指定的技能内相对路径，主说明行保留原技能标题，各行沿用对应调用的加载状态。

#### Scenario: Expand resolved snapshot
- **WHEN** 用户展开已提交 Skill 行
- **THEN** 展示真实来源、调用参数及实际提交正文快照，不读取最新文件来替换显示内容。

#### Scenario: Group fails after some bodies are prepared
- **WHEN** 用户组的一些正文已准备，但另一项失败导致整组未发布
- **THEN** 出错项显示失败，已开始的其余项显示取消及未注入原因，未派发项只作未执行说明；没有任何项提前显示 Loaded。

#### Scenario: Execution terminates after a confirmed load
- **WHEN** 加载已确认发布，但首次关联模型步骤尚未获得已接纳响应就终止
- **THEN** 历史 Skill 行保留当时的 Loaded，所属输入或回合明确显示后续执行未完成；不能把该行解释为内容已被模型成功处理。

证据入口：`crates/codegen/pager/src/scrollback/blocks/tool/read.rs` 的 Skill 标题和内容查看、`tool/mod.rs` 的 VerbGroupKind，以及 `crates/codegen/pager/src/acp/tracker.rs` 的状态和回放接收。
