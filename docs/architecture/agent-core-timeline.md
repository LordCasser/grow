# Agent Core Timeline Architecture

Grow 的会话核心以不可变 Timeline 为唯一事实源。模型上下文、用户 transcript、诊断视图与持久化缓存都是 Timeline 的投影；压缩、修剪、图片替换、回退和系统提示变化只能追加事件或切换投影，不能改写已接受的事件。

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
- 恢复边界只传递 Timeline events；不得同时携带一个预折叠 Surface。会话 attach 时的 system prompt 变更统一经 actor 的原子 head replacement 提交，不能在启动参数里预改一份临时历史。
- UI 更新与本地诊断是 Timeline 的消费者，不是恢复会话的第二事实源。

## 核心类型

每条事件具有单调连续的 `seq`。`seq` 在接受时由 Timeline 分配，调用方不能指定或复用。

Turn、request、tool 与 compaction 的 correlation id 在整个 ledger 生命周期内不可复用。Turn id 是独立的 64 位不透明身份（JSON 中编码为字符串），不能拿可回退的 `prompt_index` 充当，也不能作为 JavaScript number 传输。

```text
TimelineEvent { version, seq, at_ms, kind }

SurfaceOp = Append | Replace { start, end, shadowed }
```

消息类事件携带一个或多个完整 `ConversationItem` 与 `SurfaceOp`。普通用户、assistant 与工具结果使用 `Append`；压缩和内容重写使用 `Replace`。非消息事件不能携带 `SurfaceOp`。schema v8 的闭合事件族为：

| 事件族 | 结构约束 |
| --- | --- |
| `turn` | 同一时刻最多一个 active turn，且恰好一个终态 |
| `step` | 从属于 active turn；关闭前 request/tool 必须闭合 |
| `request` | request id 唯一；started 后才能 first-token/retry/terminal |
| `tool` | call id 唯一；started 后才能 completed，名称必须一致 |
| `workflow` | run id 唯一；spawn 从 epoch 0 开始，resume 严格递增；每个 execution 只有一个 end，暂停/失败可恢复，完成/取消/中断永久关闭；无 active execution 时只能显式 close |
| `compaction` | started、唯一 summary 与 completed/failed 构成事务；成功路径的唯一 replacement 位于 summary 与 completed 之间 |
| `recovery` | 只追加中断修复意图和依据，不改写物理尾部 |
| `observation` | 治理、权限、MCP 等 log-only 事实，不参与 Surface |
| `control` | Behavior/Goal 原子快照；revision 必须严格递增 |
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

Actor 同时维护唯一的 `surface_revision`，它只在消息 append/replace 时推进，生命周期与 observation 事件不会推进。凡是在 actor 外读取 Surface、异步计算后再提交的完整变换，都必须携带读取时 revision 做 compare-and-swap；revision 已变化则整次变换失败，不合并、不覆盖后来消息。Actor 内的 system head、图片、memory、修复与 pre-prune 变换直接在串行命令循环中计算和提交，不经过外部 read-modify-write。

Timeline 接受的消息是完整原子记录。采样流的 token/delta 只属于 sampler 到实时 UI 的传输链，不进入 Timeline。

`turn/started` 同时记录 typed identity、可回退的 `prompt_index` 与原始 `prompt_text`；`turn/ended` 记录 stop reason 与 completion kind；`compaction/started` 记录发生压缩时的 `prompt_index`。它们是从 Timeline 恢复 prompt 预览、当前分支、Goal finalization receipt 和压缩边界所需的因果元数据；恢复不得再扫描 UI 更新流猜测这些值。

## 三种读取投影

- `surface()`：折叠 `Append` 与 `Replace` 后的当前模型消息序列，是上下文组装的唯一历史输入。
- `branch_transcript()`：沿当前 rewind 分支读取 append-origin 消息，展开压缩与内容重写的 shadow，但不混入已经被 rewind 丢弃的其他分支。
- `events()`：完整事件账本，供恢复、Trajectory、审计与分叉读取。
- `rewind_surface(target)`：展开压缩、修剪和图片重写的 shadow，沿最后一次 `Rewind` 选择出的分支重建未压缩历史，再在 `target` prompt 前截断。

