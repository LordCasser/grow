## ADDED Requirements

### Requirement: Conversation item determines buffered message cause
缓冲消息写入 SHALL 根据实际 ConversationItem 推导 MessageCause；Assistant、BackendToolCall 和 Reasoning 使用 Assistant cause，ToolResult 使用 ToolResult cause。普通消息入口拒绝 System 及由 Timeline 拥有的 ProjectInstructions、SessionRules、MemoryContext。

#### Scenario: Conversation item determines buffered message cause
- **WHEN** PushToolResult 命令携带 Reasoning 项
- **THEN** actor 经统一 push_message 按 Assistant cause 追加，不因命令名改成 ToolResult cause。

证据：`crates/codegen/chat-state/src/actor/mutations.rs` — `message_cause`。

### Requirement: Durable response admission and native fragment installation
push_response_durably SHALL 先提交 Assistant 消息事实，再隔离格式损坏的工具交换；需要隔离时另提交 IntegrityRepair 并返回隔离数量。仅未隔离的响应尝试安装原生片段，缺失或不匹配时重置续传。

#### Scenario: Durable response admission and native fragment installation
- **WHEN** 原始响应提交成功而后续修复尚未提交
- **THEN** 原始响应构成独立已提交事实；两个事件不构成原子事务。

证据：`crates/codegen/chat-state/src/actor/mutations.rs` — `push_response_durably`。

### Requirement: Empty response resets native continuation
空响应准入 SHALL 重置当前续传并返回零，不为该空响应追加 Assistant 消息事件。

#### Scenario: Empty response resets native continuation
- **WHEN** push_response_durably 收到空 items
- **THEN** 以 empty_response_admission 原因重置续传，返回 Ok(0)。

证据：`crates/codegen/chat-state/src/actor/mutations.rs` — `push_response_durably`。

### Requirement: Explicit history repair dry run
显式历史修复 SHALL 在 Timeline 候选副本上计算报告；dry_run 只返回报告，实际变更才提交修复事件并更新 surface 压力。

#### Scenario: Explicit history repair dry run
- **WHEN** dry_run 检测到孤儿工具结果
- **THEN** 报告 changed 为真，但实际会话和持久化记录保持不变。

证据：`crates/codegen/chat-state/src/actor/mutations.rs` — `repair_history`。

### Requirement: Usage token arithmetic
UsageTotals SHALL 以 input_tokens 与 output_tokens 饱和加计算 total_tokens，uncached_tokens 为 input 减 cached_read 的饱和差再饱和加 output；cache_creation 不另行扣减。

#### Scenario: Usage token arithmetic
- **WHEN** cached_read 超过 input
- **THEN** 未缓存输入部分为零，输出仍计入 uncached_tokens。

证据：`crates/codegen/chat-state/src/usage.rs` — `uncached_tokens`。

### Requirement: Usage ledger model aggregation
UsageLedger SHALL 按精确 model_id 使用首次插入顺序聚合；主循环调用增加 main_loop_model_calls，子代理汇总只累加 totals 与 by_model，不增加主循环次数。

#### Scenario: Usage ledger model aggregation
- **WHEN** 登记子代理按模型汇总
- **THEN** 其 model_calls 进入总账，主循环次数保持不变；重复登记不自动去重。

证据：`crates/codegen/chat-state/src/usage.rs` — `record_subagent`。

### Requirement: Partial cost and incomplete usage
UsageTotals SHALL 仅在成本存在且 cost_missing_calls 大于零时报告 cost_is_partial；UsageLedger incomplete 是独立标记，登记不完整子结果或 mark_incomplete 将其置真。

#### Scenario: Partial cost and incomplete usage
- **WHEN** 所有调用成本缺失
- **THEN** cost_is_partial 不因缺失计数单独变真；调用者需分别读取缺失计数与 incomplete。

证据：`crates/codegen/chat-state/src/usage.rs` — `cost_is_partial`。

### Requirement: Persistence port acknowledgement
TimelinePersistence SHALL 提供逐事件 oneshot IO 结果确认端口及无返回值 flush；NullTimelinePersistence 丢弃事件并立即成功确认，不提供实际存储耐久性。

#### Scenario: Persistence port acknowledgement
- **WHEN** 使用 NullTimelinePersistence 提交事件
- **THEN** 收到成功确认，但本实现不保存事件。

证据：`crates/codegen/chat-state/src/persistence.rs` — `NullTimelinePersistence`。

### Requirement: Permanent persistence error classification
持久化错误分类 SHALL 将 InvalidData、InvalidInput、PermissionDenied、NotFound、Unsupported、BrokenPipe、StorageFull、QuotaExceeded、FileTooLarge、ReadOnlyFilesystem、OutOfMemory、WriteZero 视为永久错误，其他 ErrorKind 不归为永久。

#### Scenario: Permanent persistence error classification
- **WHEN** 提交返回 StorageFull
- **THEN** actor 走永久错误路径，不将其视为无限重试的临时空间不足。

证据：`crates/codegen/chat-state/src/persistence.rs` — `persistence_error_is_permanent`。

### Requirement: Serial actor bootstrap
ChatStateActor SHALL 先串行确认 bootstrap 事件再处理 unbounded mailbox；launch 返回 handle 不等同 bootstrap 已完成，bootstrap 失败终止 actor。

