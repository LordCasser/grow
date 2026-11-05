# TODO 与普通子任务生命周期

## 事实与 owner

`tool-types::planning::LocalPlan` 是已序列化事实，ChatState Timeline 持久化；Shell SessionActor 在 step_control_gate 下修改和发布。`PlanStep` 已有 id/content/requirements/priority/status/removed，`PlanLink` 已有 task id、parent_step_id、child_session_id、terminal、required、pending/review revision、result/dispatch failure。`ReplanChange` 保存来源修订、完整步骤快照、不可变投递进度；现已支持 link 绑定与 spawn/edit 排序、失败重投、Consumed/Handled 和 tombstone。coordinator 拥有真实 child execution，不从 TODO status 推导进程终态。

父亲自核对 TaskTool 在 Link ACK 后把父项 JSON 拼到 prompt；关联可不传。`local_planning_context` 被 turn 合入 memory_reminder，ChatState request_builder 将不同文本持久追加为 MemoryContext，违背它的 request-only 注释。当前 Replan 通过 ParentMessage 投递，普通 parent notice 会向 UI 发布。这些是此次必须改变的实际链路。

## 数据结构

保留 LocalPlan 作为唯一计划，不新增全局 Task 图或每 child 拷贝整份父计划。

| 现有记录 | 本次职责 |
| --- | --- |
| LocalPlan(session_id, scope_id, revision) | 原会话/工作范围的步骤、link、变更、wait、review权威；读取快照另带 Timeline seq |
| PlanStep(id, content, requirements, priority, status, removed) | 稳定 TODO 身份；删除保留 tombstone，ID 不复用 |
| PlanLink(id, parent_step_id, child_session_id, terminal, required, revisions, result_id, dispatch_failure) | 一项 TODO 到多个执行的关系；task ID 不同即不同执行，结果/验收独立保留 |
| ReplanChange(id, link_id, child_session_id, step_id, source_revision, step snapshot, progress) | 每来源变更的投递、消费、处理责任；提供派生 directive：removed 或 cancelled 为 stop，其余为 replan，无需再存可相互矛盾的第二状态 |
| Child Seed → Parent Spawn | 已有精确父 Timeline/Spawn/subagent 身份，Spawn 保存 parent_step_id；冷恢复核对该事实，不从 prompt 猜关联 |

stop 是针对原 link 的不可撤回终止意图。重开 cancelled 步骤可用于新执行，不撤销旧执行的收尾；Pending coalescing 不得用后来的普通编辑替换 stop，已 Dispatching/Received 不改 payload。可派生 `link_stop_requested` 查询扫描该 link 的控制事实，不新建独立永久取消登记。已 terminal child 只记录 unapplied，不重新启动。

stop分类在shared planning owner统一提供`ReplanChange::is_stop()`（removed或Cancelled）及`LocalPlan::link_stop_requested(&str)`派生查询，固定消息、renderer和child ACK复用这条规则，不另存directive字段。首次stop仍可替换同link尚未派发的普通Pending；一旦已有stop，后续同link普通编辑/重开不再产生推进新目标的Pending，保留原stop身份/payload/progress，并将后续变化记录为Unapplied原因。link的pending_source_revision从真正未结算控制重算，不被这些Unapplied版本抬高；同step后来新task/link独立接受新要求。终态/undispatched仍优先保留既有真实失败/未应用语义，不启动新执行。

child接纳stop后的handling增加`ReplanOutcome::Stopping` unit值（wire stopping），表示已接纳收尾指令，绝不表示进程或命令已结束。`advance_replan`只允许stop的Consumed→Handled使用Stopping，普通replan不能使用它；对stop提供no_change_reason明确拒绝，避免把“无需改计划”解释为继续执行许可。Shell HandleReplans对stop在同一真实Consumed证明后记录Stopping，对普通编辑沿现Updated/NoChange。parent仍逐阶段持久确认同一ACK，required交付/运行命令/后代和最终canonical Result的门槛保持。此改动约束当前写owner的新转换，不重写或丢弃历史已提交replan事实，也不为新语义添加legacy开关或第二份状态。


