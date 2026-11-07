# Frozen interface and staged delivery — 2026-11-06

## Architecture brief

保持一个ask工具，blocking缺省true。有效advisory=input.blocking==false且Shell允许；必须有每题精确合法recommended_option。原pending registry/InteractionResolved仅属于阻塞人类交互，advisory不注册guard，不等待旧tool第二个回答。迟答worker三个反证及14相关回归已通过，不能用原先“没有取消通知”的错误假设改协议。Goal窄idlewatch已经独立归档，不在此项混改。

状态分工：Tools负责结构化校验、有效参数快照、模型结果来源；Shell提供配置并执行有界presentation roundtrip；Pager负责卡片及本地编辑，后来的真实答复走原prompt durable接纳。卡片由原工具参数/成功结果恢复，不能新建无限pendingregistry。

## Exact shared interfaces (Tools owner)

- `AskUserQuestionInput { questions: Vec<Question>, blocking: bool }`；serde缺省blocking=true，schema说明true等待必要决定、false仅可选建议。
- `Question.recommended_option: Option<String>`，serde缺省None、skip none；精确匹配显式option.label。effective advisory每题必需；所有显式无效推荐都按invalid_arguments拒绝，不解析Recommended后缀。
- `AskUserQuestionParams.allow_non_blocking: bool`与同名DEFAULT常量默认true；保留timeout_enabled/timeout_secs。
- 新的共享resource `UserQuestionPolicyResolver(Arc<dyn Fn() -> Result<AskUserQuestionParams,String> + Send + Sync>)`。这是复用现有TaskModelValidator式依赖反转、让Shell拥有配置IO而Tools在每次准入取有效快照所需的唯一resolver。Shell Factory在注入UserQuestionSender的同一位置注入它（initial/rebuild/fork沿Factory）；每次调用重新按既有env > effective TOML > default解析**新增allow_non_blocking**，与Factory捕获的其余构建时Params合成结果；timeout_enabled/timeout_secs继续沿原startup语义，不能趁此热更新。磁盘解析失败返回错误而不默认放开。未注入resolver的独立Tools使用既有Params或Default。resolver返回后立即释放resource锁，不能持锁等用户。小配置同步读取沿已有resolver惯例，不引入filewatch/fanout/第二份policy缓存。
- `UserQuestionRequest.blocking: bool`保存本次**effective**值，不是原requested值。`deadline`为blocking现有可选用户等待期限；advisory固定5秒presentation期限，从enqueue前起算，含排队。新私有或共享const给两侧同一预算，不重复起表。
- `AskUserQuestionExtRequest.blocking: bool`（camelCase wire与原mode独立；serde缺省true）；原mode Default/Plan不变。
- `AskUserQuestionExtResponse::Presented` wire `{"outcome":"presented"}`；in-process `UserQuestionResponse::Presented`。仅有效advisory能接此ACK；blocking收到它是MalformedResponse。advisory收到Accepted/Cancelled等人类结果亦是MalformedResponse，不把推荐变人答。
- `AskUserQuestionOutput { message: String, advisory: Option<AdvisoryQuestion> }`，None默认/省略；`AdvisoryQuestion { tool_call_id: String, questions: Vec<Question> }`。只在Presented后Some；源session由这个工具事件的既有session身份提供，不相信额外UI填入的routing。ToolOutput的model_text仍返回message；message逐题标Agent推荐与推荐描述，明确用户尚未答复，不是permission/Plan approval。

## Shell owner promise

Resolver使用现有config结构/raw writer，新增allow_non_blocking只改其余timeout政策不动。Settings保存生效后下一ask读取文件有效值，不靠重启或异步watcher碰运气；已准入问题的effective值保持。

worker在原blocking分支之前按request.blocking分流。advisory只做ext_method，使用同一绝对deadline select timeout、result_tx.closed及shutdown；不运行“等待用户”通知Hook、不创建PendingInteractionGuard。处理serialqueue时先核deadline和receiverclosed，过期不再发出新卡片。Presented后返回一次；timeout/error为infrastructure failure，禁止返回CANCEL_TEXT伪装用户取消。原blockinghook/Opened→Resolved durable顺序保持。request/response两侧必须互验blocking与ACK类型，不从mode猜。

