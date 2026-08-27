# Agent Core Timeline Architecture

> Current Timeline schema is v20. ImageProjection now binds every ordered
> `Reasoning` and `BackendToolCall` carrier from the paired provider response
> and irreversibly replaces each with protocol-neutral text. Older schemas are
> rejected.

Grow 的会话核心以不可变 Timeline 为唯一事实源。模型上下文、用户 transcript、诊断视图与持久化缓存都是 Timeline 的投影；压缩、修剪、目标模型图片投影、回退和系统提示变化只能追加事件或切换投影，不能改写已接受的事件。

本文约束 `crates/codegen/chat-state`、`crates/codegen/shell/src/session` 与会话存储之间的边界。采样协议、foreground 调度、Behavior、权限和 Workflow 保持各自控制面 owner；它们产生的可恢复事实进入 Timeline，瞬时协调状态不进入。

## 分层与依赖方向

```text
Shell turn/runtime
        |
        v
Context assembly -----> Sampling
        |
        v
Surface projection
        |
        v
Timeline -------------> Timeline persistence
```

- Timeline 只拥有有序事件、事件身份、Surface fold 与结构校验，不依赖 shell、TUI、provider 或具体持久化后端。
- Context assembly 从 Surface 生成 `ConversationRequest`，可以对请求副本做临时的图片逐出等 wire 限制，但不得改变 Timeline。
- Shell 追加 turn、step、request、message、tool、compaction、recovery、governance 与 routing 事件；它不能直接替换历史数组。
- Timeline persistence 只追加已接受事件。不存在消息快照、投影检查点或另一条历史恢复链。
- 恢复边界只传递 Timeline events；不得同时携带一个预折叠 Surface。稳定 system head 只允许在 Timeline seed 中出现一次；client attach、模型切换、Agent/Behavior 切换、memory 与压缩均无权替换它。
- UI 更新与本地诊断是 Timeline 的消费者，不是恢复会话的第二事实源。

## 核心类型

每条事件具有单调连续的 `seq`。`seq` 在接受时由 Timeline 分配，调用方不能指定或复用。

Turn、request、tool 与 compaction 的 correlation id 在整个 ledger 生命周期内不可复用。Turn id 是独立的 64 位不透明身份（JSON 中编码为字符串），不能拿可回退的 `prompt_index` 充当，也不能作为 JavaScript number 传输。

```text
TimelineEvent { version, seq, at_ms, kind }

SurfaceOp = Append | Replace { start, end, shadowed }
```

消息类事件携带一个或多个完整 `ConversationItem` 与 `SurfaceOp`。普通用户、assistant、工具结果和 typed memory 使用 `Append`；压缩与受约束的 Surface 变换使用 `Replace`。非消息事件不能携带 `SurfaceOp`。schema v20 允许 `control` 事件额外携带至多一个受类型约束的 synthetic user `model_context`：Idle 时立即进入 Surface；active turn 中先进入 pending projection，并在该 turn 的 durable `TurnEnded` 后按因果事件顺序激活每层最后一个。状态与协议仍属于同一事实，且不会把 user 项插进 tool call/result 或排到旧 turn 的迟到输出之前。v20 保持 notification owner 与 producer retry identity 分离，并提供零 Surface 的 typed `Dismissed` 终态：owner 仍保存在首次 `Received` 中，重试 id 只由稳定 source identity 计算，按所有权策略禁止打开模型 turn 的 receipt 仍有可审计的持久终态。其余非消息事件不进入 Surface。闭合事件族为：

| 事件族 | 结构约束 |
| --- | --- |
| `turn` | 同一时刻最多一个 active turn，且恰好一个终态 |
| `step` | 从属于 active turn；关闭前 request/tool 必须闭合 |
| `request` | request id 唯一；started 后才能 first-token/retry/terminal |
| `tool` | call id 唯一；started 后才能 completed，名称必须一致 |
| `workflow` | run id 唯一；spawn 从 epoch 0 开始，resume 严格递增；每个 execution 只有一个 end，暂停/失败可恢复，完成/取消/中断永久关闭；无 active execution 时只能显式 close |
| `compaction` | started、唯一 summary 与 completed/failed 构成事务；成功路径的唯一 replacement 位于 summary 与 completed 之间 |
| `image_projection` | Surface 投影事实；精确绑定触发 runtime、提交前 Surface revision、live SurfaceId、图片 fingerprint/count、可验证的 description Sideband result ref、关联 Assistant tool call 以及同 response 的有序 `Reasoning/BackendToolCall` carriers；接受后推进 Surface revision 并不可逆改变所有模型可见载体 |
| `recovery` | 只追加中断修复意图和依据，不改写物理尾部 |
| `observation` | 治理、权限、MCP 等 log-only 事实，不参与 Surface |
| `notification` | 唯一 durable 通知 inbox；`Received` 创建通知并引用 content-addressed payload，`Consumed`/`Dismissed` 只能解析尚未解析的 receipt，状态转换幂等且不可回退 |
| `control` | Agent/Behavior/Goal 原子快照；revision 必须严格递增；可选 `model_context` 必须匹配 typed layer，Idle 时原子追加，turn 内则在终态后按因果事件顺序激活每层最后一个 pending context |
| `subagent` | 父 Timeline 记录 spawn/end；end 精确引用 child result |
| `subagent_seed` / `subagent_result` | child Timeline 记录唯一 seed-source 和唯一终态结果 |
| `session_title` | 标题事实；用户标题可覆盖自动标题，自动生成或 fallback 永远不能越过用户标题 |
| `sideband` | 旁路实体生成点；冻结输入引用必须早于 spawn，事务细节进入独立 Sideband Timeline |

`Replace` 的 `start`/`end` 引用当前 Surface 节点身份，不是 `Vec` 下标。接受替换前必须验证：

1. 两端节点都仍在当前 Surface；
2. `start` 不晚于 `end`；
3. `shadowed` 恰好覆盖被遮蔽的 Surface 节点，无重复且全部早于新事件；
4. 工具结果裁剪可以在一条事件中覆盖完整 Surface，但只能改变 `ToolResult.content`；项目数量、顺序、非工具项目、`tool_call_id` 与图片必须原样保留；
5. 事件验证失败时 Timeline、Surface 与持久化队列都不变化。

Actor 同时维护唯一的 `surface_revision`，它只在消息 append/replace、ImageProjection、Idle Control context append，或 `TurnEnded` 激活每层 pending Control context 时推进；纯生命周期、纯 Control snapshot、尚未激活或被同层后续 Control 覆盖的 pending context 与 observation 事件不会推进。凡是在 actor 外读取 Surface、异步计算后再提交的完整变换，都必须携带读取时 revision 做 compare-and-swap；revision 已变化则整次变换失败，不合并、不覆盖后来消息。图片描述在 actor 外计算后以 source revision 和 stable SurfaceId 提交 ImageProjection；typed memory append、修复与 pre-prune 变换在串行命令循环中提交，不经过外部 read-modify-write；system head 没有运行时 mutation API。

