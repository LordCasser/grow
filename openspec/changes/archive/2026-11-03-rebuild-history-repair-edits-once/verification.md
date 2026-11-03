# Verification record

## Differential tests

- `cargo test --locked -p sampling-types --lib one_pass_repairs_match_reverse_edit_oracles_at_result_boundaries -- --nocapture` passed. It compares the new dangling insertion and dedup functions against test-only copies of the before implementations; it includes every dangling reason, attached Model SkillLoad as a boundary, consecutive three-result dedup, and identical IDs in separate assistant runs.
- `cargo test --locked -p chat-state --lib retain_quarantine_matches_adjacent_reverse_splice_ranges_and_hides_reasoning -- --nocapture` passed. It compares full JSON and count against the old reverse-splice helper for adjacent ranges, Reasoning/BackendToolCall prefixes, and a valid sibling ID in one malformed Assistant that makes a later ID duplicate. The synthetic evidence excludes private reasoning.
- `cargo test --locked -p chat-state --lib repair_history_report_and_full_json_match_reverse_edit_pipeline -- --nocapture` passed. It compares full JSON and every report field against old quarantine/dedup/dangling passes composed with unchanged normalize/strip functions, then checks a second pass is unchanged.

## Kernel probes

- `cargo test --locked -p sampling-types --lib history_repair_kernel_timing_probe -- --ignored --nocapture --test-threads=1` passed: 40 old/new labeled measurements across dangling insertion and duplicate removal.
- `cargo test --locked -p chat-state --lib quarantine_kernel_timing_probe -- --ignored --nocapture --test-threads=1` passed: 20 old/new labeled measurements.
- Each kernel used 2 warmups and 7 samples, with fixture setup, input clone, output serialization, and equality comparison outside the timed region. Complete JSON and return counts matched in every case. Raw samples are in [`measurement-raw.txt`](measurement-raw.txt), and exact temporary probe snippets are in [`probe-sampling-types.rs`](probe-sampling-types.rs) and [`probe-chat-state.rs`](probe-chat-state.rs). The ignored probes and helpers have been removed from build test modules.

## Full relevant library tests

- `GROW_HOME=/tmp/grow-backlog-c2-home RUST_MIN_STACK=16777216 cargo test --locked -p sampling-types --lib -- --test-threads=1`: 320 passed, 0 failed, 0 ignored.
- `GROW_HOME=/tmp/grow-backlog-c2-home RUST_MIN_STACK=16777216 cargo test --locked -p chat-state --lib -- --test-threads=1`: 606 passed, 0 failed, 3 ignored.

Both full library runs were after removing the temporary probes. No archive or production-file edit was made by this test/measurement task.

Evidence JSON generation inside quarantine is timed; only comparison/output serialization is outside. No C2 allocator/RSS probe was run. Independent static review verified the old algorithm copies, report/JSON equivalence, retain/merge coordinates and the healthy unchanged premise used by subsequent C4; suite commands above were run by the test owner.

主代理归档前strict全量20/20、归档后archived794/794通过，任务全部完成。
