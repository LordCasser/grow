## MODIFIED Requirements

### Requirement: Attempt evidence and accepted response have distinct authority

废弃 attempt 的证据、用量及恢复决定 SHALL 保留，但不授予模型上下文、native continuation 或工具执行权。每个 canonical response admission SHALL 在同一 Timeline 事实中持久化 request/attempt identity、canonical payload、确定性 admission result 及闭合的展示来源/anchor 引用；必要 artifact 与 anchor 先完成原 durable publication，该 admission 的 file/directory barrier 确认后才允许 Accepted 和依赖动作。Replay projection SHALL 从已验证事实派生并共享正文，不再单独持久提交。相同身份相同 payload SHALL 返回原结果，不同 payload SHALL fail closed；无法准备必要展示依据时不得提交 admission。

#### Scenario: Failed generation is followed by a valid attempt
- **WHEN** 较早 attempt 被拒收，较后 attempt 获得 durable admission
- **THEN** 两者证据可追溯，只有后者进入 accepted history、Surface 或可执行工具。

#### Scenario: Admission write acknowledgment is lost
- **WHEN** Timeline 记录可能已写入但 durable ACK 不明
- **THEN** 只核对原 identity 和 payload 并完成原同步；确认前不发布 Accepted、不继续工具/provider/成功 terminal，不重新采样。

#### Scenario: Exact admission submission is repeated
- **WHEN** 同一身份及 canonical payload 再次提交
- **THEN** 返回同一 admission，不重复事实或展示，不重复安装或从历史恢复 native continuation。

#### Scenario: Admission identity is reused with a different payload
- **WHEN** 已占用身份收到不同 payload
- **THEN** 拒绝冲突，已有 Timeline、Surface 与历史不变，不执行新 payload。

#### Scenario: Replay projection cannot be confirmed
- **WHEN** response 的 anchor、正文或必要展示引用不能验证或尚未持久确认
- **THEN** admission 失败或保持未确认，不写入一个需另补 cache 才成立的成功事实。

#### Scenario: Delivery fails after admission
- **WHEN** admission 已确认，但通知编码或客户端交付失败
- **THEN** 保留唯一 canonical admission，报告交付问题；恢复只派生同一历史，不改写 admission、不重采样或重发工具。

#### Scenario: Projection is reconciled during recovery
- **WHEN** cold/replacement load 发现已确认 response 尚未交付展示
- **THEN** 从同一 Timeline 来源派生历史，不写第二份投影、不恢复 native continuation、不执行 provider/工具，不复活被排除内容。

#### Scenario: Process stops before attempt closure
- **WHEN** 进程在采样证据、结算或 admission 闭合前停止
- **THEN** 保留未确认状态和原身份，不把 candidate 当作成功；已确认 admission 的历史可直接重建，不依赖第二份 projection 提交。

#### Scenario: Large response projection and independent replay
- **WHEN** 大正文被冷恢复、resident delta 或 fork 展示
- **THEN** 内存投影共享 canonical 正文，最终 ACP 输出保持顺序，fork 不继承父 admission/candidate 权威。

### Requirement: Exact replay commits acknowledge durable barriers

响应与独立持久展示事实的 exact commit SHALL 由其唯一 Timeline 记录及必要引用承担；所需 file/directory sync 完成后才返回成功。读取到相同 key/payload 不得代替同步确认，也不得通过派生 UI 数据建立第二个 durable gate。

#### Scenario: Complete record remains readable after sync failure
- **WHEN** 完整记录可读但 file 或 directory sync 失败
- **THEN** exact reconcile 仍失败，依赖该确认的 Accepted、provider 或工具不放行。

#### Scenario: Exact retry after storage recovers
- **WHEN** 同一身份/payload 在同步恢复后重试
- **THEN** 同步原记录后成功，不重复追加；不同 payload 仍拒绝。

### Requirement: Cold writer identity recovery is independent of client replay

Cold load SHALL 在发布 writer 或新持久事实前恢复全部 Timeline 展示/领域身份占用与序列下界；noReplay、cursor、rewind 可见性不得降低该下界。派生通知 SHALL 使用稳定来源身份，不依赖已删除的物理 updates 记录；相同身份不同 payload 仍拒绝。

#### Scenario: Headless loads without replay
- **WHEN** cold noReplay load 不向客户端发送历史
- **THEN** 仍恢复全部已占用身份，新持久事实不与历史冲突。

#### Scenario: Historical row is hidden from replay
- **WHEN** rewind 或 cursor 隐藏历史事实
- **THEN** 其合法身份仍参与 writer 身份恢复。

#### Scenario: Display expectation precedes cache publication
- **WHEN** Timeline 已提交事实但进程在通知交付前停止
- **THEN** 该身份仍被占用，恢复派生同一展示而不创建另一个逻辑事实。

### Requirement: Candidate preview uses a durable payload-free anchor

Transient session persistence SHALL retain neither candidate notification payloads nor an unbounded window of independent notifications. The first candidate SHALL create one durable, payload-free Timeline ordering anchor before canonical response admission; the response SHALL reference that anchor. Independent durable presentation SHALL be backed by typed Timeline facts or references to their existing canonical source, not a separate updates ledger. During an active attempt, independent untagged ACP and one-shot Grow notifications emitted by the session actor SHALL be appended in arrival order with acknowledgement before another notification from that actor is delivered; previously buffered notifications SHALL be flushed first. A failed anchor or independent append SHALL prevent canonical admission. Live preview notifications from the session actor, including Grow notifications emitted during the attempt, SHALL have a per-session byte budget through gateway completion; exhaustion or an oversized single item SHALL fail the attempt's preview boundary before canonical admission. Replay SHALL suppress an anchor for a discarded attempt and SHALL insert admitted canonical content at that anchor before later independent updates. Live delivery MAY carry candidate notifications independently of persistence.