## 普通委派准入

模型 `spawn_subagent`（含 resume_from）必须提供非空 parent_step_id，指向当前 scope 中未 removed、状态 pending/in_progress 的步骤；省略、空串、已完成/取消或其他 scope 拒绝，且不创建 child、worktree/provider。TaskTool 调用现 Link 命令，在同一 gate 验证并 durable ACK 后才 dispatch；后台 spawn 也不能先返回无绑定的真实执行。TaskToolInput 的该字段改为无serde default的必填`String`，schema required，missing/null在解码时拒绝，空白串在工具运行时拒绝；保留原稳定ID，不按可选模型参数的sentinel规则修剪/改写。内部 SubagentRequest 的 Option 仍表达 Workflow 等非模型所有者的实际不同语义，普通工具仅传Some，不伪造步骤。required_deliverable 仍只决定验收义务，不决定是否绑定。

Link的原子准入仍归Shell SessionActor：持现step gate检查该prompt仍是admitted regular turn、canonical plan属于本session，并仅在这份当前scope快照中检查被选项pending/in_progress且非tombstone，才durable追加原PlanLink。current scope由现ChatState planning_state和SelectScope拥有，不能每个新prompt另用`work:{prompt_id}`重算后拒绝跨turn保留的合法计划，也不查旧scope补step。tools只负责必填参数和使用该已确认能力，不从TodoState/LocalPlanProjection副本自行判断eligibility。相同task身份或不可用owner保持原拒绝/未确认语义，不能先spawn后补link。gate之前已完成的edit/remove必须被准入看到；Link后edit/remove由已有变更投递及首请求最新上下文承担，不通过锁住整个child执行阻止修改。

先引入读取和内部上下文，再去掉 `prompt.push_str`，确保 child 第一请求已知道父项。coordinator/runner 在发送首请求前从本次真实 parent link/Spawn 读取，绑定初次构造和冷恢复，不借 resume 来源的旧 parent step。Link 后发生编辑/删除时，首请求使用最新事实，或保留明确补发责任；不能拿旧要求且丢通知。

## todo_read 共享契约

公开输入：`{target?: "self" | "parent", step_id?: string, cursor?: string, limit?: integer}`。默认 self，limit 默认 20、范围 1..50；无任意 session_id/subagent_id，无写操作或模型调用。self 通过新增 PlanningCommand::Read 到本 session owner；parent 经 session-bound ChannelBackend 由 coordinator 按真实 child 的 immediate_parent_session_id 解析，Shell runner 读取该精确父 handle 的 ChatState planning_state。不得复用允许 lifecycle root 的宽观察规则。根调用 parent 返回 no_parent，owner 不可用明确 unavailable，未建计划返回 plan:null，不能把不可用伪装空列表。

返回 snapshot 的 session_id/scope_id/revision/timeline_seq、可选 delegation(link_id/step_id/parent identity)、counts、steps/links/waits/replans/reviews 与 next_cursor。分页跨所有选中记录统一计数，顺序固定为 steps→links→waits→replans→reviews，每种保持原稳定顺序；step_id 过滤包括该项 tombstone 和与它有关的 link、变更、wait、review，不影响授权。单页序列化最多 256 KiB；单记录不裁决策事实，若本条不能放下明确 item_too_large；通常最多两条 64 KiB 字段记录可放下。cursor 为不透明有界值，绑定调用者/target/目标session/scope/timeline_seq/step filter/下一偏移。读快照变更后旧 cursor 明确 stale_cursor，要求从头读取，不能跨修订拼接。cursor 不授予访问权，运行时每次重新验证直属关系。只读查询不创建 scope、不递增 revision、不发 ACP Plan 通知。