#### Scenario: Serial actor bootstrap
- **WHEN** 调用方刚取得 handle
- **THEN** 后续命令等待 bootstrap 完成；不能凭 handle 已返回断言初始事件持久化成功。

证据：`crates/codegen/chat-state/src/actor/mod.rs` — `run`。

### Requirement: Durable commit retry and cancellation
commit_timeline_event SHALL 对非永久 IO 错误及丢失确认重试同一事件，延迟从25ms翻倍到1秒上限；成功确认后才 accept，永久错误使 writer poison，取消可结束等待。

#### Scenario: Durable commit retry and cancellation
- **WHEN** 一次事件确认丢失且未取消
- **THEN** 重试保留原事件身份与内容，不重新分配序号；确认等待本身没有超时。

证据：`crates/codegen/chat-state/src/actor/mod.rs` — `commit_timeline_event`。

### Requirement: Handle lifetime and query failure
最后一个共享 HandleLifetime 释放 SHALL 取消 actor；通用 query 在发送失败或回复丢失时返回 None，没有自动超时。放弃已入队查询不回滚其处理。

#### Scenario: Handle lifetime and query failure
- **WHEN** 最后一个 handle 释放
- **THEN** 触发取消，不保证 mailbox 排空或隐式 flush。

证据：`crates/codegen/chat-state/src/handle.rs` — `HandleLifetime`。

### Requirement: Sampling route replacement resets continuation
ReplaceSamplingRoute SHALL 无条件重置续传再替换完整配置；UpdateSamplingConfig 直接替换配置且不重置续传；ResetContinuation 提供完成确认。

#### Scenario: Sampling route replacement resets continuation
- **WHEN** 通过 ReplaceSamplingRoute 从A切到B再切回A
- **THEN** 旧native片段不恢复，epoch已改变。

证据：`crates/codegen/chat-state/src/actor/mod.rs` — `ReplaceSamplingRoute`。

### Requirement: Snapshot projection boundary
ChatStateSnapshot SHALL 表达当前 surface、配置、凭据、prompt records、压力、路径与时间等快照，不包含完整 Timeline ledger、原生续传或 usage 账本。Credentials 的 Debug 与 serde 不作脱敏。

#### Scenario: Snapshot projection boundary
- **WHEN** 快照包含非空凭据
- **THEN** 不能将其 Debug 或序列化输出视为已脱敏数据。

证据：`crates/codegen/chat-state/src/types.rs` — `ChatStateSnapshot`。

### Requirement: Conditional tool result admission
条件工具结果 SHALL 先比较 surface revision，再比较候选结果估算与结果/上下文上限；严格大于上限才拒绝。拒绝时持久追加调用者提供的 rejection_item，不重新对替代项执行预算检查。

#### Scenario: Conditional tool result admission
- **WHEN** revision不匹配且候选也超预算
- **THEN** 返回 RejectedSurfaceChanged；成功提交替代结果后才回复。

证据：`crates/codegen/chat-state/src/actor/mutations.rs` — `push_tool_result_conditionally`。

### Requirement: Tool result pruning plan execution
工具结果裁剪 SHALL 只修改目标 ToolResult content，保留其他结构；跳过越界、非工具结果、含完整裁剪marker或已在预算内的项，预算至少1 token。实际发生裁剪时提交一次 ToolResultPrune 替换并更新压力，不发布UI重置事件。

#### Scenario: Tool result pruning plan execution
- **WHEN** 重复执行同一已生效计划
- **THEN** 已裁剪或已在预算内结果保持原样，无变化不持久化。

证据：`crates/codegen/chat-state/src/actor/mutations.rs` — `prune_tool_results`。

### Requirement: Provider pressure anchor lower bound
provider上下文anchor SHALL 不低于最终请求估算加后续surface增长的启发式下界；低报值被忽略，接受值替换 projected_tokens 并发布压力事件。

#### Scenario: Provider pressure anchor lower bound
- **WHEN** provider_total_tokens小于heuristic_minimum
- **THEN** 保留当前压力，不以低报anchor降低估算。

证据：`crates/codegen/chat-state/src/actor/mutations.rs` — `record_provider_context_anchor`。

### Requirement: Request pressure envelope replacement
请求投影压力 SHALL 先移除上次请求与surface估算差，再应用新请求差；surface变更用饱和有符号差调整当前压力。

#### Scenario: Request pressure envelope replacement
- **WHEN** 连续构造具有相同schema的请求
- **THEN** 封装估算被替换，不在每次构建重复累加；仅实际压力改变时发布投影压力事件。

证据：`crates/codegen/chat-state/src/actor/mutations.rs` — `apply_request_projection`。

### Requirement: Durable memory request injection
请求构建 SHALL 先持久修复历史，再trim memory reminder并忽略空串；当前surface已存在相同MemoryContext文本时去重，否则持久追加该上下文。

#### Scenario: Durable memory request injection
- **WHEN** 旧MemoryContext已离开当前surface
- **THEN** 后续相同reminder可重新追加，不提供跨历史永久去重。

