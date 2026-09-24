## Verification

- `cargo test -p shell --lib completed_workflow_without_sidecar_reports_seed_usage_incomplete` — passed (1).
- `target/debug/deps/shell-c2e4cf2308dd200c session::workflow::store::tests --test-threads=4` — passed (21), including missing-seed and valid-sidecar assertions.
- `cargo test -p shell --lib workflow_restore_rebuilds_missing_and_invalid_manifests_from_timeline` — passed (1), covering the actual JSONL loader's missing and invalid sidecars.
- `target/debug/deps/shell-c2e4cf2308dd200c session::workflow::manager::tests::resume_reconciles_agents_used_from_journal_no_double_charge` — passed (1).
- `target/debug/deps/shell-c2e4cf2308dd200c session::workflow::tracker::tests::reconcile_agents_used_sets_absolute_count` — passed (1).
- `rustfmt --edition 2024 --check crates/codegen/shell/src/session/workflow/store.rs crates/codegen/shell/src/session/storage/jsonl/tests.rs` and `git diff --check` — passed.

The fallback deliberately does not reproduce mutable phase or agent rows after the only sidecar is lost. It preserves Timeline terminal state, reports incomplete usage, and continues to use the journal for resume budget admission. The linker emitted the existing compact-unwind warning; tests passed.
