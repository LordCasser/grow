# 实施后来源闭合

本表记录 unified Timeline history 已采用的来源和物理表示。主账本为 `timeline.jsonl`：领域 `TimelineEvent` 与不可从领域事实推出的 typed 展示事实按单一连续物理序号写入；生产 history 投影由该账本和已验证 Timeline 派生，不从旧 `updates.jsonl` 兼容读取。domain 自身的 `seq` 保持原语义，presentation 物理位置不占用 domain 序号。

## 持久化来源

| 来源 | 账本表示 | 闭合规则 |
| --- | --- | --- |
| 已准入模型 response | 一条 canonical Timeline admission event，内含原始 canonical items、request/attempt、quarantine 和 admission 身份 | projector 从 admission 生成 reasoning/assistant 历史；不另存 response 正文或 response projection 行。 |
| Preview candidate | `HistoryContent::Candidate(ResponseKey { request_id, attempt })`，没有正文 | typed anchor 保留第一次可见的位置。Accepted admission 通过物理位置引用该 anchor；discard/未准入 candidate 不展示。无 streaming candidate 时，admission 记录自身就是来源位置。 |
| ACP 七类持久通知 | `HistoryContent::{User, Assistant, Reasoning, ToolCall, ToolUpdate, Plan, Mode}` | 保留完整 typed payload、metadata、channel 和物理先后。仅实际持久的七类进入此集合；不将任意 ACP envelope 写入账本。 |
| Grow 事实 | 闭合 `GrowHistoryFact`，wire tag 仍为 `sessionUpdate` snake_case，并拒绝未知字段 | 保留 31 个当前已确认持久化的 Grow families，以及 `DiffReview`，共 32 个 typed variants。逐字段复用现有 Shell neutral payload 类型；`SubagentProgress` 明确持久化。 |
| 用户 display echo | 必要时以 typed user presentation fact 保存所展示的 blocks 和 metadata | 保留原始 display/hide/skill/image 选择与顺序；不能从模型输入或后续 canonical response 猜出 display 内容。与 `InputEvent::Consumed::display_echo` 的 identity、块数和摘要核对。 |
| 独立工具展示 | ACP ToolCall/ToolCallUpdate 的必要 typed facts、Timeline ToolEvent 生命周期事实，以及模型 ToolResult | 各来源按各自展示/领域义务保留；ToolEvent 完成状态不等同于 UI `rawOutput` 或模型 ToolResult。仅对已准入 response 正文与 receipt 正文承诺去重，不假设不同工具来源正文必然相同或可合并。 |
| 通信通知 | `HistoryContent::Notice { event }` 引用 Timeline receipt | notice 文本/载荷经 receipt 的 `payload_ref` 从现有 artifact 读取；不重复存入独立历史行。缺失或无效引用 fail closed。 |
| 通信 UiNotice | typed `GrowHistoryFact::UiNotice` 及其 receipt 引用 | 转成引用前先核对原 body、source 和 display；已提交 receipt 索引持有 compact proof。Inquiry 按 `(correlation_id, subject)` 的 BTree 前缀查询绑定 source，source key miss 拒绝；相同 source session 的重复 phase 保留首次来源。 |
| Fork seed/display | `HistoryContent::Message { event, item, channel, chunk_meta }` 引用 fork 自己 Timeline 中的 seed/普通消息 | 新 fork 拥有自己的引用，不继承父 response admission、执行身份或 child 执行资格；引用的 event/item/channel 必须校验。 |
| 其他非推导展示事实 | `GrowHistoryFact` 的 exact wire payload；`ControlStateUpdate`、`RetryState`、`PromptUsage`、Workflow/Goal 快照等沿用现有 typed payload | 在产生时保存展示所需时点值与身份；不依赖重启时最新 manifest、goal 或状态重建旧历史。 |

31 个已确认持久化的 Grow families 包含 `PluginUpdatesInstalled`、`SubagentProgress`、`RewindMarker` 和 `TurnCompleted`。`DiffReview` 也保留为 typed fact。`SamplingAttempt`、所有 transient 更新、没有 production 持久路径的 variants 均不进入生产 history。具体 enum 字段和 serde 约束以 `crates/codegen/shell/src/session/history/grow_facts.rs` 为准。

## 写入与读取边界

`SessionPersistence` 是账本 writer 和物理顺序 owner。普通可缓冲展示写入与 durable ACK 保持不同边界；candidate anchor 仍先 durable，最终 admission 只提交 canonical Timeline event 和 anchor 关联，不等第二次 projection 磁盘提交。必要 artifact 先完成原有 durable publication，再提交引用它的 Timeline receipt/event。单条账本 record 校验版本、连续物理位置和覆盖原始 fact bytes 的 checksum。

resident history load 持有 writer replay barrier。actor 先冲刷已积累的 replay buffer，再等待 persistence drain 并建立 barrier；barrier 在调用方消费历史期间保持有效。actor 在 release 时采集当前 active preview identity，先安装 preview resync 状态，再释放 writer barrier，使后续 admission ACK 前先完成 resync。取消路径通过 guard 释放 barrier。冷读、resident、fork、transcript 与 history projector 共用同一账本来源。

