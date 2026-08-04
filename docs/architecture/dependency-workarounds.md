# Dependency Workarounds Audit

Every retained workaround in the dependency graph, with the hard evidence that
blocks its removal. Keep this list in sync when the blockers change (upstream
releases, toolchain bumps, fork rebases).

## 1. `rustls-no-provider` on reqwest 0.13 (mcp crate)

**Where**: `crates/codegen/mcp/Cargo.toml` — `reqwest = { version = "0.13",
default-features = false, features = ["rustls-no-provider", ...] }`.

**Why it exists**: reqwest 0.13's `rustls` feature is hardcoded to
`__rustls-aws-lc-rs` (verified in reqwest 0.13.4's `Cargo.toml`:
`rustls = ["__rustls-aws-lc-rs", "dep:rustls-platform-verifier", "__rustls"]`).
There is no ring-based TLS feature in reqwest 0.13. rmcp's `reqwest` feature
enables `reqwest?/rustls` the same way. The only way to use rustls without
aws-lc-rs is `rustls-no-provider`, which compiles no crypto provider and
requires the application to install one at runtime.

**Why it must stay**: dropping `rustls-no-provider` (i.e. enabling `rustls`)
re-introduces aws-lc-rs — a cmake/nasm C build that the workspace removed in
commit `f58ff2c` ("drop aws-lc-rs from the graph") because it breaks
cross-compilation for the OpenHarmony target. `Cargo.lock` currently contains
zero aws-lc-rs packages.

**Mitigation**: one shared helper, `diagnostics::tls::install_ring_provider_once`
(single definition; idempotent). Called from: CLI startup
(`cli/src/main.rs`), pager leader-cluster tests
(`pager/src/app/leader_cluster/mod.rs`), mcp test binaries (lib + integration
`repro_sse_flood.rs`), and the shell e2e tests
(`tests/{git_contention_e2e,session_load_perf,test_leader_soak,
test_registry_churn,test_session_load_memory}.rs`). There is no other
`install_default()` call site in the tree.

**Unblock condition**: a future reqwest/rmcp release exposing a ring-based TLS
feature, or a decision to accept aws-lc-rs (re-evaluate the OHOS constraint).

## 2. `[patch.crates-io] nix` → `third_party/nix-ohos` (0.26.4)

**Why**: pprof 0.15.0 (the latest release) requires `nix ^0.26`. nix 0.26.x
predates OpenHarmony support, so the vendored copy patches
`target_env="ohos"` to behave like musl.

**Blocking evidence**: pprof 0.15.0's `Cargo.toml`:
`nix = { version = "0.26", default-features = false, features = ["signal", "fs"] }`.
Remaining nix versions in the graph are also upstream-locked:
- 0.28.0 ← portable-pty 0.9.0 (= latest, requires nix ^0.28)
- 0.29.0 ← mac_address 1.1.8 (= latest, requires nix ^0.29)
- 0.31.3 ← workspace crates + vendored nono

**Unblock condition**: pprof releasing with nix 0.28+/0.30+ (native OHOS), then
the patch and vendored copy can be deleted.

## 3. `[patch.crates-io] nono` → `third_party/nono` (=0.53.0)

**Why**: nono 0.54.0 through 0.71.0 require Rust 1.95; the workspace is pinned
to 1.92.0 (`rust-toolchain.toml`). v0.53.0 is the newest compatible release.

**Blocking evidence**: nono 0.54.0's declared `rust-version = "1.95"` (see
`third_party/nono/Cargo.toml` VENDORING NOTES).

**Unblock condition**: toolchain bump to >= 1.95 (then re-vendor or return to
the registry).

## 4. `[patch.crates-io] sqlite-vec` → `third_party/sqlite-vec` (=0.1.10-alpha.4)

**Why**: every published package containing the upstream musl typedef fix
(0.1.10-alpha.2 through alpha.4) omits four C implementation files included by
`sqlite-vec.c`; crates.io 0.1.9 lacks the musl fix. The vendored copy restores
the files byte-identical to the upstream tag (see
`third_party/sqlite-vec/Cargo.toml` VENDORING NOTES).

**Unblock condition**: a crates.io release that both contains the musl fix and
packages every included C file.

## 5. mcp's reqwest 0.13 quarantine

