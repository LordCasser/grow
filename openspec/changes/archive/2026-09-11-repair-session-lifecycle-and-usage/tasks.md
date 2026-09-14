## 1. Cancel and load recovery

- [x] 1.1 Extend the exact-prompt watchdog to `TurnCancelling`; verify tests cover missing-session error, running re-arm, and terminal first-wins convergence.
- [x] 1.2 Track the origin of initial load placeholders and remove/return on failure; verify Welcome, existing Agent, Dashboard, retry, and in-place reload tests.

## 2. Durable lifetime usage

- [x] 2.1 Rebuild lifetime main-attempt usage from Timeline and verify cold restore preserves totals/model/cost/duration/incomplete without duplicate attempts.
- [x] 2.2 Persist and restore subagent usage settlements plus session incomplete facts; verify Agent attribution, model aggregation, idempotence, and conflict failure.
- [x] 2.3 Persist a resume segment boundary for successful cold actor loads and verify resident reconnect does not create a segment.

## 3. Usage projection and presentation

- [x] 3.1 Add lifetime segment projection to `grow/session/usage` without exposing Agent detail in existing prompt/headless shapes; verify serialization tests.
- [x] 3.2 Render lifetime totals and Initial/Resume segment summaries in fullscreen and Minimal Usage surfaces while normal status remains aggregate-only; verify formatter and dispatch tests.

## 4. Validation

- [x] 4.1 Run targeted chat-state, shell, and pager tests plus formatting; record exact commands and results in `verification.md`.
- [x] 4.2 Run strict active OpenSpec validation, archive the completed change, then validate active and archived specs; record results.
