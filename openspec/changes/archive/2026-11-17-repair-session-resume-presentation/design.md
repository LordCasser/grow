## Context

根因和代码证据见 [analysis.md](analysis.md)，范围见 [proposal.md](proposal.md)。用户已授权在一轮盲区复查后转入实现。调查基线为 `733d2c72e`，实施与验收进度见 tasks/verification。

已有所有者分别是：SessionActor/BehaviorCoordinator 持有当前控制；chat-state/Timeline 持有执行与用量事实；Shell HistoryPageCut 和 indexed projector 提供可验证的历史；Pager AgentView/ScrollbackState 管理展示。方案沿这些边界完成闭环，不引入第二个会话仓库或 token 账本。

## Goals / Non-Goals

必须使当前 Behavior、上下文占用、lifetime 消费在正文尚未加载时即有正确投影；使普通主 view 连续浏览历史、右侧时间线能定位未加载目标；使同一个挂起 Turn 的最终用量跨进程完整保留。Plan 的工具权限与 Normal 相同，规划中的调用目的严格限于研究和形成计划。

不更换 provider 计量格式、不另造持久化日志、不把历史恢复成模型输入、不改变 Goal/Workflow 业务状态或原审批身份。无证据表明有错的现有累计公式和幂等规则继续保留。

## Decisions

### 1. 当前控制随 load 完整 hydration，不依赖历史回放

SessionActor 仍是当前控制唯一 owner。冷恢复先验证/重建 Control，完成原恢复 gate 后读取当前状态；resident reconnect 直接读存活 actor，不以磁盘旧状态覆盖。Ctrl+D 的成功仍以原挂起 checkpoint/writer 退休确认作为边界。

在既有 load metadata 增加 `grow/behaviorSnapshot`，由 actor 提供：

```text
{ controlEpoch, controlRevision, behaviorId, planPhase?, planCompleted }
```

`planPhase` 仅 Plan 活跃时存在；completed plan 的正文/引用继续通过原详情查询，transport waiter 和本地确认倒计时不当持久业务状态。快照必须来自 load 末尾的同一有效 actor；客户端按 sessionId + bindingEpoch + controlEpoch + revision 接纳。epoch 只允许由当前 load 建立，不能从晚到历史页切换。相同 epoch 的旧 revision 不覆盖新状态；同版同值幂等，同版冲突视为错误。

Pager 将 load 快照与实时 CurrentModeUpdate 交给同一已确认状态 reducer。实时模式更新补齐相同 epoch/revision；历史页的 Mode/Control 仅生成历史展示，不进入此 reducer。load hydration 不伪造一次 Behavior 选择、不关闭新 intent、不运行 Plan 审批或 Goal restart。load 返回前到达的较新实时更新仍胜出。

替代方案：启动时无条件追加一条持久 CurrentModeUpdate，会混淆恢复与用户操作并产生重复历史；从最近页推导当前模式则必然受分页范围影响。故采用 load 的观察快照，保持已存在的实时更新通道。

### 2. Plan 审批约束工具用途，普通权限决定工具能否执行

移除 plan_mode_edit_gate 中根据阶段拒绝 Bash/Edit/MCP/Task capability 的分支。普通 allow/deny、权限审批、子 Agent 上限、Sandbox、工具目录和路径保护与 Normal 完全相同。Workflow/Goal 的互斥仍属于会话协议，不能用“权限相同”解除运行时所有权冲突。PlanControl 的提交、冻结、决定 CAS 和 amendment 协议不变。

同步替换 base、各 phase context 和拒绝提示中的全局 execution forbidden 文案，避免权限层与模型指令冲突。规划阶段的核心提示词约定如下，实施时可按仓库英文模板表达，但不得减弱语义：