公共类型放现 tool-types planning 模块（必要时其子模块）；读页纯投影不维护新状态。共享Rust名称固定为：`PlanReadTarget::{SelfPlan, Parent}`（wire self/parent）、`PlanReadQuery {step_id:Option<String>, cursor:Option<String>, limit:Option<u32>}`、`PlanDelegation {parent_session_id, parent_scope_id, parent_step_id, task_id, child_session_id}`、`PlanningReadSnapshot {timeline_seq:u64, plan:LocalPlan, delegation:Option<PlanDelegation>}`。`PlanReadInput`为target加query字段（wire snake_case，拒绝未知字段），`PlanReadOutput {target, plan:Option<PlanReadPage>}`；无计划时plan:null。`PlanReadPage`拥有session_id/scope_id/revision/timeline_seq/delegation、counts（源集合计数）与matched_counts（过滤后）、五类记录数组、next_cursor。共享纯函数`project_plan_read(reader_session_id:&str,target:PlanReadTarget,snapshot:&PlanningReadSnapshot,query:&PlanReadQuery)->Result<PlanReadPage,PlanReadError>`，错误为invalid_query/invalid_cursor/cursor_mismatch/stale_cursor/item_too_large；不在纯函数处理parent授权。内部snapshot直接持有真实快照；调用者传入query/cursor，投影只处理已授权来源。Backend/ChildRunner 新增 parent-planning 查询能力，工具不接触 MvpAgent 全局 sessions。

分页输出进一步固定：`PlanReadCounts {steps, links, waits, replans, reviews}` 五项均为u32；`counts`为原快照各类总数，`matched_counts`为过滤后的总数。`PlanReadPage`的五类数组直接使用现有PlanStep/PlanLink/PlanWait/ReplanChange/DeliveryReview，不复制另一套持久类型，嵌套事实保留现有wire命名；新增读取envelope字段为snake_case，输入拒绝未知字段。step_id只匹配当前计划的稳定步骤身份，保留tombstone；不存在的步骤返回五类空数组。相关wait包括明确step_ids、该link的Task dependency和scope-wide空step_ids等待；reviews通过选中link_id关联。cursor最多4096 UTF-8字节，结构格式由实现私有持有，拒绝畸形或越界offset；大小写/空白不归一化稳定身份。cursor绑定reader/target/source session/scope/timeline_seq/plan revision/step filter，limit允许在下一页改变；每次仍由owner重新授权。每页256 KiB限制包含`PlanReadOutput {target,plan}`序列化外壳与next_cursor，limit跨五类统一计数，大小限制可提前结束页，单条完整记录不能装入空页时返回item_too_large，不裁正文。新公共接口仅承载该现成读取能力，不新增缓存、分页登记或运行时状态。

来源`timeline_seq`固定为`ChatStateHandle::planning_state()`返回的计划事实seq，不取整个日志最后事件；连续只读工具的Tool/Step事件不会使同一计划页失效，计划事实改变仍使旧cursor过期。self读取的`PlanningCommand::Read {respond_to: oneshot::Sender<Result<Option<PlanningReadSnapshot>, String>>}`不要求prompt身份，不进入修改计划的step gate、不调用hydrate/commit或schedule_planning_delivery；它直接从同一个ChatState owner查询并映射seq/plan，delegation为None。读取失败保留Err，无计划返回Ok(None)，工具用已有SessionIdResource生成纯投影的reader_session_id；不能用输入参数指定reader。这个内部Read只提供快照，分页仍统一走tool-types纯函数，不新增第二份模型状态。

直属parent读取分两侧承诺：tools的session-bound `SubagentBackend::read_parent_plan(cancellation)`发送host-only `SubagentEvent::ReadParentPlanning(ParentPlanningRequest)`，其中source_session_id只取ChannelBackend绑定身份。`ParentPlanningRequest`含source_session_id、cancellation、respond_to；返回`Result<Option<PlanningReadSnapshot>, ParentPlanningReadError>`，错误固定NoParent/Unavailable/TimedOut/InvalidEvidence，wire code分别no_parent/unavailable/timed_out/invalid_evidence。unbound backend/通道失败不冒充NoParent。coordinator依据source的真实active child_session_id找到immediate_parent_session_id和该次request.id/parent_step_id，不使用lifecycle-root观测权限、不接受模型target；已知pending/suspended/completed但不可执行的来源返回Unavailable，没有child边的根调用返回NoParent。读取不改变coordinator生命周期；RetiringChild仅补保存已有active已确认的child_session_id，使completed cache淘汰后仍能辨认retiring来源并返回Unavailable，不拿task id猜session id、不新增map/cache。将已解析的parent id/task id/optional step id交`ChildRunner::read_parent_plan`异步处理，不在coordinator等待Shell或模型。readonly能力不继承ask消息的Workflow排除，若工具对该child开放，仍只沿真实直接边读取。

