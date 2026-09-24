## 1. Contract and implementation

- [x] 1.1 Add the configuration-rules delta for the 1 MiB file-backed read limit, over-limit error, and preserved `load_skill_content` snapshots; verify `openspec validate bound-explicit-skill-body-reads --strict --no-interactive` passes.
- [x] 1.2 Share a bounded source-read helper between `load_skill_content` and `load_skill_with_body`; verify exact-limit and over-limit cases with `cargo test --locked -p tools explicit_skill_body_read`.

## 2. Verification and archive

- [x] 2.1 Run `cargo test --locked -p tools explicit_skill_body_read` and record results in `verification.md`; verify the tests cover full acceptance at the limit and whole-body rejection above it.
- [x] 2.2 Validate and archive the change; verify `openspec validate --all --strict --no-interactive` and `openspec validate --archived --no-interactive` pass.
