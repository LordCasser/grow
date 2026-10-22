# Design

## 统一物理账本

session history 的主账本是单一 `timeline.jsonl`。writer 将 domain `TimelineEvent` 和 typed presentation facts 按同一个连续物理序号编码；domain event 保留自己的 Timeline `seq`，不会被展示记录重编号。每行含 record version 和覆盖原始 fact bytes、version、物理位置的 checksum。读取只接受完整、连续且 checksum 正确的前缀；末尾未完成行可忽略，完整行损坏则 fail closed。

Timeline 仍是运行时状态、canonical transcript 和采样 admission 的领域权威。HistorySnapshot 是从已校验账本和 Timeline 派生的内存引用视图，不是第二份 transcript、索引或持久 projection cache。Shell 的 `GrowHistoryFact` 是闭合 Rust enum，使用 `sessionUpdate` snake_case tag 与 `deny_unknown_fields`；它不移动既有 DTO、不携带任意 JSON payload，也不让 ChatState 依赖 ACP/Grow wire 类型。

## 来源闭合与 admission

history 中不可由领域事实推出的展示内容以最小 typed payload 保存：31 个已确认 durable Grow families 加 `DiffReview`（共 32 个 variants），以及 7 个 durable ACP families。保留 `SubagentProgress`，并保留 plugin install、rewind 与 turn completion 等已确认持久路径。`SamplingAttempt`、其他 transient 和没有 production persistence 路径的 enum variants 不进入 production history。字段级 wire 约束见 `history/grow_facts.rs`，来源对应表见 [source-inventory.md](source-inventory.md)。

已准入模型 response 的正文只在 Timeline admission 中保存一次。candidate anchor 仅含 request/attempt identity，不含 token 或 response 正文；它在首次 candidate 位置先 durable。admission 记录引用此 anchor，或在没有 candidate 时以自身作为位置。History projector 通过 identity 和物理来源位置把 admitted response 展开为 reasoning/assistant notifications。discarded candidate 不可见，quarantined response 不投影。独立 presentation facts 仍按原 owner 排序在 anchor 与 admission 之间。

用户 display echo、独立 ACP tool call/update 等内容保留重建展示所需的 typed payload；echo 保持 display/hide/skill/image 决策及顺序，并与 consumed input identity、数量和 fingerprint 核验。已准入 response 正文只从 canonical admission 派生；receipt 正文通过 Timeline 引用及既有 payload artifact 去重。ToolEvent 生命周期事实与 UI ToolUpdate.rawOutput、模型 ToolResult 各自保留所需来源，不对工具正文是否相同或可去重作一般性假设。fork seed 使用 `HistoryContent::Message` event/item/channel 引用，校验来源且不继承父 admission 或执行资格。

旧 updates replay oracle（`legacy_replay_oracle`）仅在 `test` / `test-support` 中用于语义比较，生产 history 路径没有旧 updates 兼容、cache repair 或第二次 projection commit。storage-only `ResponseReplayProjection` 也只在这两个配置可用；生产 writer 的持久 ACK 由 admission 的单次 Timeline commit 决定。

通信 receipt 的已有内存索引还保存最小来源证明：领域 event、固定展示字段的 digest，以及 agent message 的 payload ref。将 UiNotice 折成正文引用前必须核对提交正文与该证明，不能先丢弃 body 再把不同 payload 当作 exact retry；不新增持久索引或第二份正文。恢复缺 artifact 的明确降级仍只发生在读者。

UiNotice 的 inquiry 恢复按 `(correlation_id, subject)` 在 BTree 前缀范围内查找候选来源，避免全量扫描，并在 source key 缺失时拒绝；相同 source session 的重复 phase 沿用首次绑定，不被后续 peer 身份覆盖。领域合成 receipt ID 可能带有较大的数字尾部，writer counter floor 因此取所有 physical position（包括 hidden/noReplay）及已存最大 ID 的上界，以免 Pager 消费 highwater 后与历史 ID 碰撞。

## Writer owner 与 replay barrier

Cold child reconciliation 同样先完成历史事实修复与 interrupted scope 清理，再从现有 spawn/terminal maps 发布展示和完成回执。不能在清理之前启动新的 notification turn，否则清理会误关新 owner 的 Turn/Hook。此顺序复用现有映射，不增加状态机、全历史重读或独立恢复索引。既有内部 Grow 通知命令区分 restored 展示与 live 生命周期；前者经过相同持久化和投递边界，但不再次运行 SubagentStart Hook。该标志只由内部 producer 设置，不是客户端协议字段。

