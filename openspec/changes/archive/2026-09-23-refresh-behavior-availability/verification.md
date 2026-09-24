## Evidence

The stale state was reachable through several existing admission transitions: queued prompts and Goal continuations install `RegularTurn`; regular turns enter `Settling`; turn, compaction, and cancellation arbitration may release foreground ownership to idle; manual compaction claims foreground; and step controls change pending admission while a turn is active. These paths now publish through the existing command update after releasing admission locks. The update path also publishes Behavior metadata when its slash-command list is empty. Shell remains authoritative and rechecks each selection against current facts.

Projection revisions are reserved while capturing admission facts and before asynchronous capability/workflow reads. Pager retains only a strictly newer revision. Reconnect replaces the Pager update tracker when a new session load begins; the failed-load path restores the old tracker, so actor-local revisions restarting at zero do not share a comparison window with an old live actor.

## Validation

- `rustfmt --check --config skip_children=true --edition 2024` on changed Rust files — passed. The broader turn-pipeline test file contains a separate shared-agent formatting hunk and was not reformatted wholesale.
- `git diff --check` — passed.
- Shell test binary from the shared build: `queued_prompt_promotion_publishes_fresh_behavior_availability` — 1/1 passed; `settled_idle_session_publishes_fresh_behavior_availability` — 1/1 passed.
- `CARGO_BUILD_JOBS=1 cargo test -p pager behavior_projection_refreshes_an_open_settings_snapshot` — 1/1 passed; binary: `target/debug/deps/pager-6933656416a7467a`.
- `openspec validate refresh-behavior-availability --strict --no-interactive` — passed.
- `openspec validate --all --strict --no-interactive` — passed (19/19 items).
- Archived as `2026-09-23-refresh-behavior-availability`. The first archived validation found the archive task checkbox still incomplete; after updating that task record, post-archive checks passed: `openspec validate --all --strict --no-interactive` (18/18) and `openspec validate --archived --no-interactive` (380/380).
