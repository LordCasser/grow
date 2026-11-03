# 最后一轮独立对抗性审查

2026-11-03。审查者未参与本次实现，未运行 Cargo，未修改生产或测试源码；本记录区分静态核查、已读取的执行日志和没有执行的组合。结论：在下述范围内，未发现剩余必须修复的问题。全量回归、规范归档及 release 构建由主 agent 另行记录，本结论不代替这些结果。

## 审查基线与范围

- 先读取根 AGENTS.md、software-engineering skill 及交接契约、OpenSpec 索引、本 change 的 proposal/design/tasks 和三份 delta；按本次有意修改的 delta 判断行为，不拿尚未合入的旧隔离语义否决成功回写。
- 首个实现的生产差异以 `/tmp/grow-passive-coder-baseline/` 为基线；R10 的直接 Ask 等待与取消修正以 `/tmp/grow-r10-before/` 为补充。`/tmp/grow-passive-final-source-manifest.json` 只用于圈定 R9 的文件范围，没有把其旧 hash 当作本轮状态。
- 实现者确认了 R9 后 owned 文件。共享 `actor/coordination.rs` 的图片冻结、tools backend 的历史查询等外部变更不归责为本任务；核查了它们与询问冻结、原 owner 和最终成功接纳的交界。
- 实际审读的生产路径为 `coordination/{inquiry,mod,runtime}.rs`、`actor/{coordination,notification_drain,interjection,mod,spawn,completion_delivery,run_loop}.rs`、`actor/turn/{mod,admission}.rs`、`actor/tool/result.rs`、`storage/mod.rs`、`local_ipc/frame.rs`、`subagent_coordinator/interaction.rs`、tools `task/backend.rs`，以及 ChatState 的 `timeline/handle/commands/actor/trajectory` 变化。当前 22 份生产文件指纹保存于 `/tmp/grow-passive-final-independent-reviewed-sources.json`，含共享文件现状，不代表其全部内容都属于本任务。
- 用 scoped Atlas 查询辅助定位，以当前源码核对实际边界；没有全库索引，也没有用整体 git diff 把脏工作区的前序改动当作当前交付。

## 从不变量核对的结果

成功候选在已有 terminal 控制栅栏内、canonical 查重和所有准备 await 之后，以 InquiryCancellation 原子状态选择取消或成功接纳。成功接纳不等于 durable success；ACK 失败返回 AuditFailure，未确认成功不能派生收据。后到 inquiry-specific cancellation 不能反转已经接纳的成功候选，目标关闭仍由独立 Sideband 生命周期管理。直接 Ask 保留 host receiver；Peer 只把本地 cancel bool 当 intent，确认注册后向 pinned 目标请求取消，并继续读取同一个可能已部分读取的 frame future。真实结果仍来自原交换或其实际传输/期限失败。

完整问答在成功证明前同时经过既有通知正文限制与 Peer 的真实 JSON 帧 encoder。直接 delegation 不被误加 Peer 限制。拒绝不可交付的成功候选时保留已完成 Sideband Result/End 与用量，不截断正文，不创建永久无法补齐的成功义务。

canonical terminal、稳定 Received 身份、Consumed 与准确合成输入复用现有 Timeline。冷验证把新证明绑定到原回答 Session、其 InfoRequest spawn/request、问题/source/cwd、Completed Result 与完整答案，并检查 receipt 的 owner、terminal version、正文长度/hash；旧无证明 Answered 不产生义务。live reconciliation 的 suffix cursor 只在自身捕获范围已协调后推进；失败不会把已持久化的 Answered 改写为 Failed。恢复和 read-only observation 的写入权界限仍清楚。

新来源是 Session-owned context-only；它不进入 interjection buffer，不设置 parent interrupt，不通过 HumanIntent，不产生 PermissionEvidence，也不作为 idle autostart 原因。最终回复在同一 step-control gate 中捕获 candidate IDs，释放栅栏后只消费这些 IDs；payload I/O 后再次校验精确 Turn、执行 owner、terminal preemption、Behavior 与 Goal gate。first/successor/final 的必需消费错误向上传播为 fatal boundary，不能落入 completion recovery 自动重采样。Consumed 已确认但 provider 尚未发送的记录，只能证明 Surface 已提交，不能证明模型读取或认可。

