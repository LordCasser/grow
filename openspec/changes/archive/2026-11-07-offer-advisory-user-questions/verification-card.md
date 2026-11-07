# Pager advisory suggestion card — verification record (2026-11-07)

TaskA card scope: presentation-before-ACK card, dedupe/recovery, six-state reply
projection, four AppView helpers, Checking+query reply entry, tests, and this
window's compile+test verification. Environment: `cargo +1.93.1 --locked
--offline`, `RUST_MIN_STACK=16777216`, `CARGO_BUILD_JOBS=2`, `PATH~/.cargo/bin`.

## Commands and actual results

| Command | Result |
| --- | --- |
| `cargo +1.93.1 test -p pager --lib advisory --locked --offline` | **exit 0** — `21 passed; 0 failed` (7,588 filtered). Log: `/tmp/grow-ask-pager-card-target.log` |
| `… --lib question_view` | exit 0 — 57/0 |
| `… --lib question_plan` | exit 0 — 3/0 |
| `… --lib plan_mode` | exit 0 — 44/0 |
| `… --lib non_blocking_preference` | exit 0 (after fix below) — 5/0 |
| `… --lib dispatch::tests::settings` | exit 0 (after fix below) — 125/0 |
| `… --lib settings_modal` | exit 0 — 173/0 (1 ignored, pre-existing) |
| `… --lib registry` | exit 0 — 67/0 |
| `cargo +1.93.1 check -p pager -p pager-minimal --tests --locked --offline` | **exit 0**. Log: `/tmp/grow-ask-pager-card-tests-check.log` |

Regression log: `/tmp/grow-ask-pager-card-regress.log` (per-filter markers + exits).

## Advisory test names verified (not just counted)

My 11: tracker `advisory_success_result_confirms_card_from_typed_raw_output`
(counter-evidence: the old content-text parse read no advisory — cold recovery
lost), `advisory_cold_result_restores_card_without_ext_presentation`,
`advisory_failure_marks_presented_card_failed`,
`advisory_duplicate_ext_and_ext_before_tool_call_keep_single_row`,
`advisory_duplicate_success_preserves_reply_projection`; interactions
`advisory_ext_presents_renderable_card_before_presented_ack` (card renderable
before ACK; composer/question_pending/turn-pause untouched),
`advisory_ext_keeps_blocking_overlay_and_merges_duplicate_ext`,
`advisory_reply_projection_helpers_gates_and_identity` (Available-only open,
terminal absorption with precise bools, full-format identity, one-shot intent,
Checking refuses re-query),
`advisory_reply_submit_action_retained_with_identity`,
`turn_status_projection_excludes_advisory_reply_edit`,
`turn_status_child_projection_excludes_advisory_edit_keeps_guards`.
b064's 10 (registered once, in `prompt.rs` via `#[path]`): 9 `advisory_*` +
`doctor_fix_replaces_only_local_advisory_edit_and_preserves_original_composer`
— all listed `ok` in the target log.

## Compile-fix history (9 runs to green; all within card scope unless noted)

1. Exit 101 ×17 (prior window; log tail-only in tool output file).
2. Static fixes 12 → run: 101 ×6: router import, `doctor_fix_target`
   `pub(super)`, test-file imports, `SessionMatch`/`agent_id` `pub(crate)`
   (coherent with the `pub(crate)` `find_session_match` re-export), tracker
   `&tc`, fixture `preview: None` E0282.
3. E0382 ×2 (moved `block` through tuple) → per-branch `tool_call_to_block`.
4. Compile OK, 8 failed: **name-guard bug** — result rows carry the ACP title
   ("ask user"), not `ask_user_question`; the guard early-returned so every
   result-built card was dropped. Removed; discriminators are the typed
   advisory payload + a previously presented card. Handler returned
   `is_active` (false in parked fixtures) → `return true` (new scrollback
   state warrants repaint, mirroring the blocking path).
5. E0308 `true` vs `()` → explicit `return true`.
6. 30/1: my own test violated terminal absorption (reset Accepted→Unobserved)
   → rewritten against a fresh card + terminal-refuses-check assertion.
7. 31/0 with b064 tests twice (their `#[path]` in prompt.rs + my duplicate
   `mod` in tests/mod.rs) → removed my duplicate per 勿增实体; final 21/0.

## Mechanical adaptations in other owners' files (recorded)

- `app/root/dispatch/prompt.rs` (b064): `doctor_fix_target` → `pub(super)`
  (test-surface visibility only).
- `app/root/dispatch/tests/advisory_reply_admission.rs` (b064): added
  `use super::super::prompt::{doctor_fix_target, open_doctor_fix_question};`.
- `app/root/event_loop.rs` (b046): `String::parse::<toml::Value>()` is a
  single-expression parse; a config document failed at the first table header.
  → `toml::from_str::<toml::Value>(&content)` (document parse; assertions
  unchanged).
- `app/root/dispatch/tests/settings.rs` (b046): catalog coverage 42 → 43
  (`SetAskUserQuestionAllowNonBlocking` is the new 43rd writable setting;
  scenario otherwise unchanged).

## Remaining (not claimed done)

- TaskB admission semantics (`dispatch_submit_advisory_reply` /
  `reconcile_advisory_reply`) are b064-owned, now compiled and green in their
  10 tests, but await the parent's review (rev 62 binding) — the prior
  volatile-guard review findings were addressed by b064's source, not by me.
- True product-chain PTY milestone (b065, fresh CLI) still pending; these lib
  tests do not substitute for it.
- Cross-crate milestone integration (real mock provider: no-answer continue,
  blocking does not continue, preference force-blocking, late answer durable
  admission) remains the parent-organized gate.
