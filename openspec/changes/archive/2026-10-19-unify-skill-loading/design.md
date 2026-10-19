## Context

技能加载是一次有身份、结果和上下文提交边界的操作。用户斜杠调用与 Agent 调用只在“谁决定发起”上不同。见 [proposal](proposal.md)。

以下是当前代码事实，尚未实现本设计。路径均相对仓库根目录，核对基线见 [verification](verification.md)。

| 环节 | 当前行为与代码入口 |
| --- | --- |
| Pager 提交 | `crates/codegen/pager/src/slash/acp_command.rs` 的 `AcpSlashCommand::run` 发送 `/name args`，`dispatch/prompt.rs`、`dispatch/queue.rs` 管理提交与排队；Pager 没有在此读取正文。 |
| Shell 解析 | `crates/codegen/shell/src/session/slash_commands.rs` 的 `EffectiveCommandCatalog`、`ParsedSkillRef`、`expand_skill_refs_shared` 选择条目、加载正文、替换参数。成功项与失败项通过 `loaded` 区分。 |
| 首条输入与插话 | `actor/turn/admission.rs` 和 `actor/interjection.rs` 分别调用展开；`prompt_parser.rs` 的 `assemble_parts_with_skills` 把正文拼到同一用户消息。长输入还会经过 `prompt_build.rs` 的预算与 blob 路径。 |
| 内容准备 | `crates/codegen/tools/src/implementations/skills/skill.rs` 的 `load_skill_content`、`read_skill_body_file`、`apply_substitutions` 已处理正文快照、文件上限、链接和单次模板替换。 |
| Agent 读取 | `crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` 是文件工具；其技能路径特例不等于目录身份解析、参数替换或技能调用。 |
| 现有 Skill 显示 | `crates/codegen/pager/src/scrollback/blocks/tool/read.rs` 根据路径产生 Skill 标题，`tool/mod.rs` 据此分组。可复用其交互，但路径启发式不能作为加载事实。 |
| 预加载 | `crates/codegen/agent/src/prompt/skills.rs` 与 `builder.rs` 在初始化构造技能提示；这不是回合内工具执行。 |
| 持久化与投影 | `crates/codegen/chat-state/src/timeline.rs` 持有事实；`crates/codegen/sampling-types/src/conversation.rs` 的 `AgentMessage` 已有 Runtime 消息与 Provider 合法工具往返的分离模式，可复用投影原则。 |
| 工具边界 | `crates/codegen/shell/src/session/actor/tool/preparation.rs` 执行工具门控和 Hook；`event_tracker.rs` 的工具 Started 依赖已有 turn/step。`actor/tool/dispatch.rs` 的 direct Bash 已使用没有 provider 请求的 Runtime Step / ToolEvent / ACP 行，可复用其因果关系。 |

当前主规范允许技能 prompt expansion 和普通文件读取，且明确移除了未注册的旧 Skill IO。本 change 是有意修改这些行为，不能把未来要求写成现状。`AcpSlashCommand` 等位置的旧注释与实现不完全一致，后续只在改动涉及处纠正。

## Goals / Non-Goals

**Goals:**

- 一个明确入口、一套正文准备逻辑、一份持久结果、一套 Skill 交互。
- Runtime 保证显式用户调用被执行；Loaded 表示实际内容已提交，可用于下一次模型请求。
- 输入、加载和采样保持各自身份，失败和恢复不制造重复执行或虚假成功。

**Non-Goals:**

- 不建立通用强制工具执行框架、新目录服务、新持久化数据库、Skill job 生命周期或全局技能启用状态。
- 不重写 slash 语法、模板语法、技能安装发现、Workflow 定义或初始系统提示布局。
- 不保证模型永久保留正文或始终遵循技能。压缩后的内容可用性与历史加载结果是两个事实。
- 本次只交付设计和规范，不修改产品代码、主规范，不执行归档。

## Decisions

### 1. 入口统一，触发来源由 Runtime 持有

