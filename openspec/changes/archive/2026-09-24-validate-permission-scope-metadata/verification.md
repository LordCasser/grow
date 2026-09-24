# Verification

- `cargo test -p workspace permission::prompter::tests::bash_scope_metadata_must_be_a_prefix_of_the_request_command` passed.
- `cargo test -p workspace permission::prompter::tests::mismatched_mcp_scope_metadata_falls_back_to_current_tool` passed.
- `cargo test -p pager app::acp_handler::tests::permissions::mcp_scope_option_metadata_must_match_request_identity` passed. The linker emitted its existing `__eh_frame section too large` warning; test execution passed.
- Root/child ownership and raw session identity review: `find_permission_session_match` delegates to exact session matching and has no startup fallback; `dispatch_permission_select` validates the selected option against the exact front request before popping or applying always-approve side effects. This closes the remaining ownership/raw-session clause in `openspec/backlog.md`.
