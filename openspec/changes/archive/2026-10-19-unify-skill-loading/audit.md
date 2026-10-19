# 第二轮对抗性审查

日期：2026-10-18。审查基线：`4e3cccfa35ed93d6d78e08b5d58838bfaa57b8a2`，相比原提案基线新增的是会话树挂起提案，产品代码未因此变化。

以下保留第二轮审查时的六项发现；其中位置指向当时草稿。用户随后批准修订，当前设计层处理结果见文末“批准修订与再次复核”。原审查结论和反例保留，不能将设计修订当作产品实现已修复。

原审查结论：统一加载入口和复用 Skill 行的方向保留。当时草稿有三项 P1 设计缺口，以及三项 P2 边界 / 范围问题，暂不宜直接进入实施。需要补的是几个明确的提交与投影边界；无需新建通用加载框架或重做整个 UI。

## Findings

### R1 · P1：部分成功正文仍可能执行已经拒绝的输入

**位置：** `design.md:83`、`design.md:85`；`specs/input-admission/spec.md` 的 Multiple references with partial failure、Failed steering input 和 Later input after a failed prerequisite；验收 SK-10、SK-17、SK-18。

设计同时承诺“加载失败的输入不进入任务上下文”与“该输入里成功加载的正文仍对原任务可见”。这两条不能提供预期的失败隔离，因为参数本身会进入技能正文。

反例：运行中收到 `/writer 按新要求改写输出 /guard`，两个引用都已被目录识别，writer 成功而 guard 的文件读取失败。writer 使用 `$ARGUMENTS`，或者未使用参数占位符而走 `**ARGUMENTS:**` 附加。即使不发布原始插话，成功正文里仍含有“按新要求改写输出”；原任务继续时已经收到失败插话的指令。取消发生在第一项成功后也有同样问题。这是上下文内容的事实，不要求模型必然遵循该指令才能成立。

**代码证据：** `crates/codegen/tools/src/implementations/skills/skill.rs:238` 的 `apply_substitutions`，`:273` 插入整个 args，`:300` 在没有参数 token 时追加 args。普通正文与用户任务参数没有可用于安全拆分的语义边界。

**最小修订建议：** 用户同一输入的多个加载项先准备和校验，所有必需项成功后，再把该输入和独立结果作为一组发布到模型 Surface。失败或取消保留原输入、逐项诊断及已准备快照作为历史，不发布其可执行正文。不需要回滚已执行的外部 Hook，也不需要通用事务框架；边界限定为这条输入的上下文发布。UI 在该发布成功前不把已准备项称为 Loaded。模型自主工具调用仍按普通工具结果处理。

**补充验收：** 第一项分别采用参数内联和参数后缀，第二项失败 / 取消；捕获原任务下一次请求及后来新输入的请求，确认失败输入的参数和正文均未进入 Surface，历史仍能说明每项准备结果。

### R2 · P1：Loaded 的全文保证没有覆盖第一次请求前的裁剪窗口

**位置：** `design.md:101`、`design.md:107`；`specs/model-sampling/spec.md:20`；验收 SK-27、SK-28。

当前设计把正文提交与“下一次请求能收到全文”绑定，但只安排了加载阶段预算检查。工具结果提交后，第一次使用它的模型请求前仍会发生 pre-prune、异步压缩发布、模型切换和请求重建。它们不是“技能已使用之后的历史压缩”，不能简单归入 Later compaction。

反例：正文加进上下文时尚未超过模型窗口，但达到自动压缩阈值；该技能结果又超过单项 pre-prune 预算。在下一次采样前，正文被头尾裁剪。UI 已显示 Loaded，模型第一次看到的就是截断正文。模型路径是普通 ToolResult、用户路径是新 Runtime item 时，还可能出现同一入口的两种保留策略。

**代码证据：**

- `crates/codegen/shell/src/session/actor/compaction.rs:2640` 的 `maybe_pre_prune_at` 采用单项预算和总阈值。
- `crates/common/compaction/src/prune.rs:122` 的 `plan_tool_result_pruning` 只判断工具结果和大小，不区分刚加载且尚未被采样的内容；`:150` 起按顺序选取超限项。
- `crates/codegen/shell/src/session/actor/turn/mod.rs:1026`、`:1042` 在后继请求前处理模型切换压缩与后台压缩发布。
- `crates/codegen/chat-state/src/actor/mutations.rs:304` 已有 `push_tool_result_conditionally`，在 Surface owner 处复核 revision 和 headroom，可作为复用入口；它本身不提供首次请求前的保留保证。

