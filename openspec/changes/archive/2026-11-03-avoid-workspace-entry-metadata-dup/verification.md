# Verification

## Baseline

Before the implementation change:

```text
cargo test --locked -p workspace file_system::client_fs::tests::list_
6 passed; 0 failed
```

The build reported existing deprecated `Atomic::fetch_update` warnings in `mcp/src/servers.rs` and `workspace/src/permission/types.rs`.

## Behavior after the change

```text
cargo test --locked -p workspace file_system::client_fs::tests::
17 passed; 0 failed
```

The client-facing `ClientFsListReq` workspace operation now has a regression covering ordinary file and directory metadata, file and directory symlink target metadata, `is_symlink`, dangling-link omission, out-of-root symlink confinement, and stable paginated concatenation. Existing tests still cover collection cap, sorted pagination, and mid-walk symlink confinement. `rustfmt --edition 2024 --check` passed for the two touched Rust files. `openspec validate --all --strict --no-interactive` passed all 18 items.

## Comparable listing probe

A temporary ignored unit probe in `crates/codegen/workspace/src/file_system/client_fs.rs` (removed after measurement) created 10,000 ordinary files and timed six calls through `ClientFsListReq::execute`, with a 1,000-entry page. The before and after probes were separate `cargo test` invocations and therefore separate test processes, each creating a fresh temporary workspace with the same fixture shape and request. Both used the same host and debug profile; file creation and setup were outside the timed region. There was no explicit warm-up: all six measured calls are reported as raw samples. The first call may warm filesystem state, and OS cache state was not controlled across processes.

Command used for each probe:

```sh
cargo test --locked -p workspace t3_large_listing_probe -- --ignored --nocapture --test-threads=1
```

The command output was returned in the tool transcript and was not saved to a filesystem log. The exact six values are copied below from those outputs:

| State | Six samples (ms) | Median (ms) |
| --- | --- | ---: |
| Before | 51.835, 52.844, 53.736, 54.450, 54.529, 58.658 | 54.093 |
| After | 41.945, 42.196, 42.230, 43.079, 43.854, 46.435 | 42.654 |

The observed median decreased by about 21%. This is one local debug-build sample, not a production throughput guarantee. Root-confinement filtering still performs its own symlink probe; the code change removes the later unconditional follow-stat for ordinary entries. Symlink-heavy listings were not measured.

The implementation preserves the existing fallback where `symlink_metadata` fails but `metadata` succeeds, and still skips links whose targets cannot be read. No OpenSpec delta is added because the intended listing behavior and public contract are unchanged.

After integration review and archival, strict all validation passed 18/18 and archived changes passed 783/783. `git diff --check` passed.