> 当前处于 Plan 规划阶段。每一次工具调用都必须为形成、核实或修订待审批计划提供依据。可以读取源码、搜索、运行终端调查、检查环境，并做范围有限、可回收的实验来验证设计假设。
>
> 计划获批前禁止实施所规划的产品变更，禁止把实现包装为“验证”“准备”或“顺手修复”。不得通过 shell 脚本、文件编辑、MCP、子 Agent、其他会话或 Workflow 间接实施；委派调研必须携带相同限制。工具调用被普通权限批准不代表计划获批。
>
> 调研需要改变工作区或外部状态时，先判断它是否会交付计划中的产品变更。必要的临时验证应限制在隔离或可回收范围；不能以丢弃用户工作、提交/部署产品变更来完成调查。计划和实验记录可以写入项目指定位置。
>
> AwaitingApproval 时计划已冻结，不得以工具授权或新的调查结果自行进入执行。需要变更计划时沿既有审阅/修订流程；仅在审批结果进入 Executing 后实施获批范围。Amending 阶段同样只做修订所需调研，不继续未获批的新范围。

审批协议不能以命令字符串是否“看起来只读”自动分类：如编译和测试可能产生临时文件，`python` 既能调查也能实施。新增 shell 白名单会再次禁掉合法调查且容易旁路，因此用明确提示词约束用途，权限链保留实际授权责任。提示词是模型行为约束，不能宣称它构成对任意模型的强制沙箱；验证须包含模型代表性任务，不能仅断言字符串存在。

### 3. 主 view 使用有序的虚拟历史窗口

旧页进入同一主 transcript，移除逐页替换 viewer 的主浏览路径。AgentView 维护一个活动连续历史窗口、两端 continuation、source 到本地条目的映射和 pending jump；live tail 继续由原 tracker 持有。远跳到不相邻区间时明确显示未加载间隙，不把两个区间伪装成相邻历史。先实现一个活动窗口，不新建通用缓存框架。

Shell 的 cut 生命周期与该 view 的历史绑定一致，不能以 olderCursor 是否为空决定是否保留。cut 固定 session、selected branch、历史 frontier/hash 和 projection version；rewind/换绑使旧 cut 与请求失效。新 live 事件保持原 live cursor，并在截点之后接到 tail，不使用历史位置推进 live 去重高水位。

扩展现有 `grow/read_history_page`，由同一个 HistoryPageCut 校验以下请求：

```text
request:
  { sessionId, cut, query:
      { kind: latest }
    | { kind: before, cursor }
    | { kind: after, cursor }
    | { kind: around, target } }
response:
  { sessionId, cut, rows, range,
    olderCursor?, newerCursor?, targetSource? }
```

`cut/cursor/target` 为服务端签发的 opaque token，并签入边界角色与分片坐标。完整行 Before/After 边界排他；分片 Before 使用已交付区间 start 为上界，After 使用 endExclusive 为下界，不能换方向解释同一个 offset。Around 直接 seek 到目标并返回含目标的有界窗口，禁止从最新页一路读到目标。沿用已有 128 rows / 4 MiB page budget，actual range 描述已返回范围；目标附近不是加载整个 Turn 的隐含承诺。Tool 分片坐标是序列化 payload JSON 字节，不能当作可独立显示的原始输出文本。

Indexed publication 提供有界磁盘 seek。没有 publication 的来源在既有初始完整验证中捕获固定内存投影，随后窗口查询复用它，避免逐页重新扫描 journal；两种来源提供相同查询语义，但性能证据须分别记录。

为避免客户端猜测来源，每个 row 返回：

```text
source: { record, part }
entity?: Message{eventSeq,itemOrdinal} | Tool{toolCallId} | UserMessage{messageId}
fragment?: { coordinate: utf8_bytes | encoded_response_bytes | tool_payload_json_bytes,
             start, endExclusive, complete }
update: SessionUpdate
```

这是现有 source metadata 的显式投影，不新增持久 row ID。未分片按 source 去重，分片按 source + coordinate + interval 去重；重叠分片内容须一致；一个 canonical message 按范围组装，一个 Tool 按 toolCallId 保留唯一条目并按来源顺序重放更新，旧 progress 不能覆盖后来的 terminal。分页合并由历史投影 owner 执行，不能将多个独立 projector 生成的条目无条件拼接。