Timeline 接受的消息是完整原子记录。采样流的 token/delta 只属于 sampler 到实时 UI 的传输链，不进入 Timeline。

`turn/started` 同时记录 typed identity、可回退的 `prompt_index` 与原始 `prompt_text`；`turn/ended` 记录 stop reason 与 completion kind；`compaction/started` 记录发生压缩时的 `prompt_index`。它们是从 Timeline 恢复 prompt 预览、当前分支、Goal finalization receipt 和压缩边界所需的因果元数据；恢复不得再扫描 UI 更新流猜测这些值。

## Durable notification inbox

`SubagentCompleted` 的 durable schema 为 `subagent_id + owner`；`owner` 在 admission 时从不可变的 `SubagentRequest.owner` 映射：`Goal { goal_id }` 保留该 goal，`Task` 与 `Workflow` 使用 `Session`。owner 是 receipt 元数据，不参与 producer identity 或 dedup key，同一 source version 的 retry 必须沿用首次 admission 的 owner。

所有需要跨线程、跨 turn 或跨进程恢复的后台事实只有一条承载机制：Timeline 中的 `notification` 事件族。生产者写入 `Received`，消费方写入 `Consumed`；当一个通知按明确的策略不应唤醒模型时，写入 `Dismissed`，并记录原因。这三种事件以通知 id 做幂等边界，不能靠内存 buffer、UI replay、隐藏 prompt 或独立 reminder 队列表达同一事实。

通知正文写入 content-addressed payload，事件只引用其摘要、类型和 payload ref；读取时必须验证 hash，避免重复复制大型输出并保证 Trajectory 能重放当时的确切内容。生产者与通知类型的唯一映射是：bash/task terminal 写 `TaskCompleted`，monitor 的阶段进度写 `MonitorProgress`、终态写带 monitor 语义的 `TaskCompleted`，task/loop subagent 写 `SubagentCompleted`，workflow 从完成命令携带的精确 terminal manifest snapshot 格式化结果 payload 后写 `WorkflowCompleted`。terminal 生产者使用 acknowledged delivery，只有 Timeline `Received` 的 durability barrier 返回后才结束发布；ack 丢失时以同一 source version 重试。Workflow 用不会随 manifest 投影变化的 `execution_epoch + terminal status` 作为 terminal source version（同一 epoch 的 resumable Ended 与后续 Closed 是两个边界），session restore 会从 terminal manifest 重放同一身份，所以 Timeline 已落盘而 mailbox 尚未 ACK 的进程崩溃不会丢失或复制提醒。重放先查询完整 Received fold（包括已 Consumed 的收据）；身份已存在时直接视为 admitted，不再解析当前 read-tool alias/path 或重写 payload，配置变化不能把历史收据变成 payload conflict。通知的生产者 identity 由类型、subject、task kind 和 source version 组成，owner 是首次 durable admission 固化的 receipt 元数据，不参与 retry identity；因此 actor 重启后即使内存 owner 投影已经消失，重试仍解析到原始 Goal-owned receipt，不能变成第二条 Session-owned 通知。优雅关闭在最终 persistence barrier 之前把仍在运行的 bash/monitor 快照写成 `TaskStillRunning`，其 opaque checkpoint epoch 允许同一任务跨多次恢复重复形成互不冲突的事实；同 task 的 terminal receipt 会淘汰全部 pending checkpoint 并回收 payload，之后乱序到达的 checkpoint 也不会重新进入 pending 投影。旧 `background_tasks_manifest.json`、恢复时读删 sidecar 和隐藏 system reminder 已全部删除。scheduler fired 只更新 UI；实际执行由 subagent 完成后通过上述通知进入 inbox，不再以 scheduler 事件直接唤醒主 agent。

通知在 active turn 的安全采样边界消费，作为该 turn 的输入事实；进入一个新 turn 时，尚未被该 turn admission 接管的 receipt 会在首个用户输入提交前消费，保持恢复上下文先于新输入。工具结果边界使用 `input=None` 读取并确认已消费的通知，不能把通知正文伪装成用户消息。idle admission 先处理用户 FIFO，再处理可自主唤醒的 durable notifications，最后才允许 Goal continuation。`TaskStillRunning` 只为下一个真实 user/Goal/completion turn 提供上下文，单独存在时绝不花费一次模型调用；若同时存在 terminal receipt，则合并进该次必要的 notification turn。Goal-owned shell/monitor 工作在 tool batch/child admission 时捕获不可变 `goal_id`，terminal 到达时只投影该 owner，不重新读取当前 Goal；重启后的 terminal receipt 可从 checkpoint 继承 owner，而 retry 则由原始 Timeline receipt 保持身份。与当前 active Goal epoch 匹配的 receipt 留在 durable inbox，由下一次 Goal continuation 原子消费；新 Goal 不得清除旧任务的 admission owner，旧 epoch 的终态因此仍按原 owner 落盘，但不会被错误注入新 Goal。SubagentCompleted 与 WorkflowCompleted 不受 Goal task autostart 抑制。monitor 的 progress 在收到 terminal 后折叠，避免重复展示大量无意义中间行。

reminder 文本只引用当前 Agent 实际注册的输出/轮询/读取工具名称。能力是 `Option` 而不是默认字符串：没有 poll/output tool 时，task/monitor 终态以内联且有界的真实输出闭合；没有 Read tool 时只报告真实 artifact 路径并明确当前不可读取。任何路径都不得发明 `get_task_output`、`Read` 或其他不存在的恢复指令。

Trajectory 展示 `Received`、`Consumed` 与 `Dismissed` 的完整通知事实、payload ref、生产者和消费/抑制原因；它不从 stream/phase/UI 更新推断通知，也不将通知渲染成第二条历史消息。

## 三种读取投影

- `surface()`：折叠 `Append` 与 `Replace` 后的当前模型消息序列，是上下文组装的唯一历史输入。
- `branch_transcript()`：沿当前 rewind 分支读取 append-origin 消息，展开压缩与 Surface replacement 的 shadow，但不混入已经被 rewind 丢弃的其他分支。
- `events()`：完整事件账本，供恢复、Trajectory、审计与分叉读取。
- `rewind_surface(target)`：展开压缩和修剪的 shadow，沿最后一次 `Rewind` 选择出的分支重建未压缩历史，再在 `target` prompt 前截断。已接受的图片 shadow 会在新 Surface 生成前再次投影，模型不能通过 rewind、fork、resume 或切换模型恢复图片；原件只保留为不可变 Timeline 证据。

