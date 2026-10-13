# input-admission Specification

## Purpose
定义用户输入从接纳到排队恢复的契约。重点约束 input identity、payload 快照、缺失附件的失效处理和 turn 执行前的上下文发布，避免恢复时重复接纳或误用输入。

## Requirements

### Requirement: Durable input recovery
输入恢复 SHALL 复用已经接纳的 input identity 和 payload 快照。

#### Scenario: 重启恢复排队输入
- **WHEN** 已接纳但未执行的输入被恢复
- **THEN** 继续使用原 input_id，不重复执行 Hooks 或新增接纳事件。

证据：`crates/codegen/shell/src/session/actor/input_admission.rs` — `large_input_recovery_reuses_admission_and_snapshot`。

### Requirement: Missing attachment isolation
输入恢复 SHALL 将附件缺失的输入失效处理，同时保留其他可恢复输入。

#### Scenario: 附件被删除
- **WHEN** 排队图片输入的 artifact 缺失，但另一个文本输入完整
- **THEN** 缺失输入失效，完整输入仍可继续。

证据：`crates/codegen/shell/src/session/actor/input_admission.rs` — `missing_input_attachment_is_invalidated_without_blocking_other_inputs`。

### Requirement: Frozen turn behavior
turn SHALL 使用接纳时捕获的 Behavior，且执行前必须完成稳定上下文前缀的持久化发布。

#### Scenario: 前缀发布失败
- **WHEN** handle_prompt 的 ensure_prefix_ready 返回错误
- **THEN** 返回 bootstrap 边界错误，不执行该 prompt。

证据：`crates/codegen/shell/src/session/actor/turn/admission.rs` — `handle_prompt`。

### Requirement: Failed image asset batches reclaim completed files
When user-image batch persistence fails during decoding or writing, it SHALL attempt handle-relative durable removal of assets successfully created earlier in that call, preserve pre-existing files and return the original failure. Cleanup failures SHALL be reported without replacing the primary error.

#### Scenario: Later image cannot be saved
- **WHEN** a later asset write fails after an earlier asset was published
- **THEN** the earlier asset is removed when cleanup succeeds, unrelated existing assets remain and the original write error is returned.

#### Scenario: Invalid later encoding
- **WHEN** later Base64 decoding fails after an earlier asset was published
- **THEN** the same cleanup applies to the completed assets.

### Requirement: Image asset preparation is retry-safe
Repeated preparation of the same ordered normalized image batch SHALL reuse verified assets rather than allocate unbounded duplicate copies. Reused assets SHALL NOT enter a new batch rollback set. A lost durable-message acknowledgement SHALL NOT alone authorize removal of potentially referenced assets.

#### Scenario: Preparation retried
- **WHEN** an ordered image batch is prepared again with unchanged normalized bytes
- **THEN** its verified asset references remain stable.

#### Scenario: Acknowledgement unavailable
- **WHEN** image-bearing message commit acknowledgement is lost
- **THEN** referenced assets remain available pending authoritative reconciliation.

#### Scenario: Concurrent preparation
- **WHEN** two calls prepare the same ordered normalized batch concurrently
- **THEN** they return the same verified published paths and a losing call cleans only its own staging directory.

#### Scenario: Published bytes conflict
- **WHEN** an existing batch file differs from the expected normalized bytes
- **THEN** preparation fails without overwriting or removing the existing batch.

### Requirement: Inline query images retain normalization outcomes

Prompt admission SHALL 使用现有图片 normalization notice 入口处理从 query 文本提取的图片。每个内嵌图片批次的丢弃、压缩和保留原图 fallback 说明 SHALL 同时进入当前模型可见 prompt context 与对应 Session 的现有图片通知；索引和同原因分组 SHALL 沿用 normalization 结果。有效图片 SHALL 保留原顺序，且 notice 文本 SHALL NOT 包含图片二进制内容或被用作新的用户授权。

#### Scenario: Inline images are dropped

- **WHEN** 同一 query 中多个提取的图片因为相同原因被 normalization 删除
- **THEN** 当前用户消息保留一次包含全部受影响索引的分组说明，UI 收到一份相同原因的 ImageDropped 通知，被删除的图片不进入模型图片列表。

