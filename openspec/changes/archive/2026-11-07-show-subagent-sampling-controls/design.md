# Design

## 依据与目标

现 `SetSubagentModelOutput` 只有 `{subagent_id,status:queued}`，工具等待独立的 actor admission ACK，应用由 Shell finish owner 等待 closed-Step，不能为 UI 把 queued 改成 applied或让父工具等待子步骤。Shell ACP 完成投影只含 raw_output，Pager走generic Other，标题不随完成变化。send_subagent_message 已通过同 tool_call_id 的 tracker entry 原地替换，紧凑状态可复用。

子路径 `enqueue_tool → AdmitToolSampling → admit_model_selection/admit_effort_patch` 当前固定传 None intent。`publish_control_projection` 有意将无intent的hydration/recovery静默，所以工具控制也没有 Pending/Applied/Rejected 或持久终态 notice。人工控制已有完整管线，child notification路由也已存在；应修来源身份，不能把ModelChanged标签更新当控制完成或在Pager伪造成功。

## 契约与所有者

**Pager主行**：识别 typed SetSubagentModel 或现 verified tool identity，不能按人类标题推断。仍是原 tool_call_id/EntryId，默认折叠时用与通信工具一致的紧凑摘要；摘要带目标child、model/effort中实际请求字段。Pending/InProgress为Dispatching；只有typed queued输出和匹配目标才为Queued；失败为Failed/Rejected并保留原错误。Queued不是已应用、不显示模型已经切换。展开/raw保留完整参数与结果。重复completion/replay不增加第二个已完成行，不从raw输出制造额外notice；目标名称可由原Task信息解析，缺失用精确id。

**活动组边界**：这里的“默认折叠”指单项工具 block 的 compact 模式，沿既有连续工具/Thought分组规则。`set_subagent_model`与outgoing `send_subagent_message`同为真实工具成员，不能仅因工具类型或完成状态拆组；折叠活动组可以隐藏已完成成员，正常展开组后必须显示同一工具行及完成状态。产品验证须保默认group配置，保存折叠视图、按可见组入口展开后核对compact行；此正常交互不构成缺失行的补救。未知错误不能用任意导航/睡眠掩盖。既有契约见 `client-surfaces/spec.md` 的 `Continuous tool and Thought activity forms a stable group` 与 `Activity group folding preserves active and manually revealed content`。

**Shell子控制**：子actor在既有工具Sampling admission、同一step gate/state锁内生成宿主拥有的ControlIntent，进入原desired-state、持久model.changed receipt与ControlState终态路径。身份采用当前actor incarnation的固定宿主命名空间，操作序号复用现`PendingStepControls.next_sequence`的锁内分配；不得为每次操作创建一个新client_id（现control_intents按client长期保留，会变成每次调用一项），也不新增计数器/registry。序号在无效effort等拒绝前也独立消耗，不能只用成功revision+1，否则失败后的下一次操作会误命中旧terminal。原ordered controls序号可有空隙，保相对应用顺序。

模型输入schema不变，不接受caller/session/intent；工具来源由既有独立admitted回执路径辨认，不把hydration/reload的None改成可见控制。直接委派、目标incarnation、closed-Step、原默认effort与latest desired model组合规则不变。不同宿主操作仍按actor接纳顺序latest-wins；内部命令是否需携带字段服从这个所有者，不为展示将UI token注入host层。

child的Pending/Applying采用现control反馈；真正应用后才记录并显示原Control类别Sampling switched notice。失败采用原Rejected notice，no-op保留回执但无虚假切换notice，superseded保持现静默替换政策。终态原持久屏障完成后finish owner才成功；parent工具已Queued不冒称实际应用成功。冷恢复遵守现人工控制历史规则，不重发或伪造一次新切换，也不把控制写入UserPrompt或模型对话。

## 真实终端验收发现的启动发布顺序缺口

v9 CLI日志在同一child上记录：三条首次Grow通知于`20:26:39.025173..025248`因`no agent matches session_id`丢弃，`Subagent spawned`到`20:26:39.035982`才建立视图；随后`20:26:39.988448`的live control因`active_epoch=None`被拒。日志不含早期被丢包的payload，不能逐包声称具体域；源码`handle_request.rs:1968`在首prompt前只发布一次三域ControlState snapshot，这条实际到达顺序和后续未知epoch拒绝共同解释了pending不可见。真实Queued和父行已通过，不能改断言掩盖这一缺口。