Shell runner在同一调用中找到真实source child与直属parent handle，只查parent的ChatState planning_state，完整映射同一个seq/plan。存在parent_step_id时，必须在该快照找到同task id、child_session_id、parent_step_id的原link，才生成PlanDelegation（scope来自该快照）；缺失/不匹配返回InvalidEvidence，不能借来源prompt或root的计划补关联。没有parent_step_id的既有内部执行仍可读取实际直属parent的计划，delegation为None；本change后续强制普通spawn绑定后，这一分支只表达真实无模型TODO的内部入口。parent无计划且没有已绑定step返回Ok(None)；已绑定step但计划缺失属InvalidEvidence。不存在或已关闭owner为Unavailable，5秒只读期限耗尽为TimedOut。查询不创建消息receipt、Plan/Goal通知、provider工作或任何持久事实；完成时释放所有临时快照。

无计划路径仍先用共享`PlanReadQuery::validate() -> Result<(), PlanReadError>`检查limit、step_id及cursor结构/字节界限，不在工具层复制规则；纯投影同样复用该校验。合法query且无cursor、来源确实无计划才返回plan:null；带合法旧cursor而当前计划已不存在返回stale_cursor，不能把快照消失变成一页空事实。source不可用或无parent仍按各自错误返回，不用None掩盖。

todo_read注册沿现原生Tool/ToolInput/ToolOutput与registry入口，不用Dynamic旁路。具体工具为`TodoReadTool`、固定协议名`todo_read`，直接使用共享PlanReadInput/PlanReadOutput；ToolOutput trait在现tool-runtime的render模块为共享输出补实现，避免另建透明wrapper。工具类型增加`ToolKind::PlanRead`（模板key为`tools.by_kind.plan_read`），与现Plan写入口分开，防止同kind→name映射互相覆盖；主/子标准toolset纳入同一工具。主规范client-surfaces要求闭合词表新增kind时共享版本与schema同步推进，因此本次canonical grow/tool版本由6改为7，所有既有kind保持原语义，schema/接口说明与相应回归同片适配。ToolCapabilities max_access为None，读取仍须经过既有tool授权和behavior/profile限制，不授予进程或文件访问能力。self用PlanningRuntimeHandle Read+SessionIdResource，parent用绑定backend；统一调用query.validate及project_plan_read，错误code保留共享值。ToolInput/Output增加TodoRead变体，ACP只完成当前工具行及提供原始结构输出，不发送ACP Plan或修改TodoState/LocalPlanProjection；模型输出完整JSON不走TodoWrite摘要。现资源查询和工具执行取消遵循5秒有界观察，只结束本次等待，不修改session/child生命周期。

## 请求内上下文

ChatState BuildConversationRequest 新增独立 optional runtime_context，和已持久 memory_reminder 分开。已有 build_request wrapper 无 context 时传 None；在 request 副本追加 host-owned synthetic reminder，加入 token/byte/envelope 计算及 source_projection，绝不调用 Messages/Surface append、不建立新 SurfaceId、不污染恢复历史。不能在 sampler 发出前临时拼接而绕过 budget/native projection。

Shell 每次请求从自身计划及直属 parent 快照构造有界上下文，最多 8 KiB。关联步骤必须优先出现（即使在第12项以外），包括来源 session/scope/seq/revision、link、步骤当前内容摘要/requirements摘要、位置/总数、少量邻居/整体摘要、待 stop/replan，以及 `todo_read` 获取完整事实的指引。自身无 TODO 但有父绑定时仍注入。父不可用明确说不可用，保留从 Seed/Spawn 或已确认 link 得到的身份，不当成没有任务。模型工具名通过现 template renderer 解析，不能写死随机化后的别名。无需把完整父 prompt/同级子任务正文复制给 child。

