# History paging implementation map

## Boundary

Interactive session load needs two independent products from the same pinned
history cut:

1. Runtime restoration, which validates and hydrates the working state, then
   applies the durable tail before opening execution admission.
2. A bounded display page, which can arrive before or after runtime readiness.

Display position is not the live ACP delivery cursor. An older-page read must
not rewind `last_seen_event_id`, update current control authority, or delay the
runtime load. Complete-history operations such as export, rewind and search
keep their explicit complete-read path.

## Shell storage and projection

The durable journal is `timeline.jsonl`. The current full reader is
`shell/src/session/storage/jsonl/ledger.rs::read_journal_from_directory` /
`read_journal_file` / `read_journal_prefix_data` / `scan_journal_range`. It
validates every physical record through `history::AppendState`, folds Domain
facts into `TimelineEvent`s, and stores Presentation facts in
`HistorySnapshot.sources`. `jsonl::OpenedSession::validated_timeline` then
validates the source snapshot against the folded Timeline, loads communication
notices, verifies prompt blobs and reads sideband ledgers.

The display projection is
`shell/src/session/history/projector.rs::HistorySnapshot::prepare` and
`project`. It resolves admitted responses from the selected Timeline,
reconstructs presentation from Domain and Presentation sources, applies rewind
filtering, binds communication notices, and stamps a replay cursor. The current
implementation materializes all events and sources before projection; slicing
`PreparedHistory.updates` afterward would bound only delivery, not journal
scanning or peak memory. Projecting a physical suffix alone is incorrect: a
page can depend on selected-branch response admission, rewind, response/tool
anchors, receipts, and source records preceding its visible range.

The persisted working-state/index design must therefore expose one validated
read contract with:

- the exact session identity and selected branch/lineage;
- a fixed physical history cut and integrity identity;
- the newest logical display range and older-range continuation position;
- the canonical prefix-derived projection context needed at that range (for
  example, an open response/tool boundary or stateful lifecycle summary);
- exact stable source identities and original order.

An older-page request binds all of these values. If the session, branch, cut or
projection version changed, Shell returns stale/unavailable instead of an
empty page. Page cursors are opaque positions in this branch-bound display
sequence; they are not ACP event IDs and are never compared with live
delivery IDs. Shell constructs a page using the same pure history projector,
but must read only the indexed range and its verified dependencies rather than
calling the current full-history reader and slicing its output.

## ACP and Pager delivery

The executable load entry is `shell/src/agent/mvp_agent/acp_agent.rs` at the
`load_session` replay call and
`shell/src/agent/mvp_agent/mod.rs::replay_session_updates`. Today it invokes
`HistorySnapshot::prepare` over the complete pinned snapshot, then sends ACP
and Grow replay updates in source order. Keep this path for runtime restoration
and current live delivery. Extend its load result with a separately identified
latest display page and branch-bound continuation token; do not make old-page
requests replay through the regular live update channel.

Pager starts the load window in
`pager/src/app/root/dispatch/session/load.rs::dispatch_load_session_ungated`,
routes replay through `app/acp_handler/session_notification.rs`, and finalizes
the window in the `SessionLoaded` handler in that same dispatch module. The
reconnect cursor rules live in
`pager/src/app/acp_handler/session_notification.rs::advance_reconnect_cursor`
and `pager/src/app/agent_view/session.rs::advance_reconnect_cursor`. Leave
those rules attached only to applied live/reconnect updates. Add a distinct
history-page owner to `AgentView` containing session/branch generation, the
page continuation position and in-flight request identity. A result is merged
only when its owner still matches; an older page does not enter live ACP
dispatch or update controls.

For passive presentation, prefer the existing observational projector
`pager/src/transcript_projection.rs::TranscriptProjection`, which applies
`SessionUpdate` facts to its own `ScrollbackState` without the normal live
session handler. Any page-local projector context needed to preserve a split
tool/response or lifecycle row must be supplied from Shell's validated
boundary; do not replay historical Grow controls into the active
`AgentSession`. Tests should compare page-wise projection against complete
projection on the same branch/cut, including a response/tool boundary and
rewind, and separately prove that an overlapping live update appears once and
remains authoritative.

## Why there is no isolated prepend API yet

`pager/src/scrollback/state/mod.rs::insert_block_before` inserts before one
uncommitted anchor. Its precondition is intentional: `MinimalCommitState`
records rows already emitted into the native terminal scrollback, whose order
cannot be changed after printing. `append_entries_from` only joins a
`fresh_continuation` sibling after the existing state for cursor-tail
reconnect; it preserves live tail identities and commit state, but cannot
prepend. `capture_scroll_bookmark` / `restore_scroll_bookmark` in
`scrollback/state/mod.rs`, backed by `capture_scroll_anchor` /
`restore_scroll_anchor` in `scrollback/state/layout.rs`, preserve a logical
viewport anchor across layout changes, but do not solve native output order.

Therefore a page cannot safely be spliced ahead of the latest page once its
rows crossed the native commit frontier. A Pager-only generic prepend method
would either violate native ordering, silently mark never-printed rows as
committed, or be unusable after the first page is displayed. Do not add such a
method until the rendering owner has a real ordered-history insertion model.

The viable Pager increment is to keep the paged historical transcript in an
ordered virtual history model whose logical order is independent of native
output, while treating already committed terminal output as an immutable
presentation snapshot. The view can then materialize a virtual window from
that model and preserve the current viewport bookmark and selected stable
entry identity while older ranges are attached. If the existing terminal
backend cannot redraw that window without appending out of order, paging mode
must use its virtual scrollback path rather than claim that `insert_block_before`
can reorder native history. Define and verify that backend/view seam before
adding a public scrollback splice API.

## Implementation map and tests

1. Shell journal/index/checkpoint work: `session/storage/jsonl/ledger.rs`,
   `session/storage/jsonl/mod.rs`, `session/history/{mod,projector}.rs`.
   Verify newest-page reads are bounded by indexed records and exact
   dependencies; reject cut/branch/index mismatch and malformed records.
2. Shell ACP/page contract: `agent/mvp_agent/acp_agent.rs`,
   `agent/mvp_agent/mod.rs`. Verify normal load returns the latest page plus
   its independent token, and older-page reads do not require writer authority
   or affect runtime admission/live delivery.
3. Pager ownership and display: `app/agent_view/`,
   `app/root/dispatch/session/load.rs`, and
   `app/acp_handler/session_notification.rs`. Add explicit top-boundary fetch,
   stale request rejection after session/branch switch, duplicate suppression
   for page/live overlap, and proof that historical controls do not alter
   active controls.
4. Rendering: `scrollback/state/{mod,layout}.rs` plus
   `scrollback/scrollback_pane.rs`. First establish ordered virtual history
   insertion with stable viewport and selection anchors while respecting
   native committed rows; then add a test that pages older history after the
   newest page has been committed. Do not treat the current uncommitted-anchor
   insertion path as coverage for this case.
5. Preserve complete-history APIs and compare concatenated page projection to
   eager complete projection, including tool grouping, response splitting,
   rewind, communication receipts, hooks, branch replacement and live-update
   overlap. Exercise a failed/stale page request while the runtime remains
   ready.