证据：`crates/codegen/chat-state/src/actor/request_builder.rs` — `build_conversation_request`。

### Requirement: Request image projection scope
请求图片回收 SHALL 在canonical surface克隆上进行，覆盖User content与ToolResult images，按item和part顺序替换图片为文本占位，保留Timeline原始图片。

#### Scenario: Request image projection scope
- **WHEN** 会话图片字节达到触发值
- **THEN** 尝试回收到MAX_REQUEST_BODY_BYTES的一半；触发值为上限减3MiB，不在本层保证完整wire必定低于上限。

证据：`crates/codegen/chat-state/src/actor/request_builder.rs` — `compact_images_to_byte_budget`。

### Requirement: Image body estimate limitation
conversation_body_bytes SHALL 在图片URL置空的序列化大小上加原URL字节；该计量不包含工具schema等完整请求封装，需转义URL可能仅给下界。

#### Scenario: Image body estimate limitation
- **WHEN** 文本本身超目标且所有图片已替换
- **THEN** 回收循环结束，不通过本函数自动拒绝该请求或保证达到目标。

证据：`crates/codegen/chat-state/src/actor/request_builder.rs` — `conversation_body_bytes`。

### Requirement: Prompt cache lineage key
prompt_cache_key SHALL 对版本、timeline_id、最新Rewind序号或root、backend、base_url、model与续传epoch作NUL分隔BLAKE3摘要，使用grow-前缀及32位十六进制摘要。

#### Scenario: Prompt cache lineage key
- **WHEN** 续传epoch重置而model保持不变
- **THEN** key改变；该key不包含全部采样参数或工具schema，不证明provider缓存命中。

证据：`crates/codegen/chat-state/src/actor/request_builder.rs` — `prompt_cache_key`。

### Requirement: Continuation projection reconciliation
ContinuationLane SHALL 以SurfaceId和item序列化摘要追踪请求投影；新投影不保持已观察前缀时重置epoch。原生片段安装需backend匹配且fragment与IDs非空，投影仅保留当前连续ID匹配片段。

#### Scenario: Continuation projection reconciliation
- **WHEN** 恢复已有Timeline
- **THEN** 创建新续传epoch，将已恢复surface作为portable前缀，不恢复旧native片段。

证据：`crates/codegen/chat-state/src/actor/state.rs` — `ContinuationLane`。

### Requirement: Request token estimate components
estimate_request_input_tokens SHALL 聚合有效wire历史、工具定义、tool_choice与JSON输出schema估算；原生span不合法时回退portable历史估算。

#### Scenario: Request token estimate components
- **WHEN** 合法原生span替代一段surface items
- **THEN** 按fragment估算该段，不重复加入被替代items；本估算不等同provider精确tokenizer。

证据：`crates/codegen/chat-state/src/actor/state.rs` — `estimate_request_input_tokens`。

### Requirement: First user text actual scan
get_first_user_text SHALL 扫描surface，返回首个以Text part开头的User的首part文本；不读取同一User的后续part。

#### Scenario: First user text actual scan
- **WHEN** 第一条User以Image开头，后续User以Text开头
- **THEN** 跳过第一条User并返回后续文本；此行为与源码注释所述第一条User限定不一致。

证据：`crates/codegen/chat-state/src/actor/queries.rs` — `get_first_user_text`。

### Requirement: Timeline schema sequence and sealing
Timeline SHALL 接受schema25、与现有事件数一致的连续seq及非负at_ms；已记录SubagentResult后拒绝所有后续事件。时间戳不要求跨事件单调。

#### Scenario: Timeline schema sequence and sealing
- **WHEN** 有效schema事件的seq跳号
- **THEN** 返回NonContiguousSeq，不接受该事件。

证据：`crates/codegen/chat-state/src/timeline.rs` — `TIMELINE_SCHEMA_VERSION`。

### Requirement: Timeline prepare and replay
Timeline prepare SHALL 生成当前seq/schema/time并校验而不改变fold；from_events逐条accept重建状态，空事件列表合法。

#### Scenario: Timeline prepare and replay
- **WHEN** 仅prepare一个合法事件
- **THEN** 事件尚未进入Timeline；调用者仍需存储确认及accept。

证据：`crates/codegen/chat-state/src/timeline.rs` — `prepare`。

### Requirement: Seed prompt coordinate reset
from_seed SHALL 清除继承User的prompt_index，再逐项以Seed cause追加；每个seed item产生独立事件。

#### Scenario: Seed prompt coordinate reset
- **WHEN** 从已有对话创建seed
- **THEN** 继承的用户项不沿用父分支prompt坐标。

证据：`crates/codegen/chat-state/src/timeline.rs` — `from_seed`。

### Requirement: Interrupted lifecycle recovery boundary
recover_interrupted SHALL 保留开放子代理及拥有开放子代理的workflow，对其余中断生命周期准备恢复事件；后端存活状态由外部协调者核对。

#### Scenario: Interrupted lifecycle recovery boundary
- **WHEN** 进程恢复时仍有open subagent
- **THEN** 不因本函数调用自动终结该子代理或其所属workflow。

证据：`crates/codegen/chat-state/src/timeline.rs` — `recover_interrupted`。