Cursor 绑定 session、projector version、物理 cut、完整 prefix hash、物理 record、展开 part 和 `complete`。只有同 session、同 cut/hash 且完整的 row proof 才能续读；半个 response、外部 session 或更新 cut 均回退 full replacement。fork/import 等来源引用在读取时按 Timeline 事实验证。

writer 的 event-ID counter floor 覆盖所有已分配的物理位置（包括 hidden/noReplay facts）和已存最大 ID；Domain 合成 receipt ID 的数字尾部也会影响 Pager highwater，避免未来 ID 与历史消息冲突。canonical 多 part response 不继承 candidate preview 的 `eventId`：尾部 cursor 可能带 `isReplay=false`，按同一数字 ID 去重会丢掉后续 part；canonical 展示改用已验证 cursor 和 Message 来源。验证用例包含带 prefix 的 delta 与三 part 角色/顺序断言。

旧 updates replay oracle（`legacy_replay_oracle`）仅在 `test` / `test-support` 编译，用作比较工具；生产 history 读写不提供旧 updates 格式兼容，也不保留旧 cache repair 或 projection commit 路径。storage-only `ResponseReplayProjection` 类型同样仅在 `test` / `test-support` 可用，不能进入生产持久化通道。

## 旁路 reader

| Reader | 物理读取与状态 | 闭合规则 |
| --- | --- | --- |
| Trajectory 增量 cache（`session/trajectory.rs`） | 每条完整 journal record 经 history decoder 和 `AppendState` 校验；Domain 才进入 Timeline/Trajectory projector，presentation 仍占物理位置。 | 校验失败时从已提交 byte offset 重放并恢复 Timeline、projector 与 AppendState；未验证尾部不推进缓存。 |
| Search `TimelineLedgerReader`（`session/storage/mod.rs`，由 `session/storage/search.rs` 使用） | 对捕获的 `snapshot_len` 读取统一 Timeline journal，不消费截点之后追加的记录。 | 保留已捕获文件边界，后续 append 不改变本次 search 的输入快照。 |

旁路测试 reader fixtures 已迁到统一 journal，覆盖 Skill、root-child-grandchild、orphan、tool-duration、leader、plan，以及 export、image、search 路径。final22 Shell 19 scoped groups 为 1331 passed、0 failed、4 ignored；final21 Pager 9 groups 为 755 passed、0 failed、1 ignored；pager-pty-harness 为 96/96。final21 correctness oracle 4 个 profile 全通过；fork replay stream memory gate 为 1.714116x（原 2x 阈值），roundtrip、load peak RSS 与 e2e RSS gate 通过。final21 的 2048-turn dense PTY 保持 4 KiB/chunk 与原多帧/间隔阈值，通过并记录两次可见推进（631.0–750.0 ms，gap 118.9 ms；正式 paired 不采用此编译重叠运行时长）；512-turn 旧失败是在 287 ms 内一次性显示全历史。cancelled-leader PTY通过，T1 在 T2取消前已 admission。Orphan seq40–44 保存了旧清理次序的故障证据；生产现按恢复 child/parent、cleanup、再发 UI/receipts 排序，并通过 final21 CLI #12 的真实 orphan recovery exact-once 路径及 final22 receipt 前独立 Hook 回归。final21 CLI #13 的旧脚本失败保留；最终真实工具/cold ACP 组合已由 final25 通过。最终 build 的 paired 性能复测已完成，见 paired-analysis；root-directory paired 数据只作中间证据。未运行 reader 或未完成矩阵不推定为通过。

## 当前覆盖记录

固定 wire fixture 共 46 行：38 条持久 Grow fixture rows、7 条 ACP rows，以及 1 条必须拒绝持久化的 `SamplingAttempt` row；重复状态和两种 `TurnCompleted` 场景按行保留。Golden 包含 31 个 Grow durable families；`DiffReview` 由独立 typed round-trip 测试覆盖，合计 32 个 typed Grow variants。final22 Shell history group（9/9）逐字段核对 golden fields，并拒绝 `SamplingAttempt`。Pager 的逐 prefix Direct/Projector 对照核查混合物理账本、角色、工具映射与 root/child 顺序；真实 handler 的 prefixed 三 part `isReplay=false` canonical delta 测试也在 Pager scoped run 中。消息类型、字段、来源、角色与顺序错位由这些组合用例覆盖，但不穷举所有 variant pairwise interleaving 或 producer side effect。final21 CLI #01–12、#14 通过；#13 由 final25 的真实工具 + cold ACP 组合通过。Hook exact-once、orphan real-child 路径已有 final21 CLI #12 和 final22 receipt-before-Hook 单测通过；最终 paired 复测已完成，见 paired-analysis。这里记录实现闭合与部分运行证据，不代表所有扩展矩阵均已穷举。

最终 root 通用工具组合补验：`final-25-tool.log` 为 1 passed，验证唯一生命周期、ACP rawInput/rawOutput、模型 ToolResult 的正文与顺序，以及实际 cold ACP load 后标记文件和推理请求计数不变。通用 ToolEvent 是生命周期元数据，正文仅按原各来源义务保存。失败诊断及 fixture addressing/matcher 修正见 scenario-coverage 的最终工具补验；无生产语义更改。
