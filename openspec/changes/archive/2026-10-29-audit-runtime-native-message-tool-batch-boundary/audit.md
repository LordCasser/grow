# Runtime native message/tool batch reachability

## Evidence

- `SessionActor::receive_agent_message` persists a receipt via `receive_notification_detailed`; for an interrupting parent message it raises `parent_message_interrupt`. It does not mutate the conversation Surface.
- Run-loop `maybe_drain_notifications` delegates to an idle gate. `is_session_idle_for_injection` requires `foreground.is_idle()` and an empty pending-input FIFO, so a live foreground turn cannot promote the received receipt through the idle path.
- Direct `drain_active_notifications` production call sites are in prompt admission (`turn/admission.rs`, before turn execution and before the new model sample), the active turn loop (`turn/mod.rs`, at the pre-sampling boundary), and final steering settlement (`close_steering_and_drain`, called only after `TurnOutcome::Completed`). The admission path has two source call sites for its primary-input and pending-inbox cases.
- `process_conversation_turn` collects the sampled `tool_calls`, invokes `execute_tool_calls(...).await`, then handles its result. `execute_tool_calls` completes its batches and dispatch futures before returning. It does not drain active notifications during the per-result path.
- `drain_pending_interjections` is a separate path: it checks `events.has_active_tool()` and reads the interjection queue, not the notification inbox.
- A parent interrupt may stop an interruptible wait tool and create an early terminal result, but the other calls admitted in that batch are still processed before control returns to the pre-sampling drain.

Atlas incoming-call analysis found turn-loop and admission callers in its local Focus closure. The interjection caller was also found by exact scoped source search; Atlas explicitly warned that its caller set was closure-bounded, so absence claims rely on source enumeration, not the partial graph result.

## Result and limits

The specific sequence `native assistant calls[a,b] → result a → AgentMessage → result b` is not reachable through the audited notification drain paths for one production tool batch. No production fix or actor regression is justified by this candidate. This does not prove that every possible Timeline, provider-native adapter input, test-only direct Surface mutation, or future drain path preserves the same boundary; those are separate adapter/history contracts.

Relevant contracts read: `openspec/specs/session-timeline/spec.md` and `openspec/specs/model-sampling/spec.md`, including “Runtime agent messages project to complete tool exchanges” and “Receiving a message during a tool batch”. Historical claim checked against `openspec/changes/archive/2026-10-21-audit-runtime-thinking-projections/audit.md`.
