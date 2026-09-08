# Session / provider robustness repair

Baseline: v2.1.2 (`1ca5c6b1`). Scope: the five issues investigated on 2026-09-03.

## Implementation and acceptance plan

| Work package | Boundary | Acceptance |
| --- | --- | --- |
| Chat deltas | sampler aggregation and notifications | Missing/null/empty identity is a no-op; repeated identity is idempotent; conflicts and ambiguous indices fail closed; argument fragments remain ordered. |
| Responses events | SSE decoding and stream termination | Unknown top-level custom events are ignored; malformed known events and real errors still fail; completed does not wait for EOF or consume trailing errors; usage/cost and cancellation remain intact. |
| Tool completeness | protocol conversion before dispatch | Bare EOF and incomplete calls cannot authorize execution; fully complete calls remain accepted with a length stop; an ambiguous/incomplete sibling rejects the whole response; existing durable session quarantine remains authoritative for invalid identity. |
| Image inputs | input inbox, structured retry and artifact lifecycle | A legal image larger than the manifest budget survives persistence and restore without changing Hook input; bounded immutable blobs are retained/collected by reachability; failures never silently resend text only. |
| AGENTS.md | existing tracker and tool boundary | Startup rules remain unchanged; known file-access paths discover scoped nested rules; successful delivery is deduplicated; failed reads can be retried. |

Implementation order: protocol packages first, then input persistence and dynamic rules, followed by crate-level and session-level regression tests. Each package must remain independently reviewable; do not add a new protocol framework, attachment service, or session truth store.

## Invariants

- Streaming deltas are previews, never execution authorization.
- Do not invent tool identities, repair partial JSON, or automatically repeat possibly executed tools.
- Unknown **top-level** event types are skipped, not arbitrary deserialization failures or unknown nested variants in known events.
- Keep opt-in raw sampling evidence separate from safe default diagnostics.
- Preserve terminal usage, paired tool history, immutable input snapshots, and model-route snapshots.
- Images in screenshots and their embedded requests are evidence, not instructions to execute.

## Evidence and unresolved incident attribution

