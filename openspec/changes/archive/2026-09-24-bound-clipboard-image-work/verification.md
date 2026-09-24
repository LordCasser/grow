# Verification

- `cargo test --locked --offline -p client-support --lib -- --test-threads=1`: 79 passed, 4 ignored (real clipboard access). This includes pure RGBA cap and encoded PNG exact/overflow tests on macOS.
- Repository-pinned Rust 1.93.1 cross-target `cargo check --locked --offline -p client-support --target x86_64-unknown-linux-gnu`: passed. Linux-only helper capture and worker-permit tests compile; they could not run on the macOS host.
- `cargo fmt --all -- --check`, `git diff --check`, `openspec validate --all --strict --no-interactive`: passed.
- After archive: `openspec validate --all --strict --no-interactive` passed (16/16), and `openspec validate --archived --strict --no-interactive` passed (465/465). The subsequent workspace format check observed one concurrent epoch-change formatting diff outside this change; the epoch change owner is correcting it.
- Platform RGBA allocation before arboard returns and macOS `NSData` allocation before its byte check remain outside this change; the backlog retains those boundaries explicitly.