三种投影共享同一个 Surface 状态转换实现。任何消费者都不得独立重写 replace 语义。

## Turn、工具与恢复不变量

- 每个 turn 恰好一个持久化终态；终态提交失败时作用域保持打开，重试同一终态不会产生重复记录。
- 每个 assistant 工具调用最终恰好一个工具结果；真实、拒绝、取消、未开始和结果未知都属于结果。
- 模型请求前、工具执行前与 step 边界使用 fail-closed 持久化检查点。
- 需要改变整个 Surface 的显式修复、图片重写、工具结果 pre-prune 和 rewind 采用 `prepare → durable append → accept`；持久化失败时内存投影不变，调用方也不能看到成功。
- 崩溃恢复只追加确定性的关闭事件，不修改物理尾部。未记录工具开始时生成 `not-started` 结果；已记录开始但没有结果时生成 `outcome-unknown` 结果。
- steering 继续由 shell 的 `expected_turn_id` 栅栏治理；Timeline 只记录被接受的输入与结构化终态。

## 压缩与缓存

压缩只有一条提交链：先 durable append `compaction/start`，再从此刻的 Timeline 原子物化冻结 `input_ref + surface_revision + Surface + SurfaceId[]`。选择器保留最近真实用户轮次和约 16% context window 的原文尾部，只选择一个闭合的旧范围；后续压缩会把紧邻最早待压缩 turn 的上一条 `CompactionMeta` 一并纳入范围，形成单个滚动摘要，而不是在 Surface 永久累积摘要层。单个超长轮次则只选择其中较旧且已闭合的 response/tool group，边界不能落在 tool call/result 或 `Reasoning/BackendToolCall/Assistant` 组中间。摘要调用必须创建 `purpose=compaction-summary` 的 Sideband，并以同一 `input_ref` 作为输入。Sideband 成功写入 result+end 后，主 Timeline durable append 唯一的 `compaction/summary`，其中保存冻结输入引用、精确的单事件 Sideband result 引用、`SurfaceRange {start,end,shadowed}` 和 token/字符计量；摘要正文只存在 Sideband result，不在主 ledger 复制第二份。只有完成这一步，actor 才接受唯一一条 `MessageCause::Compaction` 局部 replacement，未被选择的 prefix/tail 保持原 ID 与原文，最后写入 completed。通用 `replace_all/replace_range` 明确拒绝 `MessageCause::Compaction`；压缩只能通过带稳定目标的 `replace_compaction_range` 提交。

Timeline 校验器强制 summary 所引用的 Sideband spawn 已存在、purpose 与 input ref 完全匹配、result ref 是单事件引用，并在 summary 落账时验证完整 shadow 集合仍恰好覆盖当前 Surface 范围；replacement 的 `start/end/shadowed` 必须逐项等于该 summary target。无 summary、范围漂移、重复 summary 和重复 replacement 全部拒绝。摘要生成或 revision CAS 提交失败时写入 failed，且失败路径不得包含 replacement；允许已经得到 summary、但尚未替换时失败。替换已经提交后，即使新 Surface 仍超过 provider context window，压缩事务也必须记录 completed，随后由 enclosing turn 单独失败。崩溃恢复只把“恰有一条 summary 且恰有一条匹配 replacement”的事务补成 completed，其余未闭合事务补成 failed。替换只遮蔽 Surface，原消息仍在 Timeline、branch transcript、session search 与 Trajectory 中。

