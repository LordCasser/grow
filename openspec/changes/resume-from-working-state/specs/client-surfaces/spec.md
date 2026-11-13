## MODIFIED Requirements

### Requirement: Accepted response history is rebuilt from Timeline authority

所有 production replay 入口 SHALL 从已校验的 Timeline 事实和选定 branch 派生确定性的 response history；canonical 正文、identity、quarantine 与 anchor 以该事实为准，不保存或对账第二份 response projection。建立 cursor cutoff 或释放 buffered live 前 SHALL 确认同一截点内的历史覆盖与顺序。运行现场就绪后允许普通输入。历史按页读取，较旧页面 SHALL NOT 改变实时交付游标，每页仍按事实顺序投影。

#### Scenario: Process stops after Timeline admission
- **WHEN** durable admission 后、public Accepted 交付前退出
- **THEN** cold load 派生恰好一份历史，不调用 provider、继续 truncation/pause-turn 或执行工具。

#### Scenario: Projection append acknowledgment is lost
- **WHEN** 某项历史展示的唯一 Timeline 来源可能已提交但 durable ACK 丢失
- **THEN** 按该来源的原 identity/payload exact 核对并同步，相同内容不重复、不同内容拒绝；不存在另一个 projection 文件的成功条件。

#### Scenario: Earlier attempt was discarded
- **WHEN** 较早 attempt 只有 transient candidate，较后 attempt 已接纳
- **THEN** 只展示 exact request/attempt 的已接纳 response，不通过文本或邻近记录认领候选。

#### Scenario: Candidate output precedes durable projection
- **WHEN** candidate 已向可撤回 live client 展示，Timeline durable ACK 尚未确认
- **THEN** candidate 仍可撤回，不提前发布 Accepted、后继 provider/工具或成功 terminal。

#### Scenario: Quarantined response is replayed
- **WHEN** 确定性 quarantine 非零，或早期 attempt 没有 admission
- **THEN** 不将其 candidate 变为 accepted/executable history；按 exact request/attempt 选择有效 response，保留原 diagnostic 语义。

#### Scenario: Tool-bearing response has later tool history
- **WHEN** response 后存在工具调用、结果及独立事件
- **THEN** canonical response 在其 anchor 和依赖边界恢复，工具历史只显示一次且不重新派发。

#### Scenario: Rewind or legacy response
- **WHEN** 当前分支经过 rewind 或 Surface 已 compaction，或读取无可验证 admission 归属的来源
- **THEN** 不复活被 rewind 的正文，也不从已压缩 Surface 反推 transcript；所选完整历史与原来源顺序一致，不猜测缺失 admission 归属，旧 schema 明确拒绝。

#### Scenario: Direct replay without writer authority
- **WHEN** child view、export 或 read-only reader 读取会话
- **THEN** 使用同一纯 projector，不取 writer lease、不写磁盘、不触发执行。

#### Scenario: Cursor intersects one projection record
- **WHEN** cursor 无法证明某个事实展开的全部通知已应用，或 lineage/projector 版本不匹配
- **THEN** 恢复有效的展示边界，完整投影该 response 或按当前分支重开窗口。已应用的实时事实保持原有顺序。

### Requirement: Historical Markdown defers only derived presentation work

Pager SHALL complete every delivered historical assistant/thinking source, including pending normalizer bytes and existing terminal lifecycle/timing, before presenting the completed page as ready. It MAY defer rebuildable final Markdown presentation for offscreen content. Raw and pretty copy, hyperlinks, source maps, diagrams and visible layout SHALL return complete results equivalent to eager finalization before those results are used. Image-reference filesystem observation SHALL remain at the existing message completion boundary. Offscreen estimates SHALL NOT depend on cache warmth; exact viewport layout and source/entry anchors SHALL remain consistent after materialization, resize and raw/pretty changes.

#### Scenario: Offscreen source completes before first drawing
- **WHEN** historical source ends with buffered normalization bytes and its message remains offscreen
- **THEN** source/timing/order are complete within its page, while first pretty copy or drawing materializes the complete final presentation without missing text or stale maps.

#### Scenario: Media and diagram observations survive deferred presentation
- **WHEN** a historical message references an image or contains a Mermaid fence, and image files later change before its first drawing
- **THEN** message-completion image reference discovery/validation retains its existing observation, and Mermaid source/affordance rows match eager output; deferred pure presentation does not postpone filesystem observation.

#### Scenario: Materialization preserves display and source anchors
- **WHEN** a user scrolls to a formerly offscreen historical block, copies it, changes width/raw mode or later receives another chunk
- **THEN** rendered content, links, source maps and logical selection/entry anchors stay equivalent to eager processing; estimates do not change merely because a cache was warmed, and normal input remains governed by runtime readiness.

### Requirement: Descendant restoration reuses only boundary-bound validated facts
Session load MAY reuse root child-ownership facts already validated for the same session and history frontier, and MAY derive descendant ownership and ordered lifecycle/control presentation from the same validated read. Reuse SHALL NOT infer authority from UI rows, cross session/load boundaries or bypass storage identity, validation of referenced records, immediate parent/child canonical Seed/Result link checks, and child ownership checks. Lifecycle presentation SHALL be applied only when its parent session, subagent identity and child session match the canonical owned spawn for that source. Missing reuse data SHALL NOT establish an empty verified tree. Restoration SHALL preserve source-local replay identities, child identity/CWD/worktree/control semantics, history order and independent runtime readiness. Repeating forced restoration SHALL NOT add duplicate task rows for valid lifecycle facts that lack eventId. Restoration remains a presentation projection and SHALL NOT grant child execution or resume authority.