### Requirement: Stable surface replacement range
Surface替换 SHALL 验证两端当前可见、顺序合法、shadowed等于整个当前闭区间IDs，并保持System head约束；Compaction使用专用稳定range路径。

#### Scenario: Stable surface replacement range
- **WHEN** 压缩目标range已被其他替换改变
- **THEN** 拒绝失配range，不能仅按旧位置覆盖新surface。

证据：`crates/codegen/chat-state/src/timeline.rs` — `validate_surface_range`。

### Requirement: Message cause shape validation
Timeline SHALL 校验MessageCause与items形状：Assistant仅接受Assistant/BackendToolCall/Reasoning，ToolResult仅接受工具结果，MemoryContext为对应单个非空合成User。

#### Scenario: Message cause shape validation
- **WHEN** ToolResult cause携带普通User
- **THEN** 拒绝InvalidMessageShape，不将其作为工具结果接受。

证据：`crates/codegen/chat-state/src/timeline.rs` — `validate_messages`。

### Requirement: Direct user evidence validation boundary
DirectUser与Interjection SHALL 验证当前turn、对应permission evidence及各自synthetic/prompt形状；非空evidence要求至少非空Text，空evidence要求图片，但本校验不比较evidence与content正文相等。

#### Scenario: Direct user evidence validation boundary
- **WHEN** 合法DirectUser结构中的evidence正文和Text不同
- **THEN** 不能依赖本校验证明两份正文一致；上游输入构造仍需负责关联。

证据：`crates/codegen/chat-state/src/timeline.rs` — `validate_messages`。

### Requirement: Control context activation boundaries
控制上下文 SHALL 按layer延迟激活：StepEnded激活AgentRole、GoalDefinition、PlanPhase，TurnEnded激活全部待上下文；同layer待项由最新项覆盖，retired layers从active与pending移除。

#### Scenario: Control context activation boundaries
- **WHEN** 同一步内同layer多次transition
- **THEN** 仅最新待项在对应边界激活。

证据：`crates/codegen/chat-state/src/timeline.rs` — `active_control_contexts`。

### Requirement: Input admission routing and reservation
输入准入 SHALL 要求已完成prompt hook；Allow原子携带合法route，Block不携route或supersedes。user turn预留1至256个唯一可用FIFO输入，internal turn输入为空。

#### Scenario: Input admission routing and reservation
- **WHEN** turn结束而预留输入尚未消费
- **THEN** 释放预留，使后续turn可使用该输入。

证据：`crates/codegen/chat-state/src/timeline.rs` — `LifecycleFold`。

### Requirement: Workflow execution epochs and closure
Workflow SHALL 从epoch0开始，恢复严格增加1；Ended要求无所属open child且epoch匹配。Interrupted、Complete、Cancelled永久关闭，Failed等未永久关闭状态可恢复。

#### Scenario: Workflow execution epochs and closure
- **WHEN** 已Complete的workflow请求Resume
- **THEN** 拒绝恢复，不重新开启已永久关闭run。

证据：`crates/codegen/chat-state/src/timeline.rs` — `LifecycleFold`。

### Requirement: Compaction summary replacement settlement
Compaction SHALL 同时最多一个open且ID不复用；一次Summary绑定稳定target，仅允许一次对应replacement。Completed要求replacement恰一次，Failed要求尚无replacement。

#### Scenario: Compaction summary replacement settlement
- **WHEN** Summary后已成功替换，再提交Failed
- **THEN** 拒绝失败结算，不把已替换压缩描述为未生效失败。

证据：`crates/codegen/chat-state/src/timeline.rs` — `LifecycleFold`。

### Requirement: Notification deterministic identity
notification_id SHALL 对 owner_session_id、去除NotificationOwner的source identity和source version序列化后计算BLAKE3，并加notification-前缀；payload不参与ID摘要但receipt校验其合法性。

#### Scenario: Notification deterministic identity
- **WHEN** 同session及source/version重投时owner从Goal变为Session
- **THEN** 身份key保持相同；payload冲突由准入另行拒绝。

证据：`crates/codegen/chat-state/src/timeline.rs` — `notification_id`。

### Requirement: Notification inbox retention
通知接收 SHALL 保留已收ID和source/version索引；每monitor只保留最近16条pending progress，终态删除对应progress，TaskCompleted删除同任务StillRunning，物理历史不删除。

#### Scenario: Notification inbox retention
- **WHEN** 终态之后迟到Progress或StillRunning
- **THEN** 记录已收事实和索引，但不再次进入pending队列。

证据：`crates/codegen/chat-state/src/timeline.rs` — `apply_notification`。

### Requirement: Notification consumption ownership
通知消费 SHALL 要求当前active turn及非空唯一pending IDs；携带模型输入时校验普通、Plan或Goal归属，input=None不进入输入owner校验。

#### Scenario: Notification consumption ownership
- **WHEN** 使用Plan owner通知构造普通消费输入
- **THEN** 必须为plan_handoff turn且整批Plan身份一致。

证据：`crates/codegen/chat-state/src/timeline.rs` — `validate_notification`。

### Requirement: Hook gate classification
HookEventType SHALL 将UserPromptSubmit映射Prompt、PreToolUse映射Tool、Stop与SubagentStop映射Stop，其余事件映射Observe；事件记录不自动授予阻止权限。

