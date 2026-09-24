# Verification: clarify-markdown-fuzz-coverage

- Compared `README.md` and the `Cargo.toml` target comment with `fuzz_targets/render_all.rs`. The target makes four renderer calls for valid UTF-8: full and streaming with pretty enabled/disabled, always passing `None` for Syntect. Streaming cycles 1/16/32-byte chunk targets, advances chunk ends to UTF-8 boundaries, calls `push_and_render`, and does not call `finish`. Outputs are discarded, so the harness is crash/panic-oriented and has no differential or structured-property oracle.
- Confirmed the documented run forms use cargo-fuzz's target plus custom corpus directories against [`Run.corpus`](https://github.com/rust-fuzz/cargo-fuzz/blob/main/src/options/run.rs) and [`exec_fuzz`](https://github.com/rust-fuzz/cargo-fuzz/blob/main/src/project.rs); the target manifest names `render_all` and disables test/doc/bench binaries. The run commands were reviewed but not executed.
- `git diff --check`: passed.
- `openspec validate clarify-markdown-fuzz-coverage --strict --no-interactive`: passed.
- `openspec validate --all --strict --no-interactive`: the final post-archive run reported 16 passed, 1 failed. The sole failure was the concurrently active `bound-slash-mru-snapshot-writes` change; this change passed focused validation. No unrelated change was modified.
- Archived as `2026-09-23-clarify-markdown-fuzz-coverage` with `openspec archive ... --skip-specs --yes`; no spec delta was produced.
- Final `openspec validate --archived --no-interactive`: passed, 372/372 archived changes, including this change.
- `git diff --check`: passed after the archived task update.
- No Cargo build, test, or fuzz campaign was run.