fork 的实际存储路径从按既有规则过滤的 Surface 建立新 Timeline，不复制原 pending/terminal ledger。冷 reconciliation 也明确跳过他属 target proof。既有 `fork_filter_surface` 会删除 synthetic User，因此本次 delta 的“按既有 fork 历史/来源规则继承”不能被描述为所有 fork 模式必然保留全部已消费咨询正文。compaction/portable projection 不获得新的消费或执行权：咨询输入是普通 NotificationDrain User，Consumed 是已有 Surface producer，原 canonical 证据仍可查。

## 验证证据的独立核对

已逐断言读取新 message_delivery 组合、双向直接路由、Peer/API、取消、帧限制、生命周期、ACK 与删除测试；不只依据测试名称判断覆盖。以下是读取他人执行日志的结果，不是本审查者运行测试：

| 日志 | 实际结果与覆盖 |
| --- | --- |
| `/tmp/grow-passive-joint-boundaries-final-v2.log` | 16 passed。真实前台/后继 provider、实际 TaskTool 等待与受控 backend、Goal 暂停/预算耗尽/显式 restart、held FIFO、实际 mailbox Cancel 与 suspend/replacement、三条消费失败、两类取消胜负、通知/Peer 大小及 ACK 丢失。 |
| `/tmp/grow-passive-joint-routes-post-r11.log` | 2 passed。真实 AskParentTool/AskSubagentTool 经 coordinator 活跃关系路由，两端独立 Sideband；真实 AskSessionTool 与外部 coordination API。来源原 ToolResult/API 响应不被另造咨询输入，回答方仅一次归属问答且无权限证据。 |
| `/tmp/grow-passive-r10-runtime.log` | 31 passed。包含原 reconnect/lease/identity/独立进程路径、cold OutgoingStarted 重试及没有外层 reconnect 可掩盖问题的部分帧取消测试。 |
| `/tmp/grow-passive-r10-tools-backend-v2.log` | 24 passed。Ask 取消后显式 poll 仍等待 host，随后保留真实 Answered 或原错误；原 backend 分支回归。 |
| `/tmp/grow-passive-r11-ipc-frame.log` | 3 passed。真实 encoder 对应的字节序、round trip 和收发帧上限。 |
| `/tmp/grow-passive-joint-delete.log` | 1 passed。公共永久删除经真实 MvpAgent/SessionThread/JSONL，等待实际 drain 后删除；active/queued inquiry 均 Cancelled，无成功 proof/receipt，旧 channel 不能重建已删除目录。 |

最终 Goal 预算测试保留了“真实 main 用量导致 durable BudgetLimited、无 successor、同一 receipt 未消费、只有原 Turn”的断言。早期 v4 用 transient provider gate 表示已经退休的 Goal window 导致 fixture 断言失败；当前检查 durable 状态和 window 退休，未放宽上述契约。最终 16 项日志为零失败。

实现者冻结后的逐项映射见 [joint-task-evidence.md](joint-task-evidence.md)，已独立核对其证据层级与上述断言一致。对 `/tmp/grow-passive-joint-final-source-manifest.json` 的 8 份 production、18 份 test/support，以及本审查的 22 份 production 指纹重新计算，均无差异。

ACK 测试把候选写入捕获为 canonical events，hold/drop 首个 ACK，再重建 ChatState，检查同一 fact 重试、一次输入和 ACK 前零 dependent provider。它证明该持久接口下的确认/重放语义，不是物理进程崩溃或真实磁盘 ACK 丢失。suspend/replacement 同样走真实 mailbox 控制与 canonical fixture 重建；物理 JSONL 证据由独立删除测试和原有恢复回归提供。

## tasks 3.3 / 3.8 的证据边界