压缩后的上下文恢复不是“解包 summary”。Grow 暴露特殊内置工具 `context_recall {query}`，agent 只描述当前缺少的事实、决定、约束或先前工作，不接触 Timeline Ref。工具实现先在 chat-state actor 内一次冻结 `timeline high-water + surface_revision + need_surface_ids + branch transcript ids + unloaded_surface_ids`；`unloaded_surface_ids` 只来自已经 completed 的 compaction target，并与当前 rewind 分支的原始叶子取交集，失败、半提交、已 rewind 的范围以及仍在 live Surface 的尾部都不能成为 archive。
compaction target 之前若发生过 tool-result prune、image rewrite、system/memory head rewrite 或更早一层 compaction，Timeline 会沿 replacement provenance 递归折叠到当前分支的原始叶子 ID；不得直接拿 target 的当代 SurfaceId 与 branch transcript ID 求交，否则一次内容改写就会让整段归档永久不可回忆。
存在可读候选时才初始化 provider 并创建 `purpose=context-recall` 的 Sideband；若确定性检索没有候选，则本地返回 `not found`，但仍须通过 cancellation、Surface revision 与 headroom 闸门。Sideband request 的 `source_refs` 保存冻结读取全集，每条 attempt 的 `input_refs` 只保存本次 shortlist 实际取材的事件范围，typed `assembly_manifest` 同时冻结 revision、当前 need 坐标、实际选中的 SurfaceId、`hybrid-causal-units` 策略版本、输入 token 估计和输出上限。
发送侧从完整 branch transcript 先构造因果闭合检索单元：普通 user+assistant 回合保持整体，assistant tool call、全部结果与后续 continuation 保持整体；单元只要有一个可见成员未卸载就整体排除，绝不从 live tail 拼出半个回合。system prompt 与 private reasoning 不进入输入；包含当前或历史 `context_recall` 调用的整个 tool 单元（包括依赖其结果的 continuation）被标记为派生并排除，避免递归强化。archive 内容始终按不可信证据处理，并以 JSONL 信封封装；候选 ID 与完整因果单元的冻结位置稳定绑定，正文只能作为转义后的 `content` 字段，不能伪造候选边界。若全文超过预算，先按 exact/identifier、CJK bigram 与进程内 BM25 做确定性混合排序，先装入命中单元，再补命中的完整邻接单元；任何单元连同信封开销放不进剩余预算就整体跳过，最终 archive 正文还要通过一次完整 token 估计，时间接近度只用于同分裁决，不能用无关 recent tail 填满预算。
Sideband 使用调用者当前模型、无工具、只读采样；其 evidence 输入按 sideband context window 留出 provider/output reserve，输出 `max_output_tokens` 则由调用者当前 Surface 的剩余 headroom 反推，并额外保留下一次 assistant 采样空间。模型必须返回 `found/not_found/ambiguous/need_more` 的严格 JSON；`found/ambiguous` 必须引用本 attempt 的候选 ID，`not_found` 不得伪造证据，`need_more` 只能提供有界检索词且至多触发一次基于同一冻结 transcript 的重检索。非法结构只允许一次纠正，整个合成最多三次 attempt。成功的结构化结果和 evidence refs 先 durable 完成 Sideband，typed tool output 携带冻结的 `surface_revision + context_window` 到唯一的主 Timeline 提交点；chat-state actor 在同一个串行命令中重新检查 revision、当前 token 投影和结果上限，并原子选择写入 recall 内容或一个有界 rejection ToolResult。后者仍闭合原 provider tool call，但绝不包含过期证据。由此 revision/headroom 检查与 Surface append 之间不存在 shell 级 TOCTOU；派生 Sideband 结果即使被拒绝也继续保留供审计。任一预算低于最小可用值时 fail-closed。每个 session 的 recall 请求通过单槽有界通道进入 LocalSet 并串行采样，模型并行发起工具调用时也不会形成无界队列；调用级 cancellation token 穿过该通道，排队时已取消的请求不会创建 Sideband，采样中取消则必须 durable append `outcome=cancelled` 的 end。

