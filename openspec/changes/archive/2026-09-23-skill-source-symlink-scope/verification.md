# Verification

- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p agent --lib prompt::skills::tests --quiet` — 98 passed, 0 failed.
- Added focused Unix regressions: `user_home_root_alias_keeps_user_scope_inside_repository`, `repo_descendant_link_to_external_skill_is_user_scope`, `user_descendant_link_into_repository_does_not_promote_scope`, and `local_root_alias_keeps_in_root_skills_local_but_downgrades_external_links`.
- The change preserves lexical paths stored on discovered entries, canonicalizes only for identity and source classification, and leaves traversal order/depth/cycle detection unchanged.
