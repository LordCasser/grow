# Verification

- Regression setup: cwd `.grow` aliases the User `.grow` root; the User skill is under `.grow/shared-skill` (outside the User root's `skills/` discovery subtree); a Repo `.grow/skills/user-alias` points to it. This ensures the skill is discovered through the Repo root before any Local alias can claim the same canonical file.
- Expected behavior: the descendant leaves the Repo source root, its canonical target matches the User root, and its effective scope remains User regardless of the overlapping cwd `.grow` alias.
- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p agent --lib prompt::skills::tests::repo_link_into_user_root_is_not_promoted_by_cwd_grow_alias --quiet` — 1 passed, 0 failed.
- `openspec validate 2026-09-23-prefer-user-root-in-skill-target-classification --strict --no-interactive` — valid.
