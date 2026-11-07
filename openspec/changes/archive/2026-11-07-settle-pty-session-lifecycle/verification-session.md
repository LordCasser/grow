# PB-03 Session owner verification

## Scope and baseline

This increment owns only ptyctl Session/PTY/wait implementation, crate-local tests and this change's Session implementation evidence. Server, CLI, Pager harness, main specs, developer docs, backlog, archive and commits remain outside this increment.

Initial inspection found no pre-existing ptyctl source changes. The shared workspace has many unrelated changes, including Cargo.lock; these are preserved. Baseline source edits added only three old-red tests to the original Session implementation; production and manifest changes waited for the parent's exclusive Cargo window. The final implementation changes only ptyctl Cargo.toml, session.rs, pty.rs and the obsolete wait-handle TODO, plus this change's Session design/evidence. No server or CLI source was edited.

Skills read: software-engineering entry, Rust principles and handoff contract. Project inputs read: root AGENTS, OpenSpec README, client-surfaces spec, this change's design/tasks/delta and complete-nondeferred-backlog/pty-lifecycle-preparation. `rg --files -g AGENTS.md` found only the root instruction file.

## Decisive platform evidence

- Locked portable-pty 0.9.0 `src/lib.rs:114`, `src/unix.rs:352`: Unix actual master fd is available; reader and writer are dup streams. Dropping a master cannot interrupt a remaining dup. Shared `O_NONBLOCK` requires retry/partial-write/stop handling on both streams.
- Locked `src/win/conpty.rs:95`: opaque boxed reader; no raw stream getter. Owned Rust thread handles permit cancellation of this Session's actual synchronous I/O workers.
- [CancelSynchronousIo](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-cancelsynchronousio): requests cancellation, returns ERROR_NOT_FOUND if no pending operation, does not await completion. Repeated requests cover the stop-check/read race; only actual join proves retirement.
- Locked `src/win/psuedocon.rs:73` calls ClosePseudoConsole on Drop. [ClosePseudoConsole](https://learn.microsoft.com/en-us/windows/console/closepseudoconsole) requires continuing output drain during close; before Windows 11 24H2 close may wait indefinitely if output is not drained. This requires a distinct owned master-close worker while reader and FIFO consumer continue, and actual join of that worker. Official page fetched directly after connector timeouts; remarks were read in `/tmp/pb03-closepseudoconsole.html:760`.
- Original `PtyChild::is_alive` discarded try_wait errors; Session reader set alive=false on EOF and thereby caused its separate waiter to skip real exit collection. Actual spawned child handle must be polled/killed/waited serially by the same owner.

Platform implementation details and parent-confirmed failure ownership are recorded in design.md. The autonomous owner thread holds child and all worker join handles on its stack until actual settlement; completion does not claim the launcher thread itself was joined. A one-second drain/five-second child-stop failure is explicit Err while the original owner retains responsibility.

## Verification plan

Tier 3 uses real directly spawned children, rather than fabricated terminal flags:

1. Old-red on original production source: output EOF before child exit; final output paired with real exit code 37; explicit stop must reap the actual direct child.
2. New-green through public Session/SessionControl: the same two cases, stop, repeated stop, cancelled stop/exit observers, zero and original timeout, natural linger followed by stop.
3. A real Python fork descendant ignores HUP, confirms the inherited slave with fstat(0), prints a marker and signals a pipe before its direct parent exits with 19. It holds the slave for three seconds. Kernel EOF may permit success; otherwise bounded drain must return explicit Err. Both outcomes require independent generation closure proving actual I/O join. A separate terminal-lock fault holds final FIFO past one second: observers get Err while generation remains open; after releasing the lock, final output is delivered and generation closes, while the failure remains observable.
4. A four-MiB write to a child that does not read forces partial writes/WouldBlock. Stop must retire that same writer under the child owner.
5. Cancel start after the actual fixture PID appears while its start future remains unobserved; the original owner must still reap its child.
6. Destroy the starting Tokio runtime and stop through a replacement runtime; drop the last external handle and verify child/I/O settlement independently of runtime lifetime.
7. Run existing crate tests for screen, resize, keys and wait behavior, then local diff/format and OpenSpec checks appropriate to this Session increment. Overall strict/archive gates remain parent-owned.

Prepared Cargo commands, only after parent grants the exclusive window:

```sh
env PATH=/Users/lordcasser/.rustup/toolchains/1.93.1-aarch64-apple-darwin/bin:/usr/bin:/bin:/usr/sbin:/sbin CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 cargo test -p ptyctl --lib lifecycle_ --locked --offline -- --test-threads=1
env PATH=/Users/lordcasser/.rustup/toolchains/1.93.1-aarch64-apple-darwin/bin:/usr/bin:/bin:/usr/sbin:/sbin CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 cargo test -p ptyctl --lib --locked --offline -- --test-threads=1
```

## Final shared interface

`PtySession::control_handle(&self) -> SessionControl` returns a cloneable handle; Session stop now takes `&self` and delegates to it. The control operations are `stop(&self).await -> anyhow::Result<()>`, `wait_for_exit(&self).await -> anyhow::Result<u32>` and `wait_for_shutdown(&self).await -> anyhow::Result<()>`.

Stop succeeds only after actual direct child settlement, I/O worker joins and FIFO delivery. It admits its stop flag before awaiting, so cancelling the observer cannot undo responsibility. Wait-for-exit is observation without a new stop. Shutdown follows non-linger natural completion or explicit stop/original timeout, including after natural linger. Status alive/exit_code describe the actual child, and are not the complete service terminal signal; Service must use the control interface. Errors remain observable and cannot become a fabricated successful shutdown. Master retirement makes Windows resize return an explicit error once the actual master is taken.

## Actual checks

All Cargo commands used the fixed 1.93.1 toolchain PATH, `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216`, background mode and timeout 0. They ran only after the parent granted the exclusive window. The only non-locked command was the expressly permitted offline dependency-edge refresh.

| Check | Actual result |
| --- | --- |
| Original production, `cargo test -p ptyctl --lib lifecycle_ --locked --offline -- --test-threads=1`, task `01a1b466-87de-7e71-9554-d41f9136f26d` | Expected failure: 0/3 passed. Lost exit 23, stop acknowledged still-running child, and final output had None instead of 37. Compiled in 64s; tests 1.08s. |
| Strengthened EOF fixture on original production, `cargo test -p ptyctl --lib lifecycle_eof_does_not_retire_child --locked --offline -- --test-threads=1`, task `01a1b468-b852-7d13-a029-2e04f13b3599` | Expected failure at 0.21s: Python closes all fds, sleeps one second, and original reader fabricated child retirement before the actual child exit. This specifically proves the early-EOF case. |
| Initial implementation, `cargo test -p ptyctl --lib lifecycle_ --offline -- --test-threads=1`, task `01a1b46e-3f16-7ba1-93bc-8909c8f52e50` | 11/12 passed. Initial shell-descendant test incorrectly required an Err, but the actual result was true code 19. The fixture was strengthened to actual fork/pipe handshake and the existing contract's bounded real retirement was asserted without forcing a platform EOF behavior. |
| Final implementation, `cargo test -p ptyctl --lib --locked --offline -- --test-threads=1`, task `01a1b470-a9fd-72e2-87f3-d93caf3bc21b` | Passed: 29/29, including 13 lifecycle cases and all 16 existing crate checks. Build 3.47s; tests 9.13s. |
| `git diff --no-index /tmp/pb03-cargo-baseline.lock Cargo.lock` | Exactly two added dependency lines in local ptyctl: libc and windows-sys 0.59.0. No other lock change relative to the window-start baseline; external package versions, sources and checksums unchanged. |
| Fixed 1.93.1 rustfmt and `git diff --check` over the owned change | Passed. |
| `openspec validate settle-pty-session-lifecycle --strict --no-interactive` | Passed. Global strict/archive remains parent-owned. |

The crate emits one `unused_mut` warning from unchanged `server.rs:263`, because Session stop now takes `&self`. Removing it belongs to the following Service consumer edit; no server code was changed here.

Windows cross-compilation task `01a1b471-1776-7473-bc7a-953835b49ee1` (`cargo check -p ptyctl --lib --target x86_64-pc-windows-msvc --locked --offline`) completed successfully, exit 0 in 177.58s. It was the last Cargo command of this window. Cargo and PTY production scope were explicitly released to the parent immediately afterward; no further Cargo or production edits are planned by this executor.

Actual execution was on aarch64 macOS only. Windows was not run, and cross-compilation cannot prove ConPTY cancellation/drain behavior. Independent lifecycle review and Service/CLI product integration remain parent-owned; this increment is pending review.