这里需要注意，`context_recall` 返回的是一次新的派生结果，不是旧 `ConversationItem` 的分页展开，也不会把任何被遮蔽节点重新插回 Surface。主 Timeline 仍然只保存原始事实、`sideband/spawn` 与正常 tool call/result；Sideband 自己保存 request/attempt/result/end。直接检查原文继续走 session search、Trajectory 或 rewind，agent 的正常 P 只接收它这次明确请求的回忆。因此压缩的语义是卸载，回忆的语义是按问题重新投影，而不是永久解压。

`SurfaceId {event,item}` 是压缩、rewind、Trajectory 与引用调试共用的唯一消息身份。Grow 不维护 DCP 式 `mNNNN ↔ rawId` 映射表，也不向正常上下文注入仅供框架解析的 ID 标签；自动选择器直接在冻结的 SurfaceId 投影上计划范围，避免第二套可漂移身份状态。

工具结果 pre-prune 使用一条完整 Surface replacement 原子提交，但校验器只允许 `ToolResult.content` 变化，因此多项目裁剪不会留下半提交状态。它是同一 Timeline/Surface 机制上的确定性变换，不拥有独立压缩归档。

Rewind 不读取 compaction checkpoint。Timeline fold 直接展开被压缩的旧节点，追加一个 `Rewind` replacement 选择新分支；后续 prompt 与 compaction 都在该分支继续追加。旧 checkpoint 文件、更新标记及 fork-copy 路径已删除，避免持久化一份与 Timeline 重复的大型历史。

模型上下文保持“冻结头 + 活动尾”纪律：

- 压缩只推进头部覆盖边界；
- 启动时的 typed runtime snapshot 作为独立 user-role 消息进入 Timeline；后续日期、工具目录与 reminder 只追加到活动尾；
- `prompt_cache_key` 由 workspace、Timeline lineage 与 model 构成，不含消息数和时间；
- 覆盖前缀指纹只哈希实际发送给 provider 的 wire 字段；
- cache warm/cold/unknown 只用于观测，不能触发历史改写。

模型选择是独立控制轴。SessionActor 持有稳定 catalog model id，ChatState 的 SamplingConfig 持有 provider wire model 与 reasoning effort；外部 SessionHandle 只是 UI 镜像，不能提供持久事件的 `from` 值。用户切换与 catalog 热加载造成的 fallback、wire route 或 effort 变化都必须先 durable append `observation(model.changed)`，完整记录 catalog/provider/effort 的 from/to 与 `reason=user_selection|catalog_reload`，之后才能修改运行配置。热加载调用必须等待每个 actor 的 acknowledgement，只有成功后才更新 handle 镜像。`summary.current_model_id/reasoning_effort` 是该事件链的可修复投影：加载时严格校验所有匹配事件及其连续性，落后时从最新 `to` 自动修复，畸形或断链时 fail closed。

## 标题与 Sideband

标题不是 `summary.json` 自己拥有的字符串。自动标题先创建 `purpose=session-title` 的 Sideband，Sideband 的 request、attempt、result、end 使用独立 seq 空间写入 `sidebands/<id>/timeline.jsonl`；主 Timeline 只记录 `sideband/spawn`。通过结构化校验的 result 或失败 fallback 随后追加 `session/title`，并引用精确的 Sideband result/end seq。`/rename` 也只能追加 source=user 的同一种事件。SessionActor 串行化在线 rename，并在用户标题提交时消耗一次性自动标题 capability；已经运行的自动 Sideband 会在提交阶段 fail closed。

`summary.json` 的 `title`、`title_source`、`title_event_seq` 只是最新 `session/title` 的可重建投影。Timeline append chokepoint 在标题事件持久后刷新这三个字段、搜索索引与 ACP `SessionInfoUpdate.title`；不存在直接标题写 API、`SessionSummaryGenerated` 扩展通知或独立标题投影消息。加载时投影落后会从 Timeline 修复，同 seq 内容冲突或投影领先于 Timeline 视为损坏。分叉只继承 Surface，不复制父标题身份；child 在自己的 Timeline 生成或接收标题。