最终回复之前收到真实成功咨询会继续同 Turn；最终 Turn 已 Ended 后的真实 Received/ACK-loss 保持 pending，下一显式用户 Turn 才消费。m1/m2 的精确候选集合由真实 selected-drain 单测与当前 `close_steering_and_drain` 的栅栏/选集传递实现共同证明；有限状态模型只是补充，不冒称完整 Rust actor 并发实测。未发现需要再扩成全组合测试的新增交互漏洞。

original owner/fork skip、terminal 缺 Received 的冷恢复和 proof/receipt 负例使用真实咨询生成的证据及 canonical 前缀重建。跨 backend 的 portable fixture 验证已完成/部分完成工具配对，并非所有 backend 都重新跑完整被动咨询生命周期。没有新增“含 InquiryAnswered 的独立进程 compaction + fork + 恢复”整条旅程；本轮依据普通输入/Surface producer 的既有契约与实际源码、原有回归、特定来源冷负例和上述真实 provider 旅程判断，不能宣称此组合已经物理实测。

其他 Model/Behavior/compaction/rewind/fork 管理仍属于原机制，本次只观察其已发布的执行资格与消费边界。没有因审查补入通用 controller、ledger、权限流程、重试墙或额外产品状态。

## 后续快照与测试启动边界复核

主 agent 随后在仍被外部修改的原工作区运行全量，记录 ChatState 610 passed，Shell 4515 passed / 12 failed / 9 ignored。该结果不是全绿，也不能用之前的定向通过替换。为固定验证和构建输入，主 agent 建立 `/tmp/grow-passive-release-20261103`，基于 HEAD `bbd2c4dfa19d2ba789d15e0d8f7e8e2608a236cb` 及当时工作区快照；源指纹为 `624eb887e3cf345ae1b831afdc0e5ddd1bc042e272b9f285e4f01446b06bd4d1`。本附录静态复核时，`/tmp/grow-passive-release-snapshot-source-before-tests.json` 中 2735 个源文件的逐文件 SHA256 与该快照一致。隔离失败项重跑日志 `/tmp/grow-passive-snapshot-failed-serial.log` 当时仍在编译，不在此声明验证通过或 release 成功。

本次追加审查仅覆盖此前独立 review 后的本任务变化：

- `coordination.rs` 的 Peer encoder 调用和 `storage/mod.rs` 的冷 Peer encoder 校验经 rustfmt 折行，调用值、错误映射与成功接纳顺序没有改变。对照 `/tmp/grow-passive-pre-format/`，`persist_coordination_inquiry_with_cancellation`、`record_coordination_terminal_notice`、`reconcile_coordination_answers` 三个本任务核心函数与快照逐字相同；此前已冻结的 InquiryCancellation、runtime、mod、interaction 与 frame 文件也仍匹配此前 owned 生产指纹。共享 tools backend 的 Ask host-wait 分支仍保留。
- `message_delivery_lifecycle_tests.rs::start_lifecycle_mailbox` 改为 async：启动真实 `run_session` 后，通过已有、只读的 `GetActiveAgent` mailbox 回应确认主循环已开始处理命令；该控制阶段有独立 10 秒期限。三个调用点均 await，随后开始原测试；原 foreground provider 阻塞等待的 5 秒期限未改，也没有删除取消、pending、精确旧 Turn 拒绝、TurnStarted/input ID、恢复 no-wake 或唯一消费断言。它把 actor 启动耗时与已经开始的流控制旅程区分开，不以放宽被测试行为的期限掩盖失败。原工作区与快照中的这份 lifecycle 文件一致。未发现此改动引入的必须修复问题。

快照中仍包含外部共享文件变更，例如 inquiry 的图片冻结/恢复采样、memory flush 及 subagent observation。`coordination.rs` 整文件相对前次审查并非只有格式变化；tools backend 也有外部查询变化。它们不被本次有界追加审查自动背书，原文的 22/26 文件指纹匹配只描述当时状态。主 agent 负责记录最终快照输入和对这些输入执行适用验证。本审查者没有运行 Cargo，没有修改上述源码或测试，也没有归档、构建或提交。

## 隔离快照的测试边界与 oracle 复审

