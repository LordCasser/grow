## Verification

- `rustfmt --edition 2024 crates/codegen/tools/src/implementations/skills/skill.rs` — passed.
- `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 cargo test --locked -p tools explicit_skill_body_read -j 1` — passed; 2 focused tests, 0 failures. The exact 1 MiB file loads fully through both loaders; a file one byte over returns a path- and limit-specific error through both loaders.
- `openspec validate bound-explicit-skill-body-reads --strict --no-interactive` — passed.
- `openspec archive bound-explicit-skill-body-reads --yes` — archived as `2026-09-23-bound-explicit-skill-body-reads`; the generated main spec contains the bounded-read requirement.
- `openspec validate --all --strict --no-interactive` — passed; 19 current specs/changes validated.
- `openspec validate --archived --no-interactive` — passed; 375 archived changes validated.
- `git diff --check` for the implementation and generated main spec — passed.

Cargo ran with one job on the shared default `target/debug` directory. Available disk remained 12 GiB after the build.
