# Verification

Work began 2026-11-04 and continued 2026-11-05 (Asia/Shanghai). Toolchain: Rust/Cargo 1.93.1, as pinned by the repository.

## Regression and implementation

Before implementation, ran:

`cargo test --locked -p tools --lib implementations::grow_build::task::tests::json_effort_reaches_runtime_with_selected_or_inherited_model -- --exact`

It failed as intended: JSON `effort: "high"` was ignored and the dispatched request contained `None` instead of `Some(Some("high"))`.

The implementation adds the optional field to the canonical shared tool schema and maps its supplied string to the existing runtime policy. It does not change the child runner's parsing, model-capability validation, provider conversion, durable facts or UI projection. For resume, the override remains absent so source pinning continues to own the selection. The canonical `none` token is never passed through optional-argument sentinel normalization.

## Completed checks

| Command | Result |
| --- | --- |
| `cargo test --locked -p tool-types -p tools --lib task -- --test-threads=4` | 28 tool-types and 292 tools tests passed. |
| `cargo test --locked -p shell --lib agent::subagent::resolution -- --test-threads=4` | 55 passed, including existing runtime precedence and source-resolution checks. |
| `cargo build --locked -p cli --bin grow` | Passed with the default production dependency graph. |
| `GROW_BINARY="$PWD/target/debug/grow" cargo test --locked -p shell --test test_subagent_effort -- --ignored --nocapture --test-threads=1` | 1 passed; four real-process cases completed in 11.12 seconds against a loopback mock provider. |
| `cargo fmt --all -- --check` | Passed. |
| `git diff --check` | Passed. |
| `openspec validate --all --strict --no-interactive` | 17 items passed before archival. |

Tool regressions cover schema publication and optionality, JSON roundtrip, explicit high/none values, effort with a selected or inherited model, omitted effort, JSON null, and a resume call carrying an invalid effort override that must be ignored.

## Real tool-to-provider boundary

The end-to-end test launches a real `grow agent stdio` for each case in an isolated Grow home and temporary Git checkout. A scripted parent response invokes the actual public `spawn_subagent` tool with an explicit child model and effort, then lets the normal child runtime run. The selected child model advertises `none`, `high`, and a default of `max`:

- `high`: the child provider request contains `reasoning_effort: "high"`, overriding the model's `max` default.
- `none`: the request contains `reasoning_effort: "none"`; it is not dropped or replaced by the default.
- `low`: the parent receives the existing `is not offered by model` failure, and no request for the child model reaches the provider.
- `bogus`: the parent receives the existing `Invalid subagent reasoning effort` failure, and no child provider request is sent.

The test contacts only its loopback mock server. No live provider, user sessions or user repository files are involved. The Shell library build also compiles the updated Task input fixture used by existing RWX projection tests.

## Adversarial review and limits

- No effort sentinel cleanup is used: `none` remains an explicit policy and invalid strings reach the existing runtime's error path.
- Omission and JSON null retain outer `None`, preserving existing default resolution. No public field for the separate internal `Some(None)` policy was introduced.
- Effort without a model retains the existing model path. Capability validation is performed on the resolved model by the same production runner used before this change.
- Resume ignores even an invalid supplied effort before request construction; the source model/effort and existing durable source validation remain authoritative.
- No provider-specific parser, alternate runtime, new dependency, persistent schema, model-selection policy or permission policy was added.
- Runtime provider behavior was exercised through Chat Completions. This change does not modify provider conversions; it does not claim a new all-provider interoperability matrix.

The binary is built at `target/debug/grow`; existing running/installed binaries have not been replaced. No Git commit was made.

## Archival and cleanup

`openspec archive expose-subagent-effort --yes` completed with all tasks checked and merged the added model-sampling requirement. Post-archive checks passed:

- `openspec validate --all --strict --no-interactive`: 16 passed, 0 failed.
- `openspec validate --archived --no-interactive`: 846 passed, 0 failed.

Removed the 67 incremental compilation directories touched during this task, reclaiming approximately 11.27 GiB. The built CLI and normal dependency artifacts remain available.