所有辅助模型调用统一为 Sideband 实体。request 必须在 provider emission 前 durable append，记录 purpose prompt、冻结的 `source_refs`、route、executor 与 output schema；每次实际调用前追加包含 `input_refs + assembly_manifest + feedback` 的 attempt，成功追加 result+end，失败追加带错误的 end。`input_refs` 必须逐项被 request `source_refs` 覆盖，manifest 的 context 坐标必须落在 source refs、实际选择坐标必须落在 input refs；result 的 `evidence_refs` 同样只能是成功 attempt input refs 的子集。引用只保存 Timeline 区间，不复制内容；当前执行入口只接受发起 session 自身已经存在的区间，跨实体引用必须先经过显式 resolver，不能伪装成本地范围。Sideband schema v3 使用严格嵌套 kind，不读取 v2；自己写出的完整生命周期必须能够无损 replay，新建目录层级也属于 durability barrier。导入与 Trajectory 读取时的跨账本校验会重放 request high-water 之前的父 Timeline：SurfaceId 必须指向真实消息条目；`context-recall` 的 revision 与 need Surface 必须精确匹配该冻结投影，selected 集合必须属于“当前 rewind 分支 ∩ completed compaction 已卸载叶子”。`info-request` 保留给 subagent→parent 的低频响应型协议；`/btw` 使用独立 `side-question` purpose，不复用跨实体语义。

会话镜像协议同样只有这一套事实：`grow/session/state` 输出 `summary` 投影、父 `timeline`、`sidebands` ledger map、Timeline 精确引用的 `blobs` 与客户端可重放的 `updates` ledger，`grow/session/import` 五列缺一即拒绝。重复导入只在五列规范化后逐值完全相同时幂等；相同 identity 的不同因果状态必须拒绝，不能把“目标存在”误报为成功。blob key 是 host-independent content identity，导入时必须与 Timeline 引用集合及内容 hash 完全一致，多余或缺失都拒绝。导入不能把原 session identity 改名，因为 Sideband input/initiator refs 是不可变事件的一部分；每条独立 ledger 必须匹配唯一父 spawn，自动标题还必须引用真实的 result+completed end 或 failed/cancelled end。旧 `grow/session_summaries/*` 列表协议已删除，当前 `grow/session/list` 只输出一个 `title` 字段，不再并行输出 `summary` 或空 `firstPrompt` fallback。

## Subagent 平行实体

Subagent 不是父会话目录中的 metadata sidecar，也不是 coordinator completed cache 的可恢复条目。父与 child 各自拥有独立 Timeline 和 seq 空间，通过三段因果链互引：

1. 父 Timeline 先 durable append `subagent/spawn`，记录唯一 `subagent_id`、唯一 `child_session_id`、冻结来源、有效模型/权限投影与执行位置；
2. child 在 staging 目录中写入继承的 Seed Surface 与唯一 `subagent_seed`，其中 `parent_timeline_id + parent_spawn_seq + subagent_id` 必须精确反向指向第 1 步，然后才原子发布 session；
3. child 完成时先把非空成功输出写成 `artifacts/subagent-output/<blake3>.json`，再 durable append 唯一 `subagent_result` 封口整个 child Timeline；父最后追加 `subagent/end`，并用单事件 `TimelineRangeRef` 精确引用 child result。

`subagent_result` 之后禁止再追加任何事件。父终态中的 outcome、duration、tool/turn/token 计数与 error 必须逐字段等于被引用的 child result；只检查引用形状不算解析成功。Completed 必须有 result ref。只有 child 尚未成功发布或 child 实体已经丢失时，Failed/Cancelled 父终态才允许没有 result ref。