#### Scenario: Root history is reused at its captured frontier
- **WHEN** a successful load supplies validated ownership facts for its exact root session and emitted history frontier
- **THEN** descendant restoration uses those facts without rereading the root, and later appends cannot enter that earlier boundary or be confused with an empty result

#### Scenario: Descendant ownership and control share one read
- **WHEN** recovery visits a valid child parent containing nested lifecycle and model/Agent control facts
- **THEN** one complete validated read supplies ownership/enrichment and the ordered source-local projection, preserving identity and preventing duplicate live/replay rows

#### Scenario: Lifecycle presentation disagrees with canonical child ownership
- **WHEN** a descendant replay record names a different parent, subagent identity or child session than the validated canonical spawn
- **THEN** Pager does not project that lifecycle row or traverse the mismatched child, while unrelated valid child presentation remains available

#### Scenario: Forced restoration repeats an ID-less lifecycle record
- **WHEN** Pager restores the same canonical child lifecycle more than once and the replayed Spawned record has no eventId
- **THEN** the existing Started row is reused and no duplicate task row is added

#### Scenario: Reuse is absent, stale or foreign
- **WHEN** ownership data belongs to another session/load boundary, is malformed, or a required child read/link is missing or conflicting
- **THEN** recovery does not accept that data as ownership authority or a verified empty tree; it validates through the existing fallback or rejects the affected recovery without fabricating success or releasing input before required execution dependencies and checks finish

#### Scenario: Optional ownership exceeds the response budget
- **WHEN** the root ownership projection would exceed the bounded optional response payload
- **THEN** the server omits the whole optimization and restoration uses the independently validated fallback, without truncating ownership into an incomplete or empty verified tree

### Requirement: Loading history preserves session navigation

While runtime load is incomplete, Pager SHALL keep registered Dashboard navigation and previous/next session actions available, including attached frame navigation clicks. Navigation SHALL retain the loading owner without admitting editing, prompts, or session control commands before runtime readiness. Completion SHALL update its own page; failure of a temporary load page SHALL retire that owner through draft checkpointing and apply its original return surface only if still foreground.

#### Scenario: Navigate while a session loads
- **WHEN** a loading session receives registered Dashboard/previous/next navigation or an attached frame navigation click
- **THEN** navigation proceeds while replay and its draft remain owned by the original page.

#### Scenario: Background load fails
- **WHEN** the user has navigated elsewhere before a temporary load page fails
- **THEN** the failed page retires without moving the current focus or discarding its retained draft.

#### Scenario: Ordinary input during replay
- **WHEN** text, paste, prompt submission, or Ctrl+X session commands arrive before runtime readiness
- **THEN** no input is admitted and the draft is unchanged. Older history pages arriving after runtime readiness do not suspend ordinary input.

### Requirement: Interactive export waits for history replay completion
交互式完整会话导出 SHALL 输出完整验证历史。存在未加载历史页面时 SHALL 使用完整历史读取入口，读取失败不得发布部分导出。

#### Scenario: History still loading
- **WHEN** 当前视图只有部分可见消息，且完整历史读取失败
- **THEN** 导出报告读取失败，不写文件或剪贴板。

#### Scenario: History loaded
- **WHEN** 完整历史读取成功
- **THEN** 导出包含选定分支全部可导出消息。

#### Scenario: An assistant message is outside the resident page
- **WHEN** `/copy N` requests an assistant message older than the resident history window
- **THEN** Pager resolves the requested ordinal in verified history on a worker and applies the result only to the original session binding. A failed read leaves the destination untouched.

### Requirement: Suspended tree execution waits for load finalization

A cold executable resume SHALL prepare and reconcile the suspended tree while execution admission is closed. Shell SHALL complete load reconciliation and commit the resume usage boundary before releasing the original tree for execution. The load response SHALL retain any preserved foreground identity. Pager SHALL enable ordinary input at its existing successful load boundary.

#### Scenario: An active Goal is restored from a suspended session

- **WHEN** the latest history page can be shown while load reconciliation or resume accounting is still pending
- **THEN** the suspended tree stays gated and cannot start a new Goal turn or continue a preserved turn. Successful finalization releases the same owners through the existing tree release path.

#### Scenario: A suspended turn is preserved by the checkpoint

- **WHEN** a successful cold load restores a checkpointed foreground turn
- **THEN** the load response identifies the preserved foreground and execution continues under that owner after the load's recovery and accounting work.

## ADDED Requirements

### Requirement: Session history loads from the newest page on demand

Interactive executable resume SHALL make the runtime available as soon as required execution state is verified. Presentation independently reads a bounded recent page of canonical history, and older pages are loaded on demand. Page reads SHALL bind session, selected branch and fixed history cut. Pages SHALL preserve ordered projection, response boundaries and exact stable identities, with positions independent of live delivery.

#### Scenario: Long closed history
- **WHEN** a session written with incremental publication first opens for interactive resume after shutdown or process loss
- **THEN** the runtime can become ready independently of the presentation worker. The most recent page is readable without scanning the closed prefix, and older pages are read on demand.

#### Scenario: Older page and live update overlap in time
- **WHEN** an older page completes after runtime readiness while new updates arrive
- **THEN** historical rows appear once at their original order, viewport and selection anchors remain stable, and the older page does not change current controls or live delivery position.

#### Scenario: Older page is unavailable or stale
- **WHEN** a requested page cannot be verified or belongs to a retired session/branch generation
- **THEN** it is not merged into the active view, and a ready runtime remains usable.
