# 首个回写增量的独立代码审查

状态：首个实现增量的独立代码审查完成。R1–R9 均已修正并复核，最终相关运行时与编译证据已汇总，未发现剩余阻塞问题。审查者：独立 agent `injection_design_review`。本结论不把尚未逐项运行的全组合旅程或未勾选的扩大验证任务声明为完成，也不单独授权归档全部提案。

## 审查基线与范围

逐文件对照 `/tmp/grow-passive-coder-baseline` 的任务开始快照，而非仓库 Git diff。Grow 工作区另有大量前序改动，不能全部归于本功能。已审查新 inquiry proof、Sideband 结果引用、Notification source/identity、suffix 查询/游标、实际通知消费与最终候选调用方、writer 初始化和冷加载验证。

本轮没有运行 cargo，避免与实现者的构建/测试争用；运行时结果等待最终交接，静态自述不记为测试通过。此前独立运行的 Python demo 仅是设计证据。

## 初审发现与修正状态

### R1 [P1] 消费 admission 必须在 payload I/O 后复核资格

初版 `notification_drain.rs::drain_active_notifications_selected` 只在查询 pending 和读取 payload 前检查 Goal/foreground。若 I/O 等待期间预算或 terminal preemption 关闭资格，仍会提交 Consumed；与“资格在消费前撤销则保持 pending”不符。

实现者已增加 I/O 后的 `step_control_gate` admission，核对捕获的 prompt/Turn、`can_continue_regular_turn`、Goal provider gate 和已捕获 Behavior，再提交消费。静态复核已确认该修正。已经进入消费 admission 后才发生的撤销保留已提交 Surface，仍不得 dispatch 新请求。最终对应边界与完整 package 运行证据见末节；不声称逐个运行所有 I/O 撤销组合。

### R2 [P1] 冷加载不能接受没有精确 canonical proof 的新收据

初版只检查 proof-bearing audit 指向真实 Completed InfoRequest Result，没有检查每条 InquiryAnswered Received 是否确由该成功 terminal 派生。缺失/错误终态、错误 version、counterpart、owner 或正文引用仍可能重放为咨询上下文。

修正后独立静态复核确认：storage validator 对每条新 Received 回查精确原 terminal version，核对 terminal 在 receipt 之前、target/counterpart/inquiry 对应，并由共享 pure `answer_context_body()` 验证 payload_ref 的字节数/hash。无需新增正文 I/O。对应运行时/反例结果见最终汇总。

### R3 [P1] 同一 inquiry 的第二个成功 terminal 不能制造第二份收据

初版 reconciler 对每个 proof 以 event seq 生成不同 source version；两个同 `(source_session_id,inquiry_id)` 的成功 audit 会产生两个独立 Received。现有 outcome 查询却采用第一个 terminal，双方语义不一致。仅去重相同 source/version 不足以阻止此问题。

修正后独立静态复核确认：只有 InquiryAnswered 的 receipt ID 由 owner+逻辑 producer identity 派生，不含 version；version 仍保留原 terminal 绑定。更换 terminal version 会与原 receipt ID 冲突，不能获得第二份收据。冷加载拒绝相同逻辑 inquiry 的重复新成功证明；实际成功 audit 持久化在既有 control fence 中按 canonical 原 terminal 复用/冲突拒绝。其他 producer 的 version 身份计算不变。对应反例结果见最终汇总。

### R4 [P2] proof 需绑定原问题，不能只绑定同 Session 的相同答案

初版冷验证只检查 InfoRequest purpose 和 `result.raw_output == audit.answer`。另一个本 Session 的 InfoRequest 若恰好也回答相同文本（如“yes”），可被错误地关联到不同 source/cwd/question 的 audit。

