# Actor validation failures (final-10)

This diagnosis is based on `final-10-session-actor-tests.log`, which reported 432 passed, 10 failed, and 1 ignored. Two truncation test fixtures have since been aligned with the archived ChatState retry contract; this report does not claim their rerun passed.

## Five cold image restore timeouts

Tests: `image_fallback_cold_restore_retries_completed_legacy_prompt`, `image_fallback_cold_restore_reuses_acknowledged_ocr_projection`, `image_fallback_cold_restore_reuses_acknowledged_projection`, `image_fallback_cold_restore_reuses_completed_description_before_projection`, and `image_fallback_cold_restore_reuses_description_without_auxiliary_route` in `crates/codegen/shell/src/session/actor/tests/image_input_recovery_tests.rs`.

All five time out at the poll for a validated timeline ending in `SidebandEventKind::End`. The poll used `is_ok_and`, which hid validation errors. The fixture's persistence worker wrote Sideband and Domain events but discarded external `PersistenceMsg::Update` / display updates. The current history validator checks each committed `Input::Consumed.display_echo` against its typed presentation source; omitting the echo update makes `validated_timeline` fail before the test can observe End. The fixture now persists updates, acknowledges buffered/durable updates, and handles `AuxiliaryGrow` delivery; the poll includes the last validation error in its timeout message. This is a test persistence-mock omission, not evidence that the production Sideband End writer is broken.

## Three inherited subagent bash status failures

Tests in `crates/codegen/shell/src/session/actor/tests/subagent_bash_permission_tests.rs`: `subagent_bash_permission_timeout_does_not_execute` (line 1146), `subagent_bash_rejected_after_prompt_does_not_execute` (1065), and `subagent_inherited_handle_bash_approved_after_prompt_executes` (606). Expected terminal `ToolCallUpdate` statuses are absent (`got []` for reject; timeout/approved likewise fail the status predicate), despite `execute_tool_calls` returning `ToolLoop::Continue` and the tests draining queued notifications to the gateway.

The fixture's `create_test_actor_ex_inner` was called with `bridge_projection_events = false`, so its receiver was the raw queue and did not need a FIFO flush. The initially added `flush_replay_actor` was therefore invalid and has been removed; the ReplayBuffer tail is still flushed after draining. The actual fixture defect was that its `_persistence_rx` was dropped on return. The fixture now keeps an in-memory persistence sink alive and ACKs buffered updates. This is a test fixture omission, not evidence of a production gateway delivery failure.

## Response-admission fatal boundary

`response_admission_fatal_boundary_blocks_tool_dispatch_and_recovery` originally returned `Ok(PromptTurnOk)` after a transient injected `io::ErrorKind::Other`; ChatState correctly retried and the mock acknowledged the retry. The fixture now injects `StorageFull`, which is permanent. Build11 reaches the fatal path; `handle_prompt` then attempts `emit_turn_ended` against the poisoned ChatState mailbox. Its unified error structure correctly reports outer phase `terminal` and retains the original fatal `response-admission` error in `data.cause`. The test now asserts both layers and verifies no accepted sampling/tool dispatch, no uncommitted assistant in Surface, and mailbox closure. No production change is indicated by this phase transition.

## Physical admission ACK loss

`response_admission_physical_ack_loss_blocks_tool_dispatch_and_recovery` originally failed at `truncation_recovery_tests.rs:650` because the helper's 30-second outer timeout could race ChatState's 30-second hard retry deadline. The helper outer timeout is now 60 seconds. The mock durably appends the admission and drops every ACK; exact-event retry continues to the hard deadline by design. The test asserts the resulting outer `terminal` boundary and inner fatal `response-admission` cause, closed mailbox, then checks from disk that admission exists exactly once and no tool event exists. Build12 additionally found `validated_timeline` returned `NotFound`: `admit_test_human_input` wrote the prompt payload through the test actor's default session directory while the journal was in the test's separate storage root. The fixture now binds `session_directory` and `session_dir` to `storage.open_session(&info)` before admitting input, so the existing full history validation checks the matching payload directory.

The physical ACK-loss mock also now handles `AppendUpdateBufferedAndAck` explicitly by persisting the typed update and acknowledging it, so the mock remains complete for that message shape. Static inspection did not establish this as the original timeout cause: the actor test bridge may translate buffered writes to `Update` before they reach this worker.

## Orphan cleanup closes a Hook created by the new recovery turn (run19; build21 pending)

`final-19-orphan-debug.log` and the persisted journal evidence show the ordering that the older diagnosis missed: physical seq40 is a new notification turn; seq41 starts its `HookTriggered` occurrence; seq42 records child recovery; seq43 completes the Hook; seq44 ends the recovery turn. Cleanup had treated the newly created Hook as orphaned and closed it, so this was a real ordering bug rather than only a stale child fixture. The root-side fix reorders restoration to recover child and parent state first, run orphan cleanup second, then publish UI and receipts. It reuses the current projection maps rather than adding another fold, and an internal `restored` flag prevents duplicate presentation repair or rerunning `SubagentStart`.

The regression obligations are not yet counted as passing: a deterministic unit must prove that a receipt command which starts a new legal Hook leaves it open, and the orphan PTY must prove the real child is restored while the old Hook is completed exactly once. Both await build21/CLI4 final verification. The old root paired numbers describe the pre-reorder candidate only; run the final paired comparison after this fix.

## Dense resume progress and cancelled leader turn (build20 evidence)

The 512-turn dense PTY did not stall: it painted the entire history in one fast frame at about 287 ms, which could not exercise multi-frame progress. The dense-only case now uses 2048 turns with 4 KiB chunks and passes the unchanged `>=2` visible-frame and `<2s` gap assertions: first visible marker 508.0 ms, last 622.5 ms, two advances, maximum gap 114.5 ms (`final-20-pty_e2e_persistence.log`). The cancelled-leader PTY also passes; its persisted evidence has an admission for T1 before T2 cancellation, so T1's body is canonical history and the previous “unadmitted body” expectation was not valid (`final-20-leader_pty_e2e.log`).

## Other actor groups (final-10 completed evidence)

The actor-updates process was stopped after more than three minutes at `goal_owned_subagent_spawn_reaches_live_client_before_goal_refresh`; its interrupted output is retained and is a failed run, not a pass. Actor spawn completed 27/27. Notification bridge completed 25 passed / 8 failed. The latter eight tests and the hanging fake still assumed live `AuxiliaryGrow` delivery before its persistence owner published the deferred transport copy. Their fixtures now model commit, then publish/ACK, preserving failure withholding and queue-budget obligations. Final-11 runtime validation is still required.