注册一个 `load_skill` 工具，模型输入为 `{ name: string, args?: string }`，缺省 args 为空字符串。name 使用目录提供的可调用名称，存在冲突时使用限定名。来源、路径、plugin identity、input identity、权限结果均由 Runtime 解析或派生，不接受模型自行声明。

SkillManager 继续是唯一目录所有者。加载入口复用其目录身份、条件可见性与启用状态；用户路径要求 `user_invocable`，模型路径要求未设置 `disable_model_invocation`，两者不互相代替。Agent 工具列表与技能目录提示一起更新，指向 `load_skill`；受工具集或子 Agent 能力上限限制的调用明确失败，不能回退成 prompt 拼接绕过限制。

原生 grow-build、concise、hashline、explore 和 grow-computer 工具集都显式包含同一 `LoadSkillTool`，不能只在全局 registry 注册却遗漏实际 Agent 的工具集。自定义工具集仍按其声明确定资格；用户专用 Skill 可以通过该入口加载，而模型直接选择该 Skill 仍须被 `disable_model_invocation` 拒绝。

最后一次 response 的 Step 已闭合后，终态 fence 仍可能接纳插话。若此时显式 Skill 准备新开了 Runtime Step，必须沿正常 Step control boundary 闭合并处理待接纳控制，然后才能启动后继模型 Step；不能因 Runtime Step 尚在而把已消费的插话当作原 response 的完成尾声结算掉。原 Turn、图片和字面文本均保留，只有后继已接纳 response 才算消费新 Skill。

用户路径直接派发相同加载操作，不需要前置任务模型决定是否加载，不依赖“请先读 SKILL.md”这类提示词保证执行。现有权限判定等辅助调用仍按原管线运行，这不构成让任务模型决定是否派发。普通文件读取仍用于检查、修改文件，不触发加载语义。初始 `skills:` 预加载只复用正文准备函数，保留初始化角色和稳定 System prefix。

未采用：保留两条正文管线只加 Notice，会继续隐藏行为差异；识别所有 `read_file(SKILL.md)` 自动激活，会误判文件审阅；强迫模型先调用工具，无法提供确定执行保证。

### 2. 解析、准入、执行分开

普通文本保留现有 slash 识别和内建命令优先级；内建命令、`/loop` 的空技能分支及未解析为技能的普通 `/word` 不新增隐式调用。命令选择器已经选中的技能则在现有输入 payload 中携带显式调用意图和所选来源标识，不能只发送文字与 displayAsSkill。Shell 复核该身份，来源消失、禁用或无法匹配时返回加载失败，不能降级为普通 prompt 或改选同名来源。此标识只是用户意图证据，不授予访问权限；手工构造 payload 同样经过复核。

一次真实输入保留原始文本、附件、显示文本及 prompt/message/input identity。Shell durable admission 解析并保存有序引用计划，含每项名称、参数和选定来源；此时不读取正文、不发出 Loading。选择器携带的来源从选择时起保留，编辑参数不重选来源；用户明确更换技能才更新选择。普通未选择命令的本地文本以最终提交版本解析。现有无队列 identity 的直接插话也在派发加载前建立 Runtime 持有并持久化的关联身份，进入同一前提处理；不能成为第二条无幂等保证的加载路径。

排队输入只有轮到消费、且编辑 hold 已解除时才派发。消费前按冻结的限定名称、路径和插件身份复核目录状态；已消失、禁用、失去调用资格或变得歧义时失败，不换成同名候选。保存排队编辑沿用现有契约：持久接纳新的 input identity 与引用计划后，再替换旧项并解除 hold，保留 FIFO 位置；失败时保留原项和 hold。旧输入不得发起加载；已进入执行的输入不能被当作仍可编辑的队列项。同身份重送只接受相同 payload，不能用“更新版本”绕过身份冲突检查。

