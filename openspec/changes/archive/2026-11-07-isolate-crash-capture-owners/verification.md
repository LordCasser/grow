# PB-05 implementation verification

## Final parent acceptance

父已接受实现与独立数据完整性 review；最终 2.6.2 CLI 两项 `pb05_tests` 实际 **2 passed / 0 failed / 0 ignored**，测试体 0.06s，原始日志 `/tmp/grow-262-cli-crash-final.log`。验证启动循环真正排空多个 capture，以及首个恢复错误停止并保留后续证据；不是仅验证 crate helper。PB06 后最终全 crash-handler suite 31 unit、9 integration、3 ownership、1 doc 通过，详见相邻归档的 fullsuite 记录；Windows 仅有既定交叉编译，未原生运行。

## State

Implementation and local verification completed; awaiting parent and independent data-integrity review. PB-07's existing `Result<Option<CrashReport>, std::io::Error>` API and two-report publication/durability behavior are retained. Each call returns at most one report. CLI drains calls until `Ok(None)` and stops with evidence preserved on `Err`, retaining its existing policy of leaving terminal restoration installed without starting capture after recovery failure.

The exclusively granted PB-05 Cargo window is released. No command or fixture remains running, and this executor will not edit crash lifecycle source further while the parent reviews it. CLI tests are synchronized but unexecuted, as required by the parent; they belong to the later CLI integration gate.

## Inputs and exact change ownership

Before old-red, `cmp` verified both shared `src/lib.rs` and `src/handler.rs` against `/tmp/grow-pb05-baseline.sSCBxG/src/`, which includes the user's PB-07 work. The initial scoped user diff is `/tmp/grow-pb05-baseline.sSCBxG/user-existing.diff`. For exact attribution, `baseline-tree/` reconstructs HEAD source plus that pre-existing diff. `/tmp/grow-pb05-baseline.sSCBxG/pb05-only.patch` contains only this task's source delta in five files: lifecycle lib/handler, existing integration path adaptations, new ownership fixture, and CLI recovery/helper tests. No manifest or lock edits, commits or archiving were performed.

The following initial SHA-256 values were rechecked unchanged after Cargo:

- `crates/codegen/crash-handler/Cargo.toml`: `a9204eb00560a2765677e242937b10b4c650a887fcfbfa81cf517dc093b0347b`
- root `Cargo.lock`: `1ee8824c63417b5f5cdb63263278e2b7840f31021816ac342461c34ca431e1c1`

Initial source SHA-256 before old-red:

- `src/lib.rs`: `3dbed5ac77f441dc83117cdc3f64c95d4a0fa036e18dfe1f6d706ab2b237f7f4`
- `src/handler.rs`: `8c22d8348f97a32d3b0465927762c92cad0312bc5eaf5243e115d2e63f16d83e`

`src/format.rs` and `src/symbolicate.rs` remain byte-identical to the initial snapshot. Main specs/docs/backlog/default capture configuration are untouched.

## Implemented ownership boundary

Installation exclusively creates an unpublished private `.capture-<16 alphanumeric>.bin`, locks its File using std `File::try_lock`, then publishes `capture-<identity>.bin` with `persist_noclobber`, returning the same File. Recovery cannot enumerate the unlocked creation interval. Unix transfers that exact close-on-exec descriptor through `IntoRawFd`; Windows uses normal sharing and `IntoRawHandle`, allowing recovery to open and distinguish lock contention from genuine PermissionDenied. Signal/exception writing keeps its existing operations and closes the owning handle after the final write. Repeated installation retires the prior owning handle.

Recovery holds `.recovery.lock` across deterministic selection, raw claim/read, primary/history publication, retention and consumption. A raw handle is exclusively claimed nonblocking before any read. Only std `TryLockError::WouldBlock` is treated as unavailable; other I/O errors propagate. Live captures are neither read nor consumed, inactive empties are cleaned while claimed, foreign/unpublished names are ignored, and the former `last-crash.bin` is accepted only for existing evidence recovery. History names include timestamp and capture identity (stable `legacy` identity for former raw), and PB-07's retention protection of the just-published history is preserved.