## Pager owner promise

扩展既有tool row展示卡片，用结构化advisory结果恢复；不用活跃QuestionView作为后台存储。收到effective advisory ext req后路由既有session view、展示后即回复Presented，不修改composer/question_pending/turn暂停计时，不替换现有blocking overlay。若terminal tool失败，卡片标未确认/失败，不显示已提交或已得到用户答案。

“修改”复用QuestionView本地kind（新AdvisoryReply variant），选择/preview/Other按旧能力；本地编辑虽会临时接管输入，但不设置session.question_pending或扣减turn时间。关闭只弃本地编辑。编辑提交把原题目、Agent推荐、真实选择/notes以及source session+call身份格式化为普通人类prompt，交给当前owning primary；primary不可用则明确失败并保留可复制内容，不新建会话。

每卡片一次真实答复，稳定身份`question-reply:{source_session_id}:{tool_call_id}`。提交后只读显示已答，后续修正可用普通prompt；相同卡片跨重连/重试不得产生不同identity。沿既有immutable prompt payload与unknown-ACK/reconcile路径，失败保留同一payload+id而不能改文本重用旧id，接受成功后才标已提交。不得将答复写成旧tool result、直接改Goal状态或向已终结child追加事件；依普通userinput仲裁。历史事实仍标原问题来自child。

## Pager A/B reply projection interface — confirmed after source review

卡片的presentation状态与人答状态分开；Confirmed只确认展示，不等于用户已答。答复正文只由既有QueuedPrompt/uncertain draft持有，卡片不复制payload。卡片新增短暂 `reply_status: AdvisoryReplyStatus` 与一次点击打开意图；新cold/replay卡片初始Unobserved。相同卡片接收成功工具结果时保留已有reply投影，不因迟到toolupdate重置已确认的人答状态。

```rust
pub(crate) enum AdvisoryReplyStatus {
    Unobserved,
    Checking,
    Available,
    Unconfirmed,
    Accepted,
    Rejected,
}
```

TaskA卡片owner提供AppView层四个入口（定位原source，覆盖child，不能用当前tab猜）：

```rust
pub(crate) fn set_advisory_reply_status(&mut self, source_session_id: &str, tool_call_id: &str, status: AdvisoryReplyStatus) -> bool;
pub(crate) fn open_advisory_reply_editor(&mut self, source_session_id: &str, tool_call_id: &str) -> bool;
pub(crate) fn advisory_reply_card_identity(&self, owner_id: AgentId, prompt_id: &str) -> Option<(String, String)>;
pub(crate) fn take_advisory_reply_open_intent(&mut self, source: &str, call: &str) -> bool;
```

identity查找只遍历指定owner的root+child卡片，完整format稳定id匹配；不能按冒号拆分source/call。若多个不同pair得到同prompt_id则返回None，不任选一张。open_editor仅Available时打开；无卡/无primary/已有blockingoverlay返回false并保留状态和有用反馈。open_intent只消费一次本地点击意图，不能授予发送或用户审批。

set_status返回值也承担单调消费资格：找到卡片且接受当前投影更新时返回true；无卡片或已有Accepted/Rejected终态而传入不同状态时返回false且不变。同终态重复允许true。TaskB必须在任何队列/在途输入或打开意图变更之前检查该结果；旧Absent/Checking/Unknown不能使已确认终态回退。card helper不决定canonical事实，只保护已经确认的不可变输入结论免受乱序观察覆盖。

TaskA Reply入口置Checking并给owning primary发既有QueryInputAdmissionStatus（query session是primary，promptId是稳定答复id）。TaskB输入owner处理existing InputAdmissionStatusResolved：保留session/bindingepoch/prompt/source=durable资格检查，匹配原卡片可作为无inflight/queue的cold查询目标。Allowed/Consumed/Handled→Accepted、不发送；Blocked/Dismissed→Rejected、不重入旧id、保留可复制文本。不能以optimistic userbubble或未确认写入作成功证据。