执行时选取该身份的当前有效正文：已有内存 body（包括空串）使用其快照；没有 body 的文件来源按既有规则读取。目录刷新可影响尚未执行的新操作，不修改已经提交的历史结果。

### 3. 加载管线复用现有内容和授权能力

共同流程：身份复核 → 工具能力、权限与 Hook 门控 → 有界正文准备 → 权威上下文预算复核与发布 → 工具结算及结果展示。门控顺序沿用现有工具执行规范。用户多项调用以输入为上下文发布边界，模型自主调用保持原工具批次的结果语义。

文件来源走现有 Runtime 文件访问边界；已加载或合成来源使用已选定的来源快照与适用的工具门控，不伪造磁盘读取。`allowed-tools` 仍是描述元数据，不能给加载或后续技能行为额外授权。显式用户请求只保证派发，不能保证权限批准。

复用 `load_skill_content`、链接处理和单次替换；保留 `$ARGUMENTS`、索引、上下文变量、无参数占位符时追加参数的语义。插入值不递归展开。文件仍至多读取 1 MiB 加一个探测字节，严格 UTF-8；合成来源缺正文失败，空正文有效。链接解析不自动加载引用文件，不递归触发其他 Skill；技能正文中的执行指令仍由后续 Agent 按正常授权处理。

显式加载的准备正文上限为 1 MiB UTF-8 字节，沿用现有文件读取上限的量级；同一用户输入的多个正文共享这 1 MiB 准备额度，单个模型调用也受单项上限约束。这是正文准备上限，不代替完整请求的 token 预算，不改变初始化预加载或已存正文快照的读取契约。模板替换先以 checked length 验证输出长度，再有界构造；参数后缀和所有替换值计入，溢出或超限在大分配前失败。遍历和构造按有界工作片段检查取消，不能等巨大 replace_all 完成后才响应；单次替换和字面插入语义不变。

Hook 绑定真实 ToolEvent 的 call identity，复用现有 Allow / Deny / Observe、on_failure 与取消规则；PreToolUse 拒绝时不读取尚未读取的正文。没有 Hook 改参、重定向或结果改写能力，也不保存为此假设的请求 / 有效参数双版本。PostToolUse / PostToolUseFailure 在对应工具终态持久确认后运行，保持观察角色；输入 Hook 不因改走加载入口运行两次。恢复按既有 Hook occurrence 事实处理，不重跑已结算或结果不明的副作用。

### 4. 输入前提与执行状态由现有 owner 持有

Shell SessionActor 持有输入前提和在途工具；工具执行器持有有界内容准备；ChatState 持有输入、ToolEvent、正文引用及 Surface 发布事实；Pager 只是投影。不新增并行状态所有者。

首条或队列输入沿既有 turn 准入，在该 turn 中打开一个真实 Runtime Step，直接调用统一 loader 的门控与执行部分，不先请求模型。ToolEvent::Started 先持久化，PreToolUse 使用其 HookCause::Tool；Step 不产生虚构的 Request、provider response 或 usage。直接 Bash 只作为此因果结构与 ACP 运输的复用先例，其用户消息正文组装和授权策略不复制。成功加载后闭合 Runtime Step，后续按现有 next_step_index 开始模型步骤。

同一用户输入按引用顺序准备，同名同参数与不同参数均保留各自 identity。第一项确定失败即停止派发余项；已准备项保持 Loading，等待整组决定，不提前完成为 Loaded。全部准备成功后，在 ChatState owner 处复核来源资格、所属输入 / turn、Surface revision、当前模型预算和取消状态，然后一次发布原用户输入与有序独立结果。扩展现有输入消费提交，使一个持久提交同时表达输入消费和该组 Surface items；不通过先消费原文、再逐条追加结果模拟原子发布，也不建立通用事务服务。图片与原始输入继续遵守现有准备规则。