恢复顺序与正常提交顺序相同：通用 Timeline interrupted-recovery 不得抢先关闭 open subagent，也不得关闭仍拥有 open child 的 Workflow；shell 先询问运行后端，Running/Initializing 保持 open 以便重连。终态 child 先验证 parent spawn ↔ child seed；child 已有 result 就复用精确 seq 和 artifact，backend Completed 但 result 尚未落盘时则先 immutable-write 完整输出 artifact，再按观测到的 duration/tool/turn/token 字段追加恢复 result；随后才关闭父 spawn，最后从已验证的 child result/artifact 补 UI projection。只有实体确实未发布或已丢失的 Failed/Cancelled child 可以生成无 ref 终态；已存在但损坏、seed 不匹配或 artifact 无法验证的 child 必须保持 parent open 并 fail closed，不能通过 `result_ref=None` 洗白。`updates.jsonl` 只回答客户端是否已经看过 spawned/finished，不推断生命周期。内存 coordinator 只提供当前 canonical inspection，不能替代 child Timeline。`resume_from` 通过 session identity resolver 读取 child Timeline，并在物化 Surface 前验证完整父子链；任一实体缺失、ref 指错事件或字段漂移都 fail closed。

父、child 与 Sideband 都是可独立归档的实体。当前 `grow/session/state` / `grow/session/import` 传输一个明确 session 实体，而不是递归复制整个后代图；因此父 Timeline 中的 child ref 可以在导入时保持未解析，直到对应 child 实体也按原 identity 导入。任何需要解引用的动作必须经过上述 resolver，不能把“引用存在”当作“目标已验证”。host-specific cwd/worktree 只用于原主机的执行恢复：普通 cwd 不存在时可以退回当前父 workspace；worktree-backed resume 必须复用仍存在的本地 worktree，或从有效 snapshot 恢复，否则 fail closed。两者都不能改写不可变 Timeline 来伪装路径迁移。

## 持久化格式

Timeline 使用独立的 append-only `timeline.jsonl` 流，每个事件携带 schema 版本。`summary.json.session_format_version` 必须精确等于当前 v6；缺失、旧版本以及没有 Timeline 的已发布会话全部拒绝加载，导入也不能改写版本号来伪装升级。Timeline 事件 schema 为 v8，其 envelope、事件 enum 和每个 typed payload 都拒绝未知字段，不能悄悄吞掉旧字段形成半升级状态。session v6 将超大 prompt 的本地绝对路径替换为 `artifact:prompt:blake3:<hash>`，并要求每个 Workflow actor 在主 Timeline 拥有严格的 spawn/resume/end/close 生命周期；只有 provider request 副本在采样前把逻辑 artifact 解析为当前 session 实体目录下的本地路径，fork/import 不再改写不可变 Timeline 文本。实体目录是运行时的显式不可变身份输入，不能由 `cwd + id` 二次推导；因此独立 child 的 prompt blob、compaction、workflow 和其他副产物始终归属 child 自己。加载时先校验版本、seq 连续性、事件结构与全部逻辑 artifact，再构建 Surface；坏事件或缺失 artifact 不能降级成“尽量恢复”的消息数组。权威 JSONL 与 summary 只从 no-follow 的 regular file 读取；写端先把 authority root 打开成 pinned directory fd，再对每个可变分量使用 `openat/mkdirat + O_DIRECTORY|O_NOFOLLOW`，最终 ledger/lock open、immutable link、atomic rename/unlink 和目录 barrier 全部相对同一个 fd 完成。`sidebands`、`workflows`、`prompts`、`artifacts`、`assets` 以及 session 根文件因此即使在验证后被并发 rename 并替换成 symlink，也不能把写入重定向到实体边界之外；非 Unix 平台在没有同等 reparse-safe handle 实现前对这些写入 fail closed。长 cwd 的 `.cwd` marker 也由同一存储入口以 immutable-create 语义发布，旧的独立目录创建器已经删除。单条 JSONL 记录上限 64 MiB、summary 上限 1 MiB，超限的已提交记录 fail closed，读取过程不再按整个账本大小一次性分配。唯一可丢弃的是没有换行终止的最终碎片，因为它从未形成已提交事件；即使尾片超长也只用固定缓冲扫描到 EOF，不升级成事实。Workflow journal 同样把 newline 作为提交边界，不再把碰巧完整的无换行 JSON 尾片升级成事实。Timeline writer 以 `seq` 做幂等 compare-and-append：丢失 acknowledgement 后重试同一事件只会重新执行 durability barrier，同序不同内容、序号缺口和内部坏行全部 fail closed；ledger 与 lock append 同样拒绝 symlink。所有进入 Surface 的普通 assistant/tool/user 写入也先等待同一 durable acknowledgement；瞬态 ENOSPC、锁超时或 I/O 错误对 actor 施加背压并指数退避重试同一个不可变 event，未提交事件不会先污染内存、后续 seq 也不会越过失败点。模型请求、工具执行、step/turn、Workflow 终态和 compaction 边界使用可向调用者返回错误的 durable append acknowledgement。进程恢复先关闭本地 request/tool/compaction 与 step/turn，再按上一节对账外部 child，最后关闭不再拥有 open child 的 Workflow，不能在因果树中保留幽灵 execution，也不能伪造外部终态。

