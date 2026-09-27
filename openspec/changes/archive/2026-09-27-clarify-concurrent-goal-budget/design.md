## Context

`GoalUsageWindow` admits compatible live foreground/background attempts under an Active Goal. `account_captured_goal_usage` durably records exact usage, then closes the shared admission window once recorded cumulative tokens reach the budget. It cannot know the final usage of requests that have not returned.

## Decision

Preserve concurrent admission without speculative reservations. Every admitted attempt remains attributable and settles once even if another attempt exhausts the budget first. After a successful durable settlement crosses the threshold, the window rejects new bound and unbound attempts until the Goal lifecycle advances. Unknown usage retains its existing fail-closed rule. Add a test with two simultaneously admitted attempts whose combined known charges cross the threshold.

## Verification

Run the new Goal regression and the Goal support test group; validate OpenSpec.
