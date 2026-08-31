# Input routing and foreground ownership

Grow routes input by intent. Behavior changes model context and scheduling policy; it does not own admission.

## State ownership

The shell session actor owns both the only foreground slot and the explicit FIFO:

```text
ForegroundState = Idle | RegularTurn(AgentTask) | Compaction
```

Goal's future continuation right, watchers, and background tasks do not occupy this slot. Every regular turn has a structured origin/kind and exactly one durable `TurnCompleted`. A stale completion whose prompt id does not match the foreground owner cannot clear a newer turn.

The pager mirrors shell state. It does not infer ownership from Goal status, prompt text, token count, or prompt-id prefixes.

Pager crash recovery owns only input that has not crossed an ACP prompt RPC. Its versioned local draft store is keyed by bound session identity, or by canonical cwd before binding, and persists the normal composer, cursor/chip metadata, deferred Behavior latch, and at most one plain staged prompt. A restored staged prompt returns to the composer and is never enqueued or auto-sent. Modal inputs, expanded commands, Shell state, and temporary image paths never enter this store; image-bearing or ambiguous multi-prompt state fails closed. Starting either text or structured-block prompt RPC atomically disarms the in-memory draft and removes both the current cwd/session key and the request's session key. This local cache is not Timeline and never becomes a second input authority.

Behavior capability follows the same ownership rule. Once the Shell publishes
`grow/behaviorAvailability`, Pager settings, pickers, and transition guards all
consume that projection. The advertised command catalog is an execution and
discoverability surface, not a second Behavior capability model. Tool/command
inspection is permitted only during the bootstrap window before the first
structured projection arrives.

## Input classes

所有真实 `HumanIntent` 共用一个 durable admission 入口；Pager 的 UI 动作只决定 intent，不直接决定执行：

```text
immutable artifacts/inputs/<blake3>.json
  → InputSubmitted(input_id, intent, payload_ref)
  → UserPromptSubmit Hook lifecycle
  → InputAdmissionResolved {
      Block
      | Allow + initial route (Fifo | Steer{TurnId}) + superseded input ids
    }
  → Fifo: TurnStarted reserves input ids
  → InputConsumed{item} | InputHandled | InputRerouted | InputDismissed
```

payload 必须先以 content-addressed immutable JSON 发布，Timeline 再提交 `Submitted`。Hook 完整闭合且 durable `Allow` 之前，输入不能出现在队列、Surface、interjection buffer、host command 执行或 provider request 中。同一 `input_id` 只运行一次 Hook；排队、Send now、double Enter、自动提升、turn 终止后的回退和进程恢复都只能改变 route，不能重复 admission。Hook block 只终止该输入，不冻结已有 FIFO 或 active turn。Goal、Workflow、Notification 等 synthetic prompt 没有 `Input` lifecycle，只产生 source-bound、observe-only Hook occurrence。

1. Plain Enter 产生 Prompt intent。idle 时进入 FIFO admission；busy 时仍先进入同一 FIFO。`follow_up_behavior = "steer"` 只允许把已获准的 plain prompt 从 FIFO 重路由到当前 exact `TurnId`。
2. Ctrl+Enter 产生 Steer intent。Hook 完成后在 `step_control_gate` 内复核 exact foreground；目标已经关闭时提交 typed `StaleSteerTarget` block 并向客户端返回失败，不能把显式指定旧 Turn 的内容擅自交给 FIFO 或新 Turn。已经 `Allow + Steer{TurnId}` 的输入若错过最终 safe point，则在该 Turn 的 terminal fence 后以同一 `input_id` 回到 FIFO。
3. Double Enter 和 queue-row “Send now”都在 `step_control_gate` 内执行 `Fifo → Steer{exact TurnId}`；目标、row 或版本在取得栅栏前已变化时保持原 FIFO 不动，持久路由提交后内存 row 必须在同一栅栏内转移，不存在向活跃 Turn 做 `Steer → Fifo` 的非法回滚。
4. leading slash / bash 仍先以普通 Prompt intent 完成 Hook admission 并进入 FIFO。`TurnStarted` 取得预留后，若命令在 host plane 内闭合而没有生成模型消息，则以 `InputHandled` 终止；命令产生的内部 prompt 不再冒充第二个 HumanIntent。
5. 仅内部 synthetic/test interjection 可以没有 input identity；生产用户 steer 一律携带可回退 payload。turn 的最后安全点未消费的 residual steer 在目标 turn 结束后 durable `Steer → Fifo`，再以原始 input identity 回到队首，绝不能泄漏进 successor 的 interjection buffer。

A successful Goal control that invalidates the running context (set/edit/enter/pause/clear) ends that exact foreground turn through normal cancellation. Read-only or non-invalidating controls (status/restart/budget), and rejected mutations, leave it running.

