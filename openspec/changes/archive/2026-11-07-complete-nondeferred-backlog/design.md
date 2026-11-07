# Implementation coordination

依据为backlog及其逐项原报告、archive/2026-11-05-coordinate-audit-backlog-repairs/design.md的owner映射、2026-11-06三份分析和当前实现。旧架构图中的未实施建议不当成代码事实；每次派发前核相关源码/调用方/测试。

父architect保留跨模块契约、状态归属、依赖与最终验收；coder一次只交付一个owner的一项结果，直接关联的调用方/错误/测试随项交付。新公共实体、状态、依赖或兼容路径须有现需。实现顺序按依赖推进；输入F3先提供canonical查询给F1/F2，journal F5再F4，插件raw mutation先于install/uninstall持久错误，MCP注册owner/合并规则先确定再适配，CLI replay、crash同文件事项顺序写入。父统一主spec归档、backlog和docs/development，coder只写自己change，避免重复权威。

首批有充分依据且可独立实施：Shell usagefreeze观察期限、Responses usage补全字段唯一性、child SessionEnd/Result顺序。child结果由子SessionActor完成所有正常producer后，提交最后canonical Result并ACK；runner负责输出artifact和父级usage/Ended/receipt。冷恢复不执行外部Hook，未知ACK/写入保留原owner，不重开封闭Timeline。文件after-snapshot的不可得语义由Workspace owner决定，不能等同于文件不存在；readiness等待与mutation尝试分别计数但原周期绝对期限不刷新。

写入与编译窗口由运行时任务所有权管理，不建第二份持久任务系统。最多保持足以验收的独立在途任务；同一文件或共享接口顺序接手。Shell并行复杂收尾在隔离worktree准备，以免改变另一项编译输入；整合前查patch与实际HEAD。Cargo共享target窗口串行分配，固定Rust1.93.1、--locked --offline及项目规定PATH/栈，相关suite通过后不重复跑无关全套。

每项最低证据按Tier1/2/3，故障修复优先实际入口old-red/new-green；无法运行的平台保留限制。并发/权限/持久完整性高风险按已存在Review Plan独立Luna review，父核关键反例、原始diff和验证结果，再在里程碑组织默认CLI/存储/协议产品链。运行时completed不等于父验收；exactresult review绑定只用todo_read提供的值。

范围门槛：ACP容量与Markdown性能先做受控负载/成本区分；public Agent seam先查实际用途，不能仅因仓内无调用删API；vendor docs只同步真实graph。跨协议目标约束不能通过伪造fullreasoning/签名、删除事实、升级DirectUser或重放旧工具解决。现场旧宿主的unknown责任继续保留，源码测试不能替代采用新构建后的实际终态。

## 已核定的后续交接边界

### 文件快照

沿用RewindPoint现schema：file_snapshots含path而after_snapshots缺该path表示该轮结束状态未知；FileSnapshot.content=None仅指已确认不存在。一次end_prompt默认5秒总预算、8路并发，保留成功项、错误/超时不伪造None，不让晚到读取后台改点。Shell rewind先找最新触及该path的point，再检查其after，未知显示snapshot_unavailable冲突，不能跨过未知回退到旧after。merge时后轮触及但未知须撤销前轮after；force语义仍由已有明确用户操作决定。本项已作为单个Workspace状态行为连同Shell消费方交接。

### 进程退出的 Session owner 观察

ordinary SessionEnd e2e补齐模型与non-leader前提后，旧CLI一次通过、当前CLI多次在flush后缺Hook marker；当前日志显示`shutdown: flushing session`后Timeline ACK lost。源码反例明确：`begin_graceful_shutdown`在Hook与最终flush前调用`cmd_rx.close()`，而`AgentActivity`把`Sender::is_closed()`当作actor已退出，随后agent LocalSet退出会中断其上的persistence owner。旧CLI一次通过不证明无竞态，因而本项用受控owner barrier固定反例。

修复归AgentActivity及其注册点：生产注册携带既有`SessionThread` clone；entry保活与flush结束使用该owner的`is_finished()`（已同时观察actor和persistence退出），不能用命令入口关闭替代owner退役。原10秒整次grace、quiesce重取快照、按cmd channel区分同ID不同incarnation及Send-safe约束保持；不新增退出registry/状态机，不推迟close admission，也不改变SessionEnd reason或强行无限等待。真实ordinary CLI marker、stdin与reason断言均保留，并独立于child Result修复验收。

### 收尾readiness

child SessionEnd顺序修复先验收，再接同文件的readiness。当前判定方向：Coordinator在原30秒周期内可继续检查原I/O与actor/persistence是否退役；没有取得接管资格的检查不消耗mutation/receipt三次机会。周期绝对期限不刷新，状态/wait不启动下一周期，unknown操作不能重发。优先通过ChildRunner现接口增加最小readiness查询/返回分类，不建新公共管理器；具体签名须在上一项最终接口冻结后核定。验收覆盖前三次tick都未ready、随后在原deadline内ready时能成功一次，以及deadline后ready仍不自动追加。