本节由迭代审查者 `injection_design_review` 追加，只读对象为 `/tmp/grow-passive-release-20261103`，不使用仍有外部并行修改的原工作区推断失败原因。已读取 `/tmp/grow-passive-snapshot-failed-serial-v2.log`：12 项串行结果为 10 passed、2 failed；随后 `/tmp/grow-passive-snapshot-full-libs-final.log` 的实际结果为 ChatState **610 passed、0 failed、3 ignored**，Shell **4,544 passed、3 failed、9 ignored**。这些日志均不能记作全量通过。以下修正已经静态复核，最终修正后定向运行仍待主 agent 汇总。

首次 disabled-memory 的 End 缺失来自 storage-backed 夹具的假 ACK，不是生产 finalizer 尚未结束：生产在 HTTP 前关闭路径显式 await `sideband.fail(Cancelled)`，然后才发送完成通知；原 support bridge 却给 Sideband 转发建立另一个 observation ACK，并立即向原调用方返回 Ok，因此真实 JSONL append 尚未完成。修正只在有真实 storage 的图片夹具中把 Sideband 原 ACK 交给实际存储 actor。Attempt 的物理 ACK 先返回，再由现有可控 gate 延迟交付；gate 等待及真实结果转发放到独立 `spawn_local`，避免堵住同一 relay 中 `/memory off` 需要的 HostControl/通知。该测试现在控制的是实际 durable Attempt 边界；同 Sideband 的下一事件仍须等待原 ACK，不以 End 轮询或 activity 空闲掩盖假确认。无 storage 的普通 mock 语义及生产代码不变。

progress publisher 的关闭断言原先在 cancel 后只 yield 一次，不能证明异步 worker 已释放 receiver。当前测试 await 原 oneshot sender 的 `closed()`，保留有限期限及原 `is_closed()` 断言；它等待的正是被检验的 receiver 关闭，而非增加等待时间后猜测结果。该修改没有改变 publisher 的生产控制机制。

全量中三项失败的 bounded oracle 修正如下：

- inquiry 的 `max_attempts` 是已冻结图片恢复策略预留的预算，当前生产允许 2；它不等同实际 provider 次数。测试改为精确预算 2，同时保留一个 InfoRequest、实际一次 Attempt、一次 HTTP 及 foreground Surface 不变，未允许额外实际采样通过。
- disabled-memory 的 provider 请求是解析后的 JSON，序列化 `Value::to_string()` 会转义 prompt 的换行，不能与原始多行 prompt 作字符串包含比较。当前测试按真实 `SystemParam` 解析并精确比较完整 `FLUSH_SYSTEM_PROMPT`，没有改成宽松关键词。provider 返回后，测试严格要求一个实际 Result、原始正文和 10/5 用量，以及精确 Completed；仍要求 flush 返回 false、一次请求、无 memory 文件、storage 保持 None、memory root 不重建。外部 change 的 WHEN/THEN 要求 response 不写入 memory，design 的 Cancelled 明确用于 HTTP 前准入关闭；它没有要求删除已经生成并结算的 Sideband 事实。`SessionMemory::is_enabled()` 与 `storage()` 读取相同 Option，当前 dedup/write 读取 None 满足关闭后的目标存储约束。本次没有为满足测试把已生成结果改成取消，也没有允许两种 Outcome 模糊通过。新 raw-output 断言曾误写 literal `\\n`；已通知主 agent 修正，并重新核对 provider 与 expected 均为同一 Rust `\n` escape。可能使用修正前字面量编译的中间结果不能替代最终重跑。
- promoted compaction 的恢复请求必须沿被冻结的原 request 模型，不能把模拟 SSE 响应里的 `test-model` 当成请求路线。该 fixture 的初始 Session SamplingConfig/provider model 是 `test`；当前测试精确比较 retry 与已捕获的初始请求 model，仍保留 vision-model、OCR 图片存在、retry 无图片且含精确截图文字、三次真实请求、两个不同 transaction、原失败与新完成事实及持久投影断言。后续运行暴露原 `text.contains(&summary)` 包含了 mock 字符串尾部空格；实际 publication 调用 `format_compact_summary_content`，其 cleaner 明确执行 `trim()`。当前验证完整 `summary.trim()` 出现在 Surface，没有退化为前缀匹配。mock 的 winning summary 本身没有截图文字，压缩替换旧范围后不能要求截图文字仍另行出现在最终 Surface；现改为精确验证一个 durable ImageDescription Result 的完整 OCR 正文及 Completed，并保留 ImageProjection 和重试请求证据。

