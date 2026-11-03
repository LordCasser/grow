# 补齐联合旅程的验证记录

> 本文保留联合阶段及最后验证的运行过程。已完成归档和 release；最终实际结果、完整源码清单、产物路径与 SHA256 见 [release-verification.md](release-verification.md)。

状态：本 coder 的 R10/R11 与关键联合旅程已实施、定向验证并冻结，等待主 agent 最终全量、独立审查、归档与 release。以下保留真实阶段失败与修正，不把只读源码或模型当实际 provider 测试。

- 第二阶段源码基线：`/tmp/grow-passive-joint-baseline/`。保护外部并行图片冻结上下文改动，不整文件覆盖。
- 实际 TaskTool→受控 backend spawn 等待，咨询生成真实 InfoRequest/proof/receipt，不取消或提前完成原工具，工具结果先于消费、后继真实请求包含两者。`/tmp/grow-passive-joint-tool.log`，1 passed，0.22 秒。控制 backend 代替 child 执行，不冒称完整 child 生命周期。
- 实际双向 AskParentTool / AskSubagentTool，两个真实 source/target provider，scoped ChannelBackend→真实 SubagentCoordinator 活跃关系检查→生产 interaction::run→真实 source audit/target InfoRequest。双方 source 原 ToolResult 各一次，回答方自己的 Q/A 各消费一次，无 PermissionEvidence。合法 late receipt 使用真正后续用户 Turn。`/tmp/grow-passive-joint-routes-v4.log`，1 passed，0.30 秒。
- 实际 AskSessionTool 与外部 grow/coordination/ask 两分支：真实 MvpAgent publisher、SessionCoordinationBackend、CoordinationRuntime 和真实目标采样，来源只得到原工具结果/API响应、没有合成咨询输入。`/tmp/grow-passive-joint-routes-final.log`，2 passed（包含前一双向用例），3.01 秒。
- 上述编译曾修正 test-only visibility/type inference、AskSessionTool 缺少 ListActiveSessionsTool 依赖。初始 seed 错误地直接等待 ChannelBackend.spawn 的终态；改为真实后台 future+started ACK，测试结束后释放并 join。未延长超时或改生产语义。
- Goal/FIFO 初轮暴露夹具错误：idle Sideband 没有 foreground Goal usage owner，不能据此假设预算触发；改为真实 Goal AgentTask/provider 先耗尽预算。FIFO 的 queued RPC responder 由 handle_completion 结算，已补真实结算，保留原 response/内容断言。最终本组 3/3 及后续消息投递组复跑结果见下文。


## R10：取消与成功接纳

- 真实 red：hold provider 返回后的 Sideband Result ACK → inquiry-specific cancel → release，原实现仍发布 Answered。`/tmp/grow-passive-r10-before.log`，1 failed，0.71 秒。
- 生产修正复用 InquiryCancellation 原子状态，在已有成功 terminal gate 内、canonical 查询后/append 前选定取消或 success admission；durable ACK 才形成上下文义务。队列返回实际 committed terminal outcome。直接父子保留原 host response；peer 在确认注册后向 pinned 目标请求一次取消并保持原帧 future，不再本地无条件合成取消。
- `/tmp/grow-passive-r10-cancellation-v2.log`，3 passed，0.20 秒：取消先赢无 proof/收据；success admission 先赢后到取消不反转，成功 ACK 才有收据，实际 StorageFull/AuditFailure 不形成成功义务；实际 peer 的两种胜负均沿原流，provider 各一次。其后强化 fault writer 捕获 Received 写入=0，已在最终 10 项及 16 项消息投递组复跑中确认。
- 冷来源 red：旧 durable OutgoingStarted、原目标已接纳成功，来源重建后携带取消 intent，不得误判确定未派发。`/tmp/grow-passive-r10-cold-before.log`，1 failed，0.26 秒。修正从本次 admission 之前读到的旧事实派生局部不确定性初值，不新增 record 字段或持久账本。
- `/tmp/grow-passive-r10-runtime.log`，31 passed，3.04 秒：包括原 identity/lease/reconnect/取消/独立进程回归、新冷来源反例和直接 request_inquiry 部分帧回归。部分帧测试没有外围 reconnect 可隐藏读帧错误，明确只有一个 Ask 与一个 Cancel 连接；Progress 和下一帧头两字节在同一 prelude 中发送。
- `/tmp/grow-passive-r10-tools-backend-v2.log`，24 passed，5.01 秒。新增公共 ChannelBackend Ask 在取消边界被显式 poll 仍 Pending，随后严格返回 host 的 Answered 或原错误；不借取消 intent 关闭 host receiver。原 Send/Replan 分支不变。阶段编译有宏词法位置错误，修正后该组全绿。
- 生产 R10 前文件快照：`/tmp/grow-r10-before/`，采用当时最新 coordination.rs，保留外部图片冻结改动。Tools backend_tests.rs 基线是在其现有格式化源码上仅移除本任务唯一新增测试块重建，作为 scoped review 对照，不声称捕获于编辑之前；未覆盖该文件的前序变更。

