# Verification

- `cargo check --offline -p tools`: passed; refreshed `Cargo.lock` for the already-present `same-file` dependency.
- `cargo test --locked --offline -p tools --lib implementations::grow_build::search_replace::tests -- --test-threads=2`: 70 passed.
- `cargo test --locked --offline -p tools --lib computer::local::file_system::tests::conditional -- --test-threads=2`: 6 passed.
- `cargo test --locked --offline -p tools --lib cooperating_processes_allow_one_commit_per_source_version -- --nocapture`: passed; two child test processes read the same source before a shared release barrier, and exactly one committed.
- `cargo test --locked --offline -p tools --lib existing_edit -- --nocapture`: 2 passed; conflict and unsupported-adapter cases left bytes unchanged and emitted no `FileWritten` notification.
- `git diff --check`: passed.
- `cargo check --locked --offline -p tools -p workspace -p shell`: passed after the unrelated permission change's in-progress edits settled.
- `rustfmt --edition 2024 --check --config skip_children=true` on the four edited tools Rust files: passed.
- `openspec validate 2026-09-24-guard-local-search-replace-commits --strict --no-interactive` and `openspec validate --all --strict --no-interactive`: passed before archive.
- A focused lock-contention test holds the target lock beyond a short injected deadline and verifies timeout without changing the file.

The local adapter uses an advisory lock on the opened target file and compares exact bytes while holding it. The tests cover stale content, parent path aliasing, target replacement and separate process writers. They do not assert an impossible atomic content CAS against an external writer that ignores advisory locks.
