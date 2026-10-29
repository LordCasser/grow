## Verification

- Root-coordinated `leader_pty_e2e` run, `/tmp/leader-pty.log`: 5 passed.
- Root-coordinated `test-support` Core run, `/tmp/affected-final.log`: 71 passed.
- Dedicated real-daemon owner regression, `/tmp/daemon-owner-final.log`: 1 passed. It observes the concrete Grow daemon PID and verifies that both normal `TestProcess` drop and simulated panic/unwind followed by wait/reap report `ECHILD`; it does not infer process termination from sandbox or socket removal.
- `rustfmt --edition 2024 --check crates/codegen/pager-pty-harness/src/leader.rs`: passed.
- `git diff --check`: passed.
- `openspec validate --all --strict --no-interactive`: passed in the documentation closeout run (33 items).
