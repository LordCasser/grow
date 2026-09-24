# 验证

- OrbStack Ubuntu (Linux 7.0.14, aarch64): `CARGO_TARGET_DIR=/home/lordcasser/grow-sandbox-target cargo test --locked --offline -p sandbox --lib -- --test-threads=1 --quiet` — 72 passed. Includes the real fork/exec regression proving connected TCP/UDP descriptors do not survive restricted exec and approved state pipes remain usable.
- OrbStack Ubuntu: `CARGO_TARGET_DIR=/home/lordcasser/grow-sandbox-target cargo check --locked --offline -p tools -p shell` — passed, compiling Linux launch call sites.
- `cargo fmt --all -- --check` and scoped `git diff --check` — passed after formatting the changed Rust file.
- `openspec validate --all --strict --no-interactive` — passed (15 active specs/changes). `openspec validate --archived --strict --no-interactive` — passed (460 archived changes), including this change.