ScrollbackState 继续负责 EntryId、折叠、选择和布局；沿用 fresh_continuation 的 ID 空间、ScrollAnchor/bookmark。补页前记录稳定实体和逻辑行/sub-row，补页后恢复同一视觉位置，不以增加的行数粗调 offset。重读重叠窗口不重复正文、工具或完成行。历史 completion 仅显示原结果，不结算消费或结束 live Turn。

将触发条件收敛为“可见窗口接近未加载边界”：PageUp、方向键、半页/整页 action、滚轮合并 delta、滚动条和 turn 导航在同一 AgentView 边界检查后发请求。同一边界只保留一个 in-flight；错误保留 cursor 与可重试提示，既有内容和执行仍可用。around 跳转有独立 generation，后点击目标使旧跳转失效；被动补页不能覆盖新跳转。

Normal 与 Minimal 共用窗口协议与历史投影。Minimal 浏览历史时由既有渲染 seam 切到可重绘的虚拟 transcript 窗口，native 已打印输出仍是不可变快照；不能通过 reset committed/frontier 来“前插”。回到 live 时只提交尚未提交的实时输出，历史页不重新打印，不重复计划/工具结果。此 seam 是本次明确交付项，不能把 Minimal 留在独立逐页 viewer 后宣称整体完成。

Minimal 的具体入口采用 PageUp，Esc 返回 live；浏览屏用临时 alternate screen 与原 ScrollbackPane，进入和退出不重置 printed owner 或 native frontier。浏览期间暂停 native commit，实时事件仍进入原 tracker。退出后恢复原 viewport，仅提交这期间尚未打印的 live 内容。新权限或审批窗口优先于历史浏览；终端切换失败不得提前采用绘制状态。该临时屏幕是渲染宿主，不能持有第二份历史投影。

临时 alternate screen 的清理不能依赖最后一帧：同一批双 Ctrl+D 可能直接进入退出路径。提交 Enter 时须保留本功能的终端资源清理责任，正常 Leave 成功后解除；现有 writer drain 与 terminal teardown 在退出或错误时消费该责任，在 Minimal 的原生 cursor 恢复前离开临时屏幕。不为此增加新的 renderer 生命周期框架，也不向从未进入临时屏幕的 Minimal 会话无条件发 Leave。

首次恢复的 Latest 页是当前显示基线，其中尚未打印的实体交给既有 native commit 提交一次；不能在读页时提前全部标为已打印。这个选择复用 `history_latest_pending` 的首次成功采用边界。无论随后到达 Before/After/Around 还是重复 Latest，安装前已存在的同 EntryId 实体都保留 native 事实：已提交的仍已提交，pending 的仍 pending；仅后续读页中新引入的历史实体标为已提交。扫描位置停在前缀首个 pending 实体，避免页面采用后、首次绘制前的并发补页吞掉基线。无需增加单独的 native 账本或 renderer owner。

### 4. 时间线使用独立轻目录，点击走 around

增加同 cut 的 `grow/read_history_timeline`：

```text
request:  { sessionId, cut, limit,
            query: { kind: newest }
                 | { kind: before, cursor }
                 | { kind: around_ordinal, ordinal } }
response: { sessionId, cut, totalEntries,
            entries: [{ ordinal, source, messageId?, displaySegment?, preview, target }],
            olderCursor? }
```

limit 在服务端限制，preview 在现有派生索引发布时生成有界文本；请求不读取完整消息正文。目录枚举与当前 transcript 的可见 UserPrompt 定义保持一致，不能仅枚举 user-origin Turn；重复 prompt 文本按每次 message/source 身份保留。messageId 在原来源确实存在时返回，缺失仍以稳定 source 定位，不能伪造或按文本合并身份。同一消息的多个正文块只形成一个目录发生；coalesced 的多个可见 UserPrompt 共享实际物理 source，以 displaySegment 区分同一行内的显示段，不能把显示段序号伪造成 source.part。复用 user echo/boundary 索引技术，不能使用会文本去重的 prompt_history。