#### Scenario: Inline image is compressed

- **WHEN** 提取的图片经 normalization 压缩后仍有效
- **THEN** 模型消息保留处理后的图片及压缩说明，UI 收到对应 ImageCompressed 通知。

#### Scenario: Re-encoding falls back to the original image

- **WHEN** 提取的图片无法重编码到目标预算而 normalization 保留原图
- **THEN** 原图与 indexed fallback 说明都保留，UI 使用既有 fallback 通知语义，不仅记录日志。

#### Scenario: Healthy and mixed inline images

- **WHEN** query 含正常图片，或同时包含正常与被删除图片
- **THEN** 正常图片按原顺序保留；只有实际发生的处理结果生成说明，无变化的图片不产生压缩或丢弃通知。

### Requirement: Queued input editing hold prevents admission to a turn

A confirmed edit hold on a still-pending user FIFO item SHALL prevent that item from being combined, promoted, or consumed while editing. The hold SHALL preserve the item's FIFO position; arbitration SHALL NOT skip a held head item to run a later user item, notification turn, or Goal continuation. This rule SHALL be independent of the current Behavior.

#### Scenario: Goal turn completes while the queued head is being edited
- **WHEN** an Active Goal turn finishes and the next user FIFO item has a confirmed edit hold
- **THEN** that item remains pending and unconsumed, no later work overtakes it, and the Goal remains Active without starting a continuation through the held item.

#### Scenario: Earlier FIFO work before a held item
- **WHEN** a later queued item is held for editing while an earlier unheld user item is pending
- **THEN** the earlier item may run first, but arbitration stops once the held item reaches the head.

#### Scenario: Hold races with promotion
- **WHEN** an edit-hold request and promotion target the same queued item at the same control boundary
- **THEN** either the hold is confirmed before promotion and blocks it, or promotion wins and the hold is rejected; the client SHALL NOT be told an already-running item is protected.

### Requirement: Completion of queued editing returns to ordinary FIFO arbitration

Saving a queued edit SHALL durably admit its replacement content before releasing the hold, preserving the row's FIFO position and replacing the old input identity exactly once. Discarding the edit SHALL release the hold without changing its admitted content. Removing the item SHALL durably dismiss its pending input before removing it. Each successful completion SHALL wake ordinary idle arbitration, not directly start or steer a turn.

#### Scenario: Save during an active turn
- **WHEN** the user saves an edited held item while another turn is running
- **THEN** the replacement waits in the same FIFO position and is eligible at the next normal send point after the current turn settles; it is not injected into the running turn.

#### Scenario: Save while idle
- **WHEN** the user saves an edited held head item while the session is idle
- **THEN** the next idle arbitration may immediately promote that replacement once, ahead of Goal continuation.

#### Scenario: Save admission fails
- **WHEN** replacement payload or input admission fails
- **THEN** the original queued input and hold remain intact, no replacement is sent, and the client receives a failure rather than a success projection.

#### Scenario: Discard or remove
- **WHEN** the user discards the edit or removes the still-pending item
- **THEN** discard makes the original item eligible at the next normal send point; successful removal makes that item ineligible forever without stopping the foreground turn or Goal.

#### Scenario: Editing client leaves
- **WHEN** the editing client disconnects without completing its edit
- **THEN** its transient hold is released, the original admitted input remains pending, and normal FIFO arbitration resumes without an indefinite Goal stall.

### Requirement: Published user-image assets follow durable Timeline references
The session SHALL reclaim recognized published user-image batches and crash-left staging directories that have no committed physical User message reference. Reclamation SHALL derive roots from a validated committed Timeline under the session writer epoch and SHALL be serialized with live publication and user-message admission. If the committed Timeline cannot be established, reclamation SHALL leave assets unchanged.

#### Scenario: Admission fails before a durable User message
- **WHEN** an image batch was published but its user message was not committed, and reconciliation reads the committed Timeline
- **THEN** the unreferenced batch is removed without touching another referenced batch.

