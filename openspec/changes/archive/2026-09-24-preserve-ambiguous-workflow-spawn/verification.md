# Verification

- Audited all production `WorkflowRunStore::remove` callers. They are pre-Spawned launch compensation; there is no post-admission user forget API. The backlog's proposed `Forgotten` event would neither serve a current operation nor close the crash interval between Spawned and a subsequent event.
- `ChatStateActor::RecordTimelineEventDurably` prepares before persistence. An `Invalid` preparation result proves no append; a failure to accept after a successful write now has a distinct `CommittedProjectionInvalid` result. The manager tombstones only the former. Any other failure reports the run ID and preserves immutable source files so Timeline-driven restore can decide existence.
- The new regression offers Spawned to manual Timeline persistence, acknowledges its write, cancels the actor before the caller reply, and checks that the manager returns an uncertain outcome without removing source/tracker or creating `cleared`; replay of the recorded event retains the Run lifecycle. Existing restore tests cover missing sidecar reconstruction and Timeline-owned discovery. `oversized_initial_projection_rolls_back_before_timeline_spawn` covers the pre-append rollback.
- `cargo test --locked --offline -p shell --lib session::workflow::manager::tests::`: 33 passed.
- `cargo test --locked --offline -p shell --lib workflow_restore_`: 5 passed.
- `cargo test --locked --offline -p chat-state --lib`: 521 passed, 2 ignored.
- `cargo fmt --all -- --check`, `git diff --check`, and `openspec validate --all --strict --no-interactive`: passed before archive.