## Actual Tier 3 old-red / new-green

All Cargo commands used the exact environment prefix:

```sh
env PATH=/Users/lordcasser/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216
```

Long commands ran in the background with wrapper timeout disabled. No Shell or CLI Cargo build was run.

1. **Valid old-red, exit 101:** `cargo +1.93.1 test --locked --offline -p crash-handler --test ownership -- --nocapture` against the verified old PB-07 source. Compilation and subprocess readiness succeeded. Three behavior assertions failed; the direct A/B regression was `B startup truncated live A`, with A's valid marker changed to `[]`. Concurrent capture recovery returned zero identities instead of two; inactive empty cleanup after exec also failed. Raw log: `/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b462-4c59-7af0-8fd6-86c16d2f4918/terminal/call_0440094763894f74a0c6fe7c9257a57b.log`.
2. **Same fixture new-green, exit 0:** identical command and fixture, after applying lifecycle hunks: **3 passed, 0 failed, 1 ignored fixture entry**. Raw log: `/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b462-4c59-7af0-8fd6-86c16d2f4918/terminal/call_2d6a1019f46b4bd8aae63739b9ad3e05.log`.
3. **Crate regression, exit 0:** `cargo +1.93.1 test --locked --offline -p crash-handler -- --nocapture`: **29 unit + 7 existing integration + 3 ownership + 1 doctest passed**, with two intentionally ignored subprocess entry points. Raw log: `/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b462-4c59-7af0-8fd6-86c16d2f4918/terminal/call_11000b82c3a642f98e21d94b91e498ef.log`.
4. **Windows compile, exit 0:** `cargo +1.93.1 check --locked --offline -p crash-handler --target x86_64-pc-windows-msvc --lib --tests`. Raw log: `/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b462-4c59-7af0-8fd6-86c16d2f4918/terminal/call_ef3cf296ffa8499a926be4ea399f0b12.log`. The existing PB-07 `publish_owner_only` builder has an `unused_mut` warning under Windows because its mutation is Unix-only; no warning was promoted to an ownership failure.

The real process fixture uses isolated temp directories, readiness/command pipes and guards owning precisely their actual spawned process. A and B hold live captures; B startup preserves A's bytes/inode and creates a distinct file, recovery leaves live A and empty B untouched, A gets a real SIGABRT, and C consumes only A. The same-second case supplies two valid completed captures and starts two real recovering processes together; observed returned identities and distinct history contents prove no duplicates or overwrite. The exec case proves the installed descriptor does not extend its lock across exec. Fatal signals and failure cleanup target only owned fixture processes; core dump creation is disabled inside them.

Unit checks additionally cover recovery-lock contention, live malformed/empty raws, exact filename recognition, legacy success/malformed/primary/history failure and retries, new-capture publication retry identity, owner-only report modes and retention. Unreadable raw returns actual PermissionDenied with bytes preserved. A history directory with write/search but no read permission permits report rename and then rejects the actual directory sync; the test confirms the history file exists, raw bytes remain unchanged, and retry succeeds. Both permission tests actually passed under the ordinary macOS user (root would explicitly skip them).

## Static checks and coverage limits

`rustfmt +1.93.1 --edition 2024 --config skip_children=true` and scoped `git diff --check` passed after final formatting. Final `openspec validate --all --strict --no-interactive` plus scoped `git diff --check` returned exit 0: **31 passed, 0 failed**. Raw log: `/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b462-4c59-7af0-8fd6-86c16d2f4918/terminal/call_09bc8f78537342eabaeda8bcdb1c2bd4.log`.

Native runtime evidence is macOS aarch64. Windows has compile coverage only; no Windows or Linux runtime is claimed. CLI's `pb05_tests::startup_crash_recovery_drains_multiple_captures` and `pb05_tests::startup_crash_recovery_stops_on_error_and_preserves_pending_evidence` are pending the parent-owned CLI build. Independent review, main documentation/spec integration and archive remain parent-owned gates.