#### Scenario: Hook gate classification
- **WHEN** 处理PostToolUse观察Hook
- **THEN** 使用Observe gate，不作为PreToolUse阻止工具准入。

证据：`crates/codegen/chat-state/src/timeline.rs` — `HookEventType`。

### Requirement: Frozen hook handler lifecycle
HookFold SHALL 校验handler数组index、全局唯一run ID、执行计划与结果；RunStarted要求Execute且Pending，Completed要求所有handler关闭且聚合结果精确一致。

#### Scenario: Frozen hook handler lifecycle
- **WHEN** 后一个Execute handler仍Pending而前一个未结束
- **THEN** 此fold未强制串行启动顺序；串行执行须由调用方实现。

证据：`crates/codegen/chat-state/src/timeline.rs` — `HookFold`。

### Requirement: Hook aggregate decisions
HookFold SHALL 对Prompt/Tool取顺序首个block，Observe固定观察；Stop优先成功ForceStop，否则汇总成功KeepWorking的reason/context，均无内容才AllowStop。

#### Scenario: Hook aggregate decisions
- **WHEN** 前一个KeepWorking只有additional_context
- **THEN** decisive_before不据此认定后续handler必须跳过。

证据：`crates/codegen/chat-state/src/timeline.rs` — `decisive_before`。

### Requirement: Hook failure policy restriction
Hook失败策略Block SHALL 仅用于UserPromptSubmit或PreToolUse；失败/超时/取消在这些gate及Block策略下携带Block reason，其他失败不携控制结果。

#### Scenario: Hook failure policy restriction
- **WHEN** 为观察Hook配置Block failure policy
- **THEN** handler plan校验拒绝，不把观察Hook变成准入门禁。

证据：`crates/codegen/chat-state/src/timeline.rs` — `valid_hook_handler_plan`。

### Requirement: Sideband independent event ledger
SidebandTimeline SHALL 校验schema6、canonical UUID、连续seq和非负时间并在ended后拒绝追加；from_events拒绝空列表，prepare在克隆上校验。

#### Scenario: Sideband independent event ledger
- **WHEN** 提供旧schema事件
- **THEN** 拒绝接受，不隐式迁移到当前schema。

证据：`crates/codegen/chat-state/src/sideband.rs` — `SidebandTimeline`。

### Requirement: Sideband frozen request budget
Sideband请求 SHALL 冻结purpose、source refs、route、执行身份、预算和可选输出schema；请求model/initiator/executor非空、max_attempts及input预算非零、output不可Some0。

#### Scenario: Sideband frozen request budget
- **WHEN** 请求指定输出上限而attempt省略上限
- **THEN** 拒绝该attempt，不取消冻结的输出约束。

证据：`crates/codegen/chat-state/src/sideband.rs` — `SidebandTimeline`。

### Requirement: Sideband attempt source containment
Sideband attempt SHALL 顺序编号且不超过次数与token预算；每个input range须被单个同timeline request source完整覆盖，selected IDs事件须落在attempt输入范围，context IDs事件落在request范围。

#### Scenario: Sideband attempt source containment
- **WHEN** 一个input range只能由两个相邻source拼接覆盖
- **THEN** 拒绝该范围，不合并多个source作为覆盖证明。

证据：`crates/codegen/chat-state/src/sideband.rs` — `SidebandTimeline`。

### Requirement: Sideband result and terminal validation
Sideband result SHALL 唯一且关联最新attempt，source_event_seqs精确为请求seq0与最新attempt seq，raw_output和finish非空；有schema时必须有符合schema的structured output。Completed需result且无error；Failed/Cancelled需非空error。

#### Scenario: Sideband result and terminal validation
- **WHEN** 无schema但raw与structured内容不一致
- **THEN** 本层不进行两者一致性验证；Failed/Cancelled也不强制已有Request。

证据：`crates/codegen/chat-state/src/sideband.rs` — `SidebandTimeline`。

### Requirement: Sideband parent and recall linkage
validate_parent SHALL 校验实际parent spawn与request的ID/purpose/source refs、initiator及所有SurfaceId；ContextRecall额外用冻结parent前缀限制selected为branch transcript中已完成压缩卸载且仍可追溯的项。

#### Scenario: Sideband parent and recall linkage
- **WHEN** ContextRecall将仍live未卸载项列为selected
- **THEN** 拒绝该选择，不把当前surface项当作卸载历史召回。

证据：`crates/codegen/chat-state/src/sideband.rs` — `validate_parent`。

### Requirement: Subagent seed linkage
validate_subagent_seed_link SHALL 将child首seed与给定parent timeline/spawn逐项比较subagent身份、security parent、context source/ref及normalized。

#### Scenario: Subagent seed linkage
- **WHEN** child seed的parent spawn seq不匹配
- **THEN** 拒绝链接；此函数比较调用者传入spawn，不自行读取真实parent存储。

证据：`crates/codegen/chat-state/src/timeline.rs` — `validate_subagent_seed_link`。

### Requirement: Subagent terminal result linkage
validate_subagent_result_link SHALL 先验证seed，再要求terminal result_ref精确引用child单个result事件，匹配身份、outcome、duration、工具次数、turns、tokens及error。