启用右侧 UI 时额外请求目录；按需加载目录页，rail 依据 totalEntries 和全局 ordinal 映射位置。未加载目录位置按 rank 查询附近目录页后获得 target，不能伪造本地序号。点击已加载目标复用本地 source 映射；未加载目标请求 around，成功并验证原 session/cut/window generation 后再移动 viewport，失败保留原窗口。live 新 prompt 以其稳定身份追加到目录 tail，与固定 cut 的目录去重。

### 5. 三类 token 口径分别有权威来源

| 数据 | 来源与留存 | 展示/使用 |
| --- | --- | --- |
| 当前上下文占用 | ChatState 当前 request projection、provider calibration | context bar；属于估算/测量投影，可随 compaction 变化 |
| lifetime 消费 | 既有 Timeline Usage 结算 + working publication 聚合 | session status、usage modal、预算所需消费 |
| 一次逻辑 Turn 消费 | 按原 prompt 归属的同一套结算事实派生 | 完成行、ACP/headless result；跨挂起不清零 |

metadata 中把当前 `totalTokens` 改为明确的 `contextUsedTokens`，同步所有生产者、Pager 和 headless 调用方；不保留两个不同语义的别名。累计消费只从已存在的 PromptUsage/SessionUsage 读模型取得，正文加载多少不参与账本计算。full input 含 cache read/write，累计总量为 full input + output；reasoning 已属于 output，不再相加。缓存分类未知保留已知桶及覆盖分母，不伪造为零；总消费未知才标记下界。

保留现有 `(request,attempt)`、`(sideband,attempt)` 和 child final bill 去重，失败/重试已知消费都计入；接纳正文和补载历史均不再次计量。模型/Agent 分项与 lifetime 共用同一事实。进行中的 attempt/child 尚未最终结算时，展示须标明“已记录消费/仍有待结算工作”；瞬时待结算状态来自原运行时 owner，不把未知消费填零，也不为此污染持久账本的 incomplete。child 的阶段性快照不能再与其终态累计 bill 相加。版本比较沿用既有 usage 投影规则并核对所有出口，不能只比较 token 数大小，否则不增加 token 的 incomplete/分类更新可能被丢弃。

### 6. 恢复原逻辑 Turn 的用量归属与结果留存

chat-state 负责活动 Turn 的用量恢复。持久归属使用既有稳定 `TurnId` 与 `prompt_id`，`captured_prompt_index` 仅用于位置一致性核对；rewind 后可复用位置，不能据 index 单独归属。主/aux attempt 与 child 结算均保存其实际准入/折叠时的原 owner；子账单只有实时 fold 已确认 attribute_to_prompt 时才有 Turn owner，否则明确 session-only。不能按“最后一个 prompt”猜测迟到 child 账单归属，也不能把恢复时的当前 prompt 当成原归属。已有事实能从 event/Turn 边界证明归属时复用；缺证据时保留未知而非位置推断。`last_turn_usage` 是最近单次模型响应，只在新真实调用后覆盖；恢复前保持 None，不能从 prompt 累计账本反推。

`UsageCheckpoint` 已进入既有 USAGE_CUT publication，直接扩展其可选 active_prompt（UsagePromptOwner、prompt_index、UsageLedger），覆盖边界复用 checkpoint 既有 frontier，不新增并列快照文件或 publication。恢复加载已验证投影再折叠未发布后缀。它是可重建投影，不是第二个账本，不为恢复结果新增一次全历史扫描。只有与有效 TurnContinuation 对应的活动账本可恢复；普通新 Turn 按既有准入创建新账本，不继承上一轮消费。

同一 Turn 跨 resume 后，挂起前后消费进入原 prompt 总计；resume segment 仍按实际结算时的 incarnation 分开，lifetime 不因此变化。原任务 output grant 的 task_output_usage 不是完整 token 账单，不能替代恢复。