上述测试和夹具的最小变化已静态接受，未发现需要新增生产修改的证据。中间定向 v2 的 1 passed / 2 failed 保留为阶段结果，不能用静态接受替换；当时分别尚有已修正的换行 literal 和摘要尾部空格 oracle。最终修正后的 v3 仍待主 agent 取得运行结果。本节不是新的全量绿测或 release 结论；主 agent 负责最终修正后的编译、运行结果、快照指纹和构建记录。审查者没有运行 Cargo，也没有修改生产或测试源码。

## 冷观察测试的进程级路径隔离复审

主 agent 取得后续完整日志 `/tmp/grow-passive-snapshot-full-libs-green.log`；文件名不是结果状态。独立读取确认 ChatState **610 passed、0 failed、3 ignored**，26.91 秒；Shell **4,546 passed、1 failed、9 ignored**，724.93 秒。唯一失败是在 `public_subagent_get_reads_only_the_authorized_canonical_cold_result` 的 `init_session_with_summary` 处得到 `AlreadyExists`，路径为复用的 parent GROW_HOME 下固定 `cold-observation-root`，不是 public observation 的权限或结果断言失败。此前的完整通过与新独立进程诊断不能替代该失败日志，但支持继续定位夹具隔离。

静态原因已确认：`config::paths::grow_home()` 通过进程级 OnceLock 初始化；测试在初始化后改 EnvGuard 无法改变 `JsonlStorageAdapter::new()` 的路径。该 fixture 使用固定 root/child ID，故复用已经初始化且含旧数据的 parent home 会触发目录冲突。

原工作区和隔离快照现均采用已有 consultation-deletion 测试的子进程模式。外层创建独立 TempDir，通过当前 test binary 的完整 `--exact` 名称执行同一测试，显式设置 child marker 和 GROW_HOME，检查退出成功及实际一项通过；child 执行原 public API、真实 canonical history 和全部原结果/父子身份/授权 scope/损坏链接断言。没有新增忽略项、吞掉 AlreadyExists、删除残留目录后继续或更改生产路径缓存。该 wrapper 仅改变本 fixture 的运行隔离。

此时对整个 observation 模块在已初始化、复用同一 parent home 的条件下顺序执行两次，是与改动相称的验证：它直接检验污染来源，同时保留相关公开查询回归。若这两次通过，且主 agent 证明相对刚完成的完整运行只有该 test wrapper 变化、生产输入不变，则不需要仅为这个隔离修改再重复全部 Shell 测试。应分别记录完整运行的真实失败及修正后的定向结果，不能把原日志重新标成零失败。当前定向结果仍待主 agent 获取；最终 CLI/workspace 检查、指纹、归档与 release 继续由主 agent负责。本次静态复核未发现必须进一步修改生产的证据，审查者没有运行 Cargo或修改源码。

### 最终独立处置

独立审查者现已读取 `/tmp/grow-passive-snapshot-observation-fixed-first.log` 和 `...-second.log`：完整 observation 模块分别 **3 passed、0 failed**，0.27 秒与 0.30 秒。两次都包含 nonblocking inspect、真实 canonical cold-result API 与 observation deadline 用例；隔离 wrapper 的 child 必须实际运行原测试并通过。主 agent 记录两轮复用同一已有 parent home，没有清空其旧数据。

独立比较 `/tmp/grow-passive-release-snapshot-source-final-tests.json` 与 `...-source-final.json` 的 2,735 份输入清单，差异精确只有 `crates/codegen/shell/src/agent/mvp_agent/tests/subagent_observation_tests.rs`。此前完整运行之后没有新的生产输入变化。随后对最终清单中的全部 2,735 文件重新计算 SHA256，与当前隔离快照全部匹配；最终聚合指纹由主 agent 记录为 `94070283c23b2e96f93749610eb53840e702e4cb6c451aff49c515d67f986377`。