**最小修订建议：** 明确“首次关联请求”边界，将新结果及用户输入的整体预算在权威 Surface 提交点复核；在首次请求构造时，校验这些结果仍被完整投影。首次请求前的 pruning / compaction 不能裁剪它们，放不下则明确终止该输入的执行。保护基于已有 Surface identity 和请求边界，不建立永久技能 pin 状态。相应补一条窄范围 `context-compaction` delta，并写清模型 / 路由切换后不能满足全文条件时的结算方式；不要把已确认的历史提交改写成“从未加载”。

**补充验收：** 刚加载成功尚未采样时触发 pre-prune、异步压缩发布和切换较小窗口模型，分别检查用户与模型来源。首次请求要么包含全文，要么明确停止，不能带截断正文继续。

### R3 · P1：用户加载尚未接上合法的生命周期，且漏评了已有复用路径

**位置：** `design.md:60`、`design.md:66`、`design.md:70`、`design.md:89`；tasks 2.2、2.4、3.1。

设计要求用户加载遵守 PreToolUse / PostToolUse、没有前置模型采样、不伪造模型 step，但没有确定其真实开始、结束以及 Hook cause 由哪条已有生命周期承载。仅新增上下文结果类型并不足以让当前工具门控合法运行。

**代码证据：**

- `crates/codegen/shell/src/session/event_tracker.rs:340` 的工具开始要求 active turn 和 active step。
- `crates/codegen/chat-state/src/timeline.rs:4768` 要求 PreToolUse 指向 open tool，PostToolUse 指向已经关闭的 seen tool；不是只给一个稳定字符串就能通过。
- 已有更轻的复用先例：`crates/codegen/shell/src/session/actor/tool/dispatch.rs:339` 的 `handle_direct_bash_command`，在 `:397` 起创建无 provider 请求的 Step / ToolEvent，并在 `:414` 起发布 ACP 工具行。`timeline.rs:5287` 的 Step 校验也不要求它必须含模型请求。因此“没有模型 tool call”不等于“必须新建整套执行生命周期”。
- 原始输入也不必先进入模型 Surface：`crates/codegen/shell/src/session/actor/input_admission.rs:1453` 的 `complete_unmodeled_fifo_inputs` 使用现有 Handled 事实。相反，已被 turn 预留的输入不能直接走 Dismissed（`timeline.rs:5015`）。
- 现有通用结果路径不能整段照搬：`crates/codegen/shell/src/session/actor/tool/result.rs:369` 先发送完成 UI，`:498` 才异步推入普通 ToolResult。这不符合本 change 的“确认上下文提交后 Loaded”。

**最小修订建议：** 明确用户加载在现有 turn 中的真实 Runtime Step 执行，复用 ToolEvent、HookCause::Tool 和 ACP 行；该 Step 不产生虚构的 Request / provider response / usage。失败输入使用合法的未送模型结算路径，成功输入在独立正文提交后进入模型执行。插话先闭合原模型工具批次，再安排同类 Runtime 执行边界。只为 Runtime 来源的上下文表示、必要来源信息和提交确认补类型；不要另建 Skill job 生命周期。Skill 的成功 UI 必须在已确认上下文结果之后发布，不能沿用当前提前 Completed 的调用顺序。

直接 Bash 的上下文最终写成用户消息，不能照搬；这里建议复用的是执行因果关系与 UI 运输，不是它的正文表示或命令执行授权策略。

**补充验收：** 从无 active step 的首条技能输入开始，真实执行 allow / deny / cancel 全链路并验证 Timeline fold；Hook cause 均合法，失败输入结算不会重复入队，首次模型请求之前没有 provider usage，ACK 丢失时不能出现成功 UI。

### R4 · P2：首次准入前目录变化会丢失显式加载意图

**位置：** `design.md:50`、`design.md:52`；`specs/input-admission/spec.md` 的 Builtin and ordinary slash text、Selected entry removed while queued；验收 SK-12、SK-14。

当前稿处理了“持久接纳之后来源消失”，没有处理“用户已从命令选择器选中技能，但请求到 Shell 前目录变了”。这时还没有 frozen reference。继续按当前文本解析，会把用户明确选中的加载降级为普通 prompt；如果裸名重新指向别的来源，还可能加载另一个技能。

**代码证据：** `crates/codegen/pager/src/slash/acp_command.rs:183` 起只发送原始 `/name args`；`crates/codegen/pager/src/app/root/dispatch/queue.rs:563` 只附加显示元数据。`crates/codegen/shell/src/session/slash_commands.rs:1058` 只收集当前目录里能匹配的引用，`:1232` 起无技能命中最终返回普通 prompt。

**最小修订建议：** 命令选择器已经确定的技能调用，应在现有输入 payload 中携带显式调用意图和所选来源标识；Shell 按真实目录复核，陈旧或消失时明确失败。不能把 displayAsSkill 直接当成授权证据，也不应把所有未知 `/word` 都改成错误。原始普通文字路径与显式命令路径只在意图证据上区分，最终仍进入同一个 loader。