修正后独立静态复核确认：共享 pure `inquiry_prompt` 用于 live 请求与冷验证，按 typed source/cwd/question 和固定 DEFAULT_REMINDER_TAG 构造；存储验证要求 Sideband Request.prompt 精确相等。没有解析文案或新增证明 hash/独立 I/O。对应“同答案不同问题 Sideband”反例结果见最终汇总。

### R5 [P2] 新成功证明不能前向引用尚未创建的 Sideband

复核 R1–R4 后补充发现：spawns map 已保留父 Timeline 的 spawn seq，但初版新 proof 验证只核对 purpose；proof 可放在其引用的 Sideband parent spawn 之前，而 request/question/result 仍能一致。其冻结来源甚至可能包含该未证明的成功 audit，形成前向/循环证明。

修正后独立静态复核确认：本地新 proof 必须满足现有 spawn_seq 严格小于成功 audit 的 event.seq，同时保持 InfoRequest purpose 校验。此条件由正常 `begin_sideband → complete → terminal` 路径自然保证；没有新类型或 I/O。对应负向验证见最终汇总。

### R6 [P1] 咨询收据不能用 input=None 消费

按主 agent 指定的 demo 直接不变量复核，`Timeline::validate_notification` 初版允许新 InquiryAnswered 收据通过 `Consumed{input:None}`：原有 None 表示目标已有真实工具结果，因此不再物化通知；但回答方咨询没有此工具结果。重放会清除义务却不生成模型输入，违反 Consumed 与精确输入同提交的契约。

修正后独立静态复核确认：仅对所选收据中含 InquiryAnswered 且 input=None 的消费返回 InvalidNotification，保留其他 producer 原 None 语义及普通混合批次 Some 输入。原有 Some 类型/权限验证继续适用，没有任意正文解析。实现者已增加包含普通 Task 收据的混合批次精确回归，已包含在最终 ChatState 610 项通过结果中。

### R7 [P1] 挂起树恢复准入之前不能派生新收据

主 agent 与实现者进一步核对发现，初版 startup reconcile 位于已有 `!runtime_recovery_gated` 分支之外；与同位置 interrupted-inquiry recovery 不同，可能在挂起树尚未验证/开放执行时追加派生 Received。

独立静态复核修正后确认：`spawn.rs` 将协调恢复和新收据补齐共同置于 `!runtime_recovery_gated` 内；该 gate 来源于原 runtime phase，不从 UI 推断。live reconcile 在已有 control fence 内复查 `execution_open()`，关闭时返回且不推进覆盖该未补齐义务的 cursor。正常开放后的首个采样仍从 canonical 未协调范围处理，不丢原义务，不唤醒挂起 Session。

### R8 [P1] 清理失败不能丢掉最初的通知消费失败原因

最终 full-v2 的真实 first/successor/final 故障测试发现：故意的 Notification Consumed 写失败使 writer fail-stop，随后 `complete_unmodeled_fifo_inputs` 的 input_terminal 清理也失败；初版替换 execution 错误，导致最初 notification-consumption 原因丢失。测试严格要求原原因，没有接受任意 fatal 作为通过。

实现者按相同函数既有最终 terminal wrapper 的方式修正：构造 input_terminal boundary_error 后，在替换前把原 execution Err 保存为 data.cause。独立静态复核确认该最小修改保留清理失败分类和最初原因，没有改变任何状态/调度，也没有放宽回归断言。修正后精确三分支测试及最终完整 Shell 检查均通过。

### R9 [P1] 不能提交永远无法投递的超限成功义务

主 agent 的实际旅程复核提出较长合法回答的场景。独立源码核对确认：JSONL 单项上限 64 MiB，Sideband Result 不限制非空 raw_output 的字节数，InfoRequest 的 max_output_tokens 未指定；但 Notification 正文上限为 1 MiB。仅 raw answer 恰好 1 MiB，加入题目和归属 header 后，便超过既有投递上限。初版先提交 Answered proof，后写收据时才拒绝；每个后续采样/冷 writer 都会遇到同一无法完成的义务。