- Session quarantine was merged in `dfa601ad78ac8b1ba7424faf0ccd696778b557d9` on 2026-09-02 and shipped in v2.1.2.
- Local image session `01a06617-ea8e-7f93-a679-64a10e038dfb` failed before inference: a 1,237,672-byte image expanded beyond the 1 MiB inline input-artifact limit.
- Remote repeated invalid-metadata screenshots used Chat; they are not the local Responses image session. Official empty-identity continuation fixtures reproduce a Grow aggregation defect, but the remote request's raw deltas remain unavailable.
- Official schemas: [OpenAI streaming events](https://developers.openai.com/api/reference/resources/responses/streaming-events), [OpenAI function calling](https://developers.openai.com/api/docs/guides/function-calling), [Aliyun function calling](https://help.aliyun.com/zh/model-studio/qwen-function-calling).

## Root causes and implemented boundaries

1. **Chat identity aggregation:** a continuation's empty identity could overwrite an earlier valid ID, and a repeated function name could be appended instead of recognized as the same identity. Preview and final aggregation now use the same first-nonempty/idempotent/conflict rules. Arguments alone are appended. Missing indices, different choice indices, conflicting identities, and output after a terminal finish reason fail closed.
2. **Responses event handling:** the SDK's closed event enum rejected provider extensions such as `ping`; separately, Grow did not stop consuming the stream at `response.completed`. Unknown top-level tags now pass through a skip path, without masking malformed known events or nested variants. The SDK's own discriminator defines the known set, with no duplicated event-name allowlist. Completed/incomplete are terminal; unknown events do not refresh the content idle timer.
3. **Tool completion/session integrity:** conversion previously erased the Responses item completion status before dispatch decisions, and Chat EOF could leave a seemingly executable call without a finish reason. Completion and JSON checks now precede conversion. Existing durable quarantine still handles invalid tool identities; incomplete/ambiguous protocol output never becomes executable history. A failed response's reported tool-validation usage is carried through sampler settlement and the session's usage ledger, without anchoring context length to rejected content.
4. **Image persistence and retry:** inline Base64 consumed the small immutable input manifest budget. Images are now separate, content-addressed blobs within the existing input-inbox capability boundary; Hooks and runtime receive the original ACP snapshot. A failed/unconfirmed client submission retains structured attachments and requires explicit retry, instead of losing images or reconstructing them from display placeholders.
5. **Project AGENTS.md discovery:** startup loading existed, but dynamic discovery was not connected to real tool results and tracker seeding could silently do nothing when no tracker was registered. Seeding now installs the existing tracker, and native file/directory access results discover scoped instructions. Discovery is bounded and retryable; acknowledgement happens when the reminder is attached, not merely when a path is found.

## Follow-up: uniform three-endpoint stream integrity

The follow-up keeps three protocol-specific accumulators, not a new transport
framework. A small shared decoder skips only unknown top-level tagged events;
Chat's normal chunk shape takes precedence over any custom discriminator.
Malformed known events, unknown nested variants, and real provider errors still
fail. Full frames remain confined to the opt-in sampling log.

| Endpoint | Acceptance boundary | Failure isolation |
| --- | --- | --- |
| Chat | One candidate with a nonempty, consistent finish reason; complete JSON tool arguments. Optional usage tail is bounded to two seconds (or the shorter idle timeout). | Bare EOF, conflicting response IDs/finish reasons, unsupported tool kinds, and post-finish output fail locally; heartbeats cannot hold a finished request open. |
| Responses | A matching completed/incomplete terminal snapshot. Announced function identities, argument prefixes, argument-done snapshots, and item-done snapshots must agree with it. | Duplicate indices, orphan deltas, conflicting snapshots and missing terminal events are non-retryable protocol errors, not fabricated HTTP 500s. Unknown incomplete reasons do not imply token exhaustion. |
| Messages | message_start → typed, closed content blocks → message_delta with a stop reason → message_stop. Multiple usage deltas remain supported. | Invalid JSON objects and unclosed tools reject the whole response, including healthy siblings. Initial input objects and streamed fragments are kept separate; empty streamed input is never repaired to `{}`. message_stop ends consumption immediately. |

Streaming previews never authorize tools. Protocol failure ends only the current
turn: valid prior history and already executed tool results remain intact, the
normal Session completion path releases foreground ownership, and rejected
tool calls cannot reappear on continue, model switch, or persisted replay.
The existing fatal persistence boundary is unchanged; provider/protocol errors
must not be misclassified as a corrupt Timeline or failed durable write.

Known terminal usage remains available when local tool validation fails; a
Messages start-only usage snapshot or missing optional usage must not be
invented into a final total. Refusal, valid text truncation, pause_turn and
fully completed tools accompanying a length stop retain their control semantics.

`test_session_protocol_recovery.rs` exercises the real ACP process, Session
admission, completion and storage paths without manually setting `Idle`. It
scripts a successful tool followed by a failed sample in the same turn, checks
that neither rejected siblings nor the earlier tool execute again, then checks
continue, all backend-switch directions and process restart/load. Earlier
direct-turn unit fixtures reset foreground state themselves and are not a
substitute for this lifecycle test.

All checked-in tests use constructed payloads, loopback Mock servers and
isolated test configuration/placeholder credentials. Live API/key tests, if
performed separately, are not part of repository fixtures, defaults or uploads.

### Protocol follow-up verification (2026-09-03, before the UX follow-up)

Final reruns passed: sampler unit tests (215), sampler HTTP/actor integration
tests (22), sampling-types unit tests (269), chat-state unit tests (451), Shell
unit tests (3,651 passed, 3 existing ignored), shared SSE fixture tests (9), and
the built-binary ACP Session recovery test (1, explicitly enabled). This is
4,618 passing tests in the follow-up verification set; it overlaps the earlier
five-package record below and must not be added to that historical total.
Scoped rustfmt checks and `git diff --check` also passed.

The ACP test ran against a freshly built v2.1.3 executable. It covered three
source backends, same-session continuation, all six cross-backend switch
directions (checking the actual wire model), and process restart/load. Valid
history and executed tool results survived, rejected tools never ran, and
previously executed work was not repeated. All traffic used isolated loopback
Mocks and placeholder credentials; no live-provider/key test was run or added.

The full Shell run exposed two stale fixture assumptions, both corrected before
the successful rerun: the shared Responses fixture streamed arguments without
announcing its function item, and the missing-Chat-terminal test treated an
interim usage snapshot as final usage. Production validation was not relaxed.
The ACP fixture also now uses explicit provider/model configuration, matching
the current model catalog instead of relying on the legacy remote catalog.

```sh
RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=4 cargo test -p shell --lib --offline --quiet -- --test-threads=4
CARGO_BUILD_JOBS=4 cargo test -p sampler --lib --test test_actor --offline --quiet
RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=4 cargo test -p sampling-types -p chat-state --lib --offline --quiet
CARGO_BUILD_JOBS=4 cargo test -p test-support --lib sse::tests --offline --quiet
CARGO_BUILD_JOBS=4 CARGO_PROFILE_DEV_PANIC=unwind cargo build -p cli --bin grow --offline --quiet
RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=4 cargo test -p shell --test test_session_protocol_recovery --offline -- --ignored --nocapture
```

An initial build exhausted disk space. A package-scoped
`cargo clean -p shell --profile dev` removed approximately 30.5 GiB of
regenerable build artifacts, followed by successful rebuilds with four build
jobs. No source, configuration or Session data was removed. At that verification
checkpoint approximately 36 GiB remained free. At the time of writing that
checkpoint, version 2.1.3 was still a working-tree change only.

Sources: [Chat streaming schema](https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events),
[Responses streaming schema](https://developers.openai.com/api/reference/resources/responses/streaming-events),
[Messages streaming contract](https://platform.claude.com/docs/en/build-with-claude/streaming).

## Notification UX follow-up

Recoverable endpoint problems are diagnostic facts, not user alerts. Unknown
extension frames remain invisible; automatic retries retain normal running
activity without a retry warning, toast, transcript block or retry title.
This changes notification timing, not transport retry eligibility/budgets,
tool-dispatch safety, usage accounting or Session repair authority.

An active prompt holds at most one pending `RetryState` failure, keyed to that
prompt's ID. Resuming outer recovery clears it. Exact-owner completion discards
it on success/cancellation and sends it through the existing retry-failure
notification path on final failure, before the prompt RPC is answered. Fatal
Timeline failures do not replay a pending provider error in place of the actual
storage failure. Request logs and accounting remain immediate; recovered
failures are not written as error notices into the UI replay cache.

Invalid tool-call metadata uses that same final-failure UI path. Pager shares
one per-turn `model_failure_reported` flag across both completion rails, so the
existing `Model request failed` block is not followed by another `Turn failed`
block or error toast. Prompt-stamped late/foreign failures do not alert in a
newer turn. Standalone command failures retain their existing notification path.

The outer completion-requirement exhaustion boundary remains explicitly visible:
an agent that exhausted recovery without satisfying its required task is not a
harmless recovered request, even if the last individual model call succeeded.
Its pre-existing return-value semantics (it may return that last successful
response despite an unmet completion requirement) are separate architecture
debt, not redesigned by this UX patch.

Verification includes constructed-data tests for deferred notices, exact-owner
consumption, recovered-error suppression, one-time display on either terminal
rail, and late-event isolation. The freshly built Grow ACP test also injects a
temporary HTTP 503 on each endpoint: all three recover without a terminal
failure notice. Invalid tool arguments produce exactly one standard failure
notice, followed by successful continuation, six cross-backend switch
directions, and process restart/load without repeating executed tools.

Final UX verification: Shell unit tests passed (3,652 passed, 3 existing
ignored); Pager unit tests passed (7,044 passed, 10 existing ignored); the
extended built-binary ACP test passed (1, explicitly enabled). Scoped rustfmt
checks and `git diff --check` passed. These runs overlap the protocol and
initial-package records above. All new cases use constructed data and isolated
local Mocks, never a real provider key. At that verification checkpoint,
version 2.1.3 was uncommitted and unreleased.

After verification, `cargo clean -p shell -p pager --profile dev` removed
27.3 GiB of regenerable build artifacts. Approximately 41 GiB remained free;
the freshly built `target/debug/grow` still reports 2.1.3. Source files,
configuration and Session data were not removed.

## Initial five-package verification record (before the follow-up)

Protocol implementation and review are complete. An exact item.done snapshot can establish completion when the terminal snapshot omits optional status; it never overrides an explicit incomplete status. Incomplete or invalid JSON calls fail the whole response, without a guessed repair or tool dispatch.

Passed: chat-state unit tests (451), sampler unit tests (205), sampling-types unit tests (269), sampler HTTP/actor integration tests (21), and the full Shell unit suite (3,651 passed, 3 existing ignored tests). Session regression covers four incomplete-tool scenarios across Chat/Responses with zero dispatch, no hidden retry, preserved reported usage, and a successful next turn. Repaired-history projections remain non-executable in all six backend-switch directions. No live supplier inference was used.

AGENTS.md implementation and review are complete: 27 tracker unit tests, 12 real ToolBridge integration tests, and 21 startup prompt tests passed. The existing tracker was changed from discovery-time acknowledgement/negative directory caching to snapshot scans and result-time acknowledgement. Concurrent distinct directories are queued within the same total deadline; concurrent same-directory accesses deliver once. A timed-out blocking worker retains the shared scan permit, preventing worker accumulation. Capability-based directory access, no-follow regular-file reads, nested gitignore/read-deny rules, byte/file/depth limits, retry after failed reads, and compaction generations are covered. The hanging-I/O permit test is deterministic simulation; the FIFO test uses a real named pipe.

Image implementation and review are complete. The 16 targeted Shell codec/admission tests are also covered by the full Shell run. Pager's full unit suite passed (7,042 passed, 10 existing ignored tests), including six failure/retry cases. Image coverage includes >1 MiB roundtrip and Hook identity, size/count/hash/path validation, legacy inline snapshots, missing-attachment isolation, reachable/orphan GC, one admission after a failed write and explicit retry, queue attachment retention, forked materialized images, and trace artifact roundtrip.

All five work packages are implemented and verified in the working tree. These are local regression results, not a live-provider certification or a release. Across the suites listed here, 11,699 tests passed and 13 were ignored; overlapping targeted runs are not counted twice.

Reproducible verification commands:

```sh
RUST_MIN_STACK=16777216 cargo test -p sampler -p sampling-types -p chat-state --lib --offline --quiet
cargo test -p sampler --test test_actor --offline --quiet
RUST_MIN_STACK=16777216 cargo test -p shell --lib --offline --quiet -- --test-threads=4
RUST_MIN_STACK=16777216 cargo test -p pager --lib --offline --quiet -- --test-threads=4
cargo test -p tools --lib types::agents_md_tracker::tests --offline --quiet
cargo test -p tools --test agents_md_discovery --offline --quiet
cargo test -p agent --lib prompt::agents_md::tests --offline --quiet
git diff --check
```

The default-stack full Shell attempt aborted with a stack overflow in `notification_drain::tests::idle_goal_owned_task_is_consumed_with_goal_continuation_evidence`; the complete rerun with a 16 MiB test-thread stack passed. This is a verification environment adjustment, not a production stack-size change or proof that every default-stack failure is baseline. The linker also reports the existing large `__eh_frame` warning.

## Deliberate boundaries

- These changes do not replace the existing output normalization and model request budgets. General stream/body resource limits are a separate transport hardening task.
- An unconfirmed input submission is retained for explicit user review; it is not automatically replayed. A general durable cross-transport idempotency redesign is not part of these fixes.
- Client-side failed image drafts are retained in-process, not across Pager restarts. Successfully admitted server inputs use durable manifest/blob storage. Historical inline snapshots keep their original manifest/hash integrity contract; the new image admission limits are not retroactively applied to consumed history.
- Image storage keeps the 1 MiB manifest cap, adds a 64 MiB serialized image aggregate cap and a 16-image cap, and reuses the existing decoded-pixel ceiling. Missing attachment bytes invalidate only the affected pending input; corrupt authoritative manifests still fail integrity checks.
- Forks inherit materialized conversation images, not parent pending-input authority. Existing forensic trace export includes the blobs under its original limits (64 MiB/file, 128 MiB total); Markdown `/export` is not a restorable archive, and already-missing blobs cannot be reconstructed.
- Dynamic project rules are attached to the result of explicit native file/directory access or a reported shell cwd. They guide the next model decision; this is not a pre-write permission gate and does not infer subprocess file accesses from shell command text.
- Dynamic-rule delivery acknowledgement is local to the existing tool tracker, not a new durable session transaction. Native grep/image/MCP results without a reliable accessed-path contract are outside this wiring.
- At the time of the initial repair, no release tag, installed binary, supplier configuration, or remote user session was changed.

Strict Clippy is currently blocked by pre-existing lints outside the changed logic: `manual_checked_ops` in `token-estimation/src/lib.rs:61` (with dependencies), and `unnecessary_sort_by` in `sampling-types/src/types.rs:960` (`--no-deps`). These are recorded separately rather than folded into this repair.

## Windows long-path follow-up

The reported Windows creation failure has not yet been tied to an exact Grow
version or failing path. Current main already switches CWD components longer
than 255 URL-encoded ASCII characters to a <=57-character slug/hash, and its
Windows storage capabilities traverse directories component by component.
Do not attribute every long-path failure to the CWD encoder or change the
canonical namespace without evidence: physical identity validation, direct
lookup, enumeration and resume all rely on that same encoder.

Confirmed gaps repaired in this follow-up:

- Creation, fork and import shared a `.<target>.<UUID>.staging` name. A legal
  255-character final name failed at staging with `File name too long` in the
  new regression test. Staging now uses only `.<UUID>.staging` (41 ASCII
  characters), independent of the final name. Publication remains atomic,
  contained and no-replace; rollback still removes only the temporary entity.
- The same boundary test then exposed `.<id>.writer.lock` exceeding the
  component limit after publication. Oversized lock names now use a stable
  full BLAKE3 digest in a disjoint namespace; already-legal lock names remain
  unchanged. Lock acquisition and removal share one function. Delete quarantine
  names also use only UUIDs, so successfully created long names remain removable.
- Code navigation's index cache bypassed the bounded encoder. Its constructed
  Windows/CJK path test failed before the change. It now uses the shared
  encoder; this derived cache requires no reverse-lookup metadata.
- Pager's plan-file lookup also bypassed the encoder and looked in a different
  directory from storage for long CWDs. It now uses `sessions_cwd_dir`.

Formal session names, CWD keys, `.cwd` markers, Timeline identity and the
on-disk format are unchanged. No historical Session or index data is migrated
or deleted. No extra TUI warning is introduced for successful operations.

New constructed-data coverage includes the 255/256 ASCII encoding boundary,
Windows drive/UNC/verbatim CWDs with Chinese, spaces and emoji, paths beyond
260 characters, same-prefix distinct paths, creation/reload/list/by-ID lookup,
fork, writer exclusion, deletion, staging cleanup and canonical plan-file lookup. The tests use isolated
local filesystem directories and do not require credentials or real APIs.

The Windows component limit is distinct from its legacy full-path limit;
[Microsoft's path-length documentation](https://learn.microsoft.com/en-us/windows/win32/fileio/maximum-file-path-limitation)
also notes that extended-length paths do not remove per-component limits.
These changes do not promise support for arbitrarily deep `GROW_HOME` values,
components exceeding filesystem limits, or every redirected/network volume.
Native Windows execution is still required to certify those OS-specific paths;
the development host for this follow-up is macOS.

Verification on the final source: Shell unit suite 3,655 passed / 3 existing
ignored; Pager unit suite 7,045 passed / 10 existing ignored; config path tests
13 passed; workspace index tests 8 passed. The 238 storage tests and 19 plan
tests are subsets of those full suites, not additional counts. Scoped rustfmt
and `git diff --check` passed. At this source-verification checkpoint, version
remained 2.1.3 and no release, installed binary, provider configuration or
remote Session was changed. The CLI binary
from the preceding follow-up was not rebuilt for this additional source patch.
After testing, `cargo clean -p pager --profile dev` removed 3.2 GiB of
regenerable TUI build artifacts; approximately 30 GiB remained free. No source,
configuration, or Session data was removed.
