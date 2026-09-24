# Verification

The original two-client PTY scenario failed when viewer B loaded during A's first streamed turn. The captured Timeline had `Turn::Started`, the accepted assistant response, then `Recovery::close_interrupted_work` and `Turn::Ended(interrupted)` for that same turn; A's terminal then failed with `TurnMismatch { active: None }`. The leader log showed B's `session/load` reconnecting to the existing actor. Code tracing located the unrelated recovery append at the unconditional final `recover_interrupted_durably` call in `reconcile_orphaned_subagents_with_backend`, invoked by resident `load_session`.

After gating that call on `spawn_new_actor`:

- `cargo build --locked --offline -p cli --bin grow` passed.
- `leader_attach_during_active_turn` passed with a paced stream, asserting B loaded before the first `turn_completed`, that the real terminal was `end_turn`, and that B received A's next turn.
- `leader_two_clients_shared_session` and `leader_n_clients_shared_session` passed individually with exact-name PTY runs. Both now wait for the first durable terminal before exercising completed-history replay.
- Shell tests `recovery_repairs_receipt_after_durable_parent_terminal` and `backend_running_inspection_keeps_parent_spawn_open` passed.
- `cargo fmt --all -- --check`, `git diff --check`, and strict OpenSpec validation passed.

The three exact multi-client cases are listed in `core-regression`'s serial PTY loop. Local runs used the built `grow` binary and isolated mock-backed Git fixtures; the Ubuntu workflow itself was not run locally.