三种投影共享同一个 Surface 状态转换实现。任何消费者都不得独立重写 replace 语义。

## Turn、工具与恢复不变量

- 每个 turn 恰好一个持久化终态；终态提交失败时作用域保持打开，重试同一终态不会产生重复记录。
- 每个 assistant 工具调用最终恰好一个工具结果；真实、拒绝、取消、未开始和结果未知都属于结果。
- 模型请求前、工具执行前与 step 边界使用 fail-closed 持久化检查点。
- 需要改变 Surface 的显式修复、工具结果 pre-prune、ImageProjection 和 rewind 采用 `prepare → durable append → accept`。图片能力降级只提交 ImageProjection 投影事实。持久化失败时内存投影不变，调用方也不能看到成功。
- 崩溃恢复只追加确定性的关闭事件，不修改物理尾部。未记录工具开始时生成 `not-started` 结果；已记录开始但没有结果时生成 `outcome-unknown` 结果。
- steering 继续由 shell 的 `expected_turn_id` 栅栏治理；Timeline 只记录被接受的输入与结构化终态。

## 压缩与缓存

压缩只有一条提交链：先 durable append `compaction/start`，再从此刻的 Timeline 原子物化冻结 `input_ref + surface_revision + Surface + SurfaceId[]`。选择器保留最近因果 prompt turn 和约 16% context window 的原文尾部，只选择一个闭合的旧范围；后续压缩会把紧邻最早待压缩 turn 的上一条 `CompactionMeta` 一并纳入范围，形成单个滚动摘要，而不是在 Surface 永久累积摘要层。单个超长轮次则只选择其中较旧且已闭合的 response/tool group，边界不能落在 tool call/result 或 `Reasoning/BackendToolCall/Assistant` 组中间。摘要调用必须创建 `purpose=compaction-summary` 的 Sideband，并以同一 `input_ref` 作为输入。Sideband 成功写入 result+end 后，主 Timeline durable append 唯一的 `compaction/summary`，其中保存冻结输入引用、精确的单事件 Sideband result 引用、`SurfaceRange {start,end,shadowed}` 和 token/字符计量；摘要正文只存在 Sideband result，不在主 ledger 复制第二份。只有完成这一步，actor 才接受唯一一条 `MessageCause::Compaction` 局部 replacement，未被选择的 prefix/tail 保持原 ID 与原文，最后写入 completed。通用 `replace_all/replace_range` 明确拒绝 `MessageCause::Compaction`；压缩只能通过带稳定目标的 `replace_compaction_range` 提交。

Timeline 校验器强制 summary 所引用的 Sideband spawn 已存在、purpose 与 input ref 完全匹配、result ref 是单事件引用，并在 summary 落账时验证完整 shadow 集合仍恰好覆盖当前 Surface 范围；replacement 的 `start/end/shadowed` 必须逐项等于该 summary target。无 summary、范围漂移、重复 summary 和重复 replacement 全部拒绝。摘要生成或 revision CAS 提交失败时写入 failed，且失败路径不得包含 replacement；允许已经得到 summary、但尚未替换时失败。替换已经提交后，即使新 Surface 仍超过 provider context window，压缩事务也必须记录 completed，随后由 enclosing turn 单独失败。崩溃恢复只把“恰有一条 summary 且恰有一条匹配 replacement”的事务补成 completed，其余未闭合事务补成 failed。替换只遮蔽 Surface，原消息仍在 Timeline、branch transcript、session search 与 Trajectory 中。

replacement 还必须重投影压缩时的权威活状态，而不能依赖旧 reminder 恰好落在保留范围内。跨 harness 的 Agent 状态包含当前 session 自己的 bash/monitor、todo、subagent 和 MCP；共享 terminal 的任务必须按 `owner_session_id` 隔离，root、child 与 sibling 不得互见。Shell 另外从运行时 owner 获取非消费型 Workflow Run 快照和 root session 的 scheduler/loop 快照；该读取不推进 ordinary-turn reminder revision、不复制 Run/loop，也不向 child 暴露 parent scheduler。Goal 与 Behavior 继续由 typed Control context 重投影，Plan 继续从 hash 校验后的 artifact 重投影。这样 compaction 只重建模型视图，不成为任何运行机制的第二事实源。