真实集成修订（依据 `review-reply-input.md` U0/U3）：先接通既有Shell ACP顶层路由；下层已有session内缺记录的正确语义是UnknownUnconfirmed，不能要求它返回全局Absent。没有该稳定id的冻结queue/inflight时，合法durable UnknownUnconfirmed或Absent可仅开放本地编辑（Available不宣称canonical不存在）；Submitted和读取/transport/parse/timeout错误不开放。已有冻结正文时Unknown不开放改选。首次Submit固定正文+稳定id，直接进入普通literal FIFO/checkpoint，由F3的串行immutablepayload准入owner原子决定新接纳/原状态观察/冲突；不再要求首次提交前必须查询Absent。既有终态仍拦截，card无正文副本。

未知ACK后的自动观察保留冻结正文、不发新请求。实际显式重试入口与正常完成后的自动接纳观察分别按U2/U1实现、独立验收；显式重试仅可按同一冻结payload/id调用F3观察，不能把新text复用为原id，也不能因generic缺记录Unknown永久阻断。重试意图来自卡片实际Check动作；QuestionView只生产首次Submit，提交后关闭且冻结正文不再开放editor，因此重复Submit只观察并保原文，不把后续自动Absent变成隐式重送授权。queue-only失败reviewrow与有inflight的失败同样保持冻结；未发送ready首发row保原FIFO资格。该修订替代旧版“unknown一概不开编辑/所有首次Submit先查Absent”的consumer限定，保留旧query wire及服务端Unknown语义。

这个接口是现有card projection与现有input observer的连接，不新增wire/backend/全局表。TaskA仅写card/lookup/editor/Reply查询入口；TaskB写prompt准入/query回调与固定payload队列，各自相关测试。结合后仍须实际编译和产品链验证。

## Incremental Coder Task Specs / Review Plan

1. **Tools一次advisory调用的契约**：写Tools ask模块/Output及必要Rust字面量调用方适配（仅补默认字段，不实现Shell/Pager行为）。Tier2真实Tool::run资源channel，合法推荐+Presented才完成、无答复不冒Accepted、5秒/queue/ACK失败、prefdisabled强阻塞、未知标签拒绝、旧Plan/Other/timeout格式保留。独立Tools crate可编译；父review公共字段/缺省/schema与来源。
2. **Shell一次提问准入**：依1接口，写resolver/config writer/Factory注入和workeradvisory分支，以及实际worker测试；不做UI。Tier2真实gateway/policy变更和queue期限；existing14blocking回归。配置存储与resolver为这个准入保证的调用方，不顺带修其他tools reload。
3. **Pager一次建议卡片的稍后答复**：依1/2，负责实际ext处理、结果恢复/编辑/普通输入提交及其UI验证，不做Settings。必须mock provider端核无用户时继续、lateanswer新prompt原tool只有一个result、结束child回primary、StoppedGoal不重启。
4. **Pager持久偏好操作**：依Shell writer/1字段，独立Settings registry/action/effect/snapshot接线（共享文件写顺序依父owner协调）。Tier2实际write/raw字段保存+Settings action；Tier3开关后下一ask有效模式及cold启动保留。

Pager实现时使用已有 `QueuedPrompt.prompt_id=Some(stable_id)`；当前dequeue_combined_prompt已将有prompt_id的行排除合并，不另加combineflag。display card需要由原source session+tool event恢复身份。`replace_question_view`目前把任何QuestionView视为question_pending，提交/关闭又无条件扣turn暂停时间；AdvisoryReply本地kind必须在这些现有消费点明确排除，不能仅让卡片构造端不设置字段却被后续通用路径重新标waiting。

阻塞旧基线附加边界见 `../2026-11-07-settle-late-question-replies/ui-transport-review.md`：实际SDK forwarding有两层oneshot，孤立send失败目前只证明silentdrop，尚不证明用户stall。Pager集成检查迟答后是否新普通prompt/问答仍继续，避免新模式靠绕开老链让潜在残留问题隐身。

父组织里程碑：真实mock provider产生advisory工具→无人回答继续下一模型请求；blocking无人答不继续；关闭偏好后false请求进入原blocking；真实Pager提交lateanswer以稳定id被Shell durable接纳、旧tool结果数不变、write/unknownACK failure可重试。各模块自测不代替此集成。独立review按交互归属和迟答实际风险在集成后安排；未完成里程碑前不归档。模型池仅设计、仍需用户批准，与此实现授权分开。
