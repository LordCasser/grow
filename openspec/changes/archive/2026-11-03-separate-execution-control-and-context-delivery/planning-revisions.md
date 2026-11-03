# Demo 驱动的规划修正

用户要求先造 demo 进行状态机测试，依结果修正规划后再派发实现。demo 与记录位于 [demo](demo/README.md)，它是有界设计模型，不代表 Rust 运行时已验证。

## 本轮发现与决策

1. **候选快照不同于最终关闭。** 原文把所有“快照后到达”的消息限定到下一 Turn，过强且需要额外 durable min_turn 路由。修正为：只有快照内候选能作为本次续同 Turn 的理由；候选促成合法后继 Step 后，新到消息可进入新安全边界；不可逆最终关闭胜出才等待后续 Turn。实现捕获精确 IDs，不释放 gate 后再把所有 pending 当原快照。
2. **不能只保证 request 没有未提交输入，还要保证它没有漏掉已接纳义务。** 第一版 demo 的 dispatch 只检查已有 Surface，允许跳过 pending；现修正为当前已冻结的 eligible obligations 全部协调并消费确认后才允许发请求。
3. **pending 不只等于 Received。** 成功 terminal 已提交但收据缺失时，也存在 canonical 同步义务。独立审查给出的反例为 terminal→开启正常 Turn→Received 集为空→发请求。采样前必须先协调捕获前沿内的成功来源，之后才能消费消息；前沿后的来源留后续边界。
4. **持久化错误必须抵达真实准入。** 当前 `drain_active_notifications` 的 bool 会把 payload/Consumed 失败压成 false；不能只新增 source enum。真实采样、首请求和最终回复消费路径需要保留 Result，失败禁止依赖该上下文的请求；未同步失败不能改写已提交 Answered。
5. **取消/预算变化须作为转移，不能只作为初始条件。** 消费前资格撤销保留 pending；消费后资格撤销保留已提交 Surface，但不再发请求。不得为了回写强开下一 Step。
6. **结果引用复用现有返回值。** `SidebandRun::complete` 已返回 durable `TimelineRangeRef`；捕获并绑定原回答 Session 即可，不新增 Sideband getter 或复制冻结上下文。
7. **恢复协调不能每 Step 克隆全历史。** 使用现有 Timeline 查询的 seq 后缀形式与一个非持久、非权威的单调 cursor；启动重建一次，之后仅处理新增 canonical 来源。cursor 只在捕获前沿内所有义务协调成功后推进；失败不推进，幂等收据仍由 Timeline 决定。不另建 outbox/pending 状态库。
8. **抽象模型的单一成功终态假设必须由实际事实校验落实。** 运行时审查发现两个都合法的成功 audit 若共享 source/inquiry 却有不同 EventSeq，可能派生两份版本不同的收据。原 inquiry 第一个终态才是绑定结果；新增同步义务不能接受其后的第二个成功证明。冷加载也须核对每个新收据与原成功身份、owner、精确结果和正文指纹的对应，不能只验证成功 audit 的单向引用。
9. **在真实提交入口重新检查资格。** 读取通知正文有 await，读取前检查不能代表消费提交时仍有合法 Turn。沿既有控制 fence 在消费 admission 处复核精确 owner 与可继续状态；已经提交 Consumed 后再失去资格则保留历史并停止请求，不回滚事实。
10. **成功来源要有因果顺序，消费不能省略输入。** 新 proof 必须指向早于成功审计的 Sideband Spawn；原有工具结果通知可使用 input=None，但新咨询收据不能借该路径被“消费”却不产生模型输入。对应事实折叠与冷加载分别做最小校验，不扩展到其他 producer。
11. **先确认整个问答可投递，再发布成功义务。** 实际旅程审查发现 Sideband/JSONL 64 MiB 与通知 1 MiB 的已有上限不匹配，原实现可能把合法长回答记为 Answered，再让目标永久卡在不可同步义务。复用现有完整正文 renderer 与既有字节上限，成功终态前拒绝不能投递的咨询，保留已完成 Sideband 的真实证据/费用，不截断或重试。边界测试包括 UTF-8、问题/说明开销、恰好上限与多一字节，以及后续正常任务可继续。