压缩后的上下文恢复不是“解包 summary”。Grow 暴露特殊内置工具 `context_recall {query}`，agent 只描述当前缺少的事实、决定、约束或先前工作，不接触 Timeline Ref。工具实现先在 chat-state actor 内一次冻结 `timeline high-water + surface_revision + current Surface/SurfaceId + branch transcript/SurfaceId + unloaded_surface_ids`；shell 随后从同一份冻结 Surface 派生受预算约束的 `need_context`，而不是另发一次读取。`unloaded_surface_ids` 只来自已经 completed 的 compaction target，并与当前 rewind 分支的原始叶子取交集，失败、半提交、已 rewind 的范围以及仍在 live Surface 的尾部都不能成为 archive。
compaction target 之前若发生过 tool-result prune 或更早一层 compaction，Timeline 会沿 replacement provenance 递归折叠到当前分支的原始叶子 ID；不得直接拿 target 的当代 SurfaceId 与 branch transcript ID 求交，否则一次内容改写就会让整段归档永久不可回忆。ImageProjection 会推进 Surface revision，并把被投影叶子的身份替换传播到 branch/rewind fold；completed `CompactionMeta` 继续拥有其原始叶子 provenance，因此同一投影还会从当前摘要中精确清除由 source image URL 或 managed `<image_files>` envelope 派生的 asset 引用，并用 durable description 替换。摘要其余文本不变，原始事件仍只作为 Timeline 证据存在，后续模型视图不能借压缩摘要恢复本地图片路径。Session rules 与 memory 都是 typed append，不存在 head rewrite provenance。
存在可读候选时才初始化 provider 并创建 `purpose=context-recall` 的 Sideband；若确定性检索没有候选，则本地返回 `not found`，但仍须通过 cancellation、Surface revision 与 headroom 闸门。Sideband request 的 `source_refs` 保存冻结读取全集，每条 attempt 的 `input_refs` 是本次实际发送的 `need_context` 与 archive shortlist 坐标并集，typed `assembly_manifest` 分别冻结两组 SurfaceId、revision、`hybrid-causal-units` 策略版本、输入 token 估计和输出上限。`need_context` 优先保留当前滚动摘要，再从最近的非 recall 派生因果单元反向装入固定上限；system、private reasoning 与当前 recall tool exchange 不进入。父链重放只接受冻结 Surface 的 need 子集和“当前 rewind 分支 ∩ completed compaction 已卸载叶子”的 archive 子集。
发送侧从完整 branch transcript 先构造因果闭合检索单元：普通 user+assistant 回合保持整体，assistant tool call、全部结果与后续 continuation 保持整体；单元只要有一个可见成员未卸载就整体排除，绝不从 live tail 拼出半个回合。system prompt 与 private reasoning 不进入输入；包含当前或历史 `context_recall` 调用的整个 tool 单元（包括依赖其结果的 continuation）被标记为派生并排除，避免递归强化。archive 内容始终按不可信证据处理，并以 JSONL 信封封装；候选 ID 与完整因果单元的冻结位置稳定绑定，正文只能作为转义后的 `content` 字段，不能伪造候选边界。若全文超过预算，先按 exact/identifier、CJK bigram 与进程内 BM25 做确定性混合排序，先装入命中单元，再补命中的完整邻接单元；任何单元连同信封开销放不进剩余预算就整体跳过，最终 archive 正文还要通过一次完整 token 估计，时间接近度只用于同分裁决，不能用无关 recent tail 填满预算。
Sideband 使用调用者当前模型、无工具、只读采样；need 与 evidence 的实际装配共同受 sideband context window 限制，并为 provider 开销和输出留出 reserve，每次 retry/refine 都在发送前按实际请求重新计数，必要时收窄 archive，不能越过 `Ws`。输出 `max_output_tokens` 由调用者当前 Surface 的剩余 headroom 反推，并额外保留下一次 assistant 采样空间。模型必须返回 `found/not_found/ambiguous/need_more` 的严格 JSON；`found/ambiguous` 必须引用本 attempt 的候选 ID，`not_found` 不得伪造证据，`need_more` 只能提供有界检索词且至多触发一次基于同一冻结 transcript 的重检索。非法结构只允许一次纠正，整个合成最多三次 attempt。成功的结构化结果和 evidence refs 先 durable 完成 Sideband，typed tool output 携带冻结的 `surface_revision + context_window + 完整 ToolResult token 上限` 到唯一的主 Timeline 提交点；chat-state actor 在同一个串行命令中重新检查 revision、当前 token 投影和实际完整结果，并原子选择写入 recall 内容或一个有界 rejection ToolResult。后者仍闭合原 provider tool call，但绝不包含过期证据。由此 revision/headroom 检查与 Surface append 之间不存在 shell 级 TOCTOU；派生 Sideband 结果即使被拒绝也继续保留供审计。任一预算低于最小可用值时 fail-closed。每个 session 的 recall 请求通过单槽有界通道进入 LocalSet 并串行采样，模型并行发起工具调用时也不会形成无界队列；调用级 cancellation token 穿过该通道，排队时已取消的请求不会创建 Sideband，采样中取消则必须 durable append `outcome=cancelled` 的 end。

这里需要注意，`context_recall` 返回的是一次新的派生结果，不是旧 `ConversationItem` 的分页展开，也不会把任何被遮蔽节点重新插回 Surface。主 Timeline 仍然只保存原始事实、`sideband/spawn` 与正常 tool call/result；Sideband 自己保存 request/attempt/result/end。直接检查原文继续走 session search、Trajectory 或 rewind，agent 的正常 P 只接收它这次明确请求的回忆。因此压缩的语义是卸载，回忆的语义是按问题重新投影，而不是永久解压。

`SurfaceId {event,item}` 是压缩、rewind、Trajectory 与引用调试共用的唯一消息身份。Grow 不维护 DCP 式 `mNNNN ↔ rawId` 映射表，也不向正常上下文注入仅供框架解析的 ID 标签；自动选择器直接在冻结的 SurfaceId 投影上计划范围，避免第二套可漂移身份状态。

工具结果 pre-prune 使用一条完整 Surface replacement 原子提交，但校验器只允许 `ToolResult.content` 变化，因此多项目裁剪不会留下半提交状态。它是同一 Timeline/Surface 机制上的确定性变换，不拥有独立压缩归档。

Rewind 不读取 compaction checkpoint。Timeline fold 直接展开被压缩的旧节点，追加一个 `Rewind` replacement 选择新分支；后续 prompt 与 compaction 都在该分支继续追加。旧 checkpoint 文件、更新标记及 fork-copy 路径已删除，避免持久化一份与 Timeline 重复的大型历史。

模型上下文保持“冻结头 + 活动尾”纪律：

- 压缩只推进头部覆盖边界；
- 启动时的 typed runtime snapshot 作为独立 user-role 消息进入 Timeline；后续日期、工具目录与 reminder 只追加到活动尾；
- 稳定 system head 只含 Mandatory Core 与固定 Audience；Agent composition、工具名、memory capability 与 role 通过 `system.role` Control layer 追加，client rules 与检索 memory 使用各自的 typed user item；
- `prompt_cache_key` 由 workspace、Timeline lineage 与 model 构成，不含消息数和时间；
- 覆盖前缀指纹只哈希实际发送给 provider 的 wire 字段；
- cache warm/cold/unknown 只用于观测，不能触发历史改写。

模型选择是独立控制轴。SessionActor 持有稳定的完整 `provider/model` catalog id，ChatState 的 SamplingConfig 持有 provider wire model 与 reasoning effort；认证刷新、auth-provider、模型级超时和重试策略只能以 catalog id 解析，绝不允许用 wire model 反查 provider。外部 SessionHandle 只是 UI 镜像，不能提供持久事件的 `from` 值。用户切换与 catalog 热加载造成的 fallback、wire route 或 effort 变化都必须先 durable append `observation(model.changed)`，完整记录 catalog/provider/effort 的 from/to 与 `reason=user_selection|catalog_reload`，之后才能修改运行配置。模型变化从不选择 concise prompt 或重写 system head。热加载调用必须等待每个 actor 的 acknowledgement，只有成功后才更新 handle 镜像。`summary.current_model_id/reasoning_effort` 是该事件链的可修复投影：加载时严格校验所有匹配事件及其连续性，落后时从最新 `to` 自动修复，畸形或断链时 fail closed。

## 标题与 Sideband

标题不是 `summary.json` 自己拥有的字符串。自动标题先创建 `purpose=session-title` 的 Sideband，Sideband 的 request、attempt、result、end 使用独立 seq 空间写入 `sidebands/<id>/timeline.jsonl`；主 Timeline 只记录 `sideband/spawn`。标题通过 provider 原生 JSON output / response format 约束生成并在本地严格校验，不再伪装成 `session_title` 工具调用。通过结构化校验的 result 或失败 fallback 随后追加 `session/title`，并引用精确的 Sideband result/end seq。`/rename` 也只能追加 source=user 的同一种事件。SessionActor 串行化在线 rename，并在用户标题提交时消耗一次性自动标题 capability；已经运行的自动 Sideband 会在提交阶段 fail closed。

