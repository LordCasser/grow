# Verification

## Reproduction

- macOS arm64, Rust 1.93.1, branch `codex/v2-4-0`, draft version 2.4.0.
- Both new suspend PTY cases fail before any provider request. Existing `continue_resumes_session_with_history` also fails at its first response.
- `/tmp/grow-v2-4-0-pty-root-unified-diagnostic3.jsonl`: queue drain, checkpoint start, then error after 7 ms: `input intent is absent from the local draft snapshot`. Root Timeline contains startup facts only, with no InputSubmitted or model request.
- Logs: `/tmp/grow-v2-4-0-pty-suspend5.log`, `/tmp/grow-v2-4-0-pty-baseline-diagnostic.log`. These are pre-fix diagnostics; the completed verification below supersedes their pending status.

## Fix and post-fix verification

- `DraftSnapshot::from_agent` no longer permits an old disk draft to restore over a snapshot with `in_flight_prompt`; an empty composer alone does not prove no newer input exists.
- Pinned Rust/Rustdoc 1.93.1 Pager suite: `session_bind_does_not_restore_staged_cwd_prompt_over_first_send_checkpoint` **passes**, using the real serial DraftIoWorker, CWD staging, SessionCreated dispatch and exact session-key input-intent checkpoint. `/tmp/grow-v2-4-0-pager-regression3.log`. That full suite had unrelated failing UI fixtures and is not recorded as a whole-suite pass.
- Real `continue_resumes_session_with_history` **passes** (1/1, 11.00 s), using the rebuilt 2.4.0 CLI and a fresh process: `/tmp/grow-v2-4-0-pty-baseline-after-bind.log`.
- Current Pager library passes **7495 tests, 0 failed, 12 ignored** in `/tmp/grow-v2-4-0-pager31.log`, including the real draft worker/session-binding regression. Shell's final run passes **4219 tests, 0 failed, 5 ignored** in `/tmp/grow-v2-4-0-shell-final82.log`.
- CLI73 (`/tmp/grow-v2-4-0-cli-build73.log`) and persistence harness76 pass every invocation in `/tmp/grow-v2-4-0-pty77-results.json`: ordinary history continue, root Ctrl+C/cold continue/export, nested root/child/grandchild cold continue/export, three partial-leaf-release runs and concurrent cold resume. The first input reaches its provider in each scenario, with its original prompt identity and no replacement submission. The source ledgers remain unchanged by export.
- Commands: pinned `cargo build --locked -p cli --bin grow`; the PTY executable `pty_e2e_persistence-29e142b3b220b870` runs from `crates/codegen/pager` with absolute `PAGER_BINARY=target/debug/grow`, `RUST_MIN_STACK=16777216`, each named test, `--ignored --test-threads=1 --nocapture`. Exact names and output are recorded in the seven `pty77-*.log` files. Developer explanation is linked in `docs/development.md` under local draft/session binding.
- Final checks: pinned Pager `cargo fmt -p pager -- --check` passes (`bind-fmt83.log`); this change's strict validation passes (`bind-strict83.log`), and full strict validation passes **24/24** (`openspec-all83.log`). The existing worker and real-process cases cover the delta before archive.