请求构造的路由身份只需host内不可变三元组`ParentPlanBinding {parent_session_id, task_id, parent_step_id}`，可作为`StartupHints`新增`#[serde(skip)] pub(crate) delegated_parent_plan: Option<ParentPlanBinding>`字段，拥有者为本次spawn/restore，child身份沿现session info取得。不复用StartupHints现parent_session_id：它在nested执行仍指lifecycle root，不能作为读父计划的直接身份。新spawn使用已提交本次Spawn/request及`ctx.security_parent_session_id`构造；恢复从已经验证的原Spawn还原并核对本次request，不从resume来源旧prompt/MemoryContext猜。它只在parent读取失败时保留已证的委派身份，不缓存plan、seq、scope或目标正文，不增加第二份持久关系；无TODO绑定的内部执行保持None。

`local_planning_context`每次通过现ChatState查询self快照；有上述绑定的普通child再通过session-bound backend读取parent，复用2.2c/d的真实直接边及exact link验证。父读取成功后核对delegation三元组与出生身份一致，再用该snapshot的真实scope/seq/revision/关联步骤位置生成摘要；不匹配明确显示invalid_evidence，不能退到root或旧snapshot。查询失败用实际error code与已知绑定标明不可用，不能提前return None导致自身无计划的child失去任务归属。只读parent查询最多5秒，不因临时缺失创建消息、恢复执行或另开provider；self owner错误沿既有构造错误传播。读取和快照构造保持在本会话step gate之前，不等待parent的mutation gate。

renderer只负责从这些已确认快照生成字符串，采用纯函数或现模块内helper；不负责再次授权或读取磁盘。8 KiB是最终runtime_context字符串UTF-8字节上限，含固定指引、所有身份、JSON转义和摘要分隔符。先放关联parent项及来源/位置/stop责任，再按剩余空间放自身义务与少量邻居；JSON逐字段摘要或裁减前需核最终字节数，不按字符上限替代。可省略的字段注明摘要/数量，不把截断内容冒充完整决定；完整事实通过todo_read按需读取。工具名只来自现bridge/TemplateRenderer的PlanRead映射；未开放该工具时如实标记读取入口不可用，不输出写死的fallback调用。资源读取和父观察共用绝对5秒观察期限：若名称尚未解析即超时则入口不可用；若已解析成功而后续父观察超时，保留真实alias并单独标明parent timed_out，不能把一次观察失败误报为工具未注册。该已解析值仅为本次请求的局部结果，不添缓存或延长期限。第一请求和后续请求沿同build_turn_request入口，context不持久化、不向ACP聊天投影；只有完整链路验证后再移除TaskTool的旧prompt拼接。


## 固定控制消息与收尾

扩大现 typed Replan 投递，增加 host-only `NotificationSource::PlanGuidance {parent_session_id:String, message_id:String}`（specific link/revision/directive以已确认 change 为准）。它拥有独立的SourceIdentity，`agent_message()`仍返回相同parent/message引用且interrupt固定true，owner为Session；沿现notification receipt/Consumed AgentMessage和replan进度。Timeline的parent receipt索引、已消费payload保留及精确重复接纳覆盖这一variant，普通ParentMessage/AgentReply身份不与它混用；通知版本沿现agent-message source版本，不增加平行receipt协议。它不向普通ParentMessage UI notice投影，live与cold communication history/replay均隐藏；实际模型AgentMessage与内部取证/ACK引用仍保留。展示筛选仅在现notification_inbox通知生成/读回和communication_history receipt identity/restore入口识别PlanGuidance，不从通用agent_message()移除它；后者仍负责模型输入和内部回执。history追加索引只为可展示receipt构造ReceiptSource，不能给隐藏来源调用期待notice存在的路径。用户普通消息即使正文相同，仍是原可见ParentMessage，不能因文本前缀隐藏。

