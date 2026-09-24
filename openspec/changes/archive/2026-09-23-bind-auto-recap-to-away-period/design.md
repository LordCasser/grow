## Boundary

The user-facing away period is owned by Pager's FocusTracker. Its ID must cross the `Effect::SendRecap` / ACP extension request / Shell session command / `SessionRecap` notification chain unchanged. The Shell cannot infer Pager focus state and must not create a replacement ID. Grow and ACP streams are unordered relative to each other, so arrival order cannot establish the result's period.

## Decision

Use a fresh UUID on each `on_focus_lost` and retain it through focus return; the next focus loss replaces it. An automatic request requires this ID. The Shell echoes it only with a successful automatic result. Pager rejects a live automatic result with no or mismatched ID before appending a block or changing shown state. A replayed historical result remains display-only even if it has no ID. Manual requests carry no away-period ID and keep their existing progress/feedback behavior.

This is a single correlation field across existing messages, not a new queue or retry policy. Existing per-session retry backoff is unchanged. Because a result can arrive after a later focus loss without a new main turn, the Shell's prompt epoch alone cannot solve this client ownership race.

## Validation

Exercise the production request payload and Shell echo, then inject A-result-after-B-start at the Pager notification handler. Verify it adds no block, leaves B eligible, does not clear manual feedback, and a B result succeeds. Verify replay and manual recaps preserve existing behavior.
