# Verification

- Confirmed `config::grow_home()` resolves `GROW_HOME` inside a process-wide `OnceLock`; the previous `serial(GROW_HOME)` scope did not coordinate with untagged readers or first cache initialization.
- `cargo test -p pager --lib session_title_resolve -- --nocapture`: 15 passed.
- `cargo test -p pager --lib resume_by_title -- --nocapture`: 3 passed.
- `cargo test -p pager --lib trust_folder_ -- --nocapture`: 2 passed.
- `cargo test -p pager --lib 'cta_e2e::' -- --nocapture`: 47 passed, including all child-isolated CTA persistence cases.
- `cargo test -p pager --lib mouse_reporting_toggle -- --nocapture`: 2 passed.
- `cargo test -p pager --lib slash::commands::theme::tests -- --nocapture`: 16 passed.
- The focused runs used the default parallel test harness. The four commands above launched 18 exact-test children with `GROW_HOME` set at process start: 6 session-title tests (`pin_title_resume_finds_saved_profile_and_conflicts`, `materialization_consumes_pinned_id_after_concurrent_rename`, `pin_ambiguous_title_errors_before_sandbox`, `pinned_no_match_does_not_retry_title_after_sandbox`, `pinned_non_uuid_id_is_not_reinterpreted_as_title`, `duplicate_legacy_id_title_pin_keeps_the_cwd_scoped_profile`); 3 startup tests (`title_fallback_resumes_single_match_case_insensitively`, `id_hit_beats_title_fallback`, `worktree_defer_flags_local_miss_and_local_hit_does_not`); 2 trust tests (`trust_folder_grants_and_resolves`, `trust_folder_rejects_confirmation_period_replacement`); and 7 CTA tests (`no_reload_path_enters_awaiting_mcps_directly`, `no_auth_path_settles_installed_without_modal`, `skills_only_install_settles_installed_without_fetch`, `install_error_settles_error`, `reload_error_settles_error`, `mcps_error_settles_error`, `plugin_cta_catalog_load_recomputes_match_for_typed_draft`). The runner removes the `pager::` prefix from `module_path!()` for libtest's exact filter and asserts the child output contains `test <name> ... ok`; this catches a successful `0 tests` child as a failure.
- `git diff --check`: passed.
- `openspec validate --all --strict --no-interactive`: passed (19 items).
- Existing linker emitted the known macOS compact-unwind table size warning. No full Pager suite or ignored leader-cluster scenarios were run; the latter remain documented in the narrowed backlog entry.