Steering 的客户端 `expected_turn_id` 先定位 foreground，Shell 再把当前 Timeline `TurnId` 写入 durable route。消费时必须同时复核该 `TurnId` 仍是 foreground；prompt id、队列位置或文本相等都不能代替这一身份。它只向同一 turn 的 safe-point buffer 追加输入，不创建 replacement turn 或另一个 terminal。Compaction 和 idle state 不可 steer。

Queue combine 不制造新的输入事实：组合后的 row 保存全部 `input_ids`，且永不超过 Timeline 的单 Turn identity 上限。user `TurnStarted` 在一个 transition 中校验并预留这些仍为 Fifo/Allowed 的 identities；只有 canonical user `ConversationItem` 同时进入 Timeline 时才以一个 `InputConsumed` 原子消费全部 identity。若进程在两者之间中断，recovery 先闭合旧 Turn 并释放预留，再重建 FIFO，不会丢输入或复制 Surface。queue edit 是新内容，因此必须创建新 payload、新 `input_id` 和新 Hook admission；`AdmissionResolved` 在同一个事件中让新版本进入 FIFO 并 dismiss 旧 identities，失败则旧 row 和旧 admission 原样保留。remove、session close 和孤儿清理都以 typed `Dismissed` 终止已 Allow 但未消费的输入。

## Idle admission

All regular work shares one admission sequence:

1. settle the exact foreground owner and persist its single terminal;
2. promote the oldest user FIFO entry;
3. drain pending durable notification receipts that are allowed to start a turn;
4. only if foreground, FIFO, and notification work are all settled, run Goal continuation.

The idle arbiter drains the durable inbox before it invokes the Goal driver; the Goal driver then rechecks foreground and FIFO under the same state lock before reserving its continuation turn. This closes the race where a continuation and user input or a completed background task arrive together: user input wins, then the durable receipt, and Goal remains last.

The notification inbox is the only cross-turn completion path. During an active turn, a receipt may be consumed only by a turn whose structured origin owns it; Goal-owned evidence is therefore reserved for the matching Goal continuation rather than leaking into an ordinary user turn. A tool-result boundary consumes the same inbox with `input=None`, so the result is not mistaken for a user message. During idle, a Session-owned autostart receipt is consumed into a notification turn. A receipt owned by the current Active Goal remains pending for that Goal continuation; an old, paused, blocked, budget-limited, complete, or cleared Goal receipt is durably `Dismissed(reason=goal_owned_autostart)` and never downgraded into a Normal turn. Owner is authoritative for every notification source, including subagent and Workflow completion. `TaskStillRunning` by itself remains pending and does not spend a model call. Monitor progress is folded away when its terminal receipt arrives. Every receipt remains idempotent and replayable through Timeline; no memory buffer, streaming line, hidden reminder queue, or background-task manifest participates in admission.

## Message identity

Each user submission has a stable `messageId` carried by the queue row, optimistic bubble, running notification, ACP user-message echo, and Shell `input_id` lifecycle. Pager reconciliation is keyed only by this identity:

- an optimistic bubble followed by an echo stays one bubble;
- replayed or duplicate echoes are idempotent;
- the echo may backfill server fields such as `promptIndex`;
- unrelated messages with identical or trim-equivalent text remain distinct.

No `skip_next_user_echo`, text matching, or adoption stash participates in routing.

启动恢复先关闭中断的旧 Turn 以释放未消费预留，再从 Timeline 查询 pending Allowed inputs，并按 payload reference 校验、读取和重建 FIFO；旧 Turn 已结束的 Steer 会先持久化重路由为 Fifo。缺失、超限、哈希不符或类型不匹配的 payload 会拒绝会话加载，不能把坏输入静默丢掉。未被任何 `Submitted` 引用的 input artifact 只由有界 orphan sweep 删除。恢复不重跑已经闭合的 Hook，也不把 Allow 之前崩溃的输入自动升级为允许；Timeline recovery 会给它明确的 interrupted block 终态。

## Goal interaction

An Active Goal selects the visible Goal Behavior but does not own the foreground between turns. A stopped Goal is durable thread state and may coexist with Normal, Clarify, Plan, or Workflow.

- Active Goal keeps the Goal chip active while the session may be idle or run a user turn;
- ordinary messages add context without replacing the objective;
- `/goal edit` revises the same long-lived Goal while preserving usage; Paused/Blocked remain stopped until `/goal restart`, while BudgetLimited/Complete edits create an active revised definition;
- outside Goal Behavior, `/goal set` switches to Goal and creates the objective; inside Goal it is hidden and rejected;
- after selecting Goal with no objective, the next ordinary message is captured directly as the objective without a Pager-generated hidden command;
- pause or block stops autonomous admission and releases Goal Behavior to Normal;
- budget exhaustion also releases to Normal; update or remove the budget, then restart;
- restart re-arms Paused or Blocked Goals and atomically selects Goal Behavior;
- complete releases to Normal; clear deletes only Goal state and preserves another already-selected Behavior.