#### Scenario: Acknowledgement is lost after commit
- **WHEN** the user message was committed but its acknowledgement is unavailable
- **THEN** reconciliation retains the referenced batch regardless of the caller's error result.

#### Scenario: Crash leaves a staging directory
- **WHEN** the writer restarts with an unpublished image staging directory
- **THEN** reconciliation removes that recognized staging directory after establishing the committed Timeline.

#### Scenario: Timeline is unreadable
- **WHEN** the committed Timeline cannot be validated
- **THEN** no image asset is deleted.

### Requirement: User submission retries retain one durable input identity

同一 session 中同一用户提交请求的重试 SHALL 复用稳定 prompt identity、唯一关联的 input identity 和不可变请求快照，覆盖普通 prompt、queue、steer 与 followup。相同 identity/intent/payload 重放 SHALL 返回原接纳与消费状态，不重复 Submitted、Hook 或排队；不同请求内容复用同 identity SHALL 在 Hook 或执行前拒绝。新的显式发送使用新的身份，即使文字相同。

#### Scenario: Admission committed but RPC acknowledgment was lost
- **WHEN** 用户输入已持久接纳，但客户端丢失 ACK 并以原请求重试
- **THEN** 返回原 input identity 与当前状态，不新增接纳、不重复触发对应执行，不重新运行接纳 Hook。

#### Scenario: Same identity carries different input
- **WHEN** 同 session/prompt identity 被用于不同正文、图片、intent 或原始执行路由
- **THEN** 返回身份冲突，保留原输入，新的内容不会触发 Hook 或被排队。

#### Scenario: Concurrent clients repeat one request
- **WHEN** 多个请求并发提交同一稳定身份及内容
- **THEN** 它们共享一个接纳结果或在途 owner，不各自生成新的 input_id。

#### Scenario: User intentionally sends the same words again
- **WHEN** 用户明确新建一次发送意图，内容与先前输入相同但身份不同
- **THEN** 按新输入正常接纳，不依据文字相似性合并。

证据与实施入口：`crates/codegen/shell/src/session/input_inbox.rs::InputPayload`、`crates/codegen/shell/src/session/actor/input_admission.rs::run_human_input_admission_hook`、`crates/codegen/chat-state/src/commands.rs::SubmitInputDurably`、`crates/codegen/pager/src/app/root/dispatch/prompt.rs`。

### Requirement: Unconfirmed input admission requires authoritative reconciliation

输入状态 SHALL 区分请求已 Submitted、接纳未决、已允许、已拒绝、明确未接纳和状态未知。通信失败、运行 handle 缺失或只读快照暂未发现输入 SHALL NOT 单独证明未接纳。明确未接纳必须依据原操作明确拒绝，或完整旧实例退出后在合法 writer 边界验证无接纳事实及无在途操作。恢复 SHALL 保留既有接纳 Hook 的执行证据，不自动重跑结果不明的 Hook。

#### Scenario: Status read overtakes a queued submission
- **WHEN** 状态快照尚未包含一个仍可能进入接纳流程的原请求
- **THEN** 返回未确认，而非授权客户端以新身份自动重发。

#### Scenario: Process exits during admission hook
- **WHEN** Submitted 已持久化，但进程在接纳 Hook 决定确认前退出
- **THEN** 恢复保留原输入与 Hook 证据，并按接纳中断规则处理，不猜测 Hook 未执行并自动重跑。

#### Scenario: Replacement writer proves no admission occurred
- **WHEN** 原实例及在途操作已结束，新 writer 校验原身份没有提交事实
- **THEN** 可明确报告未接纳；后续原请求重试仍使用原身份及内容。

#### Scenario: Input evidence cannot be validated
- **WHEN** 请求关联重复、冲突或 payload/附件不可用
- **THEN** 报告证据不足或输入失效，保持其他可恢复输入，不按文本猜测接纳状态。

证据与实施入口：`crates/codegen/shell/src/session/actor/input_admission.rs`、`crates/codegen/shell/src/session/input_inbox.rs`、`crates/codegen/chat-state/src/timeline.rs::InputEvent/InputBlockReason`。
