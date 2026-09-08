# Design

Replace the session ID stored alongside rewind UUID with session_binding_epoch. bind_session_id advances the epoch on an actual identity change; unbind_session_id advances it when a binding exists. Same-ID no-op binding preserves the epoch. No new counter or protocol field is needed. All production root session assignment routes through these helpers.

This only prevents stale reads from mutating a later binding. General overlay reset on session transition and execution-result binding are separate lifecycle questions; execution represents committed effects and is not guarded by this read mechanism.
