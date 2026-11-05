## Ownership and invariant

Tool→session-bound ChannelBackend→SubagentCoordinator→ShellChildRunner→用户model_switch准备/目标SessionActor。coordinator已有active/pending/completed/suspended表和immutable immediate_parent_session_id，拥有动态直属资格；ShellChildRuntime::security_context返回exact SessionHandle。无需再造目标目录/回调绑定或授权表。

工具名set_subagent_model，输入{ subagent_id:string, model?:string, effort?:string }，严格未知字段；subagent_id非空，至少一项选择；null省略，effort按ReasoningEffort canonical解析。输出{ subagent_id:string, status:"queued" }。替换set_session_model及其self-binding runtime，不保留别名。ACP用户切换接口保持。

## Cross-module contract

新类型位于tools::implementations::grow_build::set_subagent_model：

```rust
pub struct SubagentModelControlRequest {
    pub source_session_id: String,
    pub subagent_id: String,
    pub model: Option<String>,
    pub effort: Option<sampling_types::ReasoningEffort>,
    pub cancellation: tokio_util::sync::CancellationToken,
    pub respond_to: tokio::sync::oneshot::Sender<Result<(), String>>,
}
```

SubagentBackend新增async set_model(&self, subagent_id:String, model:Option<String>, effort:Option<ReasoningEffort>, cancellation:CancellationToken)->Result<(),String>，默认host不支持。ChannelBackend只用其Some(parent_session_id)填source；host observer unbound拒绝。SubagentEvent新增SetModel(SubagentModelControlRequest)。ChildRunner新增同步转发方法：

```rust
fn set_model(&self, request: SubagentModelControlRequest,
    target: <Self::Control as ChildControl>::SecurityContext,
    target_cancellation: CancellationToken);
```

默认实现回Err。coordinator只在active目标存在、immediate_parent_session_id==source、owner非Workflow、execution_outcome未设置、target cancellation未取消、request未取消/receiver未关闭时调用，target从该child.control.security_context取得。不能用只读history结果、lifecycle root可见性或名字解析代替control资格。初始化/挂起/finalization_pending/终态错误不触发恢复/重跑。

Shell实现trait方法后spawn_owned_local，不阻塞coordinator邮箱；只将exact target handle传入现有model handler。等待catalog锁之前/之后确认caller取消与target取消、target仍为active_child_sessions同一cmd_tx，并拒绝exact runtime observation的Settling/Terminal/Unavailable；不按旧task ID换成新actor。这个额外检查覆盖coordinator转发后自然完成的实际窗口：turn settlement在prompt回执之前发布Settling，runner收到回执之后才reporter.execution_returned；自然完成不会取消原token，same-channel也只证明身份。确定性回归须持catalog lock，让真实child自然结束后释放，再断言零AdmitToolSampling/零queued。沿model_reload_lock准备route并enqueue_tool后释放锁，再等actor admission。application receiver由原finish owner持有，parent工具不等待child Step结束。actor拒绝/receipt关闭返回错误，不声称queued。

普通模型必须当前catalog已知、非hidden、user_selectable/allowed_models内，effort由目标route支持。model-only采用用户默认effort，effort-only在目标actor与最新desired model合成。Workflow child由coordinator拒绝，不松动frozen route。普通tool eligibility/冻结参数/Hook/permission/one-shot permit仍先于backend发请求；sampling改变不扩张child工具/MCP/workspace/grants。模型请求当前attempt保持旧route，下一获准Step在durable切换后才用新route。source birth/parent Spawn不改写，普通resume仍读来源最后durable选择，checkpoint原identity约束保持。

目标actor必须在Sampling入队/ACK的同一step_control_gate与AdmissionState生命周期锁内重新确认native请求目标仍有开放的前台执行，覆盖Shell检查后命令排队而target先settle的窗口。该限制只用于有独立native admission ACK的路径；ACP用户请求仍可idle切换。Model和Effort两分支共享此保证，检查后不能释放生命周期锁再入队；不得只在admit_tool_sampling外检查后await另一层取锁。回归在真实child/mailbox或step gate停住控制处理、先自然结束再处理请求，要求两种选择错误且零slot/queued/model持久化变化，并保留live对照。

## Coder Task Specs

A tools owner：仅tools crate内实现新native与注册/IO、session-bound backend、coordinator dynamic dispatch/default runner方法和相关回归。删除本次先前生成的self tool代码，不修改用户初始Task行为；不得改Shell/OpenSpec/docs。公共接口严格按上文，必要偏差先反馈。

B Shell owner：按上文新方法接ShellChildRunner、复用model handler、移除自身binding安装资源/命令/重建钩子；相关Shell与test-support旧自身tool测试/CLI组合调整为parent控child，resume测试保持其独立覆盖。不修改tools crate/OpenSpec/docs。先核对A公共接口稳定后实现；其他实现细节选现有约定。

## Review Plan

A单测覆盖schema/null/none/closed ACK与coordinator的直属/孙代/兄弟/根可见但非直属/unknown/initializing/cancelled/settling/suspended/completed/Workflow不转发，调用参数与exact SecurityContext一致。B从真实native parent工具到child下一provider验证model/effort四组合、parent不变、当前child request不被重跑/中断、pending用户model与effort组合、无效/拒绝零mutation、原permit/ceiling及catalog排序边界。隔离GROW_HOME新CLI里程碑至少一条parent spawn→运行时切换→child下一请求→最终durable→省略resume链路，并验证source immutable。luna独立review实际最终diff，父默认CLI构建与相关定向验证；不为旧self控制维持别名或无关框架。