Goal continuation is an internal regular turn started by the idle hook, not a queue item or hidden control prompt. See [goal-continuation.md](./goal-continuation.md).

Goal does not persist a plan or task graph. Each continuation audits the full objective, then uses ordinary `todo_write` and `task` execution context for the next small slice. Goal detail renders only the durable objective, lifecycle status, usage, elapsed time, and status message; Pager never persists its display cache or navigation state.

Turn failure ownership follows the same structured-origin rule. A provider or
tool-definition error in a user turn remains that user's terminal and cannot
pause an otherwise healthy Goal. Only a structured `GoalContinuation` failure
enters the Goal degradation path.

## Compaction

Compaction is the only non-regular foreground owner. Manual and automatic compaction cannot overlap a regular turn or each other. While it owns foreground, user input may queue but cannot steer it. When compaction ends, the same FIFO-first idle arbiter resumes scheduling.

## Recovery

All provider and completion-requirement retry progress uses the single
`RetryState` session update. Compaction resumes inside the same foreground turn
and has no separate auto-continue terminal update; `TurnCompleted` remains the
only regular-turn terminal authority.

`TurnCompleted` is the durable lifecycle authority. Prompt response metadata is an idempotent secondary source. The pager watchdog may query shell prompt status when a submission appears stalled, but elapsed time alone never fabricates a terminal.

If a durable `TurnStarted` or `TurnEnded` append permanently fails, the Timeline writer epoch and the shell session terminate after local turn cleanup. The actor never treats that boundary failure as an ordinary model error and never admits a successor turn into a poisoned causal fold.

Cancellation settles the same foreground owner and then follows FIFO-first
idle admission. If no queued user/manual work claims the slot, an Active Goal
is woken immediately; Stop Turn Only cannot leave it dormant until the next
unrelated user message.

Image-input capability recovery is session state, persisted with the existing
tool resources in `resources_state.json`. The negative cache key is the model
name, API backend, and endpoint fingerprint; absence means unknown and permits
an image attempt. A cache entry is written only when an image-bearing request
receives an API HTTP 400 that explicitly rejects `image_url`, `input_image`, or
another image content type in favor of text. Decode errors, size/dimension
limits, content-policy failures, and generic 400s remain terminal errors and do
not teach model capability.

After an explicit rejection, the shell groups all canonical `User` and
`ToolResult` images by message. Within one bounded recovery operation, a
configured auxiliary runtime that is distinct from the rejected runtime gets
one description request per group, with attachment count and order in its
prompt. Each successful description is completed in an `image-description`
Sideband; failed groups, or every group when no auxiliary runtime is usable,
abort the projection without changing the model-facing Surface. Only the
auxiliary runtime's own explicit image HTTP 400 enters its negative cache;
resolution, transport, timeout, and empty-response failures do not teach
capability.

The chat-state actor atomically records one log-only `ImageProjection` fact.
The negative cache and trigger provenance use one `ModelImageInputKey`; no
consumer computes a parallel route identity. The fact binds the triggering
model/backend/endpoint route, source Surface revision, stable Surface IDs,
image-group fingerprints, paired assistant tool-call identities, the exact
ordered `Reasoning`/`BackendToolCall` carriers from the same response, and
Sideband result provenance. For an image-bearing tool result, the projection
consumes the result content, every paired assistant argument set, and every
provider-native carrier by replacing them with protocol-neutral text; source
paths are never sent to the description Sideband and cannot survive in the
later model-facing Surface. It never rewrites the immutable source message.
Request assembly applies every live `ImageShadow`
regardless of the later model route and then enforces the independent 50 MB
transport budget on that request copy. A known text-only route that still
produces an image-bearing request retries projection from a fresh Surface at
most twice and then fails; it never silently strips an unaccounted request.

The first accepted shadow is a causal one-way boundary: model switching,
resume, recall, fork and rewind cannot expose those image bytes to a model
again. Selected-branch and rewind materializations apply the same shadows
before creating new Surface identities. The original bytes remain available
only as immutable Timeline evidence for audit and the already-recorded
image-description Sideband. Ordinary appends preserve old shadows and project
only newly appended image groups. The resubmission does not consume ordinary
retry budget or emit a failed turn. A shadow is accepted only when every image
group has a completed, source-bound ImageDescription Sideband result; missing,
failed, or image-incompatible auxiliary routes fail the turn without changing
the Surface. `ImageProjected` therefore reports described groups only;
`ImageDropped` remains reserved for images actually discarded during input
normalization. No OCR backend participates in this recovery path.

An Active Goal reload restores the v9 objective, definition revision, lifecycle status, budget, settled usage, and elapsed time from the same Timeline Control snapshot as Behavior, then re-arms idle continuation. Goal owns no persisted plan, board, planner phase, or stage lease. Older Goal architectures and invalid v9 snapshots are rejected without migration instead of reviving a second lifecycle model.
