## MODIFIED Requirements

### Requirement: Interjection display preserves user text across resume
运行中用户输入 SHALL 将用户可见文本与模型专用插话指令分开持久化。显式技能正文 SHALL 来自关联的独立加载结果，不再追加到用户输入或其插话信封。恢复 SHALL 展示保存的用户文本；用户自己写入的标记 SHALL 保留，显示文本 SHALL NOT 通过解析模型提示标签恢复。

#### Scenario: Resume after accepted steering
- **WHEN** 运行中输入在加载前提满足后被消费，显示更新随后被回放
- **THEN** 显示清理后的原始输入，模型上下文保留适用的插话指令与独立技能结果，不把 Runtime 信封显示为用户文字。

#### Scenario: Direct interjection injection
- **WHEN** 没有队列输入身份的直接插话被接纳
- **THEN** 同样保留显示与模型内容的分离以及图片块；含技能引用时，先建立 Runtime 持有的稳定关联身份，再执行相同加载前提。

#### Scenario: User authors prompt-like markup
- **WHEN** 用户输入包含字面的 user_query 标签
- **THEN** 标签作为用户文字保留，不被当作 Runtime 包装解析或删除。

证据入口：`crates/codegen/shell/src/session/actor/interjection.rs` 的直接注入与 consume_steer_inputs、`crates/codegen/pager/src/acp/tracker.rs` 的持久显示文本回放。

## ADDED Requirements

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

### Requirement: Skill display distinguishes file reads and historical results
普通文件读取 SHALL 保持 Read 语义，初始化预加载 SHALL 不伪造显式调用行。normal、minimal、恢复回放与 headless SHALL 从相同加载事实呈现名称、来源及结果；历史成功 SHALL 不表示技能永久生效。

#### Scenario: Review SKILL.md as a file
- **WHEN** 文件工具读取名为 SKILL.md 的文件或 skills 目录中的 Markdown
- **THEN** 显示文件读取，不仅凭路径显示为技能加载成功。

#### Scenario: Replay and headless output
- **WHEN** 已提交加载结果被 normal/minimal 客户端回放或输出到 headless
- **THEN** 各入口保持相同身份与结果，headless 提供对应结构化信息，不重复执行加载或追加成功行；历史 replay 将 terminal ToolCall 与其前序 ToolCallUpdate 折叠时，仍保留 Skill 的 canonical tool identity 和 raw output（source / prepared body），并生成与实时 terminal ToolCallUpdate 相同的配对结果。

#### Scenario: Loaded history after compaction
- **WHEN** 用户查看压缩前的成功加载记录
- **THEN** 展示当时加载结果，不新增“当前持续生效”的技能状态承诺。

证据入口：`crates/codegen/pager/src/scrollback/blocks/tool/read.rs`、`crates/codegen/tools/src/implementations/skills/types.rs` 的 skill_name_from_path，以及现有 normal/minimal/headless 更新管线。
