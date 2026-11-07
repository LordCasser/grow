# Coder verification

Status: implemented, verified, accepted by parent, and locally archived. Tier 0; no behavior or persistent schema changes. No tests added.

## Change and boundary

- `crates/codegen/announcements/Cargo.toml`: replace the `tools` workspace dependency with `config`.
- `crates/codegen/announcements/src/lib.rs:209`: call `config::grow_home()` directly. `tools/src/util/grow_home.rs` reexports this same function, so path resolution and its shared authority remain unchanged.
- `Cargo.lock`: only this task's local `announcements` dependency list changes from `tools` to `config`. The preexisting `memory` dependency on `url` remains intact.

## Commands and results

All Cargo commands used `/Users/lordcasser/.cargo/bin/cargo +1.93.1` with `PATH=/usr/bin:/bin:/usr/sbin:/sbin:/Users/lordcasser/.cargo/bin`, `CARGO_BUILD_JOBS=2`, `CARGO_INCREMENTAL=0`, and `RUST_MIN_STACK=16777216`. Parent coordinated the exclusive Cargo window; it is released.

1. `cargo +1.93.1 check -p announcements --offline`: passed. This was the sole authorized command without `--locked`, allowing Cargo to refresh the local dependency edge.
2. Structural lock comparison using existing Python 3.12 `tomllib`: passed. The complete parsed lock equals the pre-command lock after replacing only `announcements.dependencies` entry `tools` with `config` and sorting that list. All 1059 packages remain; all 999 external package records are entirely equal, including version, source, checksum, and dependencies. Other preexisting dirty lock entries are unchanged. Baseline: `/tmp/grow-announcements-lock-before-01a1b44a.json`.
3. `cargo +1.93.1 check -p announcements --locked --offline`: passed.
4. `cargo +1.93.1 test -p announcements --locked --offline`: passed, 13 unit tests, 0 failed, 0 ignored; doc-tests had 0 cases. Existing persistence, FIFO admission, path and formatting behaviors were exercised by the existing suite. No new tests were introduced.
5. `rustfmt +1.93.1 --edition 2024 --check crates/codegen/announcements/src/lib.rs`: passed.
6. `git diff --check -- Cargo.lock crates/codegen/announcements`: passed. Full production diff inspected: manifest, one function call, and the local lock dependency edge only.

Cargo check/test log: `/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b44a-b96e-79a0-ad4e-35259fbe3040/terminal/call_658caf8fd473478ebbd06b309b5bdc1a.log`.

The initial system Python lacks `tomllib`; no baseline file was created by that failed attempt. Structural validation then used the already-installed Python 3.12.12 successfully without installing dependencies.

## Parent acceptance

父architect已读manifest/唯一调用/Cargo.lock三处完整diff、原tools reexport及原始check/test日志，接受Tier0行为等价和13项既有回归证据。此项不代表Agent公共seam用途核对完成。依赖收缩外没有行为、持久schema或用户配置变化；已有memory url依赖边保留。

本地归档前all strict31/31；`openspec archive depend-on-config-for-announcements --skip-specs --yes`实际exit0，归档到2026-11-07-depend-on-config-for-announcements，未改主规范。archive当时最后一项归档checkbox尚未执行，因此有一项incomplete警告；归档完成后按实际动作更新该项。后置all strict30/30、archived911/911及受影响diff check均exit0。未提交或发布。