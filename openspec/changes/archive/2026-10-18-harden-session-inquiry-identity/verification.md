# Adversarial review and verification

## Scope and evidence

Source-only review of model tools, MvpAgent's source audit boundary, authenticated local IPC, runtime inquiry caches, target actor admission, canonical Timeline recovery, offline communication restoration, and Pager row correlation. No live inquiry was sent and no installed executable was replaced. Parent/child inquiries keep their runtime-derived relationship checks and allocate new UUIDv7 identities; peer inquiries retain handshake, workspace approval and tool-free Sideband boundaries.

The previous blank model-ID fix remains in place: missing/null/blank inputs generate a new ID in the existing backend. Nonblank inputs reuse only an existing exact canonical ID. This change does not merge new calls by question text.

## Findings fixed

1. Target runtime and display keys used `(source_peer_id, inquiry_id)`. Two source sessions within one process conflicted or shared display/recovery state; a new source process identity could trigger another delivery. Keys now use `(source_session_id, inquiry_id)` and immutable request payloads. Peer/incarnation still authenticate connections and determine lease cancellation; original peer attribution remains in audit details.
2. Target actor always appended a receipt and queued the request. Losing the runtime cache through restart or expiration therefore allowed another execution. The canonical target Timeline now returns a matching terminal outcome, rejects conflicting payloads, and refuses to enqueue an open receipt again. Existing cold recovery closes open receipts as TargetRestarted; retries reuse that terminal without a new Sideband, UI receipt or approval.
3. Source durable lookup only considered completed requests. A cold source without a terminal fact could reuse the ID with another target or question. The first OutgoingStarted now binds that ID; later starts do not replace it. A completion conflicting with that first binding fails with AuditFailure. Incomplete read-only queries report SourceUnavailable; only an explicit identical retry may reach the original target, subject to its durable admission guard. Known terminal cancellation returns false without resolving another target.
4. Inquiry progress/outcome and cancel responses ignored their returned ID. All now require exact correlation; mismatches fail without relaying progress, accepting an unrelated answer/ACK, or triggering a transport retry.
5. UUID parsing accepted several textual representations while cache keys compared raw strings. Only canonical lowercase hyphenated UUIDv7 is now accepted for ask/get/cancel, including incoming IPC cancellation. This prevents aliases from creating another logical identity.
6. Source pre-cancellation was only observed after spawning delivery. Cancellation is now set on the record before spawning it. A terminal target record rejects cancellation rather than reporting it accepted.

## Regression matrix

| Boundary | Regression |
| --- | --- |
| Same ID, different source sessions in one peer | Separate target receipts/answers, cancel only one, third source cannot query either, wrong target cancellation does nothing |
| Source process identity changes | Same target runtime result reused, no second target dispatch |
| Source cold recovery, no completion | First target/question preserved despite later conflicting start; query has no execution; invalid completion cannot poison the result |
| Target cold recovery | Actual JSONL write/read, fresh ChatState/actor and fresh source/target runtimes, explicit retry through real IPC; completed answer or TargetRestarted reused with no queue/Sideband/new receipt |
| Open target receipt or conflicting retry | Busy/Conflict returned without adding receipt or queue work |
| Cancellation | Pre-cancel creates no target record; active cancellation is isolated by source/target; terminal remote cancellation returns false |
| Response mismatch | Forged peer sends wrong progress/outcome/cancel IDs; errors are non-retryable and wrong progress is not relayed |
| UUID aliases | Uppercase, compact, URN and braced forms parse to the same UUID but ask/get/cancel and incoming cancellation reject them before admission |
| UI and offline replay | Two source sessions with same peer/ID retain separate rows and phases; changing peer for the same source/ID updates one row and does not restore a duplicate phase |
| Existing behavior | Previous blank-ID regression, authenticated independent-process IPC, concurrent retries, target restart, cancellation, approvals, foreground isolation and sideband evidence |

## Validation boundary

The initial regression ran in an isolated HEAD snapshot (`8cc20454da9e62874170fb3ed1cc532591954947`) with this task's tests overlaid, because the shared working tree had unrelated in-progress changes. It reproduced seven failures before production fixes (two-source isolation, source reincarnation, incomplete source binding, response correlation, target receipt recovery, open receipt dedup, and independent receipt restoration). The pre-cancellation test and two existing identity tests passed. The snapshot includes the prior blank-ID tool fix and regression.

The final snapshot applies only diffs against saved pre-task versions, preserving unrelated working-tree changes outside validation. Temporary runtimes and corrupt-response servers use a private test home. Cold restore uses real JSONL storage; saved target answers/receipts are seeded test facts, and the active queue is held to ensure an accidental new provider request cannot run. No external provider call is required for the new recovery regressions.

## Deferred audit detail

The source backend still emits OutgoingStarted before the runtime checks an existing ID. A cached retry or conflicting call may therefore add a redundant start display fact. First-binding recovery deliberately ignores later starts and never dispatches a conflicting request. Separating operation admission from attempt display is recorded in `openspec/backlog.md`, not implemented as a broader audit redesign here.

## Results

Tests use `CARGO_TARGET_DIR` pointing at the shared build cache, `CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216` and `--locked --offline`.

- `cargo test -p shell -p pager --lib coordination --locked --offline`: **Shell 54 passed; Pager 45 passed; 0 failed**.
- After the last supplemental assertions for conflicting durable completion and cold-source cancellation, `cargo test -p shell --lib coordination --locked --offline`: **54 passed, 0 failed**. Counts overlap the previous Shell run and must not be added together.
- `cargo test -p shell --lib session::storage::communication_history --locked --offline`: **10 passed, 0 failed, 1 ignored**. The existing ignored test requires an explicitly selected live source session; it is not needed for the deterministic temporary-home regressions and was not run against a live session.
- Targeted `rustfmt --check` passed for changed production modules and new test modules. The added UI test block and changed history tests were formatted separately, preserving pre-existing formatting outside this task. The isolated target actor module also passed formatting checks; its shared-working-tree counterpart retains unrelated in-progress fixture fields.
- `git diff --check`: passed.
- `openspec validate --all --strict --no-interactive` before archive: **22 passed, 0 failed**.
- `openspec archive harden-session-inquiry-identity --yes`: archived and merged the new scenarios into the canonical specification.
- After archive: `openspec validate --all --strict --no-interactive`: **21 passed**; `openspec validate --archived --strict --no-interactive`: **583 passed**, both with zero failures.

An unnecessary duplicate history run, queued behind another build's artifact lock after the required history suite had already passed, was stopped without interrupting any other task. Its queue cancellation is not counted as a test result.

An additional attempt to run the combined coordination tests in the actual shared working tree failed before reaching tests: unrelated planning changes had missing `tool_types::planning`/plan exports, TaskToolInput fields and SubagentCompletedOutput.planning_summary. Those changes were preserved. The isolated results verify this change and its previous blank-ID dependency, not all unrelated uncommitted work together. Intermediate validation attempts also caught two misplaced test assertions and two renamed UI fixture fields; those were corrected before the successful runs.
