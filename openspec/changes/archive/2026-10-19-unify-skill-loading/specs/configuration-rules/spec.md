## MODIFIED Requirements

### Requirement: Expanded skill reference index reflects loaded blocks
技能引用结果 SHALL 按输入引用顺序分别记录加载结果，成功索引 SHALL 只包含正文与上下文贡献已经确认提交的项。运行时正文 SHALL 作为独立加载结果交付，不再拼接为用户消息中的展开信封。任何索引去重 SHALL 不合并不同调用的结果或参数。

#### Scenario: Mixed successful and failed expansions
- **WHEN** 多技能请求中有可加载技能、缺失文件或消失的目录条目
- **THEN** 按序准备，在首个失败处停止派发；整组未发布，成功索引为空，逐项诊断区分准备后取消、失败与未执行，整条输入不执行。

#### Scenario: All expansions fail
- **WHEN** 所有技能均未成功提交正文结果
- **THEN** 返回明确的失败结果，不生成空成功信封，不将该输入交给模型执行。

证据入口：`crates/codegen/shell/src/session/slash_commands.rs` 的 SkillExpansion、expand_skill_refs_shared 和 `prompt_parser.rs` 的 assemble_parts_with_skills；本条取代现有全失败返回 None 的静默继续语义。

### Requirement: Plugin skill usage reports expansion outcome
Turn admission 的插件技能使用诊断 SHALL 按实际加载提交结果设置 PluginUsed.success，不得在读取或上下文确认前宣称成功。结果不明时 SHALL 不伪造确定的成功或失败。

#### Scenario: Plugin skill expansion fails
- **WHEN** 请求的插件技能正文读取失败或目录条目已消失
- **THEN** 对应 PluginUsed.success 为 false。

#### Scenario: Mixed plugin expansion results
- **WHEN** 同一用户输入部分插件技能准备完成，另一项失败使整组未发布
- **THEN** 已实际派发的引用均不报告加载成功，诊断保留准备、失败或取消原因；未派发项不伪造使用事件。只有整组提交成功才分别报告成功。

#### Scenario: Skill interjection
- **WHEN** 技能通过 interjection 加载到运行中的 turn
- **THEN** 保留派发和加载结果诊断，不新增代表 turn 归属的 PluginUsed 事件。

#### Scenario: Commit acknowledgement is unknown
- **WHEN** 内容准备完成但持久化提交结果无法确认
- **THEN** 等待权威事实对账，不提前发布 PluginUsed.success 的确定结果。

证据入口：`crates/codegen/shell/src/session/actor/turn/admission.rs` 的 PluginUsed 与 `actor/interjection.rs` 的派发诊断。

### Requirement: Skill expansion preserves selected source identity
统一技能加载 SHALL 同时匹配解析时选定的路径、限定名称与插件身份，不得仅凭同一路径或同名使用其他来源条目。来源消失或身份无法唯一确认 SHALL 明确失败。

#### Scenario: Native and plugin skills share a path
- **WHEN** 原生技能和插件技能路径相同，调用方选定插件技能
- **THEN** 加载使用该插件条目的正文快照和插件变量。

#### Scenario: Selected source disappears
- **WHEN** 选定插件条目消失但目录仍有同路径原生技能
- **THEN** 该引用加载失败，不回退到原生条目。

#### Scenario: Ambiguous callable name
- **WHEN** 请求名称不能唯一解析为具有调用资格的条目
- **THEN** 明确报告无法解析，不任选同名技能；能够唯一解析的限定名仍可调用。

证据入口：`crates/codegen/shell/src/session/slash_commands.rs` 的 ParsedSkillRef、EffectiveCommandCatalog 与精确来源匹配。

### Requirement: Skill envelope attributes escape special characters
技能独立正文结果中的 name/args、可选引用索引的 name/path 及预加载消息的 name/description/path SHALL 在使用 XML 属性表示时转义特殊字符，正文内容 SHALL 保持原样。结构化表示 SHALL 保留原值，不把属性值解释为额外指令或字段。

#### Scenario: Quoted skill arguments and paths
- **WHEN** 名称、参数或路径包含双引号、尖括号或 &
- **THEN** 对应 XML 属性输出实体转义，字符不得形成额外属性或标签；结构化字段可还原原值。

#### Scenario: Body and duplicate references
- **WHEN** 正文包含 Markdown/标记且引用重复
- **THEN** 正文保持原样，可选展示索引仍按原始名称与路径去重，实际调用及独立结果按引用身份保留，不吞掉不同参数的加载。

证据入口：`crates/codegen/shell/src/session/slash_commands.rs` 的正文块格式化与 `crates/codegen/tools/src/implementations/skills/skill.rs` 的 build_skill_message。替换、链接、空快照、1 MiB 文件上限等未修改契约继续适用。

## ADDED Requirements

### Requirement: Explicit skill preparation bounds expanded bodies
显式加载 SHALL 将替换及参数附加后的正文限制在 1 MiB UTF-8 字节，同一用户输入的多个正文 SHALL 共享该准备额度。长度算术溢出或超限 SHALL 在大分配前失败，准备过程 SHALL 以有界工作片段响应取消。该限制 SHALL 不改写已加载快照的读取语义，不替代完整请求预算，也不改变初始化预加载策略。

#### Scenario: Repeated placeholders amplify arguments
- **WHEN** 合法小模板重复引用大参数，最终正文将超过准备额度
- **THEN** 在构造完整大对象前明确失败，不产生部分 Loaded；单次替换和字面插入规则保持。

#### Scenario: Aggregate preparation reaches its limit
- **WHEN** 多技能正文各自未超限，但同输入合计超过 1 MiB
- **THEN** 超限项失败且整组不发布；恰好额度的正文可通过准备，仍须接受请求预算检查。

#### Scenario: Cancellation during expansion
- **WHEN** 用户在模板长度检查或输出构造期间取消
- **THEN** 在下一个有界工作片段结算取消，不等待全部展开、不发布部分正文。

证据入口：`crates/codegen/tools/src/implementations/skills/skill.rs` 的 read_skill_body_file 与 apply_substitutions；当前 replace_all 无准备输出上限，本条要求在保持模板语义的前提下补齐该边界。
