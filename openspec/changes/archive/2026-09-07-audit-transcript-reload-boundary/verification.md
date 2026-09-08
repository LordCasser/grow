# Audit evidence

## Owner and identity
- minimal/api.rs::request_minimal_transcript captures active AgentId and entry IDs. full_view.rs::pump_transcript resolves app_agent(app, build.agent), not active view; absent agent drops build.
- dispatch/session/load.rs::dispatch_load_session_ungated allocates next_agent_id for a newly opened session. Existing open sessions are focused first.
- ScrollbackState::clear retains next_id. fresh_continuation resumes the old allocation counter; reload failure calls raise_id_floor before restoring the stash. These paths do not alias an old entry ID to a new replay entry.
- This establishes the inspected normal switch/reload paths, not a blanket claim about every AgentId mutation. No new runtime state is warranted solely to protect normal tab switching.

## Confirmed source-level truncation path
1. A minimal transcript has rendered a prefix into build.out and still has IDs pending.
2. AgentView::begin_session_reload moves the old scrollback into SessionReload and exposes fresh_continuation staging state.
3. pager-minimal::draw calls pump_transcript unconditionally before rendering; unlike other reload-aware consumers, pump does not inspect agent_session_reload_active.
4. Pump increments build.next before lookup. Old IDs are absent from staging and skipped. If no IDs remain it passes the existing prefix to finish_transcript, yielding a truncated file (or an empty-transcript notice if there was no prefix).
5. Pausing only while reload is active is insufficient for a complete fix: successful full replay replaces old IDs, while failed/cursor replay may restore them. A window may also begin and finish between pump calls.

## Next implementation boundary
Select an existing lifecycle hook or stable reload identity to invalidate/restart the build coherently, including full replay, cursor resume, failure rollback, and a complete reload between frames. Preserve normal active-view switches and removed-agent handling. Do not repurpose frequently changing content generations as reload identity, and do not clone the entire transcript merely to hide the lifecycle issue.

Pending ready-file notifications remain current-view scoped; this is distinct from the actual content truncation and should not be mixed into its fix.

## Validation limits
Source audit only; no new tests or CLI build ran. Reproduction above is a control-flow derivation, not a claimed PTY reproduction. A deterministic paused-build/reload regression is required before implementation is marked fixed.
