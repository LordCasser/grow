# Verification

The first Linux Core CI run failed only `header_new_agent_button_focused_is_green`: Pager 7567 passed / 1 failed / 15 ignored. See [run 41672827612](https://github.com/LordCasser/grow/actions/runs/41672827612) and `/tmp/grow-260-candidate-core-ci-failure-excerpt.log`. The original binary reproduced the failure locally with `NO_COLOR` removed and `COLORTERM=truecolor` (`/tmp/grow-260-dashboard-focus-before.log`). With the normal local `NO_COLOR=1`, both colors were Reset and the previous test passed vacuously.

The corrected test uses `Theme::grownight()` with distinguishable colors and independently covers list/button focus, dispatch input focus, and list/row focus. No rendering or input behavior changed. The adjacent renderer comment now describes keyboard focus separately from the cursor target.

- Colored Pager library suite: 7566 passed / 0 failed / 15 ignored, `/tmp/grow-260-dashboard-focus-pager-final.log`.
- Final four header render cases after making row focus explicit: 4 passed / 0 failed, `/tmp/grow-260-dashboard-focus-target-final.log`.
- Rust formatting, diff check, and strict OpenSpec validation pass. Native CI is tracked by the release closure record.
