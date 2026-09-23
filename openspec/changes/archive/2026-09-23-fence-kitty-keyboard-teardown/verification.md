# Verification

## Scope and ownership

Implementation is confined to the pager terminal lifecycle, the shared terminal probe, the stderr lock helper, and the opt-in PTY test. Existing unrelated modifications in the dirty worktree were preserved. Before this change, `event_loop::run` detached its crossterm reader and normal restoration joined `WriterThread` without a deadline, emitted Kitty pop, then drained events for a 10 ms quiet window. `init_terminal` successful connection failure has no reader (`ReaderJoin::Absent`) but may have pushed flags; `init_terminal` itself failing goes through quick best-effort teardown without DA1.

## Completed checks

| Check | Result | Evidence |
| --- | --- | --- |
| `cargo test --locked -p pager-render --lib pop_fence -- --nocapture` | exit 0, 5 passed | 7-bit/8-bit DA1 after residue; DA2/CSI-u/partial rejection; failed query never reads; real socket EOF; partial reply deadline. |
| `cargo test --locked -p pager --lib kitty_fence -- --nocapture` | exit 0, 1 passed | Joined/Absent reader ownership, timed-out reader, writer and pop gating. |
| Existing pager test binary: `app::reader_thread::tests` | exit 0, 2 passed | Absent reader and blocked reader deadline. |
| Existing pager test binary: `app::tests::restore_runs_teardown_even_when_writer_failed` | exit 0, 1 passed | Teardown after drain failure, fence disabled. |
| Existing pager test binary: `app::tests::bounded_teardown_does_not_wait_for_stalled_stderr` | exit 0, 1 passed | Stalled teardown helper returns within its grace. |
| `cargo build --locked -p cli --bin grow` | exit 0 | Updated Grow CLI binary available for PTY checks; only macOS compact-unwind linker warning. An earlier attempt exited 101 while parallel manifests and Cargo.lock were temporarily inconsistent; no unrelated lock edit was made here. |
| `openspec validate fence-kitty-keyboard-teardown --strict --no-interactive` | exit 0 | Change syntax and delta valid. |
| `git diff --check` on Kitty-touched tracked files; isolated `rustfmt --check` with `skip_children=true` on Kitty Rust files | exit 0 | No whitespace or formatting errors; no unrelated module formatting applied. |
| Initial `PAGER_BINARY=.../target/debug/grow cargo test --locked -p pager-pty-harness --test kitty_pop_fence -- --ignored --nocapture --test-threads=1` | exit 101, 0/5 passed | Harness setup error: `ContentController::start()` creates a fresh sandbox without a model config. Grow stayed at the BYOK prompt and never emitted the startup Kitty probe. Fixed harness by calling `seed_llm_config()` at each spawn; this was subsequently rerun. No Kitty runtime assertion was reached. |
| `first_sigint_uses_normal_fence` after seeding mock config | exit 0, 1 passed | Confirmed startup Kitty probe/push, pop before DA1, first OS SIGINT via normal fence and exit 0. |
| Full PTY rerun after config fix | exit 101, 4/5 passed | Late release, silent DA1 and both OS SIGINT paths passed. No-flags child had already exited after the first Ctrl+C, so a redundant second injection hit closed PTY (EIO), before the final assertions. |
| `no_kitty_flags_sends_no_teardown_query` after conditional second Ctrl+C | exit 0, 1 passed | Final exit 0 and no pop/DA1 query confirmed; quit helper now injects the second key only if child still runs. |
| Final full PTY rerun: `PAGER_BINARY=.../target/debug/grow cargo test --locked -p pager-pty-harness --test kitty_pop_fence -- --ignored --nocapture --test-threads=1` | exit 0, 5 passed | Late Kitty release consumed before shell, silent DA1 bounded, no flags no pop/query, first SIGINT normal fence, second SIGINT forced exit 130 without another fence. |
| `cargo test --locked -p pager --lib panic_hook_is_bounded_and_skips_da1 -- --ignored --nocapture --test-threads=1` | exit 0, 1 passed | Isolated child process invokes the real panic hook. Available stderr: Kitty pop emitted and no DA1. Locked stderr: hook returns within the bounded grace and no DA1; child exits while retaining the lock so a detached helper cannot write after the test. Parent enforces a 2 s child deadline. Initial compile failed only because the test used an unavailable direct crate path; corrected to existing `shell::util::stderr_lock` re-export. |

The pager test binary was rebuilt after the 10 ms exit-drain and `init_terminal` early-error adjustments; focused tests were rerun against it. An initial broad `rustfmt --check` recursively traversed unrelated dirty modules and produced unrelated formatting diffs; no formatter write was applied. The subsequent isolated check used `skip_children=true` and passed.

## Archive integration

- `openspec validate --all --strict --no-interactive`: exit 0, 16/16 before archive.
- `openspec archive fence-kitty-keyboard-teardown --yes`: exit 0; added one `client-surfaces` requirement under `2026-09-23-fence-kitty-keyboard-teardown`.
- Archived task 3.4 was checked only after archive completed; full current validation passed 15/15 and archived validation passed 370/370 (both exit 0).

## Remaining limits

DA1 has no request ID, so an extremely late earlier DA1 reply cannot be distinguished cryptographically. Typeahead during the owned reply window is consumed, not re-injected. A writer or stderr owner wedged in an uninterruptible write may still emit late output after its bounded join/teardown grace; the implementation skips DA1 and proceeds with best-effort restoration in that case.

The panic hook is exercised in an isolated pager test child rather than an interactive PTY, because intentionally stalling the process-global stderr lock in a shared test runner could leave a detached helper writing terminal escape sequences after a test finishes. The actual forced-signal branch is separately covered by the second-SIGINT PTY scenario.