`summary.json` 的 `title`、`title_source`、`title_event_seq` 只是最新 `session/title` 的可重建投影。Timeline append chokepoint 在标题事件持久后刷新这三个字段、搜索索引与 ACP `SessionInfoUpdate.title`；不存在直接标题写 API、`SessionSummaryGenerated` 扩展通知或独立标题投影消息。加载时投影落后会从 Timeline 修复，同 seq 内容冲突或投影领先于 Timeline 视为损坏。分叉只继承 Surface，不复制父标题身份；child 在自己的 Timeline 生成或接收标题。

所有辅助模型调用统一为 Sideband 实体。Sideband 的能力面固定为空：provider request 不得携带 tools 或 tool choice，`attempt_selected` 在任何 durable attempt 与 provider emission 之前统一 fail closed；`/btw`、Recap 与 Compaction 都不能借 KV-cache 对齐为理由复制主 Agent 的动态工具目录，结构化目的只能使用 JSON output。旧 `compaction_tool_choice` 配置链已经删除，不存在一条可重新开启工具的旁路。request 必须在 provider emission 前 durable append，记录 purpose prompt、冻结的 `source_refs`、跨 attempt 不可扩张的 `budget_policy`、闭集 backend route、executor 与 output schema；每次实际调用前追加包含 `input_refs + assembly_manifest + feedback` 的 attempt，成功追加 result+end，失败追加带错误的 end。预算策略冻结最大 attempt 数、每次物化输入上限，以及显式输出上限或“保持 provider 默认”这一配置状态；每条 attempt 在 durable append 时按实际请求重新计数，任何重试、纠正或 refinement 都不能扩张该 envelope。每次 attempt durable append 前，实际 provider backend 与显式 request model 必须精确匹配 durable route，不能用 provider 默认模型或自由字符串形成审计旁路。`output_schema` 不是描述性元数据：request durable append 前必须通过统一的有界、自包含 schema 编译；provider JSON wire constraint 必须与账本的 typed backend 和 schema 精确一致；成功 result 必须携带符合该 schema 的 `structured_output`，raw output 只保留为审计事实。purpose-owned 的校验/纠正发生在 result commit 前，因此无效 attempt 不产生 result；若已经通过 purpose 校验的结果仍违反账本 schema，则视为 harness invariant failure，durable 记录 failed end，不能留下开放 ledger。`input_refs` 必须逐项被 request `source_refs` 覆盖，manifest 的 context 坐标必须落在 source refs、实际选择坐标必须落在 input refs；result 的 `evidence_refs` 同样只能是成功 attempt input refs 的子集。引用只保存 Timeline 区间，不复制内容；当前执行入口只接受发起 session 自身已经存在的区间，跨实体引用必须先经过显式 resolver，不能伪装成本地范围。Sideband schema v6 使用严格嵌套 kind，不读取任何旧版本；自己写出的完整生命周期必须能够无损 replay，新建目录层级也属于 durability barrier。导入与 Trajectory 读取时的跨账本校验会重放 request high-water 之前的父 Timeline：SurfaceId 必须指向真实消息条目；`context-recall` 的 revision 与 need Surface 必须精确匹配该冻结投影，selected 集合必须属于“当前 rewind 分支 ∩ completed compaction 已卸载叶子”。`info-request` 保留给 subagent→parent 的低频响应型协议；`/btw` 使用独立 `side-question` purpose，不复用跨实体语义。

会话镜像协议同样只有这一套事实：`grow/session/state` 输出 `summary` 投影、父 `timeline`、`sidebands` ledger map、Timeline 精确引用的 `blobs` 与客户端可重放的 `updates` ledger，`grow/session/import` 五列缺一即拒绝。重复导入只在五列规范化后逐值完全相同时幂等；相同 identity 的不同因果状态必须拒绝，不能把“目标存在”误报为成功。blob key 是 host-independent content identity，导入时必须与 Timeline 引用集合及内容 hash 完全一致，多余或缺失都拒绝。导入不能把原 session identity 改名，因为 Sideband input/initiator refs 是不可变事件的一部分；每条独立 ledger 必须匹配唯一父 spawn，自动标题还必须引用真实的 result+completed end 或 failed/cancelled end。旧 `grow/session_summaries/*` 列表协议已删除，当前 `grow/session/list` 只输出一个 `title` 字段，不再并行输出 `summary` 或空 `firstPrompt` fallback。

## Subagent 平行实体

Subagent 不是父会话目录中的 metadata sidecar，也不是 coordinator completed cache 的可恢复条目。父与 child 各自拥有独立 Timeline 和 seq 空间，通过三段因果链互引：

1. 父 Timeline 先 durable append `subagent/spawn`，记录唯一 `subagent_id`、唯一 `child_session_id`、冻结来源、有效模型/权限投影与执行位置；v19 的 `security_parent_session_id` 必须非空，并且必须等于该 spawn 的直接安全父 session identity；
2. child 在 staging 目录中写入已经确定稳定 System head 的 Seed Surface 与唯一 `subagent_seed`，其中 `parent_timeline_id + parent_spawn_seq + subagent_id` 必须精确反向指向第 1 步；Seed 同样必须携带非空 `security_parent_session_id`，且必须与对应 `subagent/spawn` 精确匹配，然后才原子发布 session；normalized/new child 在这里选择 child Audience head，resume 与 verbatim mirror 保留来源 head，actor 发布后不存在二次改写；
3. child 完成时先把非空成功输出写成 `artifacts/subagent-output/<blake3>.json`，再 durable append 唯一 `subagent_result` 封口整个 child Timeline；父最后追加 `subagent/end`，并用单事件 `TimelineRangeRef` 精确引用 child result。

`subagent_result` 之后禁止再追加任何事件。父终态中的 outcome、duration、tool/turn/token 计数与 error 必须逐字段等于被引用的 child result；只检查引用形状不算解析成功。Completed 必须有 result ref。只有 child 尚未成功发布或 child 实体已经丢失时，Failed/Cancelled 父终态才允许没有 result ref。

恢复顺序与正常提交顺序相同：通用 Timeline interrupted-recovery 不得抢先关闭 open subagent，也不得关闭仍拥有 open child 的 Workflow；shell 先询问运行后端，Running/Initializing 保持 open 以便重连。终态 child 先验证 parent spawn ↔ child seed；child 已有 result 就复用精确 seq 和 artifact，backend Completed 但 result 尚未落盘时则先 immutable-write 完整输出 artifact，再按观测到的 duration/tool/turn/token 字段追加恢复 result；随后才关闭父 spawn，最后从已验证的 child result/artifact 补 UI projection。只有实体确实未发布或已丢失的 Failed/Cancelled child 可以生成无 ref 终态；已存在但损坏、seed 不匹配或 artifact 无法验证的 child 必须保持 parent open 并 fail closed，不能通过 `result_ref=None` 洗白。`updates.jsonl` 只回答客户端是否已经看过 spawned/finished，不推断生命周期。内存 coordinator 只提供当前 canonical inspection，不能替代 child Timeline。`resume_from` 通过 session identity resolver 读取 child Timeline，并在物化 Surface 前验证完整父子链；任一实体缺失、ref 指错事件或字段漂移都 fail closed。

