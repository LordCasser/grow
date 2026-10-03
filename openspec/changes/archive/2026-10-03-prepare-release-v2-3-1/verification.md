# Verification

## Source and release identity

- Source commit `548491069af921e37ad24cb6b468a2b94972f248` contains the completed replay-surface and ordered-resume changes with both behavior changes archived in OpenSpec.
- Workspace metadata and all 33 versioned workspace packages in `Cargo.lock` resolve to `2.3.1`; `cargo metadata --locked --no-deps --format-version 1` succeeds.
- `crates/codegen/shell/changelogs/2.3.1.md` summarizes only those archived changes and uses an absolute comparison link from `v2.3.0`.
- `git ls-remote --tags origin refs/tags/v2.3.1 'refs/tags/v2.3.1^{}'` returned no existing remote tag before release preparation.

## Local validation

- `cargo update --workspace --offline` updated the 33 workspace package entries without changing third-party dependency versions.
- `openspec validate --all --strict --no-interactive`: 15/15 passed.
- `openspec validate --archived --strict --no-interactive`: 570/570 passed.
- `git diff --check`: passed.

The annotated tag and final `scripts/validate-release.sh v2.3.1` preflight must run after this change is archived and committed, because the preflight requires a clean tree with the tag at HEAD. Cross-platform builds and published assets are produced and verified by the official release workflow after push.