修正后独立静态复核确认：在 canonical 成功证明提交之前，检查共享 renderer 生成的**完整正文**是否满足既有 Notification 上限；只有通过后才设置成功 proof。没有只检查 answer、增加新上限、截断回答、无限重试或把已提交 Answered 再改写为失败。正文不可接纳时询问返回 typed failure，保留实际已完成的 Sideband 证据/用量而不创建成功同步义务。冷验证也检查已计算的正文上限，不接受无法补齐的成功证明。真实 InfoRequest 的 UTF-8 exact-fit、+1 byte 和 raw 1 MiB 反例/后续正常请求测试已在最终四项真实消息旅程检查中通过。

## 其他已核对点

- Sideband `complete()` 的真实 Result range 在 Result 与 Completed End durable 后返回，本功能复用该值，没有新增 id getter 或跨请求重扫 Sideband 的流程。
- suffix cursor 是非权威 monotone frontier。当前实现只在自身捕获范围的全部义务协调成功后执行 `max(next_seq)`；并发调用重复核对由既有收据幂等串行，不会因 max 本身越过一个尚未验证的范围。发生失败时该调用不推进。
- 新 source 不进入 interjection buffer，也不设置 parent interrupt；`notification_autostarts` 显式排除它，`agent_message()` 不将它伪装为 parent/reply。
- 三条真实路径已有 Result 传播：Turn 首次 admission、普通 Step safe point、最终候选续接。新增存储错误分类使用 fatal boundary，避免 completion recovery 将其当成普通回答失败自动重新采样。
- 最终候选在 control gate 中固定 selected IDs，后续 selected drain 不再重新协调/扩大集合；以后到达的收据只能由已获准后继 Step 的正常边界处理。真正无后继时保留 pending，不新增 min_turn。
- 未标记历史 audit 和原 target 不同的继承证明不创建本地回写义务；只读观察执行验证，修复仍属于合法 writer。

## 最终复核与验证

已按实现者确认的当前生产源码复核 R1–R9 修正，未发现剩余阻塞性的静态问题。测试夹具曾发现 `NotificationTaskKind::Command` 不存在及 `SidebandResult.output` 字段名错误，已通知修正；不是生产语义问题。

仍须核对最终实际测试命令/结果与修正后源码。当前 `/tmp/grow-passive-runtime-final.log` 中三个定向测试成功只覆盖该日志对应的先前构建，不能替代新增 R5/R6 与最终 v2 运行。本记录中的“静态已确认”不等于运行时测试通过。

最终 full-v2 日志 `/tmp/grow-passive-full-libs-v2.log`：ChatState **610 passed、3 ignored**，含 R6 mandatory input 回归；新的真实 blocked SamplerActor 非抢占/同 Turn 续接测试通过。Shell **4,488 passed、2 failed、9 ignored**，两项失败分别是原 watcher 的 project config event timeout，以及揭示 R8 的新严格通知故障测试。本轮全量不记为全绿。最终结论等待 R8 修正后的精确测试、watcher 核查和最终相关验证汇总。

独立审查者随后核对实际日志：R8 修正后的 `/tmp/grow-passive-fault-focused.log` **1 passed**，内部 first/successor/final 三个分支保持原严格原因与请求数断言；`/tmp/grow-passive-watcher-isolated.log` **1 passed**。未修改 watcher 源码或放宽测试。孤立通过不单独证明 full-v2 超时的根因。

已对 `/tmp/grow-passive-final-source-manifest.json` 的 **29 个任务文件**逐个重算 SHA256，全部匹配。对照任务基线另外核查 `run_loop`、`completion_delivery` 与 `tool/result`，仅增加必要 source 穷尽匹配，不改变其产品语义。最终 full-v3 `/tmp/grow-passive-full-libs-v3.log` 当前已确认 ChatState **610 passed、3 ignored**，Shell 尚在运行；最终结论等待该冻结状态的完整结果及 Pager 调用方检查。

