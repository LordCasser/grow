# Verification

- `cargo test -p shell --lib` passed: 3,954 passed, 4 ignored, 0 failed. This includes bridge, persistence, Goal/Workflow, subagent, and candidate admission tests.
- Bridge tests verify a 1 MiB completed-task output is absent from both Grow persistence and live copies, and that exhausted monitor gateway credits mark the exact active attempt while retaining the model notification command.
- Persistence tests verify a failed acknowledged auxiliary Grow append returns an error and rejects the attempt barrier; stale-attempt failure remains isolated.
- `cargo fmt -p shell`, `cargo check -p shell --tests`, and strict OpenSpec validation passed.
- `docs/development.md` records the output ownership and bridge credit behavior.
