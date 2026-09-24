# Verification

- Confirmed listed `#[ignore]` tests and their Cargo targets in `crates/codegen/pager/tests/pty_e2e/{minimal/mod.rs,show_thinking_blocks_toggle_hides_existing_pty.rs,verb_group_fold_expand_collapse_pty.rs,resize_preserves_scroll_position.rs,read_tool_header_selection_copies_path_only_pty.rs,queued_bash_promotion_renders_output_pty.rs,send_now_tip_after_mid_turn_queue.rs,esc_cancels_running_turn_from_prompt_preserves_draft.rs,esc_cancels_running_turn_from_scrollback.rs}` and `crates/codegen/pager/tests/leader_pty_e2e/{leader_two_clients_shared_session.rs,leader_n_clients_shared_session.rs}`; checked target declarations in `crates/codegen/pager/Cargo.toml`.
- `git diff --check`: passed.
- `openspec validate --all --strict --no-interactive`: passed.
- PTY tests/builds were not run; this change only documents their ignored status and invocation targets.
