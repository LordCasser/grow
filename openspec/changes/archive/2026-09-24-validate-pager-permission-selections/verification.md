# Verification

- `cargo test -p pager --lib app::root::dispatch::tests::permissions::` — passed (18 tests, 0 failed). Includes the malformed child selection regression and existing valid selection/queue transition tests.
- `rustfmt --edition 2024 crates/codegen/pager/src/app/root/dispatch/permissions.rs crates/codegen/pager/src/app/root/dispatch/tests/permissions.rs` — passed.
- `openspec validate --all --strict --no-interactive` — passed (17 items, 0 failed).

The linker emitted the existing macOS compact-unwind table size warning while building Pager tests.