控制的归属验证放在Shell runner已解析source/target的interaction边界：发送前只读source的同一committed plan，核对session、原link/task、child真实SID、step及同一已claim不可变change身份/payload。不能只验证“直属父可以发普通消息”，也不能用当前step文本取代旧已claim payload；progress可前进而payload不变。可接纳的source事实为Dispatching/Received/Consumed/Handled，Pending尚未claim、Unapplied已明确处置，均不能提供新投递依据；比较来源与incoming的不可变字段时只忽略进度差异。request选择的task、source真实handle身份、resolved target SID必须与原link及change一致。读取owner失败为unconfirmed，缺失/不匹配的来源证据为rejected，不向target发SessionCommand或创建receipt/plan。观察完成后仍核对调用取消和目标可用性，不能因一次等待越过现有接纳边界。tools的model interaction入口继续拒绝Replan/ReplanHandled，模型Send只能走普通消息；不新开可传source的模型工具。

child在同一step gate内先验证接纳边界与不可变incoming payload，将完整`ReplanChange::Received`持久提交到已有LocalPlan，再接纳同一`PlanGuidance`的durable notification receipt，最后发送soft-interrupt信号。接纳时核对本次host-only ParentPlanBinding的parent/task/step和本child SID；binding从current Spawn恢复，不能通过正文或后代root猜身份。仅新的控制接纳要求现execution/steering边界仍开放；同一完整责任加exact receipt的确认重试仍沿原只读确认语义，不重复中断或推进进度。这样中断后不存在“模型已消费控制而本地责任尚未写入”的窗口。若在plan已提交、receipt尚未确认时失败或退出，原LocalPlan保留未Handled责任，parent保留Dispatching并用原identity/payload重试补齐receipt；不能清掉该前缀或重开child。接纳重试先复核已存payload，不把Received/Consumed/Handled倒退；已有receipt也不能跳过缺失规划责任的校验。只在receipt与local责任均已确认后向parent返回received；ACK丢失沿现exact retry收敛，无需跨事实原子事务或第二份持久关系。生产failure-prefix验证须覆盖这两次提交之间的断点，以及receipt ACK丢失和cold replay。

控制接纳和处理使用typed receipt身份。新PlanGuidance与普通ParentMessage即使parent/message_id相同，也有独立receipt identity；Shell的agent_message_receipt helper不能因只比较sender/message而让普通消息占用控制身份。该helper仅新增typed控制与普通消息的身份域隔离；ParentMessage与AgentReply在各自既有sender/message冲突检查上的行为保持，避免顺带放宽普通reply规则。新PlanGuidance只能以完整、exact匹配的AgentMessage输入Consumed，沿现Timeline对AgentReply的同一消费边界，不能用input:None或泛User消息跳过模型接收。HandleReplans复用现`consumed_notification_turn(source,version)`查询精确PlanGuidance receipt，并核其parent和本次绑定；不能继续以`consumed_agent_messages(ids)`仅按message_id的集合来证明控制消费。反例为先消费同parent/message_id的普通ParentMessage，再接纳尚未消费的typed控制：此时不能ACK stop/replan；只有typed receipt实际Consumed后才允许进度前移。不增加平行消费登记或另一套持久化证明。该消费判定归接收child的SessionActor HandleReplans；它须用本次host binding生成exact PlanGuidance source，并在写Consumed/Handled前证明source/version的真实消费。向父发送Handled时同一owner只选择相同binding与change对应的typed receipt；coordinator继续保证实际直属reply route，runner沿现child canonical Handled事实/owner receipt核对，不另建消费fold或可由模型指定source的ACK入口。普通Replan继续Updated/NoChange；stop在验证typed消费后写Stopping，提供no_change_reason须在任何新规划提交前拒绝，不能先推进再尝试撤销停止。

固定英文前缀由 Grow 生成，后接有界结构化身份和步骤摘要：