#### Scenario: Subagent terminal result linkage
- **WHEN** parent terminal统计与child result不同
- **THEN** 拒绝结果链接；output_ref或snapshot_ref内容不在此函数读取校验。

证据：`crates/codegen/chat-state/src/timeline.rs` — `validate_subagent_result_link`。

### Requirement: Image projection identity and provenance
Timeline图片投影 SHALL 要求当前surface revision、唯一branch图像来源、精确fingerprint/image_count、非空replacement与匹配工具call/carrier集合；描述来源须链接已有ImageDescription spawn覆盖的source事件。

#### Scenario: Image projection identity and provenance
- **WHEN** 调用方只提供合法格式的结果引用
- **THEN** 仍需满足spawn与source覆盖关系，但本层不读取外部描述结果正文。

证据：`crates/codegen/chat-state/src/timeline.rs` — `ImageProjection`。

### Requirement: Branch transcript preserves pruning provenance
branch provenance SHALL 展开选定分支leaf，ToolResultPrune保留原leaf正文；Rewind与ContextRebuild重建branch身份，图片投影更新对应leaf及引用，IntegrityRepair尽可能保留来源与出生序号。

#### Scenario: Branch transcript preserves pruning provenance
- **WHEN** 当前surface工具正文已裁剪
- **THEN** branch transcript仍可保留该工具原始正文，不等同当前surface文本。

证据：`crates/codegen/chat-state/src/timeline.rs` — `fold_branch_provenance`。

### Requirement: Completed compaction unloaded history
已卸载branch IDs SHALL 只来自最终Completed的压缩及有效完整target；失败或未完成压缩不据Summary单独标记卸载。

#### Scenario: Completed compaction unloaded history
- **WHEN** 存在Summary但压缩未Completed
- **THEN** 不将该目标作为completed compaction unloaded历史。

证据：`crates/codegen/chat-state/src/timeline.rs` — `completed_compaction_unloaded_branch_ids`。

### Requirement: User title override permanence
SessionTitle SHALL 按trim后1至160字符验证但保存原title；Generated/Fallback须关联SessionTitle sideband spawn，历史存在User title后拒绝后续自动标题。

#### Scenario: User title override permanence
- **WHEN** 用户设置标题后又生成自动标题
- **THEN** 自动标题被拒绝，不覆盖用户选择。

证据：`crates/codegen/chat-state/src/timeline.rs` — `SessionTitle`。

### Requirement: Trajectory projection and snapshot hydration
TrajectoryProjector SHALL 生成schema4展示投影；增量rows的details保持Null，snapshot按传入Timeline事件填充details，dirty索引显式清除。该projector不独立验证事件序列或与snapshot参数身份一致。

#### Scenario: Trajectory projection and snapshot hydration
- **WHEN** 仅调用snapshot而未clear_dirty_rows
- **THEN** dirty集合保持，调用者仍需显式清除。

证据：`crates/codegen/chat-state/src/trajectory.rs` — `TrajectoryProjector`。

### Requirement: Trajectory surface visibility
Trajectory投影 SHALL 追踪每行当前surface项数；替换移除全部当前项后标Shadowed，仅部分替换仍为Current。Input及携input通知消费贡献surface项，无input消费不贡献。

#### Scenario: Trajectory surface visibility
- **WHEN** 混合控制行仍有一项当前可见
- **THEN** 该行保持Current，不因其他项被替换而整行Shadowed。

证据：`crates/codegen/chat-state/src/trajectory.rs` — `TrajectoryProjector`。

### Requirement: Trajectory repair provenance links
工具完整性展示 SHALL 为隔离保留原行details及反链，修复行最多保存16个不同source seq但另保留完整计数；pairing修复不冒称隔离。

#### Scenario: Trajectory repair provenance links
- **WHEN** 一次修复涉及超过16条来源
- **THEN** 前向列表有界，所有来源仍可得到repaired_by关系。

证据：`crates/codegen/chat-state/src/trajectory.rs` — `project_tool_integrity`。

### Requirement: Trajectory state outcome and severity
Trajectory严重级别 SHALL 按state与outcome的ASCII小写精确值映射，Error优先于Warning；ToolCompleted的state固定completed，真实失败可由outcome及severity表达。

#### Scenario: Trajectory state outcome and severity
- **WHEN** 工具outcome为not_dispatched
- **THEN** 即使state为completed，仍报告Error，不视为成功。

证据：`crates/codegen/chat-state/src/trajectory.rs` — `trajectory_issue_severity`。

### Requirement: Trajectory summary truncation
Trajectory摘要截断 SHALL 按Unicode scalar字符数保留前N个字符，超长追加省略号；不将此长度理解为UTF8字节数或grapheme数。

#### Scenario: Trajectory summary truncation
- **WHEN** 摘要含多字节中文
- **THEN** 按字符计截断，不按UTF8字节切断编码。

证据：`crates/codegen/chat-state/src/trajectory.rs` — `truncate`。

### Requirement: Complete turn boundary heuristic
完整轮次扫描 SHALL 要求User块后至少有Assistant且声明工具ID全部有结果；重复ID用集合合并，不验证身份唯一性。