最新实际旅程逐项判断见 [user-journey-audit.md](user-journey-audit.md)，与最初只列抽象状态的 demo 互补。非抢占的在途旧响应、咨询与正式决定的区别、预算成本仍是明确产品边界；不因分析这些边界而改变控制语义。

实际 Timeline 使用 Session identity 与 EventSeq 定位领域事实；实现以该真实坐标绑定 source version，不制造不存在的 event_id 字段。

## 首次实现任务

首个增量仍是一项完整功能：成功 ask 的回答方被动上下文回写，穿过真实入口、持久化、消费、provider 请求和恢复。不先建设全局框架。必要的 enum match、查询 plumbing 与错误传播会跨超过六个文件，这是该闭环的直接调用链与编译约束，不能为行数上限拆成交付半语义。

实现 handoff 以 [implementation-boundaries.md](implementation-boundaries.md)、本 change 的 delta、[tasks](tasks.md) 为准。生产改动限于 inquiry 成功证据、Notification 来源/消费、Timeline 后缀查询、相关 SessionActor 初始化与真实采样边界，以及必要测试/开发者说明。其他上下文变更机制只读。

demo 的实际状态数、转移数、变异反例和局限以运行生成的 results.json 及 demo 审查记录为准；不采用第一版较弱模型的通过数作为验收。


## R10：inquiry 取消与成功接纳只有一个赢家

实际旅程审查发现：provider select 之后，usage、Sideband Result/End 和 canonical terminal 准备仍有 await。原 inquiry token 与 target-close token 独立；取消可以先被来源报告为 Cancelled，回答方随后却继续提交 Answered proof 和收据。不能用一次末尾 flag 读取跨过持久化 await 来解决。

修正规划：复用 `InquiryCancellation.reason` 的既有原子状态，在回答方现有成功 terminal 控制栅栏内、实际 append 之前仲裁取消或 success-sealed；这只是进程内接纳点，durable ACK 才产生 canonical 成功义务。取消先赢则保留真实 Sideband/usage，写 typed cancellation，无 proof；成功先赢则不再取消该 inquiry，等待原终态或真实持久化失败。不因 target close 丢弃已持有活动权的合法 Completed Sideband 收束。

直接父子来源在取消分支尊重该赢家；peer 来源和目标不共享同一原子状态，因此保留已派发 Ask 的原 read exchange。首个 matching Progress 证明目标已注册后，向固定目标请求一次 Cancel，再继续原响应；不丢弃请求、无条件合成本地 Cancelled，也不重采样。已有 `cancel_session` 本地 outgoing bool 仍表示 intent 已接纳，远端 cancel_inquiry bool 才表示目标仲裁，最终 typed outcome 来自原 Ask 或实际传输/期限错误。不新增协议、全局 ACK、tombstone、manager 或第二个账本。

验证先用真实 provider 后的受控 Result/terminal ACK 边界重放取消前赢和成功前赢，再覆盖真实直接父子及 peer 原流，保留 source 工具结果与 target 上下文的一致解释。当前尚未实施 R10；不能把先前 R1–R9 的绿灯当成此问题已解决。


R10 冷来源补充：独立核对并真实反例确认，来源已有 durable OutgoingStarted 但无完成时，目标可能早已接纳/完成。新来源 runtime 的重试不能把派发状态重置为“确定未派发”并本地合成 Cancelled。复用启动前已查到的 canonical fact，给既有 run_outgoing/deliver 传一个局部不确定性初值；不新增 record 字段或持久化派发账本。该 bool 只从旧事实得到，不能把本次新 admission 刚写入的 start 当成历史派发。

- R11: raw answer/notification length cannot prove actual Peer JSON result frame eligibility. Escaped answers can fit the former and exceed the latter. Admit the actual encoded response before success seal/proof and apply the same invariant on cold load.
