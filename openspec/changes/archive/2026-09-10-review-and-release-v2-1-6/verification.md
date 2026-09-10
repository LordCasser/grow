# Verification

## Local execution

- Initial core suite: 12,412 passed, 0 failed, 14 ignored across chat-state, sampling-types, sampler, memory, workflow, shell, Pager and pager-minimal. Log: `/tmp/grow-v2.1.6-core.log`.
- First supplementary compile failed after the review's partial formatting operation. The operation was fully reverted using the saved diff; the restored source reproduces the original rustfmt output exactly. This failed run does not count as a validation pass.
- Final 2.1.6 sampler/tools library and integration tests: 2,678 passed, 0 failed, 13 ignored. Sampler library 240 and integration 33; tools library 2,357 and integration 48. Log: `/tmp/grow-v2.1.6-additional-final.log`.
- Final 2.1.6 core: 12,412 passed, 0 failed, 14 ignored. chat-state 473, sampling-types 270, sampler 240, memory 302, workflow 65, shell 3,791, Pager 7,185, pager-minimal 86. Log: `/tmp/grow-v2.1.6-core-final.log`. Both final commands used `CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216` and `--locked -- --test-threads=4`.
- Unique library/integration total: 14,850 passed, 0 failed, 27 ignored; sampler's 240 library cases were run twice and counted once.
- CLI build passed with `CARGO_INCREMENTAL=0 cargo build --locked -p cli --bin grow`; `--version` reports 2.1.6. The native unoptimized session stack overflow found during process validation is addressed in `allow-debug-session-stack-overhead` by a debug-only stack allowance; release capacity remains unchanged.
- Complete independent-process coordination passed all ten scenario groups. A stale resident-reload assertion was corrected separately in `align-coordination-reload-regression`, matching the existing latest-approved-state projection and retaining every scenario. Log: `/tmp/grow-v2.1.6-coordination-final.log`.
- Post-correction standalone shell regression: 3,788 passed, 0 failed, 3 ignored. Log: `/tmp/grow-v2.1.6-shell-final.log`. The combined core run enabled three additional synthetic-replay testkit cases through feature unification; those passed in that invocation.
- Cargo.lock changes exactly 33 workspace package versions from 2.1.5 to 2.1.6; no external dependency entries changed. Offline resolution succeeded.
- `git diff --check`: passed.
- OpenSpec after archiving preparation and the two maintenance corrections: all strict 17 passed / 0 failed; archived 308 passed / 0 failed. Logs: `/tmp/grow-v2.1.6-spec-final.log`, `/tmp/grow-v2.1.6-spec-archive-final.log`.
- Final package-scoped Cargo cleanup removed 8.4 GiB of generated outputs after all local executions; about 20 GiB remained available. No local cross-target/distribution build or release-asset download was performed.
- Pre-commit source fingerprint (Cargo, crates and scripts, including new files): `1b5ff8392e87783c0e7469247ef8d89724f8085d25e9d4d0ccb2c2d0a762f1a7`.

## Publication boundary

The user explicitly requested inclusion of the other agent's concurrent portable-tool-history correction before publication. A fresh run on the combined working tree reused the existing build products (Cargo finished preparation in 0.94 s): chat-state 474 passed / 1 ignored, sampler 240 passed, sampling-types 274 passed; total 988 passed / 0 failed. Log: `/tmp/grow-v2.1.6-portable-final.log`. The complete Shell suite and remote gates are repeated for the combined candidate; previous CI results alone do not validate this newly included product change.

The combined candidate's complete Shell regression passed: 3,789 passed, 0 failed, 3 ignored, in 90.33 s. Command: `RUST_MIN_STACK=16777216 cargo test --locked --lib -p shell -j 2 -- --test-threads=4`; log: `/tmp/grow-v2.1.6-merged-shell-final.log`. Together with the three-library run above this is 4,777 passed / 0 failed / 4 existing ignored tests after the concurrent correction was included. The three-library run reused the other agent's existing incremental profile; Shell required a rebuild of affected crates. After all local executions ended, package-scoped Cargo cleanup removed 8.9 GiB and restored approximately 15 GiB free space. No installed Grow executable or live user session was modified.

Archiving this preparation record does not claim publication. GitHub core regression, platform checks, annotated-tag preflight and the all-platform release workflow will be verified from their actual run results before publication is reported to the user.

## Final candidate

Commit `667e1b33` contains the provider-termination provenance repair, the portable tool-history and Windows storage fixes, and the final ordinary-response coordination fixture. The final remote gates all passed:

- Core library regression and CLI build: [34496475770](https://github.com/LordCasser/grow/actions/runs/34496475770).
- Windows session storage boundary suite (all eight exact tests): [34496475428](https://github.com/LordCasser/grow/actions/runs/34496475428).
- Linux, macOS and Windows coordination libraries, CLI builds and independent-process scenarios: [34496475683](https://github.com/LordCasser/grow/actions/runs/34496475683).

The local affected-crate run on the same source passed 476 ChatState, 240 sampler, 274 sampling-types and 3,794 Shell tests, with four existing ignored tests. The additional Messages terminal-evidence correction was verified by 28 focused sampler tests before this commit. These checks do not invoke a real paid provider and do not claim that a natural provider stop proves the user's task is complete. The FinishTurn protocol is explicitly excluded from this release.