- replan: `The parent TODO changed. Pause work that depends on the previous objective, inspect the latest parent TODO with todo_read, and adjust only affected work. Ask the parent with ask_parent if the changed objective is unclear. Preserve valid completed work and acknowledge this exact change.`
- stop: `The parent TODO was removed or cancelled. This delegation is terminated. Stop advancing its objective now, preserve existing work, perform only necessary cleanup and settlement, then report completed work, remaining work, artifacts, and unresolved outcomes to the parent. Do not resume this delegation because the TODO is later reopened.`

通知正文仍按现agent消息16 KiB最终UTF-8预算（含固定前缀与JSON转义）生成，不序列化整份可能含64 KiB正文/requirements的ReplanChange。完整change先存入child LocalPlan；通知只带完整来源/child/link/step/change身份、source revision、派生directive与显式摘要/省略数量。大正文通过原生读取取得，不把通知摘要当成完整输入，不因长合法步骤让固定控制无法接纳。所有身份与receipt保持精确，不能截断message/change/link或result引用；格式化归本Shell规划控制owner，不另添持久payload schema。相同身份的正文必须由本次固定父身份和已claim immutable change确定，不掺入当前plan进度、后续步骤文本或可变工具别名，使ACK丢失/cold重投仍逐字节一致；实际工具别名继续由每请求的新runtime context提供。

两类控制在 receipt durable 后使用现 parent_message_interrupt：打断采样和 interruptible waits，在 Step/工具批次安全边界交给 child；已产生副作用的命令保持真实 owner，不能因提示语就声称 killed。stop 不使用 kill_task 或取消整个 child token，因为那会使模型没有机会收尾报告；子任务进入收尾后，保留累计预算、已运行命令/后代的实际结算责任，最终正常 canonical result 报告。终止的 HandleReplans 必须确认 stop已接纳，不能用普通 no-change取消停止意图；完成 gate 仍要求处置真实必需交付/wait，不要求凭空伪造处理成功。现权限/工具接纳继续适用，立即收尾是固定控制语义，不承诺阻断非合作模型的一切工具，也不把待通知错误报成已停止。

投递扫描中的消费证明按记录隔离：某条历史Handled只有普通receipt或typed尚未消费时，该条继续未确认，不发送ACK、不降进度；后续独立合法Handled ACK与本地后代的Pending/Dispatching仍须有机会投递。沿现未确认集合和backoff保持重试，不新增队列或兼容状态，单项错误不能用提前退出整轮形成永久队首阻塞。该规则不放宽exact typed消费和当前binding要求，实际owner整体不可用仍保错误与原责任。

通知不可送达：parent记录pending/unconfirmed；child已经完成时记录Unapplied并保留结果。收到/Consumed/Handled/terminal是不同事实，todo_read可区分，不为满足“删除”抹掉任何未闭合link。连续编辑可合并未派发普通replan，stop优先且不能被覆盖。相同值、排序、纯review/receipt更新不触发；仅completed不终止工作或自动验收，内容同批改变仍通知。

## 增量交接与 Review Plan

1. ChatState owner提供request-only runtime context；Shell只做已定调用参数适配，修正原计划持久化旁路。第一片可独立验证 request有上下文、Timeline/Surface无新增、memory语义不变、预算包含context，Tier2。
2. 只读规划类型/投影、owner Read 与直属parentbackend和todo_read注册依次闭合；Tier3验证授权、分页/stalecursor、无写副作用。父确定schema后再分派，不让两侧猜字段。
3. 普通Task强制绑定和首请求的委派上下文，去除prompt拼接；同owner必要caller/tests适配。Tier3真实spawn/resume拒绝/成功及edit race。
4. 独立纯规划变更分类/stop保留，再Shell host投递固定信息/隐藏UI/安全中断/处理ACK；同一个文件按依赖顺序交接。
5. 父集成实际TODO→spawn→firstrequest→read→edit/ask→remove→收尾result→父review；nested直属、同项多child、cold恢复、终态竞争。独立luna审控制身份、持久回执和请求/展示隔离。所有验证固定Rust/Cargo/rustdoc1.93.1、独立/tmp GROW_HOME，不干扰正常Grow、不安装/重启、不提交。其他backlog候选未验收事实保留，不混入本change。