父、child 与 Sideband 都是可独立归档的实体。当前 `grow/session/state` / `grow/session/import` 传输一个明确 session 实体，而不是递归复制整个后代图；因此父 Timeline 中的 child ref 可以在导入时保持未解析，直到对应 child 实体也按原 identity 导入。任何需要解引用的动作必须经过上述 resolver，不能把“引用存在”当作“目标已验证”。host-specific cwd/worktree 只用于原主机的执行恢复：普通 cwd 不存在时可以退回当前父 workspace；worktree-backed resume 必须复用仍存在的本地 worktree，或从有效 snapshot 恢复，否则 fail closed。两者都不能改写不可变 Timeline 来伪装路径迁移。

schema v19 额外把 `SubagentSpawnEvent.security_parent_session_id` 与 `SubagentSeedEvent.security_parent_session_id` 作为不可变安全谱系：两者都必须非空，并且 Seed 值必须精确等于对应 Spawn 值。nested child 的展示与取消生命周期可以归并到根 Session，但 resume 只允许根 owner 或同一直接安全父级；兄弟 child 不能借根 Timeline 互相取得 Surface。

## 持久化格式

任何会导出模型 Surface 的 load、fork、resume 与 copy 都只能消费同一个 `ValidatedTimeline`：Timeline fold、prompt blobs 和全部 Sideband ledger 来源证明必须在 pinned directory 上一起通过，不能先物化 Surface 再分别验证附属实体。Sideband/Shadow 不一致与缺失证明统一 fail closed。

Timeline 使用独立的 append-only `timeline.jsonl` 流，每个事件携带 schema 版本。`summary.json.session_format_version` 必须精确等于当前 v6；缺失、旧版本以及没有 Timeline 的已发布会话全部拒绝加载，导入也不能改写版本号来伪装升级。Timeline 事件 schema 为 v20，其 envelope、事件 enum 和每个 typed payload 都拒绝未知字段，不能悄悄吞掉旧字段形成半升级状态。session v6 将超大 prompt 的本地绝对路径替换为 `artifact:prompt:blake3:<hash>`，并要求每个 Workflow actor 在主 Timeline 拥有严格的 spawn/resume/end/close 生命周期；只有 provider request 副本在采样前把逻辑 artifact 解析为当前 session 实体目录下的本地路径，fork/import 不再改写不可变 Timeline 文本。实体目录是运行时的显式不可变身份输入，不能由 `cwd + id` 二次推导；因此独立 child 的 prompt blob、compaction、workflow 和其他副产物始终归属 child 自己。加载时先校验版本、seq 连续性、事件结构与全部逻辑 artifact，再构建 Surface；坏事件或缺失 artifact 不能降级成“尽量恢复”的消息数组。权威 JSONL 与 summary 只从 no-follow 的 regular file 读取；写端先把 authority root 打开成 pinned directory fd，再对每个可变分量使用 `openat/mkdirat + O_DIRECTORY|O_NOFOLLOW`，最终 ledger/lock open、immutable publish、atomic rename/unlink 和目录 barrier 全部相对同一个 fd 完成。Windows 使用拒绝 `FILE_SHARE_DELETE` 的 reparse-safe directory capability；待发布的临时文件与 session staging capability 在创建时取得 `DELETE` 权限，并直接通过各自的同一句柄完成 no-replace rename。Windows 不使用硬链接实现 immutable-create，因为同步盘、重定向用户目录和网络文件系统可能允许普通原子 rename 却拒绝 hard link；也不能重新按路径打开一个会与 pinned handle 冲突的删除句柄。文件内容在 namespace commit 前逐个 flush；Windows 没有与 Unix directory `fsync` 等价的 capability barrier，目录 namespace sync 因而显式为 no-op，不能把只读目录句柄的 `AccessDenied` 误报成 session 创建失败。`sidebands`、`workflows`、`prompts`、`artifacts`、`assets` 以及 session 根文件因此即使在验证后被并发 rename 并替换成 symlink/reparse point，也不能把写入重定向到实体边界之外；其他平台在没有同等 handle-relative/no-follow 实现前对这些写入 fail closed。长 cwd 的 `.cwd` marker 也由同一存储入口以 immutable-create 语义发布，旧的独立目录创建器已经删除。单条 JSONL 记录上限 64 MiB、summary 上限 1 MiB，超限的已提交记录 fail closed，读取过程不再按整个账本大小一次性分配。唯一可丢弃的是没有换行终止的最终碎片，因为它从未形成已提交事件；即使尾片超长也只用固定缓冲扫描到 EOF，不升级成事实。Workflow journal 同样把 newline 作为提交边界，不再把碰巧完整的无换行 JSON 尾片升级成事实。Timeline writer 以 `seq` 做幂等 compare-and-append：丢失 acknowledgement 后重试同一事件只会重新执行 durability barrier，同序不同内容、序号缺口和内部坏行全部 fail closed；ledger 与 lock append 同样拒绝 symlink。所有进入 Surface 的普通 assistant/tool/user 写入也先等待同一 durable acknowledgement；瞬态 ENOSPC、锁超时或 I/O 错误对 actor 施加背压并指数退避重试同一个不可变 event，未提交事件不会先污染内存、后续 seq 也不会越过失败点。模型请求、工具执行、step/turn、Workflow 终态和 compaction 边界使用可向调用者返回错误的 durable append acknowledgement。进程恢复先关闭本地 request/tool/compaction 与 step/turn，再按上一节对账外部 child，最后关闭不再拥有 open child 的 Workflow，不能在因果树中保留幽灵 execution，也不能伪造外部终态。

Windows `FlushFileBuffers` 要求文件句柄具有 `GENERIC_WRITE`。因此 staging tree 的最终 barrier 必须无创建、无截断地以读写权限重开已有 regular file，不能复用只服务验证读取的只读句柄；后者会稳定产生 `AccessDenied (5)`，并在目录发布之前中止 session 创建。

Windows 的 staging capability 只在 namespace commit 期间保留 `DELETE`。提交后必须通过 `ReOpenFile` 对同一文件对象先建立 share-delete bridge、关闭发布句柄，再恢复为不含 `DELETE` 且拒绝 `FILE_SHARE_DELETE` 的普通 capability；不能把发布权限带入长期 session cache，否则同一实体的独立扫描、resume 或第二个 adapter 会被 Windows 的双向 share check 拒绝。