full-v3 随后完成：ChatState **610 passed、3 ignored**；Shell **4,490 passed、9 ignored**，均零失败。该构建在 R9 cap 修正之前，可证明 R1–R8 的完整相关 package 验证，不能冒称包含之后新增的 cap 边界测试。真实 queued-consultation 与阻塞前台合并的 `/tmp/grow-passive-joined.log` 另有 **1 passed**。R9 后的最终相关重跑、Pager 检查和新版源码 manifest 尚待汇总。

R9 后最终 `/tmp/grow-passive-message-delivery-final.log` **4 passed、0 failed**（1.16 秒）：实际 blocked sampler 非抢占、真实 queued inquiry 与同 Turn 后继衔接、first/successor/final Consumed 故障及保留原错误、完整 UTF-8 负载精确上限和越界拒绝/后续健康前台。独立审查者重新读取了实际日志，并对重新生成的 29 文件 SHA256 manifest 全部复算匹配；包含当前 producer/cold-validation cap 修正。最后完整 Shell 重跑与 Pager fixture 编译检查仍在进行，不提前记为通过。

最后完整 Shell 重跑已完成：`/tmp/grow-passive-full-shell-final.log` **4,492 passed、0 failed、9 ignored**（406.05 秒）。ChatState 源码在其 full-v3 **610 passed、3 ignored** 后未变。独立审查者再次复算最终 29 文件 manifest，全部匹配。Pager `cargo check --locked -p pager --tests -j 4` 仍在顺序执行；它是共享 audit 类型调用方的编译检查，不冒充 Pager 全量运行时测试。

## 实际关闭/重试旅程的额外核对

主 agent 指定核对“existing-outcome retry 在 acquire Queue activity 前协调收据，是否可能越过挂起 checkpoint”。未发现可达缺口，不增加防御 wrapper：生产 peer/delegation 调用都发送 `RunCoordinationInquiry` mailbox，`run_loop` 内联 await `enqueue_coordination_inquiry`，同 mailbox 的 `SuspendRuntimeNode`/shutdown 必须等待该 retry 返回。MvpAgent 可以提前置 shared runtime_suspend、写 SuspendRequested，但实际节点 checkpoint 必须由后续 `SuspendRuntimeNode` 启动。已开始的节点挂起则 execution gate 已关闭。普通新回答另持 Queue activity；前台协调由挂起流程 join/abort 后才 checkpoint。允许已接纳事务在 SuspendRequested 后完成，不等于允许在已提交 checkpoint 后继续追加。此结论来自真实 caller/owner 路径，不是只按是否持 activity permit 推测。

## 最终结论与证据界限

独立审查者最终读取了上述实际执行日志，并再次核对 29 文件源码 manifest，全量 SHA256 匹配：

- 最终 R9 源状态 Shell lib：**4,492 passed、0 failed、9 ignored**；最终四项真实消息旅程 **4 passed、0 failed**。
- 未再变化的 ChatState lib：**610 passed、0 failed、3 ignored**。
- Pager 调用方检查 `/tmp/grow-passive-pager-check.log`：`cargo check --locked -p pager --tests -j 4` 正常完成，3m41s；这是编译检查，不是 Pager runtime 全量测试。
- 最终 OpenSpec 日志 `/tmp/grow-passive-openspec-final.log`：**17 passed、0 failed**；fmt/diff 检查由实现者记录通过。本审查者没有并行启动 cargo，也不把阅读日志写成自行执行测试。

首个被动咨询回写增量可交付，独立代码审查完成，未发现剩余可行动阻塞问题。边界按 [实际旅程审计](user-journey-audit.md) 保留：已经在途的输出仍可能不知道新问答；no-wake 不能提供继续执行资格；咨询事实不是前台采纳或人类许可；普通通知/agent message 不承诺跨类型全局正文顺序。