任一项失败或发布前取消，整组不进入 Surface：出错项显示 Failed，已开始但未提交的其他项显示 Cancelled 并说明所属输入未发布，尚未派发项只在输入结果中说明未执行。准备快照和错误可保留为历史证据，参数及正文均不能混入原任务或下一条输入。工具终态确认后按现有规则派发对应 Post Hook；观察 Hook 的失败不回滚已确认的上下文发布，存储失败仍关闭 owner。

前台 FIFO 输入在原 turn 内使用现有 Handled 语义完成未送模型结算，不能对已 reserved 的输入直接 Dismissed。未消费的失败 steer 使用现有 Dismissed / Invalidated 路径并关联加载错误；不取消原任务。插话先闭合原模型工具批次，在同一 turn 的 Runtime Step 执行上述流程，不能插进未配对调用之间。正常后续工作继续既有仲裁，不重排队失败输入、不让 Goal 代为执行它。模型自主加载失败返回正常工具错误，不结算最初用户输入。

| 状态 | 判定 | 后续行为 |
| --- | --- | --- |
| 待消费 | 输入已接纳但未到消费边界 | 使用既有队列显示；不宣称 Loading。 |
| Loading | 开始执行加载，含等待门控与提交 | Skill 行显示加载中；不能提前显示成功。 |
| Loaded | 完整正文已获权威上下文发布确认；用户项以整组发布为准 | 后续请求仍须通过首次消费完整性检查。 |
| Failed | 来源、门控、正文或预算等确定失败 | 可见原因；该输入前提不满足。 |
| Cancelled | 操作在成功提交前被明确取消且结算确认 | 不自动重试，不把取消当成功。 |
| 提交状态不明 | durable ACK 丢失或 owner 故障，无法判断提交结果 | 阻止依赖采样，显示无法确认；从权威日志对账后才能归入终态。 |

最后一项是存储故障处理，不创建新的永久技能业务状态。发布后的取消不把历史 Loaded 改成未加载；尚未完成首次模型消费的组按 §7 从可执行 Surface 撤下并记录后续执行终止，防止以后延迟执行。持久事实与当前 Surface 的区别必须在恢复时保持。

### 5. 独立结果需要真正的 Runtime 上下文事实

用户输入保持原文，技能正文不再追加到 `assemble_parts_with_skills` 或夹在 interjection wrapper 中。用户结果以 typed Runtime 上下文项关联输入、引用序号及真实 ToolEvent，携带选定身份、调用参数和正文快照引用。开始 / 完成及错误沿 ToolEvent 和输入结果表达，避免再建一套同义的 Skill 生命周期。继续使用现有 Timeline 与正文 artifact 能力；失败准备事实不自动成为模型上下文。

模型触发有真实 tool call id，沿普通工具结果进入上下文，同时带结构化 Skill 展示信息。不能再插一份 Runtime 结果导致正文重复。用户触发没有模型 call，必须使用 Runtime 来源事实；不能伪造 provider response、sampler request/attempt、采样用量，不能追加没有 call 的孤立 ToolResult。

在 sampling-types 中为 Runtime Skill 结果实现中性投影，复用 `AgentMessage` 的“内部事实与协议表示分离”原则，不复用其 agent 类型身份。需要工具往返形式的 Provider 在请求构造时生成本地保留命名空间的配对 call/result；这些仅是投影，不重新进入调度器。名称与真实注册的 `load_skill` 一致，call id 与真实模型调用去重，整个配对放在安全批次边界。

Chat Completions、Responses、Messages 和 native continuation 都需验证。native 增量上下文如无法安全追加该事实，使用现有可重建的 portable 路径或明确使输入失败；不能默默丢掉正文。新结果加入可变上下文，不修改稳定 System prefix，不伪造 provider 缓存或用量事实。

### 6. 持久化幂等不等于重复执行

用户加载 identity 从既有 input identity + 引用序号派生，模型加载关联真实 call identity；均限定在所属 session。相同 identity 与相同 payload 重送只返回既有结果；同 identity 不同参数/来源是冲突。用户再次主动输入相同命令得到新 input identity，允许新加载。