永久 `TurnStarted` 或 `TurnEnded` 持久化失败会先 unwind 本地 usage、file-state、idle hook 与 subagent scope，然后关闭整个 shell session writer epoch。该错误不会降级为普通模型 turn error，也不会 admission 下一 turn；恢复器先闭合 incomplete turn，再重新开放 Goal 或普通输入。

同一 writer epoch 内的 foreground owner 若意外 unwind，shell 先关闭仍 open 的 request/tool/compaction，再追加 `StepEnded` 与 `TurnEnded(error)`；该终止链与 Ctrl+C/Stop 使用同一 EventTracker 顺序。若 durable `TurnEnded` 已经存在，不能再生成一个替代终态或不同的 UI completion，writer 必须 fail closed。进程级中断无法由已死亡的 actor 写入，下一次加载通过 `recovery` 事件和同样的子项到父项顺序补齐。

普通模型工具调用只走 typed `tool/call -> tool/result` 单轨：执行前等待 call 落盘，结果消息先进入同一 actor 队列，随后等待 result 终态 ACK；actor 顺序保证该 ACK 同时是结果消息的持久化屏障。shell 不再生产第二套 `ToolStarted/ToolCompleted` observation，也不把 stream/phase 伪装成生命周期。

Grow 不为旧的可变 Chat 快照格式或旧 Timeline schema 保留执行兼容层；schema v20 直接拒绝任何先前版本的日志。新格式加载只认 Timeline，Behavior/Goal 同样只从 Timeline `control` 事件恢复，不存在 `session-control.json` sidecar。Workflow 恢复也不扫描目录来发现实体，而是只枚举已验证 Timeline 中最近的有限 `workflow/spawn`，再按 run id 精确读取对应 manifest/script/args；没有 spawn 的孤儿目录不是候选事实。Workflow 的 metadata、epoch、execution 终态与永久关闭状态只从 Timeline 自己维护的 `workflow_lifecycle` fold 读取，shell 不再二次扫描原始事件重建另一份生命周期。Workflow manifest 只是当前运行投影，普通进度与 acknowledged 边界都通过同一个 persistence actor 写入；host service 和 manager 不得绕过 writer 直接锁文件。pause/cancel 只向 run-owned watcher 写 intent，manager 返回前必须等待该 watcher 完成 child drain、terminal manifest 投影和权威 Timeline Ended durability barrier，调用方不得提前改 tracker 或轮询猜测尚未提交的终态。分叉先在同级 staging 目录完整构造新摘要、Seed Timeline 和附属状态，递归同步 staged 文件与目录后再通过原子 rename 发布；目标已存在时直接拒绝，任何失败都不得暴露目标目录或遗留 staging。分叉后的 prompt/compaction 坐标从新 Timeline 重新派生，不继承父会话的可变标记；fork 截断与 child-context 摘要共享 chat-state 唯一的 complete-turn scanner。若显式继承 control，则只保留无 runtime ownership 的 Normal/Clarify 选择，并在 child Timeline 中追加清除 Plan、Workflow、Goal ownership 及 Plan artifact/approval 残留的新 Control 事实。回退先发布严格、限长的 `rewind-transaction.json` intent，再依次提交文件、完整 rewind-point projection 与 Timeline branch replacement；actor 只在 intent 清除后确认成功。运行中失败会停止该 actor，加载阶段依据 source/target prompt index 幂等向前补齐，因此进程死亡不能把半回退会话暴露给下一条命令。

旧事件流和 Chat 快照均已删除。`updates.jsonl` 只保存客户端 replay / display stream，`TurnCompleted` 不再承担 durable terminal 语义。它可以重复、丢失或重建，不能参与 agent 恢复决策；prompt 文本、压缩位置和跨压缩 rewind 同样禁止从它或 compaction checkpoint 恢复。

## Trajectory 调试面

`grow trajectory [session-id]` 在 `127.0.0.1` 启动独立页面。未指定 session 时选择当前目录最近活跃的主会话，不把活跃的 subagent 误当作入口；端口默认由系统分配。服务只接受 loopback bind 与 localhost/loopback Host，并为每次启动生成不可猜的 URL token；页面/API 响应强制 `no-store`、`no-referrer`、`nosniff`、禁止 frame，并用只允许 self fetch 与内联静态页面资源的 CSP 封闭调试数据。页面与 API 从主/child/Sideband Timeline 以及被主 Timeline `workflow/spawn` 精确拥有的 Workflow journal 构建 `TrajectorySnapshot`，不读取消息快照或 UI updates。stream chunk、first-token 和运行期 phase 只属于 ACP replay/即时 UI，不进入 Timeline；request terminal 已携带 TTFT。Workflow manifest 只保存当前运行投影，不再携带第二份 mutable history；`execution_epoch` 恢复时必须按 Timeline 的最新 lifecycle 纠偏，run 的存在、顺序、恢复和终态仍以主 Timeline 为唯一权威，host-call 历史只存在于 Workflow journal。