#### Scenario: Complete turn boundary heuristic
- **WHEN** 遇未完成工具组
- **THEN** 停止后续轮次判定。

证据：`crates/codegen/chat-state/src/compaction_utils.rs` — `complete_turn_ends`。

### Requirement: Summary preparation modes
普通摘要准备 SHALL 删除全部ToolResult、清Assistant calls并加入工具名提示，再删除Reasoning；verbatim按参数去Reasoning并仅移除尾部未完成调用Assistant。

#### Scenario: Summary preparation modes
- **WHEN** ToolResult携带图片
- **THEN** 图片随结果删除，不保证所有图片保留。

证据：`crates/codegen/chat-state/src/compaction_utils.rs` — `prepare_conversation_for_summarization`。

### Requirement: Conversation budget fitting heuristic
预算fit SHALL 优先保留首System及尾部可容纳后缀，必要时回退截断；不提供严格整轮或整体token上限保证。

#### Scenario: Conversation budget fitting heuristic
- **WHEN** User包含多个Text part及图片
- **THEN** 逐part截断和保留图片仍可能超预算。

证据：`crates/codegen/chat-state/src/compaction_utils.rs` — `fit_conversation_to_budget`。

### Requirement: User query metadata extraction
query提取 SHALL 优先首个完整user_query块并去固定metadata块；无完整wrapper时处理全文，不解析任意XML属性或嵌套。

#### Scenario: User query metadata extraction
- **WHEN** 最新User提取为空
- **THEN** last query返回None，不回退更早User。

证据：`crates/codegen/chat-state/src/compaction_utils.rs` — `extract_user_query`。

### Requirement: Real user query classification
真实User判定 SHALL 排除synthetic_reason，非合成图片输入视为真实，纯文本依提取结果及固定继续提示判定。

#### Scenario: Real user query classification
- **WHEN** 输入仅含图片
- **THEN** 真实query列表可包含空串。

证据：`crates/codegen/chat-state/src/compaction_utils.rs` — `is_real_user_turn`。

### Requirement: Compaction range planning
range规划 SHALL 使用prompt_index、suffix保留预算及source最小预算选择稳定ID区间，必要时尝试最新轮response-group边界。

#### Scenario: Compaction range planning
- **WHEN** surface和IDs长度不同或无prompt坐标
- **THEN** 返回None，不猜测目标区间。

证据：`crates/codegen/chat-state/src/compaction_utils.rs` — `plan_compaction_range`。

### Requirement: Compaction state context assembly
CompactionStateContext SHALL 原样组装调用者任务、子代理、server与todo，并将BTreeSet路径转有序Vec；不查询实际运行状态。

#### Scenario: Compaction state context assembly
- **WHEN** 传入已完成todo
- **THEN** 仍保留，不自动过滤。

证据：`crates/codegen/chat-state/src/compaction_utils.rs` — `CompactionStateContext`。

### Requirement: Compacted history sanitation scope
sanitize SHALL 删除此前无Assistant声明的工具结果，声明ID不被消费且不在User边界清空。

#### Scenario: Compacted history sanitation scope
- **WHEN** 此前声明ID有重复结果
- **THEN** 该检查不单独拒绝重复或保证邻接配对。

证据：`crates/codegen/chat-state/src/compaction_utils.rs` — `sanitize_compacted_history`。

### Requirement: History quarantine and pairing sequence
repair_history SHALL 先隔离损坏工具身份，再去重结果、剥离错位结果、补悬空结果；身份损坏包括空ID/name或重复ID。

#### Scenario: History quarantine and pairing sequence
- **WHEN** 参数非法JSON但身份合法
- **THEN** 不按身份损坏隔离，留给preflight。

证据：`crates/codegen/chat-state/src/compaction_utils.rs` — `repair_history`。

### Requirement: Actor usage attribution
模型调用 SHALL 同时入prompt与session账本，缺失或空model ID回退配置model；子代理用量始终入session，仅attribute_to_prompt时入prompt。

#### Scenario: Actor usage attribution
- **WHEN** 空子代理汇总且incomplete为false
- **THEN** 直接无操作，不创建prompt账本。

证据：`crates/codegen/chat-state/src/actor/mutations.rs` — `record_subagent_usage`。

### Requirement: Independent incomplete usage flags
mark_usage_incomplete SHALL 按prompt/session独立开关置不完整标记，可创建空prompt账本。

#### Scenario: Independent incomplete usage flags
- **WHEN** 只指定session不完整
- **THEN** 不把当前prompt一并标记不完整。

证据：`crates/codegen/chat-state/src/actor/mutations.rs` — `mark_usage_incomplete`。

### Requirement: Durable rewind bookkeeping
rewind SHALL 要求target小于next_prompt_index，先提交Rewind再清capture与prompt账本，更新surface投影；session账本保留。

#### Scenario: Durable rewind bookkeeping
- **WHEN** target大于等于当前next_prompt_index
- **THEN** 返回InvalidRewindTarget，不提交替换。

证据：`crates/codegen/chat-state/src/actor/mutations.rs` — `rewind_durably`。

### Requirement: Turn capture event provenance
BeginTurnCapture SHALL 记录next_seq并重置压缩标志；take先移除capture再按branch消息出生来源提取。

