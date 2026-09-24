# Verification

- `cargo metadata --locked --offline --format-version 1`: passed; the complete workspace lockfile resolves and the CLI package version is 2.2.0.
- Release notes link check: passed; all Markdown links are absolute and the compare link targets `v2.1.11...v2.2.0`.
- `openspec validate prepare-v2-2-0-release --strict --no-interactive`: passed.
- `openspec validate --all --strict --no-interactive`: passed; 14 current specs validated.
- `openspec validate --archived --no-interactive`: passed; 526 archived changes validated, including this release record.
- `git diff --check`: passed.
- Initial `scripts/validate-release.sh v2.2.0`: passed for commit `7b235f4d` and its annotated tag.
- Initial GitHub release workflow run `36015438966`: cancelled before publication after Cargo `--locked` reported stale versions for workspace packages in `Cargo.lock`. Offline Cargo metadata regenerated all workspace package versions; the corrected full lockfile now passes locked metadata validation.
- No runtime tests were run because this change only updates release metadata and documentation.