未逐项新增运行实际阻塞工具与咨询联合、三种方向各自完整 source-side 跨进程工具链、进程重启加新咨询等全部组合；本结论依据共享实际入口、代表性真实 provider/故障旅程、原有控制回归、冷验证与有限模型，不宣称全部组合穷尽验证。tasks 第 3 节仍未勾选的扩大验证/归档项不能由本结论自动关闭；如归档，应先明确所归档增量与对应验证范围，不把未执行组合伪记为通过。

## 后续联合旅程与 R10/R11 迭代复审

用户随后授权补齐联合旅程。以上“尚未新增运行”的表述保留为首个增量当时的记录；本节更新已经实际补齐的部分，逐项映射以 [joint-task-evidence.md](joint-task-evidence.md) 为准。审查者继续只读核对生产代码、测试 seam 与实际日志，没有运行并行 Cargo，也没有改写实现者的新验证文档。工作区有其他任务的共享文件变更，复审按 `/tmp/grow-passive-joint-baseline`、R10 前快照及本次实际修改位置限定，不把整个文件差异归给本功能。

### R10 [P1] inquiry 取消与成功终态接纳必须选出单一赢家

真实 red 将实际 provider 返回后的 Sideband Result ACK 暂停，再取消 inquiry 并释放 ACK；旧代码仍返回 Answered。独立审查确认这不是 mailbox 顺序可以排除的假竞态：回答队列是独立活动，inquiry 取消 token 与 target Sideband 关闭 token 不同，usage、Result/End 和终态准备均有 await。

修正复用 `InquiryCancellation` 的原子状态，在已有 answer gate 内、既有 canonical 查询之后、成功 terminal append 之前仲裁。取消先赢则保留真实 Sideband 结果/用量而不发布成功 proof；成功接纳先赢则后到取消不能反转，仍必须由实际 durable 结果决定是否建立上下文义务。持久化失败保持 AuditFailure，不把进程内 seal 当成持久成功。

独立复核还覆盖两条来源路径：ChannelBackend 的 Ask 保留 host responder，直接父子 interaction 在取消未获胜时继续等待原响应；peer 不再由本地取消 intent 无条件合成 Cancelled。peer 必须先从匹配的 Progress 确认远端注册，再向固定 target 发 Cancel，并保留原 Ask 和已开始的 `read_json` frame future。取消 RPC 的 false 不是成功结果，最终仍来自原 exchange 或真实 transport/deadline failure。

旧 OutgoingStarted 的冷来源重试也已修正：该事实只说明可能派发过，不能按确定未派发处理。实现从本次 startup admission 之前读到的旧事实派生局部初值，沿原 task/deliver 传递；首次可能写入 Ask 后，该标记跨既有 reconnect 保留。没有新增持久账本、取消 tombstone 或协议。部分帧测试直接调用 `request_inquiry`，没有外围重连可掩盖丢帧；完整 Progress 与下一帧头两字节在同一 prelude 中发送，原连接必须返回正确结果。

已读取实际证据：`/tmp/grow-passive-r10-before.log` 为上述真实 red，1 failed；`/tmp/grow-passive-r10-runtime.log` 为 31 passed，含冷来源、部分帧、原 lease/identity/reconnect 回归；`/tmp/grow-passive-r10-tools-backend-v2.log` 为 24 passed。最终 16 项消息边界组再次包含取消两种胜负及成功 ACK 失败不尝试写 Received 的断言。

### R11 [P1] Peer 成功答案必须满足真实序列化帧限制

原始 UTF-8 咨询正文可小于 Notification 上限，但引号、反斜杠在实际 `Response::Inquiry` JSON 中转义后仍可能超过既有 IPC 帧上限。真实 red 使用 600,000 字节的答案，旧代码错误地发布 Answered，不能靠原始正文长度证明可交付。