**补充验收：** 命令被选中后、首次 RPC 接纳前删除 / 禁用来源，或新增同名冲突。应显示显式加载失败，不启动普通任务，不替换来源；纯文本里的未知 slash 保留既有行为。

### R5 · P2：Hook 参数 / 结果改写属于没有当前依据的设计范围

**位置：** `design.md:66`、`design.md:89`；`specs/tool-authorization/spec.md:29`；验收 SK-16。

设计以“现有 Hook 如可改写”为条件，随后在 delta 和验收中把合法改参、结果正文修改变成了要支持的路径，还持久化请求参数与有效参数两个版本。当前工具 Hook 没有这些行为；实现者可能为满足验收而扩展 Hook 协议。

**代码证据：** `crates/codegen/hooks/src/result.rs:5` 的 HookDecision 只有 Allow / Deny；`crates/codegen/shell/src/session/actor/tool/preparation.rs:709` 取决策后只处理拒绝。`crates/codegen/shell/src/session/actor/tool/mod.rs:662` 先确认工具完成，再在 `:722` 以 Observe 派发 PostToolUse，不是结果改写器。

**最小修订建议：** 删除 Hook 改参、重定向和改写正文的新增路径及验收；按现有 Allow / Deny / Observe、失败策略和取消语义复用。只保存实际调用所需参数，名称解析与模板替换仍按现有规则处理。Shell 已有的路径显示转换等输出处理要核对，但不借此引入 Hook mutation 能力。

**补充验收：** 将 SK-16 收敛为 Hook 拒绝、失败策略、取消、同身份恢复不重跑及 PostToolUse 顺序；不为不存在的合法改参制造测试接口。

### R6 · P2：展开后才检查预算，不能限制展开过程的内存增长

**位置：** `design.md:60`、`design.md:64`、`design.md:107`；验收 SK-07、SK-27。

提案正确区分了 1 MiB 文件读取上限和展开后大小，但执行顺序仍是完整展开后再检查上下文预算。重复占位符会在预算拒绝前产生大对象；多项预备内容会进一步累加。文件上限和下一请求 token 检查都不能限制这一阶段。

**代码证据：** `crates/codegen/tools/src/implementations/skills/skill.rs:258` 使用 `replace_all`，`:275` 为每个 `$ARGUMENTS` 复制整个参数，`:298` 接收完整结果后才返回调用方。约 10 KiB 的模板含 1,024 个 `$ARGUMENTS`，若参数为 256 KiB，准备结果约为 256 MiB；本轮没有执行这种大分配。

**最小修订建议：** 保持单次替换语义，先用 checked length 估算或在有界输出构造中限制准备内容；单项和输入聚合使用已有合适预算，没有时只增加本入口必要的明确界限。超限在大分配前失败，不能新增通用资源管理服务。准备过程的取消检查也不能只放在完整替换之后。

**补充验收：** 使用小测试预算和重复占位符，证明准备在超过界限时及时失败、不会产出部分 Loaded，取消可结算；无需在测试中真的分配数百 MiB。

## UI 复用与范围收敛

UI 没有发现必须推翻现有交互的原因，以下属于实施路径收敛，不另列缺陷：

- 当前 Read 块的 Skill 标题来自文件路径，另有 `OtherToolCallBlock` 包装的旧 Skill variant。新操作需要明确身份映射，但可以继续复用折叠和正文浏览。证据：`crates/codegen/pager/src/scrollback/blocks/tool/read.rs:105`、`crates/codegen/pager/src/acp/tracker.rs:2606`。
- 现有分组只累计调用数和失败数，不能保留各技能名称。最小办法是显式 Skill 行不参加匿名数量合并，仍允许单行折叠正文；先不扩展通用分组器或创建新面板。证据：`crates/codegen/pager/src/scrollback/state/verb_group.rs:314`、`:367`。
- 用户触发的 ACP 工具行可以复用现有运输机制；ACP ToolCall 是 UI 事件，不等于必须存在 provider tool call。持久事实、UI 事件和 provider 请求表示的身份要关联，不能互相冒充。
- 四个主要展示状态可以保留。正文准备完成但尚未发布时继续 Loading；提交未知使用既有错误 / 不确定反馈。无需增加永久 active skill 注册表。
- 七个 capability 的跨度本身不是过度设计：输入、持久化、协议和显示确实都受影响。应减少重复生命周期与假设能力，不能通过删掉失败 / 回放验收来减少文档数量。R2 所需的 context-compaction 补充应保持窄范围。

## 建议的收敛方向

复用 SkillManager 和正文准备规则；用户显式意图与模型调用进入同一个 loader。执行因果关系优先复用已有 Runtime Step / ToolEvent / Hook / ACP 路径。用户多技能输入先准备，再统一发布原输入与独立正文；发布失败只保留可查历史。成功 UI 跟随权威提交，首次模型请求前保证完整投影。文件读取继续是 Read，Skill 行不被匿名分组合并。

