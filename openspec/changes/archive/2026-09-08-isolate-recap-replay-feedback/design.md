# Evidence and design
session_notification.rs permits recap replay even while busy, then unconditionally calls mark_recap_shown and apply_recap_block. The latter clears live recap feedback when auto=false. Both are current UI state, unlike the historical block being restored. SessionRecapUnavailable already explicitly ignores replay.

Split only this notification branch: replay pushes its block; live notification marks the away period and uses the existing helper. No new data model, request identity or tracker redesign. Verify through the real ACP handler with loading_replay enabled, zero-threshold focus tracker and pre-existing manual progress; check displayed block and both state side effects for auto/manual and live/replay combinations.

# Separate debt
FocusTracker remains app-global. A live background-session recap currently satisfies the active session's away flag. Session-aware automatic recap accounting requires separate design; not mixed into replay isolation.