**Why**: rmcp 2.1 (the MCP protocol client used by the mcp crate) requires
`reqwest >= 0.13.2`, while the rest of the workspace (11 crates) used reqwest
0.12. The mcp crate intentionally quarantined 0.13 (see its `description`).

**Status (2026-08-04): RESOLVED.** The async-openai fork was rebased onto
upstream 0.41.3 (which uses `reqwest = "0.13"`) with the `ReasoningEffort::Max`
patch re-applied (fork: `https://github.com/LordCasser/async-openai`, rev
`a2bae99`); the workspace's 13 reqwest crates migrated to reqwest 0.13.4 with
`rustls-no-provider` (see §1), and `reqwest-middleware` moved 0.4.x → 0.5.x.
`Cargo.lock` now contains a single reqwest 0.13.4. The only remaining
`reqwest 0.12.28` entry is jsonschema 0.30's dev-dependency (locked but never
compiled; disappears when jsonschema is upgraded).

**Note**: the fork upgrade does NOT remove `rand 0.9` (async-openai 0.41.3
still requires `rand = "0.9"`) or `nom 7.1` (still `eventsource-stream =
"0.2"`). Switch back to upstream async-openai once upstream ships
`ReasoningEffort::Max`.

## 6. Upstream-locked version exceptions (verified 2026-08-04)

These multi-version packages remain after the phase 2/3 consolidation; every
non-latest version is pinned by an upstream crate that is itself at its latest
release:

| Package (non-latest) | Held by | Evidence |
|---|---|---|
| rand 0.9.5 | async-openai fork | fork `Cargo.toml`: `rand = { version = "0.9", optional = true }` enabled by the `_api` feature (used via `responses`) |
| nom 7.1.3 | eventsource-stream 0.2.3 | eventsource-stream 0.2.0–0.2.3 all require `nom ^7.1`; 0.2.3 is the latest |
| signal-hook 0.3.18 | crossterm 0.29.0 | crossterm 0.29 (latest) declares signal-hook 0.3 as an optional dep, enabled by its `event-stream` feature (used by pager/pager-minimal/markdown) |
| rustix 0.38.44 | procfs | procfs (Linux-only `/proc` crate) pins rustix 0.38 |
| thiserror 1.0.69 | portable-pty 0.9.0 (via filedescriptor), termwiz 0.23.3, reqwest-eventsource / reqwest-middleware 0.4.x (paired with reqwest 0.12), anstyle-syntect | all consumers are upstream-locked releases |
| fixedbitset 0.4.2 | termwiz 0.23.3 (dev-dependency) | termwiz 0.23.3 is the latest release |
| nix 0.28.0 | portable-pty 0.9.0 | portable-pty 0.9.0 is the latest release |
| nix 0.29.0 | mac_address 1.1.8 | mac_address 1.1.8 is the latest release |

## 7. Resolved items (2026-08-04)

- **serial_test 3 → 4.0.1**: previously blocked by MSRV 1.93.1 > pinned 1.92.0;
  the workspace toolchain was bumped to 1.93.1 (`rust-toolchain.toml`) to
  unblock it. All serial_test users unified on 4.0.1.
- **gix 0.83 → 0.86.0** and **gix-status 0.30 → 0.33.0**: upgraded with zero
  code changes (the used API surface was stable across the minor bumps).
- **zip 3 → 8.6.0** (`tools` crate): upgraded with zero code changes; zip 9
  remains out of reach (pre-release only).
- **reqwest 0.12 → 0.13.4** across all workspace crates + the async-openai
  fork rebase onto 0.41.3: see §5 (RESOLVED).

## 8. Test-infra workarounds (consolidated, not removable)
- **rustls provider in tests**: mcp test binaries install the ring provider via
  a `#[ctor::ctor]` because they bypass CLI startup (see #1). This is the
  documented pattern; keep using `diagnostics::tls::install_ring_provider_once`.
- **BYOK config seeding for spawn-based pager tests**:
  `ContentController::seed_llm_config()` writes a mock provider into the
  sandbox `$GROW_HOME/config.toml`. Since the provider-neutral refactor
  (095ab55), the pager's BYOK gate refuses to start without a configured LLM,
  so every spawn-based PTY test must seed it. `scroll_correctness_ptyctl.rs`
  and `scroll_matrix/session.rs` rely on it.
