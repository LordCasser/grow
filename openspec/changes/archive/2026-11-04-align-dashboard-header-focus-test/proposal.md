# Align the Dashboard header focus test

## Why

The focused-button render test sets the new-session cursor target but leaves keyboard focus in the dispatch input. The renderer correctly paints the button green only when both the list and the button own focus, so the fixture's expected green state did not match its setup.

## What Changes

- Set list keyboard focus explicitly before asserting the button's focused green style.
- Keep an assertion that the button remains gray while the dispatch input owns focus, and retain the existing row-focused gray case.
- Use a fixed GrowNight theme and assert its focused/unfocused colors differ, independent of terminal color environment.
- Correct the test comment to distinguish cursor target from keyboard focus.

This is test maintenance only. It does not change Dashboard behavior or its contract, so this change uses `skip_specs: true` and has no delta spec.