#### Scenario: Long interleaved attempt

- **WHEN** an attempt emits many candidate chunks interleaved with independent untagged ACP or one-shot Grow notifications
- **THEN** only one payload-free candidate anchor is persisted, earlier buffered updates are flushed first, independent facts stream to Timeline storage with acknowledgement rather than accumulating in the sender, and admitted replay places canonical content at the first candidate position.

#### Scenario: Preview persistence fails

- **WHEN** an anchor or independent append cannot be confirmed before response admission
- **THEN** the response is not admitted to the canonical Timeline, no candidate body enters replay, and the turn reports a projection-boundary failure.

#### Scenario: Discarded or interrupted attempt

- **WHEN** an attempt is discarded or stops without canonical admission
- **THEN** its anchor is suppressed in replay, while independently committed untagged notifications remain in their arrival order.

#### Scenario: Slow live gateway exhausts preview credits

- **WHEN** client completion stalls until the per-session preview byte budget is full, or one preview notification exceeds that budget
- **THEN** no additional candidate or Grow preview payload is enqueued to the gateway, the attempt records a preview-boundary failure, and canonical admission is refused without persisting candidate body text.

#### Scenario: Buffered update precedes an independent Grow notification

- **WHEN** a persistence writer holds an earlier merged ACP update while an active-attempt Grow notification arrives
- **THEN** it writes the earlier update first, confirms the Grow append before the actor can deliver another independent notification, and rejects canonical admission if either write fails.

## ADDED Requirements

### Requirement: Timeline owns reconstructable durable history

Timeline SHALL 保存完整重建历史所需的 typed 事实或不可变来源引用、稳定展示身份与顺序依据；同一 canonical 正文不得为 replay 再持久化一份。现有独立 ACP/Grow 持久展示义务 SHALL 由其 Timeline 事实承担，保留 preview budget、脱敏、原 owner 与准入失败边界；瞬时状态不因本次改造获得永久或执行权威。客户端原始更新不得未经领域校验变成通用 payload 事件。

#### Scenario: Display metadata alone cannot reconstruct a row
- **WHEN** Workflow/Control 或用户 echo 只有 identity/revision/digest，缺少重建所需来源
- **THEN** 新格式写入前必须保留对应版本的事实引用或最小 typed 内容；不可只写悬空 expectation 再依赖 updates 正文。

#### Scenario: Anchor and independent event interleave
- **WHEN** anchor A、独立事实 B、引用 A 的 admission C 依次提交
- **THEN** 展示为 A 位置的 response 后跟 B；C 不把 response 改排到 B 后，discarded A 不产生 accepted 正文。

#### Scenario: Artifact publication succeeds before fact failure
- **WHEN** artifact 已写入但引用事实未确认
- **THEN** 不宣称 admission 成功；允许无引用对象沿原策略回收，不允许已确认事实引用未持久正文。

### Requirement: Stored Timeline facts validate their encoded integrity

新格式 SHALL 为明确版本的事实编码保存 checksum，覆盖 seq、类型、时间和 payload；恢复对原字节校验并执行全部领域验证，不为 checksum 重新序列化整个 response。已有必要 artifact 验证 SHALL 保留；checksum 不替代 exact identity/payload 判定或执行所有权。

#### Scenario: Payload remains valid JSON after modification
- **WHEN** 持久正文被修改，仍可解析但与保存的 checksum 不符
- **THEN** 恢复拒绝该完整记录，不发布部分 Timeline 或修改原文件。

#### Scenario: Incomplete final record or incompatible schema
- **WHEN** 文件含未完成尾片或旧 schema
- **THEN** 尾片沿既有完整行规则处理；不支持的 schema 明确拒绝，不自动重写旧会话或绕过验证。

#### Scenario: An incremental observer reads interleaved physical facts
- **WHEN** Trajectory 或搜索观察读者读取含领域与展示事实的统一物理前缀
- **THEN** 校验每条物理记录与连续位置，只按原 Domain seq 投影领域事件；读取预算覆盖实际物理记录，捕获后追加不可混入，刷新失败不推进已缓存前缀。

### Requirement: Cold child reconciliation settles historical scopes before delivery

Cold reconciliation SHALL 先核对并补齐 child result 与 parent terminal，关闭旧进程遗留的本地 scope，随后才发布可触发当前 actor 工作的恢复展示与完成回执。恢复清理不得关闭本次 owner 新启动的通知回合或 Hook；补发历史 child 展示不得重新执行 SubagentStart Hook。已有 identity、结果引用、独占 lease 与持久确认要求保持。

#### Scenario: Recovered completion starts a notification turn
- **WHEN** orphan child 的结果与 parent terminal 已恢复，完成回执可启动新的通知回合
- **THEN** 旧 scope 的 interrupted 清理先完成，新通知回合不被该清理关闭，session/load 保持有效 owner，parent terminal 仍引用同一 canonical child result。

#### Scenario: Spawn hook completed before its display was committed
- **WHEN** parent Spawn 与对应 SubagentStart Hook 已完成，但进程在持久展示前退出
- **THEN** 恢复补发缺失展示并保留原 Hook occurrence，不再次执行或追加该 Hook。
