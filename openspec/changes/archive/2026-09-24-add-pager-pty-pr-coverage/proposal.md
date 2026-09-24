# Change: Run selected Pager PTY regressions in PR CI

## Why

The Pager coverage backlog names 18 user-visible PTY regressions, but every case is `#[ignore]` and no pull-request workflow runs them. The existing core regression workflow already builds `grow`, so it can run the 16 cases across four stable integration targets plus the real pager restoration case without enabling every expensive PTY test in ordinary Cargo runs. Two leader multi-client cases exposed a separate turn-boundary failure and remain outside this change.

## What Changes

- Run the 16 backlog scenarios across the minimal, config UI, scroll selection, and queue integration targets serially in the existing Linux core-regression workflow, passing its built binary through `PAGER_BINARY`.
- Keep the selected cases ignored for ordinary local `cargo test`; the CI step opts in by exact test name.
- Include the real interactive `less` transcript restoration scenario and install `less` in the runner so the test cannot silently skip.
- Leave the two leader multi-client scenarios in the Pager backlog; their second-turn completion currently violates the Timeline causal fold and needs an independent fix.

## Impact

CI and developer test tooling only. This does not alter application behavior or an archived product contract, so this change uses `skip_specs: true`.