根因所有者是Shell child bootstrap：`emit_subagent_notification`只把父级Spawned排入父actor邮箱，child自己的`PublishControlState`却走独立actor/gateway路径。原注释假设Spawned已创建Pager view，跨actor之间没有满足该假设的发布屏障。`handle_grow_session_notification`还须经过父SubagentStart Hook和持久化，不能绕过父owner或从runner自行伪造eventId。Pager在未知child上丢弃通知及拒绝未知epoch是安全不变量；fullscreen本身已把`live_status`传给running control suffix，不需要调整展示优先级。

### 独立修复任务：child bootstrap 的发布顺序

在同一Shell生命周期所有者内，为启动的父Spawned取得明确发布回执，完成原observe Hook、持久化与gateway FIFO接纳之后，才允许child发布初始控制快照和首prompt。优先复用现通知/网关completion；若原`GrowSessionNotification`缺回执，允许为该内部命令增加可选oneshot `Result<(), String>` publication receipt，由父actor在原处理路径成功发布后完成。其他fire-and-forget生产者显式不请求回执；无需新公共wire/schema、registry或Pager未知child缓冲层。receipt不能只证明send进父mailbox，也不能在serialize/preview budget/gateway拒绝时回Ok。

调用方等待必须沿既有取消/启动期限及owner退役规则有界；parent unavailable、receipt丢失/失败、gateway阻塞或取消都不能启动未建立展示权威的child prompt。调用方取消保持原Cancelled outcome，不把取消伪装成普通launch error；真正发布失败走原launch_error与终态清理，不遗留半启动actor。正常新spawn（含派生resume）在现`restore.is_none()`的Spawned路径等待；原checkpoint restore不凭本次推测扩改恢复状态机。已绑定parent actor的命令入口关闭时视为发布失败，不能绕过该owner改走无持久化fallback；只有原本明确没有parent actor的受支持路径，才沿原直接gateway fallback取得发送completion，并按同一FIFO先Spawned后child快照。不能把重新发送同一Spawned或伪造snapshot当成功。具体内部helper落点由Shell coder沿现completion能力选择，最终行为必须明确到以下验收。

回执路径还核实了原持久化边界差异：无active preview时，原`persist_grow_notification`只等待buffered append ACK，不能称为durable ACK。因此`Some`请求publication receipt时，该分支改用既有`NotificationSender::append_update_durably`，同通知只写一次，再等待gateway completion；active preview沿原`PreviewIndependent`的独立持久屏障和预算。`None`保持旧buffered策略，不追溯声称其具备新增durability。durable append失败/未知仍不得成功，沿原错误/fail-stop，不加重试或回退；watchdog到期不假称已撤销在途写入。

SessionActor的publication确认本项明确选择30秒上限，从处理开始覆盖Hook、持久化到gateway completion；这不是既有统一启动期限。命令仅加可选`respond_to`，不增加共享deadline字段。独立watchdog可等待publication阶段并回应调用方，父actor不等gateway completion；原Hook/persist仍保自己的执行与fail-stop。确认到期、durable_ui_cancel或接收端关闭后，completion监测与preview permit必须实际释放，不得late success。到期只报告未确认，不假称已取消原持久操作。bootstrap consumer也以自身取消和本次30秒publication等待边界约束接收端；未成功不得发送child快照/首prompt。

Review：①阻塞父Spawned observe/persistence/delivery时，child初始snapshot和provider均不得越过；②释放后依序收到Spawned、child三域snapshot、真实Pending；③失败/取消不启动provider、无虚假queued；④保持父durable/eventId、Goal-owned顺序和子权限不变量；⑤保未知epoch/退休epoch拒绝的现Pager回归。最后freshbuild CLI，原exact PTY不降低pending/nextprovider/notice断言通过，并核对日志不再出现本child的startup未知路由或unknown-epoch。该修复独立委派，PTY候选准备与生产修复范围分离。

## 增量与Review Plan

S由一个Shell coder交付工具来源控制回执，范围model_switch host/commands/actor与对应现测试；P由一个Pager coder交付主会话同一行样式，范围tracker/现Other展示与相邻测试。两者wire沿既有SetSubagentModel输入输出/ControlState，无彼此代码前置，但共享Cargo窗口按owner串行。父先核各自有效旧红新绿、源diff与回执屏障，再验证真实工具producer→子actor pending/apply→子view和父row投影的完整链。

最低验证：busy子Step未关闭时工具queued已返回、子current不变且Pending可见；关闭后一次Applied与durableterminalnotice；effort-only组合最新desiredmodel；拒绝/无效请求不生控制成功；no-op、连续supersede及hydration/replay控制沿旧规则。Pager Pending→Queued/Failed须保同EntryId及单行数，长id/narrowwidth不藏状态、展开/raw保参数。Tier2展示与跨模块测试；因接入既有durable control receipt，补终态写失败控制，父验收不增加另一个控制存储。未运行平台明确记录，scope外债务分开。