本轮独立审查完成：已发现的问题均完成对应的最小修正和适用验证，未发现剩余可行动阻塞项。完整运行仍如实为 Shell 4,546 passed / 1 failed，修复后的模块另有两次 3/3；不把两个结果拼写成一次不存在的全量零失败。仅因该测试隔离 wrapper 无需再次重复全部库运行。最终默认 CLI、workspace all-targets、规范归档及 release 构建仍由主 agent 记录，本处置不提前宣称这些步骤成功。审查者未运行 Cargo，未修改生产或测试源码。

### workspace all-targets 暴露的测试编译依赖

必需的 workspace all-targets 检查随后发现 Pager 的 `tests/pty_e2e_auxiliary_image_commands.rs` 有 12 个 E0433/E0425：既有调用缺少 `InferenceEndpoint`、`InferenceRequestMatcher` 导入和 `wire_image_count` 定义。主 agent 只给隔离快照补入这两个导入及原作者当前文件中逐字相同的纯 helper，没有整文件复制原工作区之后的 PTY 运行时修改。v2 再暴露三个 E0596，现仅把既有 rejected/description/retry expectation 绑定声明为 mut，以供原来的 `wait_received` / `wait_satisfied` 调用借用；匹配条件、测试动作、期限及断言均未变。

独立静态核对确认 helper 与原作者版本逐字一致。把快照中这两个导入、该 helper 和三个 mut 反向移除后，文件 SHA256 精确恢复到此前 `940702…` 输入清单的值；对该清单其余 2,734 文件逐项比较，没有其他内容变化。修正仍只属于一个测试编译单元，不涉及生产代码。

主 agent 同时把编译输入清单由 2,735 扩为 2,739，补记原先未纳入的四个 top-level `include_str!` 输入：`.grow/workflows/deep-research.rhai`、`agent.md.example`、`docs/architecture/agent-core-timeline.md`、`docs/workflow-rhai.md`。独立定位了各自实际 include 调用；这是指纹覆盖范围的补齐，不是新增这些功能或修改相应内容。前述 `940702…` 是当时清单的历史指纹，最终指纹须由主 agent 按扩大清单及本测试编译修正重新记录。

上述最小编译修正静态接受，未发现必须再改生产或断言的理由。v3 all-targets 当时仍在执行，尚未在本节声明通过；编译检查也不等同本 PTY 文件的运行时端到端测试通过。主 agent 负责保留初轮及 v2 编译失败记录，汇总最终检查结果与输入指纹。审查者未运行 Cargo，未修改源码。

v3 随后完成 Pager 测试目标的检查，但在既有 `shell/tests/test_session_protocol_recovery.rs` 暴露两个生命周期编译错误：1129 行 E0716 从临时 `server.requests()` 返回 Vec 收集借用后继续使用；1461 行 E0502 的 pinned prompt 底层 future 仍持有 client 的不可变借用，却先调用需要可变借用的 close。独立审查者读取了实际 v3 编译错误，没有把该轮标作通过。

快照只补入两个局部 lifetime 修正：先绑定 `prompt_flush_requests`，使其覆盖后续 body 借用；为原 held-prompt 序列建立词法块，并在块内等待同一 `prompt.as_mut()` 完成，使 `tokio::pin!` 的底层 future 在 `client.close()` 前释放。mock、原动作顺序、filters、count/model/text/foreground-pending/内存文件等断言均未变，没有修改生产代码或把原作者之后的集成测试运行时变更整体复制进来。这两个 fixture 是 ignored image journeys；本次只审查其能否合法编译，没有宣称已经执行 ignored 旅程。

再次对照最近完整库运行的 2,735 文件清单，当前差异精确为三份测试文件：observation subprocess wrapper、Pager 缺失定义/可变 guard，以及上述 shell integration lifetime；没有生产文件变化。扩大清单的四个 include 输入另按前述原因记录。该最小修正静态接受，v4 all-targets 正在运行，最终结果和新指纹由主 agent 记录；审查者未运行 Cargo或修改源码。
