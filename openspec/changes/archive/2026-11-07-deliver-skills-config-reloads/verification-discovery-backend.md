# macOS discovery backend verification

## Parent final runtime evidence

父接管后使用显式 PATH 和 `cargo +1.93.1`，先输出 `rustc 1.93.1 (01f6ddf75 2026-02-11)`；真实 task `01a1b4de-8950-70e0-b9ec-7adb05231fe6` exit0。`cargo test -p shell --lib config::watcher:: --locked --offline -- --test-threads=1` 实际 **22 passed / 0 failed / 0 ignored**，测试体 35.22s，原始日志 `/tmp/grow-262-watcher-final.log`。先前预估23项已由实际22项校正，不能计零测试或估计数为通过。全模块含两项新增以及 resident config/source/recreation 等相邻回归。

父已读全部实际 diff：仅私有 discovery backend/debouncer 及两消费者类型，macOS metadata polling、timeout interval 和 compare_contents(false) 与冻结设计一致，未修改通用 fsnotify。Pager 九项与两项跨层回归另由父统一执行；此处不先声称它们通过。

## Scope and implementation

This increment fixes the shared discovery registration owner in `crates/codegen/shell/src/config/watcher.rs`. The private `DiscoveryWatchBackend` selects `notify::PollWatcher` on macOS and `notify::RecommendedWatcher` elsewhere. Both `SkillsFileWatcher` and `ProjectDiscoveryWatcher` use `DiscoveryDebouncer`. Polling uses the existing debounce duration as its interval and explicitly disables content comparison. Access filtering, event classification, watch planning, registration failure results and filesystem-identity refresh remain unchanged.

`ConfigWatchBackend` / `MacConfigWatcher`, MCP ownership, generic fsnotify, dependencies and public interfaces are outside this increment and were not edited. Existing regression names and time limits are retained.

The native failure evidence remains `/tmp/grow-ac06-native-child-92817.sample.txt:652–782`: discovery registration waits in `FsEventWatcher::watch`, while FSEvents threads wait in `FSEventStreamStart → register_with_server → mach_msg`. The accepted design also records that an isolated cwd control still failed and first native start blocked. This implementation does not batch native registration or add a timeout/fallback state machine.

## Checks

- `rustup run 1.93.1 rustfmt --edition 2024 crates/codegen/shell/src/config/watcher.rs`: passed.
- `git diff --check -- crates/codegen/shell/src/config/watcher.rs`: passed.
- `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 rustup run 1.93.1 cargo test --locked --offline -p shell --lib config::watcher:: -- --test-threads=1`: interrupted during dependency compilation after 429 seconds; no watcher test binary or test result was produced. Background task: `01a1b4d7-6d38-77f1-82d3-c29632e7cfb0`, wrapper timeout disabled. The parent inspected the actual compiler with `lsof` and confirmed `/opt/homebrew/Cellar/rust/1.99.1/bin/rustc`, so the invocation did not establish the required Rust 1.93.1 environment. The parent stopped this command and took over verification using an explicit toolchain PATH. This is an invalidated build attempt due to the toolchain environment, not a product test failure or a passing test run.

Source and Cargo ownership were released to the parent after that stop. The source remains frozen at the reviewed diff; the parent reported no findings on the private backend/debouncer/type changes and the two added tests. The expected macOS full-module selection is 23 tests, all still awaiting the parent's correctly pinned run.

The full watcher module filter includes the existing directory replacement/recreation and watch lifecycle regressions, rather than only its immediate `tests` submodule. Two necessary regressions were added:

- `custom_skill_root_observes_create_modify_and_delete` exercises the real `SkillsFileWatcher` with an explicit custom root. Its modify step explicitly advances mtime to exercise notify's observable metadata contract; creation and deletion use actual filesystem operations and the existing five-second event helper.
- macOS-only `discovery_poll_uses_metadata_without_comparing_large_file_contents` uses the production discovery debouncer with a 64 MiB sparse file. A content edit preserving mtime and length must produce no event; advancing mtime must produce an event. It also checks the selected backend kind. Source inspection of locked notify 8.2.0 `poll.rs` confirms disabled `compare_contents` leaves `DataBuilder::build_hasher` absent, so `PathData::new` does not enter `get_content_hash` or open/read arbitrary files.

## Coverage limits and integration handoff

Metadata polling observes path creation/removal and increasing whole-second mtime changes using notify 8.2.0's existing semantics; it does not promise detection of content edits preserving metadata. No native macOS registration success is claimed.

The parent owns the nine real runtime/path/reload regressions, two neighboring MCP lifecycle regressions, leader integration, final OpenSpec validation and archival. Those broader results must be recorded separately. Linux and Windows retain the prior backend and were not executed in this macOS run.
