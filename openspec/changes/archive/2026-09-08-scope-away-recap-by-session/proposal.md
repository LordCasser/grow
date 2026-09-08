## Why
Automatic recap eligibility uses an app-global shown flag and retry instant. A background session's recap suppresses the active session, and one session's attempt consumes another session's retry window.

## What Changes
Key shown/attempt state by SessionId for each away period. Query the active root session at both pre-generation and focus-return entrypoints, record attempts for the dispatched session and outcomes for the notification session. Preserve focus thresholds, 90-second backoff, replay isolation and new-away reset.
