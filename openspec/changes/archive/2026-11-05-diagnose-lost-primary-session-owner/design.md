## 当前证据

时区为本地 UTC+8，原进程47384，版本2.6.0。父Timeline物理seq64448/domain48629在11:07:05.367结束于第三次spawn工具完成，原运行期没有TurnEnded。11:07:35.601 handle_prompt.done ok:false；11:07:36.812 Pager status_unconfirmed，随后turn.complete was_cancelling:true；11:07:36.816旧coder child收到取消。11:20:16/20/25同pid三次输入检查点均成功，prompt随即0ms unknownsessionid。持久summary仍存在，最后已知摘要更新时间11:07:05。

unified日志只保留结构化事件，未发现通用tracing的supervisor/writer/catalog/disconnect条目不能排除这些路径。新进程17054于11:26:16再次load，11:26:52错误Session ended while loading; retry the session load；这次已向原Timeline追加恢复事件和SessionStart/notificationdrain turn，必须与原故障prefix分开。调查主Agent仅读取，无原目录写入/加载；另一个primary会话切换导致ask_session not_found，不重复发送或假设恢复成功。

## 所有者与待证假说

MvpAgent sessions只代表resident primary，session_handle_waiting_for_load在无加载且map缺项时unknown。父核对supervisor仅在SessionThread已经结束后reap；普通cancel不删除primary也不等待child ACK。`handle_prompt.done`在execution返回后、emit_turn_ended前；terminal提交失败随后fire-and-forget取消该turn的child，再经fatalcompletion/failedwriter teardown退出。非Workflow child drain为10s，不能用它解释此前30s。ChatState持久写有30s硬deadline，与现场时长相符但尚无firsterror，不能当根因。catalogdiverged退役、idleunload/显式关闭也需真实触发证据，拒绝先做auto-heal。

原最后ToolCompleted之后，tool/mod.rs696–755必经PostToolUse；hook_dispatch.rs即使handlers为空也先提交HookTriggered。原prefix无对应第三PostToolUse，因此probe先验证这个紧邻写入的pureprepare与isolatedappend。但缺席不证明InvalidHook；可能是ACK、存储或actor已死。tool.exec_done更早，只证明dispatch返回。读取测试都必须区分这几层。

## 隔离验证

复制原facts到独立/tmp根，保留原运行期prefix和后续recoveryprefix对比；不复制密钥/配置，禁真实provider/MCP/hooks，用fixture model/mock gateway。先用已有public storage/ChatState/SessionHandle入口拿到第一错误，再缩最小因果记录或竞争。源码当前有大量dirty，代码版本及diff需记录；并行ChatState API将为build_request_for_image_mode增加runtime_context参数，probe适配而不覆盖。

允许一次性测试/诊断工具，首个产品修复必须先补proposal/spec/design。诊断通过不等于故障会话已恢复，也不等于任一TODO backlog候选已验收。

## 已证加载竞态与修复决定

两个prefix实测相同：原待消费AgentReply domain48597合法；ChatState bootstrap恢复旧本地scope后，actor自动开启NotificationDrain Turn；`acp_agent.rs`后置调用`reconcile_loaded_children`，`subagent/mod.rs`末尾的无界`recover_interrupted_durably`把新Turn结束。随后Consumed违反active_turn匹配要求，terminal失败、actor退出、supervisor回收。纯fold4事实控制：无child/磁盘/provider，活Turn消费成功，插入后置recover则相同消费InvalidNotification，pending保留。该控制和真实prefix加载链证明加载缺陷；不证明原11:07firstfailure。

### 职责与共享接口

选择收窄后置恢复。新actor已运行，子任务reconciler只有历史child结果与其历史Workflow结算权。它不拥有新Turn、Step、Request、Tool、Hook、Input或SkillLoad。禁止再次对该actor做整会话recover。

ChatState层新增`Timeline::settle_interrupted_workflows(&mut self, expected:&[(String,u64)]) -> Result<Vec<TimelineEvent>,TimelineError>`，以及`ChatStateHandle::settle_interrupted_workflows_durably(expected:Vec<(String,u64)>) -> Result<Vec<TimelineEvent>,TimelineWriteError>`和对应Command。tuple字段为run_id/execution_epoch。每次在唯一Timeline owner内核对同一run/epoch仍open、非closed、没有open子Agent；checkpoint-preserving runtime不结算。缺失、已终态、新epoch、仍有child、checkpoint保留均no-op，不用另一个epoch替代。仅对匹配项提交Workflow::Ended(Interrupted, execution_suspended:false, handoff:Completion, message:process_interrupted)，duration沿现同epoch开始时间规则。除这些WorkflowEnded不创建新事实，不提交整会话Recovery、Turn/Step终止、Hook/Input/Skill修复。重复目标/重复调用幂等。批次逐项确认，前项已确认而后项失败时，保留已确认事实并返回错误；替换owner从持久前缀重放同一批目标不能重复首项，继续结算尚未确认的目标。沿现候选prepare→durable ACK→accept序列提交，错误不吞掉或伪造完成；可复用现recover_interrupted的workflow终止计算，不复制一整套recovery函数。

Shell在load已验证的Timeline仍在手且spawn前，以现`open_workflow_run_ids`和`workflow_lifecycle(...).execution_epoch`冻结Vec<(run_id,epoch)>；cold reconciliation只接收此集合。resident load提供空集合；checkpoint restore沿原preserves_open_turn/恢复过滤保持不误断。通过现`reconcile_loaded_children`和`reconcile_orphaned_subagents_with_backend`传递，不从新actorlive snapshot选择恢复目标。保留现cold_recovery决定childabsence等规则，但它不再授权整会话恢复。backend先确认/提交child terminal，再以新ChatState API结算历史Workflow；后续UI/receipt/plan repair顺序保留。bootstrap与storage-owned离线恢复不变。

### 取舍

直接删除后置恢复会遗留原被child阻塞的Workflow，不能接受。推迟所有新actor唤醒需新增全局startup状态并影响输入/Goal/Workflow准入，超出已证缺陷；本次用已有身份和epoch收窄准确权限。给整会话恢复增加一个布尔开关无法指明可结算对象，也不能防新epoch，因此不采用。无需新持久状态、schema或通用协调器。

### Coder Task Spec 与 Review Plan

A：ChatState owner实现上述精确Workflow结算及必要单元/actor持久回归，写入仅chat-state该功能与调用类型；runtime_context验证owner释放共享文件后才能开始。B：Shell owner在A API冻结后改loader→reconciler传参及调用，写入Shell这条链和实际回归；不改ChatState契约、不扩UI autoheal。每个task仅此单项功能，父核对差分/边界后独立luna review。

验证包括：真实公共reconciler在新NotificationDrain Turn已开启时不得追加其Recovery/TurnEnded，新通知准确消费且后续model/status健康；历史Workflow有child时不提前结束，child canonical terminal后旧epoch结束；已终态/重复/新epoch/无关新Workflow不被关闭；checkpoint restore和resident reconnect不获得cold结算权。最小反例不依赖缺child文件。完成A/B后用原prefix隔离重跑一次仅检验因果错误消除与actor持续健康，不以loadACK充当成功；原fixturechild缺失限制仍报告。固定工具链/隔离root、串行Cargo，不触碰正常session或发布安装。调用契约与开发说明同change更新，最终全量规范/归档校验。