#### Scenario: Turn capture event provenance
- **WHEN** 重复take且未重新begin
- **THEN** 返回None，不重复交付相同capture。

证据：`crates/codegen/chat-state/src/actor/mod.rs` — `BeginTurnCapture`。

### Requirement: Unavailable query return distinctions
handle查询 SHALL 保留各自错误约定：conversation可默认空，usage try查询返回Err，received_notification_id区分不可用与无receipt。

#### Scenario: Unavailable query return distinctions
- **WHEN** actor关闭且get_conversation返回空
- **THEN** 不能据此证明实际持久历史为空。

证据：`crates/codegen/chat-state/src/handle.rs` — `try_get_session_usage`。

### Requirement: Latest assistant text query
助手文本查询 SHALL 逆序忽略trim后空白Assistant并返回原始正文；in_turn版本在prompt User或新prompt语义User边界停止。

#### Scenario: Latest assistant text query
- **WHEN** 最后Assistant只有空白
- **THEN** 继续查找前一非空Assistant，仍保留其原始空白格式。

证据：`crates/codegen/chat-state/src/actor/queries.rs` — `get_last_assistant_text`。

### Requirement: Latest model metadata projection
模型metadata SHALL 仅取当前surface最后一个Assistant的字段，无Assistant返回默认值；不跳过字段为空的Assistant寻找旧model。

#### Scenario: Latest model metadata projection
- **WHEN** 最后Assistant的model字段为空而更早字段非空
- **THEN** 返回空字段，不沿用旧诊断。

证据：`crates/codegen/chat-state/src/actor/queries.rs` — `get_last_model_metadata`。

### Requirement: Automatic compaction query delegation
自动压缩查询 SHALL 用当前projected_tokens和配置context_window委托token_estimation阈值判断，触发时返回压力、窗口和截断利用率。

#### Scenario: Automatic compaction query delegation
- **WHEN** 配置窗口改变而压力保持
- **THEN** 下一次查询按新窗口判断；本接口不启动压缩作业。

证据：`crates/codegen/chat-state/src/actor/queries.rs` — `check_auto_compact_needed`。

### Requirement: Atomic surface and revision query
surface及revision查询 SHALL 在同一次actor命令处理中读取二者；materialization无事件时返回None，有事件时引用从seq0到末事件的范围。

#### Scenario: Atomic surface and revision query
- **WHEN** 调用者需要乐观并发替换
- **THEN** 可读取同一状态的surface/revision，再由替换入口验证revision。

证据：`crates/codegen/chat-state/src/actor/mod.rs` — `GetConversationWithRevision`。

### Requirement: Request and tool lifecycle ownership
Request与Tool SHALL 在当前turn/step下启动且ID全历史唯一；terminal关闭open记录，ToolCompleted名称须匹配。

#### Scenario: Request and tool lifecycle ownership
- **WHEN** 重复使用已结束request ID
- **THEN** 拒绝启动，不以结束释放身份。

证据：`crates/codegen/chat-state/src/timeline.rs` — `LifecycleFold`。

### Requirement: Input submission idempotence
输入提交 SHALL 按input ID查已有Submitted，intent及payload相同返回原事件，冲突拒绝。

#### Scenario: Input submission idempotence
- **WHEN** 同ID重试且payload改变
- **THEN** 返回InvalidInput，不追加第二个提交。

证据：`crates/codegen/chat-state/src/actor/mod.rs` — `SubmitInputDurably`。

### Requirement: Explicit repair active turn guard
显式repair SHALL 读取可选共享active flag，true时连dry-run也拒绝；无flag按false处理。

#### Scenario: Explicit repair active turn guard
- **WHEN** 调用者未提供flag
- **THEN** 本入口不独立证明当前无活动turn。

证据：`crates/codegen/chat-state/src/actor/mod.rs` — `RepairHistory`。

### Requirement: Surface replacement no operation
通用surface替换 SHALL 在候选与当前JSON相等时直接成功，不提交事件或重置续传。

#### Scenario: Surface replacement no operation
- **WHEN** 相同surface带不同cause请求替换
- **THEN** 无变化路径不验证该cause，不产生对应事实。

证据：`crates/codegen/chat-state/src/actor/mutations.rs` — `replace_conversation_durably`。

### Requirement: Chat state feature and metadata surfaces
chat-state SHALL 导出actor、命令、事件、Timeline、Sideband、Trajectory、usage与辅助类型；default-bazel为空feature，不单独改变本包实现。

#### Scenario: Chat state feature and metadata surfaces
- **WHEN** 启用default-bazel
- **THEN** 本包不因此启用额外运行时逻辑。

证据：`crates/codegen/chat-state/src/lib.rs` — `pub mod events`。

### Requirement: Input reroute transitions
Input reroute SHALL 只允许FIFO到当前turn Steer，或已结束seen turn的Steer回FIFO；批次先全部验证后应用。

#### Scenario: Input reroute transitions
- **WHEN** 试图直接Steer到另一turn
- **THEN** 拒绝该转换，不部分更新批次。

证据：`crates/codegen/chat-state/src/timeline.rs` — `LifecycleFold`。

