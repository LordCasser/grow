# Verification

- `cargo update --workspace --offline`: updated the 33 workspace packages to 2.2.1; dependency versions were unchanged.
- `cargo metadata --locked --offline --format-version 1`: passed; the CLI package version is 2.2.1 and all 57 workspace members resolve.
- Release notes link check: passed; links are absolute and the compare link targets `v2.2.0...v2.2.1`.
- `openspec validate 2026-09-28-prepare-v2-2-1-release --strict --no-interactive`: passed.
- `openspec validate --all --strict --no-interactive`: passed; 15 items validated before archive.
- `openspec validate --archived --no-interactive`: passed; 542 archived changes validated before this change is archived.
- `git diff --cached --check`: passed.
- No runtime tests were run because this change only updates release metadata and documentation.
