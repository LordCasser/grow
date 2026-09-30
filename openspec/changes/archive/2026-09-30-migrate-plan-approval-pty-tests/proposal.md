# Proposal: migrate Plan approval PTY fixtures

## Why

Three PTY tests still script the removed `exit_plan_mode` tool. Plan approval now uses `plan_control` while Plan Behavior is active, and the complete plan is supplied inline. Those tests fail before reaching the scrollback and approval behavior they are meant to cover.

## What Changes

Update the affected PTY fixtures to enter Plan Behavior through `/plan` and script `plan_control` with the complete plan in `submit` arguments. After approval is declined with revision feedback, Plan returns to Drafting, so the revised candidate is another `submit`; `amend` belongs to an already approved plan in its execution phase. Keep the existing scrollback completeness, no-duplication, quit persistence, and empty-revision approval assertions.

This is a test-only adjustment. `.openspec.yaml` sets `skip_specs: true`; no runtime contract changes.
