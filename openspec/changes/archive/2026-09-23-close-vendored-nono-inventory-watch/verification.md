## Scope evidence

- `openspec/specs/sandbox-boundary/spec.md` accepts two Grow requirements: process/child boundary and Hook write protection. Its purpose explicitly avoids asserting equivalent isolation on every platform.
- `openspec/changes/archive/2026-09-23-inventory-all-crate-features/reviews/nono.md` reports 39 static draft deltas and explicitly says Cargo, `build.rs` and platform execution were not run. Those drafts remain archived evidence, not accepted requirements.
- The separate inherited-child-FD backlog entry remains. This change does not claim any platform backend is fully verified.

## Validation

- `openspec validate close-vendored-nono-inventory-watch --strict --no-interactive`: passed.
- `openspec validate --all --strict --no-interactive`: 16 passed, 0 failed before archive.
- `git diff --check`: passed.
- No Cargo tests were run because code and accepted behavior did not change.
