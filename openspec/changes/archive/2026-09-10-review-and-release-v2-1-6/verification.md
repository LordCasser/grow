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

Archiving this preparation record does not claim publication. GitHub core regression, platform checks, annotated-tag preflight and the all-platform release workflow will be verified from their actual run results before publication is reported to the user.
