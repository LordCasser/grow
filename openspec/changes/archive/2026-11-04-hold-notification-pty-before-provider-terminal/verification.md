# Verification

## Failure and mechanism

Candidate `455bcb92475066e31a361c6c4f6160f33441ecdb` Core [41698917082](https://github.com/LordCasser/grow/actions/runs/41698917082/job/126136741894) failed `suspend_nested_task_tree::ctrl_d_notification_turn_suspends_and_resumes_background_child`: the notification Turn was already ended in the suspended Timeline. The test lasted 3.50 seconds. Earlier library, Settings, CLI, protocol and Minimal quit checks passed; the Core job as a whole failed.

Source review confirms that the helper emits `response.completed` before `[DONE]`, the scripted HTTP barrier blocks the last supplied event, and the sampler breaks on `ResponseCompleted`. A second read-only review independently confirmed that blocking `[DONE]` cannot hold the Turn active. Only the two explicitly blocked Responses event vectors in this fixture now omit the optional sentinel, asserting which event was removed; the barrier precedes `response.completed`. No production code, common harness, deadlines or recovery assertions changed.

## Final local checks

Both exact selections were compiled/enumerated/executed against the final fixture and the existing CLI from production source `fccd91b10e141d59d4ed87ca85629015a2949e55` (subsequent crate changes are tests only). Environment: Rust 1.93.1, `PAGER_BINARY=target/debug/grow`, `TERM=xterm-256color`, `COLORTERM=truecolor`, `NO_COLOR` unset.

| Exact selection (`pager --test pty_e2e_persistence --ignored`) | Result | Log |
| --- | --- | --- |
| `suspend_nested_task_tree::ctrl_d_notification_turn_suspends_and_resumes_background_child` | 1 passed / 0 failed; command 10.95s | `/tmp/grow-260-notification-final-ctrl_d_notification_turn_suspends_and_resumes_background_child.log` |
| `suspend_nested_task_tree::minimal_ctrl_d_notification_turn_suspends_and_resumes_background_child` | 1 passed / 0 failed; command 18.15s | `/tmp/grow-260-notification-final-minimal_ctrl_d_notification_turn_suspends_and_resumes_background_child.log` |

Commands use `python3 scripts/run_exact_cargo_test.py --package pager --test pty_e2e_persistence --ignored <exact-selection>`; `/tmp/grow-260-notification-final-results.json` records both commands' results. An earlier exploratory run (`/tmp/grow-260-candidate4-notification-before.log`) overlapped the fixture edit while compiling and is deliberately not used as before/after evidence.

Both final runs retain the initial no-suspension assertion, live notification and child Turns, shared suspended operation/checkpoint, original Turn/task identity on cold resume, command interruption without replay, and exactly one completion/result per child. Formatting, `git diff --check` and pre-archive strict active OpenSpec passed (18/18). The next exact-source Core CI and official release remain pending; local results do not replace either.

Post-archive strict validation passed: active 17/17 (`/tmp/grow-260-candidate5-active.log`), archived 843/843 (`/tmp/grow-260-candidate5-archived.log`). Changed Markdown local links and `git diff --check` passed. The sole crate delta from candidate 4 is the notification PTY fixture.