修正从既有 wire writer 提取同一个纯 `encode_json_payload`，writer、Peer 成功 proof/seal 前准入和冷加载的新 Peer proof 验证都使用实际响应结构及同一限额。直接 delegation 不继承 Peer 传输限制；超限保留完整 Completed Sideband/用量，返回 typed failure，不创建无法交付的成功义务。无 proof 的历史和原 target 不同的继承事实仍沿原跳过路径，不制造新义务或数据回写。冷负例只变 route，保持真实 request/result 绑定，并明确在 IPC 检查处失败，未借通知正文超限或坏 hash 冒充该反例。

已读取 `/tmp/grow-passive-r11-before.log` 的 1 failed red、`/tmp/grow-passive-r11-lifecycle.log` 的 13 passed，以及 `/tmp/grow-passive-r11-ipc-frame.log` 的 3 passed。静态复核未发现剩余的 R10/R11 阻塞问题。

### 最后 ACK、Goal 与生命周期证据

- `/tmp/grow-passive-joint-boundaries-final-v2.log` 实际为 **16 passed、0 failed**，2.35 秒。Received 与 Consumed ACK-loss 分开验证：夹具先保留实际事件，再 hold/drop ACK；原 writer 重试相同 seq/payload。Consumed ACK 未确认时没有依赖它的 provider 请求，canonical 重建只有一个输入且无 pending；Received 重建只有一份 pending，原 Turn 已结束时不追加输入或自启，明确后续用户 Turn 才消费。这是可控持久化模拟与 canonical fold 证据，不是磁盘崩溃或两个生产 writer 并行工作的证明；物化输入也不等同 provider 已读取。
- 最终 Goal 用例确实在 Goal 激活前接纳独立辅助请求，然后启动真实 Goal AgentTask/provider，在其等待期间形成实际成功 proof/receipt。主请求真实用量使 Goal 进入 BudgetLimited，只有 aux/main 两个请求、一个 Turn，咨询仍 pending 且没有 Consumed。没有伪造 foreground owner，也没有修改既有 Goal settlement fence 来强迫该组合运行。暂停/预算重启与 held FIFO 用例分别核对实际 continuation/user owner。
- 两个生命周期用例走真实 `run_session` mailbox、QueuePrompt 和 AgentTask，覆盖 Ctrl+C、旧 Turn steer 拒绝、后续用户输入以及 suspend/checkpoint/retire/replacement prepare/commit/release。replacement 使用 fixture ACK 和 canonical Timeline 重建，范围未冒称物理冷加载。
- `/tmp/grow-passive-joint-delete.log` 实际为 **1 passed、0 failed**，1.43 秒：独立进程中的真实 MvpAgent、SessionThread、JSONL 和 public delete；writer drain 前目录存在，drain 后才删除；保留 Unix journal FD 核对最终 active/queued Cancelled、无 proof/InquiryAnswered，并验证旧 channel 拒绝迟到询问。当前代码在冻结上下文之前已有 target Sideband 关闭分支，因此 queued Cancelled 是实际运行结果，不是为满足测试额外改变生产语义。
- `/tmp/grow-passive-joint-routes-post-r11.log` 实际为 **2 passed、0 failed**，2.99 秒，覆盖最终 R11 后的双向父子工具和 Peer 工具/外部 API 分支。自定义 child runner 的端点/寿命 seam 与完整 OS child 新建仍有区别，记录未混淆二者。

独立审查者已重新计算 `/tmp/grow-passive-joint-final-source-manifest.json` 中 **26 个文件**的 SHA256，全部匹配（8 个生产文件、18 个测试/支持文件）。本轮迭代复审完成，未发现剩余可行动阻塞问题。主 agent 仍负责冻结后的全量回归、静态检查、规范归档与 release；本节的定向通过不提前代表这些步骤完成。真实新咨询 source 与每种 compaction/fork/进程恢复的全交叉仍未穷举，其共享回归与精确 source 验证的组合边界已在逐项证据表说明。