普通模型工具调用只走 typed `tool/call -> tool/result` 单轨：执行前等待 call 落盘，结果消息先进入同一 actor 队列，随后等待 result 终态 ACK；actor 顺序保证该 ACK 同时是结果消息的持久化屏障。shell 不再生产第二套 `ToolStarted/ToolCompleted` observation，也不把 stream/phase 伪装成生命周期。

Grow 不为旧的可变 Chat 快照格式或 Timeline schema v1 保留执行兼容层。新格式加载只认 Timeline，Behavior/Goal 同样只从 Timeline `control` 事件恢复，不存在 `session-control.json` sidecar。Workflow 恢复也不扫描目录来发现实体，而是只枚举已验证 Timeline 中最近的有限 `workflow/spawn`，再按 run id 精确读取对应 manifest/script/args；没有 spawn 的孤儿目录不是候选事实。Workflow 的 metadata、epoch、execution 终态与永久关闭状态只从 Timeline 自己维护的 `workflow_lifecycle` fold 读取，shell 不再二次扫描原始事件重建另一份生命周期。Workflow manifest 只是当前运行投影，普通进度与 acknowledged 边界都通过同一个 persistence actor 写入；host service 和 manager 不得绕过 writer 直接锁文件。分叉先在同级 staging 目录完整构造新摘要、Seed Timeline 和附属状态，递归同步 staged 文件与目录后再通过原子 rename 发布；目标已存在时直接拒绝，任何失败都不得暴露目标目录或遗留 staging。分叉后的 prompt/compaction 坐标从新 Timeline 重新派生，不继承父会话的可变标记；fork 截断与 child-context 摘要共享 chat-state 唯一的 complete-turn scanner。若显式继承 control，则只保留无 runtime ownership 的 Normal/Clarify 选择，并在 child Timeline 中追加清除 Plan、Workflow、Goal、Deep Research ownership 及 Plan artifact/approval 残留的新 Control 事实。回退先发布严格、限长的 `rewind-transaction.json` intent，再依次提交文件、完整 rewind-point projection 与 Timeline branch replacement；actor 只在 intent 清除后确认成功。运行中失败会停止该 actor，加载阶段依据 source/target prompt index 幂等向前补齐，因此进程死亡不能把半回退会话暴露给下一条命令。

旧事件流和 Chat 快照均已删除。`updates.jsonl` 只保存客户端 replay / display stream，`TurnCompleted` 不再承担 durable terminal 语义。它可以重复、丢失或重建，不能参与 agent 恢复决策；prompt 文本、压缩位置和跨压缩 rewind 同样禁止从它或 compaction checkpoint 恢复。

## Trajectory 调试面

