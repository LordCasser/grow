# Windows child observation (exit code 259) review fix

## Scope and baseline

Independent review verified that portable-pty 0.9.0 reports a genuinely exited child as still running when its real exit code is 259 (`STILL_ACTIVE`). This increment owns only the bounded correction of that observation path: `PtyChild::try_wait` on Windows, the one windows-sys feature it needs, one Windows-only session test plus its registration hunk, and this record. Server/CLI sources, main specs, backlog, developer docs, and the original owner's session rework remain outside; Unix observation is unchanged; no PID reopen, no second waiter, no Session owner or protocol change.

Baseline was the parent-released PB-03 owner implementation (`session.rs`, `pty.rs`, `wait.rs`, manifest, `Cargo.lock` already carrying windows-sys 0.59.0). `git status` showed exactly those released modifications plus the untracked change folder; no unclaimed edits were present. Skills read: software-engineering entry (Rust principles not needed beyond the entry). Project inputs: root AGENTS, design.md "Review 修正：Windows child 观察" section, verification-session.md format.

## Decisive platform evidence

All read from the locked registry sources of the exact versions in `Cargo.lock`:

- `portable-pty-0.9.0/src/win/mod.rs`, `WinChild::is_complete`: `GetExitCodeProcess` status `== STILL_ACTIVE` (259) → `Ok(None)`; `res == 0` (observation failure) → also `Ok(None)`. Both a real 259 exit and an actual API failure are indistinguishable from "running", so the PB-03 owner never settles such a child and `wait_for_exit` never resolves.
- Same file, `WinChild::wait`: after `try_wait` returns `None` it blocks on `WaitForSingleObject(handle, INFINITE)` and then `GetExitCodeProcess` → `ExitStatus::with_exit_code(status)`. With the handle already signaled this returns immediately with the real code, 259 included — so reusing `child.wait()` after scheduler-confirmed termination is correct and bounded.
- `portable-pty-0.9.0/src/lib.rs`, trait `Child`: `#[cfg(windows)] fn as_raw_handle(&self) -> Option<RawHandle>` (no default); `WinChild::as_raw_handle` returns the owned process handle. Public surface, no dependency fork needed.
- `windows-sys-0.59.0`: `WaitForSingleObject` is declared in `Windows/Win32/System/Threading` (module gated by feature `Win32_System_Threading`); `WAIT_OBJECT_0`, `WAIT_TIMEOUT`, `WAIT_FAILED`, `WAIT_EVENT` are in `Windows/Win32/Foundation` (feature already enabled). `HANDLE = *mut core::ffi::c_void` equals `std::os::windows::io::RawHandle`.
- `portable-pty-0.9.0/src/cmdbuilder.rs`, `append_quoted` (ArgvQuote): a command argument containing spaces is double-quoted for CreateProcessW, so the PowerShell fixture passes its `-Command` string as one argument.

## Implementation

- `ptyctl/src/pty.rs`: `PtyChild::try_wait` dispatches per platform. Windows (`try_wait_windows`): missing handle → explicit `Err`; `WaitForSingleObject(handle, 0)` on the trait-provided raw handle — `WAIT_TIMEOUT` → `Ok(None)` (running), `WAIT_OBJECT_0` → the same child's `wait()` reaps the real `u32` (259 preserved), `WAIT_FAILED` → `last_os_error` as `Err`, any other status → explicit `Err`. Unix block keeps the previous body verbatim.
- `ptyctl/Cargo.toml`: windows-sys gains `Win32_System_Threading`. Features are not recorded in `Cargo.lock`; the 0.59.0 entry already exists, so no lock change is expected (parent verifies in the window).
- `ptyctl/src/session_windows_tests.rs` (new) and the registration hunk at the end of `ptyctl/src/session.rs` (`#[cfg(all(test, windows))] #[path = "session_windows_tests.rs"] mod windows_tests;`).

## Test fixture

Real PowerShell child (`-NoProfile -NonInteractive`) prints `PTY259-LAST-LINE`, then `[Environment]::Exit(259)` terminates the actual process with `STILL_ACTIVE` as its exit code. Assertions through the public session surface: bounded `wait_for_exit()` returns 259, `is_alive()` false, `status` exit_code 259, the final screen keeps the fixture line, and non-linger natural completion lets `wait_for_shutdown()` finish. The 60-second outer bound turns the pre-fix behavior (unbounded polling) into a visible failure on a Windows host.

## Verification plan — prepared for the parent's exclusive Cargo window

```sh
# 1. Windows cross-check: compiles the cfg(windows) lib path AND the cfg(all(test, windows)) fixture.
env PATH=/Users/lordcasser/.rustup/toolchains/1.93.1-aarch64-apple-darwin/bin:/usr/bin:/bin:/usr/sbin:/sbin CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 cargo check -p ptyctl --lib --tests --target x86_64-pc-windows-msvc --locked --offline

# 2. macOS regression: Unix path untouched; the new module is cfg'd out, so the suite must stay green.
env PATH=/Users/lordcasser/.rustup/toolchains/1.93.1-aarch64-apple-darwin/bin:/usr/bin:/bin:/usr/sbin:/sbin CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 cargo test -p ptyctl --lib --locked --offline -- --test-threads=1

# 3. Lock untouched check (feature-only manifest change).
git diff --stat Cargo.lock

# 4. On a real Windows host, when available (only the windows module exists under --lib there):
cargo test -p ptyctl --lib -- --test-threads=1
```

Command 1 also exercises dev-dependencies for the windows target (tokio test feature); all are pure-Rust and cached offline.

## Actual checks (this executor, static only — no Cargo, no Windows execution)

| Check | Result |
| --- | --- |
| Registry-source reading listed above | Done on the locked 0.9.0 / 0.59.0 versions; every API/constant location quoted with line evidence. |
| `rustfmt` (1.93.1) over the four edited/created Rust files | Passed, no diffs. |
| `git diff --check` over the owned change | Passed. |
| `Cargo.lock` after edits | Byte-identical to the parent-released baseline (`git diff` on it shows only the pre-existing PB-03 delta; no new lines from this increment). |
| `openspec validate settle-pty-session-lifecycle --strict --no-interactive` | Passed (specs untouched by this increment). |

Not executed here: any Cargo command (the parent holds the exclusive Cargo window; commands 1–2 above are the ready-to-run gate) and any Windows execution (macOS-only host). The 259 fixture has therefore never run; the pre-fix red on Windows is asserted analytically from the `is_complete` source, not demonstrated.

## Limitations

- Cross-check proves compilation of the cfg(windows) code, not ConPTY runtime behavior; real Windows execution remains explicitly reserved, per the design's review section.
- The fixture requires Windows PowerShell on PATH (default on supported Windows); if absent, `start` fails with a clear spawn error instead of a false pass.
