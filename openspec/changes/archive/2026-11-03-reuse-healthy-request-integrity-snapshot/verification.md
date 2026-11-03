# Verification

Commands used with `GROW_HOME=/tmp/grow-c4-home` and
`RUST_MIN_STACK=16777216`:

```text
cargo test --locked -p chat-state --lib request_time_ -- --test-threads=1
```

Result: 2 passed. This covers request-time repair retry and the two
cancellation points, using only confirmed ACK prefixes in cold replay.

```text
cargo test --locked -p chat-state --lib final_request_projection_accounts_for_goal_shadows_and_json_schema -- --test-threads=1
```

Result: 1 passed. This verifies request-only Goal shadowing, persisted memory
context, full request/cold projection equality, and no second event for the
same reminder.

```text
cargo test --locked -p chat-state --lib healthy_request_integrity_clone_cost_probe -- --ignored --nocapture --test-threads=1
```

Result: 1 passed; the raw samples are recorded in [measurement.md](measurement.md).
The ignored timing test and its fixture helpers were then removed from the
build; the source is retained in [probe.rs](probe.rs).

```text
cargo test --locked -p chat-state --lib
```

Result: 608 passed, 0 failed, 3 ignored. This ran after the temporary timing
probe and helpers were removed. It includes the existing
`build_request_repairs_dangling_tool_calls`, durable memory retry, and durable
`push_user` coverage.

Formatting and whitespace validation completed with:

```text
rustfmt --edition 2024 crates/codegen/chat-state/src/actor/tests.rs crates/codegen/chat-state/src/actor/request_builder.rs
git diff --check -- crates/codegen/chat-state/src/actor/tests.rs crates/codegen/chat-state/src/actor/request_builder.rs openspec/changes/reuse-healthy-request-integrity-snapshot
```

Independent review and strict OpenSpec validation remain for the parent
integration pass. No C4 heap/RSS measurement was run; see the memory boundary
note in [measurement.md](measurement.md).

归档前strict全量19/19、归档后archived795/795通过，任务全部完成。