修正后的终态快照经既有 freeze -> PromptTurnOk -> TurnCompleted 持久展示事实传播，补齐 Turn 完成索引的结果用量查询；历史窗口可随完成记录取出该结果，无需加载别的正文。查询只读，不能重投 completion、再计量或更改当时完整性。之后才结算的 session-only 后台用量继续只影响 lifetime，不回写已冻结结果冒充原轮已包含。

实现核对补充：现有 TurnCompleted 展示事实已进入按物理记录定位的 DISPLAY 索引，主历史页可有界读取它；没有独立的根 Turn 结果查询接口。本轮优先验收该事实中的冻结用量及其索引读页，不因文书中的“完成索引”另建结果账本或额外缓存。

无法从既有事实证明的历史归属明确不完整；不从可见正文估 token、不伪造精确值。schema/publication 版本按现有机制更新；只为完整性做必要转换，不构造双写或兼容账本。

所有结果格式保留 incomplete、未知分类与未知费用。streaming-messages-json 补齐其丢失的 completeness，未知 cost 使用 null/缺失而非 0；普通 JSON、ACP、TUI 同一结果的 token/完整性一致。完成行保存的是该逻辑 Turn 的结果，状态栏保存的是 lifetime，不能互相覆盖。

## Risks / Trade-offs

- 只把 older viewer 改成 prepend 会重复跨页 Tool/response → 先落实 source/fragment 合并，再接真实滚动。
- 远跳会离开 live tail，native 输出无法倒插 → 历史窗口与 live owner 分离，保留明确间隙和 native commit 高水位；验证返程。
- 对稀疏 prompt 目录做临时全扫会抵消分页价值 → 在已有派生索引发布时增补有界目录字段，按 rank/source seek。
- 当前状态快照与实时通知乱序 → session binding + actor epoch + 领域 revision，不使用历史 eventId 代替控制版本。
- 提示词不能构成绝对执行隔离 → 如实保持普通权限边界，使用真实模型任务验证 Plan 不直接实施；不把提示词校验宣称为强制沙箱证明。
- 原记录缺少 child prompt 归属 → 只有可验证归属计入原 Turn，其余显式下界；lifetime 不丢弃已知账单。

## Migration Plan

先完成控制 hydration 与 Plan 权限/提示词两个独立结果；历史先提供双向页/目录契约，再接合并与各输入入口，最后串联时间线与 Minimal。用量先补完整归属及活动投影，再修最终结果输出。各实现任务按一个 owner 的一个行为分别交付，具体顺序与验收见 [tasks.md](tasks.md)。

正文索引/working publication 为派生数据，变化按现有版本与验证机制重新生成或明确拒绝，不改写原 Timeline 消费事实。保留用户现有会话和 artifact；不以“默认不兼容”为理由删除历史。更名 metadata 和结果字段时同步全部当前调用方与文档。

## Review Plan

架构 owner 核对每项实际 diff、调用方、最终测试版本和源事实；按一个模块的一项行为分别交给 coder 实施。单项验收与整链里程碑分别记录。

集成主链：创建非 Normal 会话并产生至少三页历史和多来源用量 → Plan 调查/审批 → 工作中 Ctrl+D → 新进程 resume → 当前模式/上下文/lifetime 先正确 → 滚轮加载、时间线远跳/返程 → 原 Turn 完成且结果用量等于挂起前后结算总和 → 再次 resume 仍一致。另测 resident reconnect 不新增 segment、不替换 live owner。

最强反例：同一个 Tool/长 response 跨页，分页乱序与 live terminal 并发；同一个 Turn 在挂起前有主/aux/child 账单，恢复后无额外模型调用直接完成；零新增 token 的 incomplete 更新；相同文本的两个 timeline 目标；新行为控制先于旧 load/page 到达。这些场景必须阻止重复、回退、伪精确和跳错位置。