`SessionPersistence` 是单一物理 writer 和 record 顺序 owner。presentation 写入按普通缓冲 ACK 或 durable ACK 保留原先提交等级；durable 边界不因统一文件而升级。必要 artifact 先按原流程完成 publication，再提交引用 artifact 的 Timeline 事实。append 失败仍区分未提交与已提交但后续同步失败，调用方按原 ACK 语义处理。

resident `session/load` 由 actor 协调 writer barrier：先 flush actor 内的 replay buffer，再请求 persistence drain 已排队事实并建立 barrier。barrier 在 history 读取/交付期间保持，后续 writer append 不会越过该截点。release 时 actor 再读取此刻的 active preview（包括 replay 静默期间开始的 attempt），安装 preview resync，然后才 drop barrier；因此 writer 可继续处理 admission 前，resync 已就绪。RAII guard 在取消时释放 barrier。完整历史加载后才开放普通输入；该屏障不增加后台恢复、分页服务或新订阅框架。

## Cursor 与 projector

projector 遍历物理来源顺序，结合 canonical Timeline admission、receipt 和 fork reference 生成只读通知。cursor 包含 session、projector version、物理 cut、已验证 prefix hash、record、expanded part 和完成标记。仅在 session/cut/hash 匹配且 cursor 指向完整交付边界时才可继续；一个 response 展开多条通知时，中间 part 不能作为跳过依据。外部 session、更新 cut、未知版本、损坏 cursor 或半个 response 均触发完整替换。

candidate preview 的 event ID 与 canonical response 展开身份分离。尾部 candidate cursor 可能带 `isReplay=false`；若 canonical 多 part 复用同一数字 ID，消费者会把后续 parts 当重复项丢弃。canonical 输出使用已验证 cursor 和 Message 来源，不复制 preview event ID；prefixed delta 与三 part 的角色/顺序用例锁定此边界。

Rewind 依照现有 target prompt 分支过滤历史。compaction 不删除物理历史；load 与 fork 根据来源引用重建显示，不用当前 Surface 猜测 transcript。通知 metadata 的 cursor/timestamp 是 projector 输出，不是重复的 durable payload。

## 物理日志的旁路读者

Trajectory 的增量观察缓存也必须通过统一 record decoder 和物理 AppendState，再只向既有 TrajectoryProjector 交付 Domain 事件。缓存的 offset/hash、失败回滚和完整前缀重放一并携带物理序列状态；展示记录不能被当作 Domain seq，也不能被静默跳过 checksum 校验。测试中的 fault injection、Skill、挂起树和 leader 历史检查同样复用 journal codec，不拼写第二份 checksum 规则。

## Oracle 与覆盖记录

变更前格式只作为 test/test-support oracle 输入，不参与生产读写或兼容迁移。history wire golden 共 46 行：38 个 durable Grow 行、7 个 ACP 行、1 个被拒绝持久化的 `SamplingAttempt` 行；重复状态按独立 row 核对。`DiffReview` 另有 typed round-trip 测试，不计入 golden 46。

最终 build、correctness 和配对测量的结果、范围与限制统一见 [verification](verification.md)；不从变更前基线推定收益。测试直接比较消息字段、来源与顺序，性能报告保留 TUI 未改善项和内存不确定性。

## 内存中的来源所有权

同一 projector 接受借用或拥有的物理来源。runtime 需要复用恢复快照时借用；只读 reader 不再使用原快照时消费它，将独立展示 payload 移到输出，避免保留两份全部正文。索引、rewind、cursor 和 metadata 规则仍只有一套。Grow 的较大稀疏 variant 与独立展示 fact 在内存中装箱，连续位置数组只保留紧凑领域序号或展示指针；移动输出时同时释放已消费 payload，不把一个最大 enum 大小常驻在每个 Domain 位置上。serde 编码不变；这不是新的缓存或持久格式。

## 原则与限制

不建立第二份 history ledger、响应正文缓存、digest-only projection expectation 或新的通用 commit service。Checksum 检测单行 accidental corruption，不认证能够同时改写正文与 checksum 的攻击者；跨 artifact/Timeline 写入也不宣称事务原子性。若某来源缺少展示必需内容，应在唯一 owner 持有该内容的位置补最小 typed 事实，并为其来源、顺序和拒绝/失败路径提供验证，不得通过忽略未知行或删除负例掩盖。

既有 scenario 标题保留用于 OpenSpec 场景身份连续性；其中 cache/expectation 是历史命名，修改后的 WHEN/THEN 均以统一事实、物理截点或未交付历史描述现契约，不保留第二份持久 cache。