### 流内错误包的接纳资格

`try_parse_stream_error`只拥有SSE错误资格，HTTP非2xx的`parse_structured_error`保持原契约。本轮混合包政策明确为：普通事件形状（非`error`的非空顶层type、Chat的choices或`chat.completion.chunk`对象）同时携带顶层error时是本地Serialization协议错误，即使普通typed部分完整也不作为服务错误或完成事件。`event:error`不能覆盖正文的普通事件身份。无普通事件身份的既有bare legacy、完整named error及合法typed error继续沿共享分类；未知扩展帧不因附加error取得自动重试资格。原始字节、attempt结算、候选撤销和总重试预算保持，三reader真实SSE组合验证。此政策来自Grow的malformed ordinary契约，不声称OpenAI官方SDK采用同一优先级。

### Workflow journal 的恢复屏障

F5 已由共享fold拒绝第二条completion，F4沿现有`JournalStorage::read_bounded(max_bytes)`收紧恢复读取承诺：返回成功的字节必须来自已确认持久的同一打开文件前缀，文件与所属目录屏障成功后才交给`Journal::load_storage`或Shell suspended-run校验。接口不增加并行状态表或第二journal；缺文件仍按原NotFound语义处理，已存在但同步失败的前缀不得解释为empty、Completed或可恢复执行。

Shell `SessionJournalStorage`是文件/目录durability的owner。它使用pinned目录内不创建、可读写、拒绝symlink/reparse的文件句柄，取得与append/truncate一致的文件锁，再有界读取并同步该同句柄及目录，成功后才返回字节。Windows目录屏障沿现有ContainedDirectory平台约定，不虚称新增目录fsync保证；所有底层I/O失败均向恢复调用方传播。原writer先退出、session writer lease与Run恢复准入保持，读取屏障不授予并行写入权。测试注入完整append之后file-sync/dir-sync失败，证明live失败后可读字节仍不能让cold恢复提前接纳；解除故障后保留原operation身份恢复，不能重发已完成的child。

### 稳定输入及恢复

父已亲核input_admission、prompt_queue、Timeline::input_state、PromptTurnOk、ACPprompt及既有input/admission_status。canonical fold已有InputLifecycle及RouteSuperseded，不应在Shell再扫描一份状态。后续F3以稳定input_id_for_prompt查询同一事实、核对原不可变payload/requested_route，FIFO场景服务端Prompt/Followup派生差异不作为用户改intent；显式Steer/路由/正文/图片变化仍冲突。同一输入重试须返回原input identity/current state，且不发送另一份TurnCompleted/rosterIdle，不继续用RemovedFromQueue伪装重试；精确ACP元数据契约在分项change冻结后交coder。新建readonly ChatState query只暴露既有canonical投影，不加第二登记表。F1消费runtime保留Turn事实，F2消费supersedes原槽位；两者在F3接口验收后分别实施。

F3提供方的接口定为 `ChatStateHandle::input_state(&self, input_id: &str) -> Result<Option<InputLifecycle>, String>`：查询失败与身份不存在分别表达，actor直接消费`Timeline::input_state`。Handled应在Timeline fold内与Consumed分开表示，由现有Handled事件推导，不能让Shell另扫事件。`InputState::Handled { route }`只是内存查询投影，不改变持久event schema；Shell既有input/admission_status继续输出原`handled`形状。后续Shell先按稳定input_id查此投影，只读取命中payload_ref核对不可变请求，不为查一个prompt遍历读取所有历史blob。

F3调用方的重复提交应以新的内部`PromptCompletionKind::AdmissionReplay`返回原input_id和canonical state；ACP响应在`_meta["grow/inputAdmission"]`放`{ "inputId": ..., "status": ... }`，status沿用input/admission_status的标签及route/reason结构。其StopReason为EndTurn，表示此RPC观察结束，不表示原Turn已结束；在ACP normal completion/rosterIdle/TurnCompleted分支前短路。Submitted返回submitted观察，Blocked/Consumed/Handled/Dismissed返回各自原状态；它们都不再执行Hook、入队或新增Turn。新输入的原有错误/Hook政策不变，payload或显式requested_route不同仍冲突。重复RPC的元数据只包含身份与观察状态，不附带另一回合的usage/totalTokens或structured output。直接Steer现有扩展的应答形状保持，只同步消费canonical状态；新的AdmissionReplay应答用于session/prompt重试。重复RPC不能发新的Working/Idle roster投影。接线复用QueuePrompt已有`persist_ack`：ACP只在本次实际用户消息持久确认后发布Working；队列对重复观察先返回AdmissionReplay，再回原持久确认。ACP优先消费观察应答并在任何回合结束/usage/roster路径前返回；未取得本次提交确认的接纳错误也不伪装成回合结束。无需新增并行admission频道、状态表或通用通知机制。正常新回合的持久确认与结束通知保持。