正文快照引用与上下文贡献在同一权威发布边界确认，Loaded 从该确认派生；用户发布还包含输入消费及整组成员。ToolEvent 的结算、Post Hook 与 UI 依照已确认事实推进，崩溃后先对账，不重做准备。不能照搬 `handle_bridge_tool_success` 先发 ACP Completed、后异步 push_tool_result 的现有顺序：Skill 成功行必须晚于权威发布。模型路径复用并扩展现有 conditional tool-result commit，校验 Surface revision 与实际完整请求预算；拒绝时提交合法错误结果，不能保留成功 UI。

若 UI 发布失败，恢复可重建行。若发布 ACK 不明，停止依赖执行，按 identity 对账；不得通过重读磁盘、重跑 Hook 或创建新调用猜测成功。已开始但只有准备快照、没有发布事实的操作不成为 Loaded。

恢复、导出、导入、fork、rewind 对已有事实做观察和投影，使用原正文及参数，不读取当前文件、不执行工具。缺失/损坏的被引用快照明确报错，不能以磁盘内容填洞。rewind 的可见事实及加载前提随既有因果边界一起截断，不遗留全局 loaded 标记。仍待消费的输入继续既有准入恢复；在途且未确认完成的操作只由当前执行 owner 处理，历史 replay 本身不重启它。

### 7. Loaded 的预算含义和 UI 含义一致

§3 的字节限制防止准备过程失控；完整请求仍使用现有上下文预算管理，计算原输入、附件、技能结果、工具定义、协议包装及输出预留。已有历史压缩只能在既有安全边界完成；不能在加载的 Step / ToolEvent 尚未闭合时启动压缩。准备后当前预算不足就明确失败，不另建暂停工具等待压缩的流程。不能用“完整正文在 blob，模型以后自行读”代替全文发布。

Loaded 表示完整内容已提交，不保证下一次模型调用必然成功。从发布到首次关联模型步骤获得已接纳响应期间，该组 Surface IDs 是请求的必需内容：pre-prune、前后台 compaction 和 native / portable 重建都必须保留全文，提交前与最终请求构造时复核。预算检查采用当前 revision / model / route，变化则重新检查，不能使用旧预算放行。初次请求的恢复重试也在此边界内；第一次请求发出本身不解除保护。首次成功消费由既有 request / response admission 事实与关联 Surface IDs 确认，不创建永久技能 pin 表或独立调度器。图片等已有合法投影可以更新用户项的 Surface identity；组关联沿既有来源关系跟随当前项，不能因旧 ID 消失而提前解除保护或遗漏后续撤下，技能正文仍须全文交付。

若首次消费前模型缩窗、协议投影失败、取消或请求终止导致无法继续：保留原发布与 Loaded 历史，明确记录后续执行未完成；用户组整体从当前 Surface 撤下，避免输入及正文进入后续独立请求。模型自主工具结果则替换为同 call id 的简短交付错误，保持真实调用 / 结果配对。复用已有 Surface replacement 机制，在现有事件与校验路径补齐加载结算原因及精确归属，不能冒用压缩或 rewind，也不能改动无关输入及其他工具结果。原始 Timeline 保留；撤下不能确认时停止采样，不能带残留正文继续。失败插话撤下后原任务可继续，原任务自己的普通终止规则保持。

首次关联步骤获得已接纳响应后，加载结果按普通历史参与压缩；后续取消不撤回已经被模型处理的工作。重启 / fork / rewind 根据已有发布、响应和终止事实恢复尚未消费的边界，纯 replay 不执行它们。新输入不得越过尚未结算的首次消费边界；当前执行 owner 必须先继续原步骤或明确终止并撤下。