`grow trajectory [session-id]` 在 `127.0.0.1` 启动独立页面。未指定 session 时选择当前目录最近活跃的主会话，不把活跃的 subagent 误当作入口；端口默认由系统分配。服务只接受 loopback bind 与 localhost/loopback Host，并为每次启动生成不可猜的 URL token；页面/API 响应强制 `no-store`、`no-referrer`、`nosniff`、禁止 frame，并用只允许 self fetch 与内联静态页面资源的 CSP 封闭调试数据。页面与 API 从主/child/Sideband Timeline 以及被主 Timeline `workflow/spawn` 精确拥有的 Workflow journal 构建 `TrajectorySnapshot`，不读取消息快照或 UI updates。stream chunk、first-token 和运行期 phase 只属于 ACP replay/即时 UI，不进入 Timeline；request terminal 已携带 TTFT。Workflow manifest 只保存当前运行投影，不再携带第二份 mutable history；`execution_epoch` 恢复时必须按 Timeline 的最新 lifecycle 纠偏，run 的存在、顺序、恢复和终态仍以主 Timeline 为唯一权威，host-call 历史只存在于 Workflow journal。

- `GET /<token>/api/trajectory` 支持互斥的 `after` / `before` cursor 或 exact `entry` 定位，以及四维 `layer`、`actor`、`class`、`producer`、`visibility`、`search`、`limit` 交集过滤；exact entry 返回它所属的完整因果 root group，使长账本中的稳定 ID 深链不依赖当前尾页窗口；
- 任意实体行统一使用 `t:<timeline-id>/<seq>`；合并视图以 `parent_entry_id` 精确指向 direct spawn，以可递归的 `nesting_path` 给出确定性因果顺序，不保留只能表达一层关系的 `parent_seq`；
- 服务从父 Timeline 的 `subagent/spawn` 出发递归解析 child session，逐层验证 summary lineage、spawn ↔ seed-source，并在父终态引用 child result 时验证 exact result；运行中 child 与无 result ref 的 Failed/Cancelled child 可以暂缺，Completed、任何携带 result ref 的终态或已发布但谱系被篡改的 child fail closed；
- 服务从 `workflow/spawn` 加载 `workflows/<run-id>/journal.jsonl`，journal 行使用独立身份 `t:<run-id>/<seq>` 并挂在 exact spawn 下；同一 spawn 下以固定 path namespace 区分 journal 与尚未链接的父 Timeline lifecycle，并强制所有合并行的 `nesting_path` 唯一。Workflow 发起的 subagent 先挂在 run spawn，journal 已记录且能反向验证对应 owned `spawn_agent.result.agent_id` 后再精确挂到该 host-call 行，child Timeline 继续递归嵌套；
- current / shadowed / log-only 由统一 Surface fold 计算；
- request 展示 TTFT、总耗时、token/cache usage，tool/turn/compaction 展示真实终态耗时；
- `model.changed` 作为独立 lifecycle 行显示模型、effort 或 provider route 的 from/to 与 reload 原因；`control` 行按前后原子快照归纳 Behavior/Plan phase 与 Goal create/edit/status/budget/checkpoint 变化，不再只显示无语义的 revision 编号；
- 每次查询只构造一个 relocation-aware session storage view，主实体和全部递归 child 都从这一个权威目录快照解析；服务再按字节 offset 增量 fold 各 Session / Sideband Timeline 与 Workflow journal，完整批次中任一坏行会让整批拒绝且不推进对应 cache，未换行尾片等待下一次刷新；所有 cache 在复用增量 fold 前重新计算完整已消费前缀的 BLAKE3，任何 truncate、前部同长度替换或 replacement+append 都会丢弃旧投影并从 seq 0 重建，不使用只能证明末尾 4 KiB 的采样探针；
- 每次刷新对整棵递归树共享一个预算，同时限制 nesting depth、Session/Sideband/Workflow 实体数、被打开的源文件数、源字节总量和物化事件总数；API 的 root-row `limit` 只负责分页，不能被误当成读取阶段的资源边界；
- 进行中的事件不伪造 duration；页面以 Input / Model / Tools 三条时间泳道总览同一批事件，支持向前分页、每秒刷新、tail-follow、稳定 ID 深链与 canonical JSON 检查；账本表格只挂载 viewport + overscan 行，长会话刷新不再反复创建数千个 DOM 行。

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