## 最终真实路由与生命周期增量

- Goal/FIFO 最终 `/tmp/grow-passive-joint-goal-fifo-v3.log` 3 passed，1.30 秒（含实际 TaskTool wait）；重启 Goal 的 Started origin/Goal revision 和用户 FIFO 的 Started origin/prompt identity 已加断言，后续 message_delivery 总过滤复跑通过。
- R10 最终 `/tmp/grow-passive-r10-all-delivery.log` 10 passed，1.88 秒，包含强化后的 writer fail-stop 路径：已尝试的 Received 写入为 0，不把 poisoned actor 的查询缺失当作空集合。
- R11 真实 red `/tmp/grow-passive-r11-before.log`：600,000 字节 quote/backslash 答案正文可接纳，而实际 Response::Inquiry JSON 超现有 1 MiB IPC 帧限制；旧生产仍 Answered，1 failed，0.46 秒。修正复用实际 wire writer 的纯 encoder，Peer live seal/proof 前与冷 proof 接纳均检查真实响应；直接 delegation 无 Peer 限制，保留全部 Completed Sideband/用量。
- `/tmp/grow-passive-r11-lifecycle.log` 13 passed，2.26 秒，含 R11 上述 producer、冷导入 route-only 负例及既有完整通知 cap 边界，并包含两个真实 mailbox/AgentTask 生命周期用例。取消后新 user Turn、replacement 后新 user Turn 均断言 Started user origin/原 prompt ID/非空 input IDs。
- `mailbox_cancel_to_idle_keeps_pending_consultation_until_new_user_turn`：真正 run_session/QueuePrompt/AgentTask/Ctrl+C；source actual InfoRequest 成功后 pending，旧 Turn steer 拒绝，零自动 foreground；明确新输入只消费一次。
- `suspended_consultation_stays_pending_through_replacement_prepare_commit_and_release`：真实 suspend/checkpoint/retire/replacement prepare/commit/release，关闭执行时保持 pending；释放后仍不 idle 自启，新用户 Turn 消费一次。Timeline 来源为 fixture canonical records/ACK，不冒称磁盘进程重启；物理 JSONL 另由删除和既有 cold/process recovery 证明。
- `/tmp/grow-passive-joint-delete.log` 1 passed，1.43 秒：真实 MvpAgent/SessionThread/JSONL，public permanent delete 在 drain 前保留目录，active 与 queued inquiry 都由当前实际 target-close 分支结算 Cancelled。unlink 后 retained journal FD 仍可验证两条 terminal/no proof/no InquiryAnswered，旧 channel 拒绝 late producer 且目录不重建。未修改生产来满足测试。

## 最后边界与源码冻结