这仍需要一个 Runtime 来源的上下文表示，但不需要第二个目录、Skill job 系统、Hook 改写框架、独立 UI 运输或永久技能 pin。

## 验证与限制

- 使用 Atlas MCP 的 project(open) 与 scoped search；结果只证明局部结构范围，关键判断均另读当前源码确认，未进行全仓索引。
- 一轮 Luna 子 Agent 只读核对 UI 分类、分组和 tracker；主 Agent 核对加载、Hook、Timeline、输入及上下文边界，并复核直接 Bash 的 host 工具事件先例。
- `openspec validate unify-skill-loading --strict --no-interactive` 通过。全量校验结果见 [verification.md](verification.md)。
- 未运行 Rust、真实 Provider 或 UI 测试；上述反例是源码与契约推演，不冒充运行复现。
- 第二轮审查当时保留 proposal / design / delta / acceptance 原文，只新增审查记录，并登记待修订状态。后续批准修订的结果如下。

## 批准修订与再次复核

2026-10-18，用户批准按 R1—R6 修订，要求优先复用现有管线、删掉过度设计，仍不实现。本次同步修订 [proposal](proposal.md)、[design](design.md)、八个 capability 的 delta、[tasks](tasks.md) 和 [acceptance](acceptance.md)。六项在**设计层关闭**；全部运行验收仍未执行。

| 发现 | 设计层处理 | 契约与验收 |
| --- | --- | --- |
| R1 | 同输入按序准备、首败停止；原输入与独立结果一次发布。失败或发布前取消不把任何参数 / 正文放入 Surface，已准备项保持 Loading 至整组决定。 | design §4、§6；input-admission、session-timeline、configuration-rules、client-surfaces；SK-10、SK-18、SK-19、SK-35、SK-40。 |
| R2 | 权威发布及请求构造复核完整预算；从发布到首次关联响应被接纳期间保留全文，包括恢复重试。不能满足时明确终止，保留 Loaded 历史并撤下未消费用户组，或将模型结果换为同 call id 的交付错误。 | design §7；model-sampling、新增窄范围 context-compaction delta、session-timeline、input-admission；SK-27、SK-28、SK-36、SK-41。 |
| R3 | 复用真实 Runtime Step、ToolEvent、HookCause 与 ACP；FIFO 失败用 Handled，未消费 steer 用 Dismissed / Invalidated。发布确认先于 Skill 成功 UI，输入消费与组发布不拆成多次提交。 | design §4—§6；session-timeline、tool-authorization、client-surfaces；SK-16、SK-21、SK-22、SK-37、SK-40。 |
| R4 | 选择器携带明确意图及选中来源，Shell 首次接纳和执行前分别复核。陈旧选择明确失败，不退回普通任务、不换来源；普通 slash 文本保持原路由。 | design §2；input-admission、extension-runtime；SK-12—SK-14、SK-38。 |
| R5 | 移除 Hook 改参、重定向、正文改写及双版本参数。只复用现有 Allow / Deny / Observe、on_failure、取消与 occurrence 恢复事实。 | design §3、§4；tool-authorization；SK-16；tasks 2.4。 |
| R6 | 单项与同一用户输入聚合的准备正文均限制为 1 MiB；checked length 后有界构造，包含参数后缀、溢出与分片取消。请求 token 预算单独复核。 | design §3；configuration-rules；SK-07、SK-27、SK-39；tasks 2.1。 |

UI 保留现有单行折叠、展开及正文浏览，显式 Skill 行退出匿名数量合并。没有新增分组器、面板、独立运输、Skill job 或永久技能状态。正文准备、输入消费、模型 ToolResult 提交、Surface replacement 和正常 Hook 生命周期均有现有复用入口；只补该加载闭环所需的类型、关联和校验。

再次复核覆盖发布前失败、发布 ACK 不明、发布后首次消费前终止，以及已有图片投影更换用户项身份的情况。最后补充两处澄清：无需前置任务模型决定加载不影响现有权限辅助调用；用户项合法换 ID 后按来源关系保持组关联，终止结算只处理所属组、不改无关输入或工具结果。首次消费边界统一指向已接纳响应，而不是“请求已发出”或笼统的步骤完成。

一轮 Luna 子 Agent 对当前设计、delta、tasks 与验收作了限定范围只读一致性复核，未发现必须修正的剩余矛盾；主 Agent 另核对现有 Timeline、Hook 和 Surface replacement 源码边界。未运行 Rust、Provider 或 UI 测试，没有把推演记成复现。结构、场景保留与全量静态校验结果见 [verification](verification.md)。
