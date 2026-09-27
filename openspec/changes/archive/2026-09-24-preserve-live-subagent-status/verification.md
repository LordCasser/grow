# Verification

The reported parent Timeline contains a `SubagentSpawned` fact while the child Timeline was still active. The old Shell forwarded a higher-ID Goal refresh before that spawn, and Pager's Grow highwater discarded the spawn. This matches the empty running Tasks pane in the screenshots.

- `cargo test --locked -p shell --lib goal_owned_subagent_spawn_reaches_live_client_before_goal_refresh -- --nocapture`: passed. The live spawn precedes the Goal refresh and shares its event ID with the persisted projection.
- `cargo test --locked -p shell --lib emit_subagent_notification_sends_one_metadata_only_parent_command -- --nocapture`: passed. Parent commands are unstamped until actor publication; direct fallback remains stamped and metadata-only.
- `cargo test --locked -p pager --lib late_subagent_lifecycle_preserves_running_status_and_live_cursor -- --nocapture`: passed. Late spawn/finish are applied once, retain the higher highwater/cursor, and do not revive a terminal child.
- Existing Pager regressions `grow_session_update_dedup_drops_already_applied_event` and `nested_subagent_lifecycle_registers_flat_descendant_route`: passed.
- `cargo fmt -p shell -- --check`, `cargo fmt -p pager -- --check`, `git diff --check`, and `openspec validate --all --strict --no-interactive`: passed. An initial workspace-wide formatting check exposed a pre-existing unrelated formatting difference in `crates/codegen/workspace/src/file_system/fuzzy.rs`; this change leaves that file untouched.
- `cargo build --locked --profile release-dist --features release-dist -p cli --bin grow`: passed in 9m 13s. `cp ./target/release-dist/grow ~/.local/bin/grow`: passed. Both files have SHA-256 `e8de8b354ab0dbc21fd79f4d465ba3a5f00482323b78276f7eed817c6ade22c8`; installed binary reports `grow 2.2.0 (b51a1007)`.
- `cargo clean --profile test` removed 9.5 GiB of test build artifacts before the release build.

The already-running TUI process still has the previous executable mapped and is not updated by replacing the file on disk. Reattaching or restarting with the installed binary is required to exercise the fix in that live session; the active child was not interrupted for this verification.