- 最终 `/tmp/grow-passive-joint-boundaries-final-v2.log`：**16 passed / 0 failed**，2.35 秒。包括所有 message_delivery 新旧定向测试、两类 ACK 丢失与真实 Goal 最终资格测试。
- `lost_received_ack_retries_same_receipt_without_rerunning_answer_or_waking_idle` 先完成普通用户 Turn，再发生真实 InfoRequest/proof/Received。fixture 已保留 Received 后 hold/drop ACK，原 writer 重试完全相同 seq/payload，canonical 重建只有一个 pending，不新增 Surface 或 Turn；明确下个用户 Turn 的请求只包含一次答案。该路径与 terminal→Received 之间根本缺失的前缀恢复分开，不冒称物理崩溃。
- `lost_consumed_ack_retries_same_fact_before_provider_and_replay_keeps_one_input` 已保留 Consumed+Some(input) 后 hold/drop ACK，dependent provider 在 ACK 前尚未调用；独立 canonical 重建已有唯一输入/无 pending，原 writer exact-event 重试后真实 provider 只接收一次。物化不是模型已读取。
- `actual_goal_budget_closes_final_successor_with_consultation_still_pending`：真实 aux 在 Goal 激活前 capture 无 Goal owner；新 Goal 实际 main 等待时释放 aux，产生真正 proof/receipt，再释放 main。实际用量触发 BudgetLimited，只有 aux+main 两请求、一个 Turn、原 receipt pending 无 Consumed。早期错误排程把新 Sideband 放在同 Goal main 未结算期间，被既有 settlement fence 合法阻塞，未修改调度/放宽 timeout。随后错误地在 durable BudgetLimited 后断言瞬态 Exhausted flag，已改成正确 lifecycle + active window 退休约束，仍保留精确请求数/单 Turn/无消费断言。ACK 测试曾漏传真实 SamplingConfig 和增加前置 Turn 后未同步请求索引，均为 fixture 编译/断言问题，最终未放宽语义。
- 最终 R11 后真实 source routes 复跑：`/tmp/grow-passive-joint-routes-post-r11.log` **2 passed**，2.99 秒；实际 IPC encoder/writer：`/tmp/grow-passive-r11-ipc-frame.log` **3 passed**，0.02 秒。
- 本阶段 8 个生产文件、18 个测试/支持文件的当前 SHA256：`/tmp/grow-passive-joint-final-source-manifest.json`。最后 IPC 检查后逐项复核 **无 hash 差异**。共享文件的外部改动仍由对应任务归属，manifest 表示当前完整输入，不能拿整文件差异归责本任务。
- `git diff --check` 通过。后续全库、fmt、OpenSpec、归档及 release 由主 agent 执行；本 coder 没有进行 archive/commit/release/安装或停止外部 Cargo。
- 逐项 3.1–3.9 证据和组合边界见 `joint-task-evidence.md`，其中明确 generic compaction/process/fork 回归不是新咨询 source 的全交叉进程 E2E。

## 主 agent：最终验证的隔离与夹具修正

本节在最终全量进行中更新；归档和 release 结果须以下面的实际收口记录为准。

第一轮主 agent 全量命令为 `cargo test --locked -p shell -p chat-state --lib -j 4 -- --test-threads=2`，使用独立 GROW_HOME、16 MiB 测试线程栈及 macOS 原生工具优先的 PATH。日志 `/tmp/grow-passive-joint-full-libs.log`：ChatState **610 passed / 3 ignored**（20.62 秒）；Shell **4515 passed / 12 failed / 9 ignored**（569.43 秒）。保留全部失败，未把它们算作最终通过。该轮同时观察到原工作区编译输入持续变化，同名测试二进制也被外部构建替换，不能据此声称最终文件集已经通过。

后续在原工作区重建失败组时，又碰到外部改动中的 helper 可见性编译暂态。主 agent 因此建立 `/tmp/grow-passive-release-20261103` 独立工作树，复制当时完整工作区文件，构建缓存独立于原 target，位于 `/tmp/grow-passive-release-target-20261103`。原工作区的其他修改保持原状。最终产物按此冻结代码的指纹追溯，不把后来原工作区的更新计入该产物。

隔离重建补正了测试本身的边界：

- lifecycle fixture 启动真实 `run_session` 后，先等待既有 `GetActiveAgent` mailbox barrier，再开始 provider 计时；三个调用点均 await。控制阶段为既有 10 秒边界，原 provider 5 秒期限及取消、no-wake、owner、一次消费断言保留。此前日志仅能证明 provider 等待超时，不能据此确认某个具体启动步骤就是根因。
- 新图片恢复调用链使测试 future 的 layout 查询超过编译器默认深度；在既有 `run_user_turn_with_schema` 的真实 `handle_prompt` 调用处用 `Box::pin` 限定测试包装的 future 布局，没有提高全 crate 的递归限额。
- 快照中另一项并行观察测试的事件类型歧义及临时请求借用编译错误，采用其原作者已在原工作区完成的 test-only 修正。
- progress publisher 的取消测试改为等待 pending delivery receiver 实际关闭，然后保留原关闭断言；一个 scheduler yield 不等于取消 owner 已退出。
- storage-backed 图片 fixture 原来把 Sideband 写入传过一个提前返回成功的 mock ACK bridge，导致所谓 durable Attempt 以及 Cancelled End 的物理落盘边界并不真实。该模式下改为转发真实 storage ACK；无 storage 的单元模式保留原 mock。可控 gate 仅延迟原结果的交付，真实结果（包括错误）保持不变；gate 的等待放到独立测试任务，outer relay 继续服务 `/memory off` 所需控制消息。同一 Sideband 仍须等原 ACK 才能发下个事件。保留零 HTTP、Cancelled End、用量和无 Result 等原断言。

