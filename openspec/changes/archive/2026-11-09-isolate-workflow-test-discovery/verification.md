# Verification

## Source and product boundary

Implementation commits: `d90aecf3` and `f78cc3d7`. Integrated source is `ed2027c0d4dab5a517f97971af5fee084ae244e0`, tree `bf0f734ad420b8fbe9d30a7d053ca9678ffeee22`. This is test-only maintenance. Production discovery, snapshots and the 512 KiB manifest limit are unchanged.

## Full remote Shell library execution

The Core library step in [run 42504739402](https://github.com/LordCasser/grow/actions/runs/42504739402/job/128804818308) passed on tested commit `5655f951ae4b295472d3b248c11cb4d51c8b1691`, whose tree is exactly `bf0f734ad420b8fbe9d30a7d053ca9678ffeee22`. PR head was `a3768b3b077b59695314d0fe58e7782181bc9fab`.

- Full Shell library: **4,794 passed, 0 failed, 13 ignored**.
- `session::workflow::`: **148 passed**, including the Unix adversarial catalog control.
- `session::actor::tests::workflow_launch_tests::`: **3 passed**.
- `session::actor::tests::runtime_restore_tests::`: **2 passed**.
- Adjacent `mcp_catalog_lifecycle_tests::`: **17 passed**.

These are test-execution results, not merely a successful build or enumeration. The remaining Settings/CLI/PTY stages and the overall CI result are separate; this record does not label the entire workflow successful from the library step alone.

## Separate scratch-only polluted-environment control

The complete local Shell test binary was killed during code generation in three attempts. Integrated `cargo clippy -p shell --lib --tests` passed type checking, but those attempts executed no tests. To obtain the additional fixed-artifact pollution control without repeating that full build, a separately authorized harness uses the exact committed Shell source and the existing cached dependency artifacts.

### Mechanical trimming boundary

- Extract the full committed repository with `git archive ed2027c0d4dab5a517f97971af5fee084ae244e0`, preserving relative `include!`/`include_str!` inputs.
- Keep all 14 files below byte-identical: every file under `session/workflow/`, plus the three selected actor test files.
- A Rust-aware lexical scan skips strings, character literals, raw strings and nested comments. In other Shell Rust files, replace only real `#[test]` and `#[tokio::test(...)]` attribute spans with equal-length whitespace, preserving newlines. Exactly **4,661 unrelated registration attributes in 346 files** are removed. No function body, production item, helper, module, `cfg(test)` branch, assertion or selected test is rewritten or deleted.
- Verify all **471 Shell Rust source files** against the committed originals, allowing only those recorded attribute spans to differ. Recheck selected file hashes after compilation.
- Invoke the original Rust 1.93.1 `rustc --test` command directly, with its same **113 cached `--extern` artifacts** and native library paths. Source and output paths are remapped to scratch, metadata/output names are unique, and the existing 512 codegen-unit setting is retained. Only dead-code/unused-import/unused-variable diagnostics introduced by deregistering tests are suppressed. `CARGO_PKG_VERSION=2.6.2`, `CARGO_PKG_NAME=shell` and the scratch `CARGO_MANIFEST_DIR` provide Cargo's compile-time values. No Cargo invocation, dependency rebuild, repository source edit or shared-target write is performed.
- This is a trimmed targeted test executable, **not** a full local Cargo/Shell suite or substitute for the independent full remote CI execution above.

Source archive SHA-256: `45786670b1d1036f5b3a94bea325bac85c86e700a38161e099824ae3e8b4504d`.
Selected-file hash manifest SHA-256: `85452728fbaefba036a4c29667d6306ea537124c2518f94d82205dd4742e1fb2`.
Provenance manifest SHA-256: `e38099ff46f424ad09791d4a71c934a8ed1528650b017ed2b58d4c67894d6140`.

### Preserved selected source hashes

Paths are relative to `crates/codegen/shell/src/`; every SHA-256 below matches both the committed source and the targeted harness.

| Source | SHA-256 |
| --- | --- |
| `session/actor/tests/mcp_catalog_lifecycle_tests.rs` | `778667fb42ec68e8d3660021a8c48eb7a5e962f5ee148a541ad8de2109116126` |
| `session/actor/tests/runtime_restore_tests.rs` | `d3d2872d8a1fe4b64956f7c84a76d088d27cfbc7768f11211f4590ac64e2f2b2` |
| `session/actor/tests/workflow_launch_tests.rs` | `d19e0425f2ef47e6665c98e9eee1efbfcbc89b210b488ec0b20be294dcd303f8` |
| `session/workflow/deep_research_tests.rs` | `2d63edd428ffb218113aec8772adf7d5e65d71e01d12c3561c2cf68e3b1d7b4b` |
| `session/workflow/host_service.rs` | `a12a4dab5ef12e6dd3862db4d7def569e15244eddb411b38634b32ebcada3b94` |
| `session/workflow/manager.rs` | `7e0435354ec92dcbe62c5b5de6c52ae2f60a59c9d0ce108b4cad0f9eea5b6b29` |
| `session/workflow/mod.rs` | `0613c37e7ea528749752b4cf2670fec78ff0883c05d996047df6b254c8b8e1c3` |
| `session/workflow/notify.rs` | `04f458b928938d742034f21578824898c84267945fc6c4ef51649b1d1ceb016e` |
| `session/workflow/registry.rs` | `34d1bddaff363736dce5ab2b2055512ce53fe6da8f45542be349445f5f4048f4` |
| `session/workflow/schema_contract.rs` | `a45706c6a3c3c1088ce49d16086fbab2d71d184f441f48487656ebfc9b5f5041` |
| `session/workflow/store.rs` | `1afcbf6d8a42f7c1073e03cdfc403ab8912ba24274e09c9e3292375e31a386b4` |
| `session/workflow/test_support.rs` | `d8eadc057feff97da8c19673740dbc5501c5a448d7ef58d16a7ada33413a6aa1` |
| `session/workflow/tracker.rs` | `2aef83d145f7c3e367370fce0a8b28340a46c47a93cc3aa2a3bd3d29bd965cde` |
| `session/workflow/workspace.rs` | `b7476b6d17c5506505f22e3e026a4bdec11288cfb0888baf11ba6304f5b3ce8d` |

### Execution results

The targeted executable compiled successfully in **129.84 s**, peak compiler RSS **7,510,840 KiB**. It registers exactly **170 tests**: Workflow 148, launch actor 3, restore actor 2 and MCP catalog 17. The complete enumerated name set exactly matches those groups in the full remote Shell run. Post-build verification again confirmed all 471 source files match the committed originals except the recorded unrelated registration spans; all 14 selected files remained byte-identical.

Fixed executable SHA-256: `01c0586456cbba867ac236e520133fe5a6451601320549eae7abd336e08b7061`.

The independent pollution driver creates a temporary HOME/GROW_HOME containing a **600,000-byte skill body** and a user Workflow named `project-only`, which collides with the existing folder-trust fixture. It keeps that outer environment unchanged between phases. The initial control explicitly sets only the test helper's exact-name child marker to bypass isolation for `session::workflow::manager::tests::launch_completes_and_updates_tracker`; the subsequent phases omit that marker and run normal isolation.

| Fixed-artifact selection | Result |
| --- | --- |
| Exact launch, isolation bypassed | **0 passed / 1 failed, exit 101**, `Workflow manifest exceeds 524288 bytes` |
| Same exact launch, normal isolation | **1 passed / 0 failed, exit 0** |
| Full `session::workflow::`, 4 test threads, polluted outer environment | **148 passed / 0 failed / 0 ignored, exit 0** |
| `workflow_launch_tests::`, 4 threads, same polluted outer environment | **3 passed / 0 failed, exit 0** |
| `runtime_restore_tests::`, 2 threads, same polluted outer environment | **2 passed / 0 failed, exit 0** |
| Preserved `mcp_catalog_lifecycle_tests::`, 1 thread, separate clean temporary HOME | **17 passed / 0 failed, exit 0** |

This is a **same-post-fix-artifact unisolated-path versus isolated-path control**, not a claim that a historical pre-fix full Shell artifact was rebuilt. The production manifest guard is identical in both phases. The full polluted Workflow run also executes the new nested regression: its first child proves actual discovery sees a user Agent, an oversized user Skill, a user Workflow and a workspace-user Skill; its isolated child excludes them while still discovering an explicit local Agent and local Skill body. Existing oversize-manifest rollback assertions remain active and pass.

The trimmed suite's passing counts are limited to these 170 preserved tests. The independent full remote Shell execution provides broader regression evidence; neither result is substituted for the other.

## Platform and historical limits

The new adverse-home control is Unix-only. `dirs` 6 on Windows uses `SHGetKnownFolderPath(FOLDERID_Profile)`, not USERPROFILE. Agent's `_with_home` and Skills' `_with_roots` helpers are private, and no existing test-support feature exports them to Shell. The subprocess sandbox still owns GROW_HOME and its child environment on Windows, but this change does not claim Windows Agent/Skill user-source isolation or Windows native execution.

The synthetic pollution control proves this test isolation boundary. It does not measure the exact serialized field contribution of the historical developer's resources or claim Skills alone caused the original 25 failures.

## Repository checks

- `cargo fmt --all -- --check`: passed on the integrated-source closure worktree.
- `git diff --check`: passed before integration and after the verification documentation update.
- Before archive: strict OpenSpec validation passed **23 current items** and archived validation passed **924 items**. After archive: strict validation passed **22 current items**, archived validation passed **925 items**; no product spec was changed (`skip_specs: true`).
