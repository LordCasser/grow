# Resume presentation attribution (P0)

## Why
Release RPC gains have not consistently improved the last visible TUI history frame. Existing write timers omit writer queue/delay/lock time; aggregates also include post-ready typing. Attribution must precede presentation optimizations.

## What changes
Add opt-in diagnostics around Markdown finalization, image probing, ACP application and frame delivery. Retain loading-window timestamped PTY evidence and compare identical modern fixtures in release builds, with counting-only RPC as a separate lower-work control.

No behavior, protocol, persistence, readiness or history-order contract changes. This diagnostic/tooling change uses `skip_specs: true`; the existing client-surfaces and session-timeline contracts remain authoritative. No SQLite, pagination, early readiness or background history restoration.