隔离首次重建的类型/借用和 future 深度错误记录在 `/tmp/grow-passive-snapshot-failed-serial.log`。修正后 `/tmp/grow-passive-snapshot-failed-serial-v2.log` 为 **10 passed / 2 failed**（135.67 秒）；剩余两项为上述异步关闭与真实存储 ACK 夹具。中间一轮 gate 阻塞了控制消息，主 agent 仅终止自己的隔离诊断测试进程，日志 `/tmp/grow-passive-snapshot-last-two.log` 保留 SIGTERM 结果。修正 relay 后 `/tmp/grow-passive-snapshot-last-two-v3.log` **2 passed / 0 failed**（0.19 秒），随后启动完整库回归。

最终全量前 2735 个编译输入及 HEAD 指纹保存于 `/tmp/grow-passive-release-snapshot-source-before-full.json`，当前为 `8aa01f78903f51cb4d8dbfbc544aefed94d46b4d42e83f1197261e53dcd545d0`。库测试函数串行运行，联合场景内部的 actor、provider、Sideband 和 barrier 交错保持原样。全量日志为 `/tmp/grow-passive-snapshot-full-libs-final.log`，完成后的结果和输入复核另行追加。

## 冻结快照：完整回归后的精确 oracle 修正

`/tmp/grow-passive-snapshot-full-libs-final.log` 实际完成为 ChatState **610 passed / 3 ignored**（26.11 秒）；Shell **4544 passed / 3 failed / 9 ignored**（667.04 秒），该轮前后编译输入指纹均为 `8aa01f78903f51cb4d8dbfbc544aefed94d46b4d42e83f1197261e53dcd545d0`。三项失败都保留，不能把这轮记成通过。

逐项核对当前已存在的图片恢复和 memory 契约后，只修正测试：inquiry 预留两次图片恢复预算，但普通错误仍严格一次 HTTP/Attempt；memory 请求按 Messages SystemParam 解码后精确检查完整 prompt，不在序列化 JSON 中搜索多行原文；promoted compaction 重试必须使用最初冻结的 request model，不能用模拟响应的 model 字段替代路线。摘要 publication 原本会 trim 首尾空白，测试因此比较完整 `summary.trim()`。mock 摘要未包含截图正文，不能要求被替换范围中的截图文字仍另行保留在最终 Surface；改为保留 retry 的完整截图文字、ImageProjection、精确一个持久 ImageDescription Result 的完整正文及 Completed。memory 关闭后的要求是禁止 memory 写入/重建，已生成 Sideband 则严格保留唯一 Result、原始正文、真实 10/5 用量和 Completed。没有为满足这些测试修改生产逻辑。

`/tmp/grow-passive-snapshot-last-three-v2.log` 的 **1 passed / 2 failed** 是中间结果：新增 raw-output 预期误写 literal slash-n，及上述摘要清理 oracle 尚未修正。两者修正后 `/tmp/grow-passive-snapshot-last-three-v3.log` **3 passed / 0 failed**（1.07 秒）。独立审查者复核了这些精确断言与生产调用路径，见 `final-independent-review.md` 末节。

Tools 已在同一私有 target、冻结源码重建；首个 `backend_tests` 文件名过滤实际选中 **0** 项，不能算验证。随后直接运行刚编译的测试二进制，过滤真实模块 `implementations::grow_build::task::backend::tests`，`/tmp/grow-passive-snapshot-tools-backend-final.log` **27 passed / 0 failed**（5.03 秒）。这是本快照该模块全部测试，含新的 Ask 取消 intent 不关闭 host receiver。

最终修正后 2735 个编译输入指纹为 `8156943cb788e51014ba5a327a6ef41899d91763ee8dfb43f979db54d749649d`，记录于 `/tmp/grow-passive-release-snapshot-source-final-tests.json`。该源码已启动新的完整 Shell/ChatState 回归，日志 `/tmp/grow-passive-snapshot-full-libs-green.log`；仅在实际通过后记录 green。后继 Pager tests check 日志为 `/tmp/grow-passive-snapshot-pager-check.log`。最终完整结果、归档与 release 实际情况在完成后另行追加。
