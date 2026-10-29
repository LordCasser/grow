## Verification

- Root-coordinated affected test run, `/tmp/affected-final.log`: `pager-render` reported 1,022 passed and 1 ignored. This includes `completed_leader_uses_post_exit_cleanup_after_expired_deadline`.
- The focused Unix test uses the existing `TMUX_QUERY_TIMEOUT` for its readiness check and retains the existing `POST_EXIT_CLEANUP_GRACE` and TERM/KILL sequence. It verifies readiness before leader exit and an exit-handler marker after group cleanup.
- `rustfmt --edition 2024 --check crates/codegen/pager-render/src/terminal/tmux_probe.rs`: passed.
- `git diff --check`: passed.
- `openspec validate --all --strict --no-interactive`: passed in the documentation closeout run (33 items).
