# Verification

- `cargo metadata --locked --no-deps --format-version 1`: passed; the CLI package resolves to 2.2.0.
- Release notes link check: passed; all Markdown links are absolute and the compare link targets `v2.1.11...v2.2.0`.
- `openspec validate prepare-v2-2-0-release --strict --no-interactive`: passed.
- `openspec validate --all --strict --no-interactive`: passed; 14 current specs validated.
- `openspec validate --archived --no-interactive`: passed; 526 archived changes validated, including this release record.
- `git diff --check`: passed.
- `scripts/validate-release.sh v2.2.0`: not run; this preflight requires a clean committed tree and a local annotated tag pointing at HEAD.
- No runtime tests were run because this change only updates release metadata and documentation.