- `GET /<token>/api/trajectory` 只返回不含 canonical payload 的定长摘要窗口和固定 180 桶活动密度概览；每行携带过滤结果内的 `ordinal`，使已加载窗口可以无歧义地映射回唯一 overview bin。`after` / `before` 使用稳定 `entry_id` 游标并与 exact `entry` 定位互斥，`layer`、`actor`、`class`、`producer`、`visibility`、`issue`、`search`、`limit` 做交集过滤；`correlation`、`turn`、`step` 是 Inspector 关联导航使用的精确范围，不退化为全文搜索。exact entry 返回以该行为中心的摘要窗口；`GET /<token>/api/trajectory/event?entry=...` 复用已物化的无 payload 行，并按 `timeline-id + seq` 直接定位唯一 Session / Sideband / Workflow 来源，不为单条详情再次构造、排序整棵树。默认详情只返回服务端有界的 canonical preview，用户显式复制时才用 `full=true` 读取完整事件，列表轮询和详情展开都不能把超大 details 复制到浏览器主线程；
- 任意实体行统一使用 `t:<timeline-id>/<seq>`；合并视图以 `parent_entry_id` 精确指向 direct spawn，以可递归的 `nesting_path` 保存确定性因果位置，不保留只能表达一层关系的 `parent_seq`。服务首次物化时以 `(at_ms, nesting_path)` 建立可读的初始顺序，随后为本次 token/server 生命周期内首次观察到的 entry 分配只增 arrival order；pending replacement 保留原 entry 的位置，晚到的 child/Sideband/Workflow 或时钟回拨事件只能追加到 cursor 尾部，不会插回客户端已经翻过的窗口。arrival 索引只保留权威投影中仍存活的 entry，已经移除后再次出现的 ID 视为新的尾部 arrival，不能让 replacement churn 积累无界历史索引；
- 服务从父 Timeline 的 `subagent/spawn` 出发递归解析 child session，逐层验证 summary lineage、spawn ↔ seed-source，并在父终态引用 child result 时验证 exact result；运行中 child 与无 result ref 的 Failed/Cancelled child 可以暂缺，Completed、任何携带 result ref 的终态或已发布但谱系被篡改的 child fail closed；
- 服务从 `workflow/spawn` 加载 `workflows/<run-id>/journal.jsonl`，journal 行使用独立身份 `t:<run-id>/<seq>` 并挂在 exact spawn 下；同一 spawn 下以固定 path namespace 区分 journal 与尚未链接的父 Timeline lifecycle，并强制所有合并行的 `nesting_path` 唯一。Workflow 发起的 subagent 先挂在 run spawn，journal 已记录且能反向验证对应 owned `spawn_agent.result.agent_id` 后再精确挂到该 host-call 行，child Timeline 继续递归嵌套；
- current / shadowed / log-only 由统一 Surface fold 计算；
- request 展示 TTFT、总耗时、token/cache usage，tool/turn/compaction 展示真实终态耗时；
- 投影从 typed terminal state/outcome 派生 `warning` / `error` 诊断级别，overview 与问题筛选复用这一字段；它只做只读诊断分类，不反向改变 durable 终态，也不把所有非 completed 结果粗暴归为 failure；
- `model.changed` 作为独立 lifecycle 行显示模型、effort 或 provider route 的 from/to 与 reload 原因；`control` 行按前后原子快照归纳 Behavior/Plan phase 与 Goal create/edit/status/budget/checkpoint 变化，不再只显示无语义的 revision 编号。带 typed `model_context` 的 AgentRole / Behavior transition 与 reprojection 进一步投影成 `prompt.agent_role.*` / `prompt.behavior.*` governance 行，使角色切换、Behavior 切换和上下文重组可见；这是同一 Control 事实的诊断语义，不增加第二条“最终 prompt”日志；
- 每次查询只构造一个 relocation-aware session storage view，主实体和全部递归 child 都从这一个权威目录快照解析；服务再按字节 offset 增量 fold 各 Session / Sideband Timeline 与 Workflow journal，同一次刷新会持续读取到稳定尾部，完整批次中任一坏行会让整批拒绝且不推进对应 cache，未换行尾片等待下一次刷新。文件长度、修改时间与平台文件身份/变更标记均未变化时直接复用已经验证的投影；同一文件身份的长度增长只能来自权威 writer 的 append，读取端校验上一提交边界的 64 KiB 指纹后直接延伸增量 BLAKE3 与现有 fold，不再复制或扫描整份历史；truncate、文件身份替换、同长度内容变化以及 append 边界不一致则完整重放新账本，成功后才原子替换旧投影。物化的无 details 合并行以全树 ledger revision 缓存，同一过滤查询也复用已序列化前的响应投影，静态长会话轮询不重复递归投影、排序、过滤或 overview 聚合；
- 每次刷新对整棵递归树共享一个预算，同时限制 nesting depth、Session/Sideband/Workflow 实体数、被打开的源文件数、源字节总量和物化事件总数；API 的 row `limit` 只负责摘要分页，不能被误当成读取阶段的资源边界。浏览器只保留有限摘要窗口，滚动到顶部再用首行 `entry_id` 自动加载前页并从另一端淘汰，单个大型 child 因果子树也不能绕过窗口上限；
- 进行中的事件不伪造 duration；页面默认以 Input / Model / Tools 三条泳道按交互账本顺序等宽分桶，并可把同一过滤结果切换为 layer、actor、class 或 producer 族的泳道投影；view 只改变分组，filter 才改变事件集合，两者不得共用状态或语义。actor 视图当前按 subagent 家族共用一条泳道，列表必须显示每条 child 的稳定 ID 后缀并在 tooltip 保留完整 identity；tool producer 使用 `tool:<name>`，例如 `tool:read_file`。密度由亮度表达，耗时只进入 tooltip/明细，禁止再次用原始 wall-clock duration 把短 Input 压成不可见刻度。Turn 按 actor 的 start/end 范围使用低对比连续染色与克制的首尾章节线，Step 使用执行列细轨道和局部首尾边界；start/end 文案只提供语义，不渲染成高对比徽章，内部随机 `TurnId` 只用于关联和诊断，不作为导航文案。摘要列按事实类型突出内容、工具输入/结果、模型请求指标、生命周期变化、治理/审计证据或失败原因，不把所有事件降成同一种无上下文文本；拥有同一 correlation ID 的 `tool.call` / `tool.result` 使用同一确定性 event 字色，同时保留 ID 文本，不能只靠颜色表达配对。Inspector 提供 Parent、Pair、Turn、Step 的精确跳转/范围动作；搜索、filter、view、关联范围和 selected entry 同步到 URL，使诊断状态可刷新、可分享。页面不提供与滚轮重复的 Earlier / Later 操作；顶部自动向前分页，Live tail 贴住实时底部，用户滚离底部或选择历史事件即暂停，并提供单一、明确的 Jump to live 动作。页面继续支持稳定 ID 深链与分层 canonical JSON 检查；账本表格只挂载 viewport + overscan 行，暂停 tail 后轮询不得重建可交互行。API 摘要窗口是只读输入，Turn/Step 分组等客户端注解只能写入独立 display projection，不能污染下一轮用于变更判定的 source row。Inspector 的 disclosure open/closed 是用户交互状态，不属于事件摘要投影：同一 entry 的后台轮询只能原位更新摘要和已展开的 canonical 内容，不能折叠 disclosure、清空已加载内容或改变 Inspector 滚动锚点；canonical 代码区使用固定有界视口独立滚动，避免异步 payload 到达造成二次布局跳变。

## 模块所有权

| 模块 | 所有内容 | 不得拥有 |
| --- | --- | --- |
| `chat-state::timeline` | 事件、seq、Surface fold、投影、校验 | sampler、shell 状态、文件 IO |
| `chat-state` actor | Timeline 的串行写入口、token/usage 投影、请求组装 | 可变历史副本、第二份事实日志 |
| shell session | turn/foreground/steering/Behavior、瞬时运行协调、事件生产 | Surface 重写实现、第二事实日志 |
| session storage | Timeline 追加、flush、前缀复制 | 会话语义与消息修复 |
| sampler | 传输、流转换、重试、取消 | 会话压缩与历史恢复 |
| Trajectory server | Timeline 查询、过滤与本地调试页面 | Chat snapshot、updates replay、会话写入 |

架构来源为 `/Users/lordcasser/workspace/projects/solaris/docs/agent-core` 的 T/P/G 模型、`deepseek-harness` 的 session/surface fold，以及 Grow 已验证的 sampler、steering、权限与压缩策略。实现冲突时以本文模块边界和不变量为 Grow 的代码约束。