主窗口复用 Skill 行：`Skill <name> · 加载中 / 已加载 / 加载失败 / 已取消`，附轻量来源“用户指定 / Agent”。展开复用正文浏览，展示实际来源、调用参数、提交快照或错误。显式 Skill 行不参加匿名数量合并，单行正文继续可折叠；不扩展通用分组器或新增面板。用户与模型调用均沿 ACP ToolCall / ToolCallUpdate 的现有运输和回放机制，以稳定 call / input 关联更新原行；ACP 展示事件不冒充 provider 工具调用。终态依据权威结果，不另加成功通知。发布后终止执行单独说明在所属输入 / 回合结果中，历史 Loaded 保留其提交含义。headless 流式消费中，实时 `ToolCall` + terminal `ToolCallUpdate` 与 Timeline 回放将二者折叠后的 terminal `ToolCall` 必须产生相同 Skill 结果；terminal ToolCall 的既有 `grow/tool` 身份及 `raw_output`（含 source / prepared_body）不可在映射中丢失，并沿现有 tool-result 配对与缓冲路径输出，不重执行或合成失败结果。

普通 `read_file(SKILL.md)` 显示 Read 文件；删除路径决定 Skill 类型的显示推断。normal、minimal、回放和 headless 从同一事实获取身份与结果，headless 使用结构化字段表达对应状态。初始化预加载不生成用户调用工具行。后续压缩可以正常改变上下文，历史“已加载”仅表示当时成功，界面不显示永久“当前生效”承诺。

Skill 结果沿现有 `ToolOutput::Dynamic` 的实际 serde 形状传输。显示、导出和回放用同一结果解析，不另造小写标签协议；跨层夹具直接序列化真实 `LoadSkillOutput → ToolOutput`，验证来源与已提交正文均可读取。

## Risks / Trade-offs

- Runtime 来源没有模型 call → 执行沿已有 Runtime Step / ToolEvent，新增最小上下文类型及合法投影，不借用虚假 response admission。
- 多项准备成功后其他项失败 → 整组未发布，保留诊断；先失败即停止派发，减少无效读取和 Hook。输入原文与正文都不会泄漏到下一请求。
- 发布后未完成首次消费 → 短期保护全文；终止时撤下当前 Surface、保留历史。涉及的权威写入失败仍关闭 owner，不能以 UI 提示代替结算。
- Hook、权限与自定义工具集存在差异 → 分别验证用户/模型来源和 child 能力上限；强制加载不构成 bypass。
- 历史加载与当前有效内容容易混淆 → Loaded 文案限定为事件结果，不做技能常驻面板；后续需要正文可再次加载。
- 与 `suspend-session-tree` 同时修改输入和 Timeline → 该 change 当前也是提案。若先落地，则复用其在途操作和挂起 owner：挂起不自动变 Cancelled，已提交结果不重放执行；若本 change 先落地，则把此事实纳入后续挂起集成。这里不实现会话挂起功能。
- 修改多条契约可能扩大实施量 → 各层只围绕一次加载闭环，目录整理、通用工具执行框架、其他恢复债务留在范围外。

## Migration Plan

1. 先明确复用 Runtime Step / ToolEvent 的执行顺序，落实输入组发布、模型结果 conditional commit、首次消费保护与 Provider 投影测试，再接入两个调用方。
2. 在同一变更移除运行时用户消息正文拼接和路径驱动的 Skill 显示，保留预加载用途的内容构造；更新开发者说明与相关注释。
3. 按 [验收矩阵](acceptance.md) 验证当前输入、队列、插话、权限、取消、存储故障、各请求协议和客户端回放。
4. 不设计旧双路径兼容开关或历史数据迁移。旧历史可沿已有合法记录显示，不能追认其为新的显式加载成功事实。发布回退应整体回退本次变更，不能仅回退显示层掩盖上下文差异。

本次不执行上述实现步骤。以上行为已按用户同意的 R1—R6 收敛；具体字段命名和现有 API 的组合在实现时确定，不能把一次输入发布拆成会泄漏部分正文的提交，也不能恢复已删除的 Hook mutation 或额外 Skill 生命周期。设计修订与复核记录见 [audit.md](audit.md)。
