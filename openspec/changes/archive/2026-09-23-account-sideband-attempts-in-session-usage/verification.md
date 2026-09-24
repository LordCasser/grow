# Verification

The owner ChatState Timeline now records an auxiliary attempt start before its provider future is polled and an immutable settlement for each `(sideband_id, attempt_no)`. Live application follows persistence ACK. A duplicate payload is a no-op; conflicting or malformed billing facts reject live application or cold restoration. On restart, an open attempt marks its originating usage segment incomplete. The frozen model route and prompt index determine attribution; the Sideband Result is not a second bill.

The shared Sideband owner settles ordinary responses with full `TokenUsage` and cost, and compaction attempts with the helper's available usage. A missing provider report is unknown unless attempt evidence proves no dispatch. A retry settles the previous attempt first. Owner drop retains exact pending payloads and the activity lease while detached settlement finishes, and marks session usage incomplete on a terminal settlement error when its actor remains available. The existing child final-bill fold remains the sole path into the parent.

Validation:

- `cargo check -p chat-state --lib` and `cargo check -p shell --lib` passed.
- `cargo test --locked --offline -p chat-state --lib` passed: 520 tests, 1 ignored; after adding the child-fold regression, `cargo test --locked --offline -p chat-state --lib usage::tests` passed 4/4. Focused coverage includes duplicate/conflicting settlement, known retry bills, late prompt attribution, child final-bill folding, cold recovery of an open attempt, and unchanged main-loop round count.
- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p shell --lib sideband::tests` passed 18/18 after the final detached error-reporting adjustment. The response tests verify Goal and session totals together, including a missing provider report; a dropped admitted Sideband marks the owner session ledger incomplete.
- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p shell --lib summary_compaction` passed: 1 test.
- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p shell --lib project_from_ledger_never_drops_incomplete_flag` passed: 1 test, including auxiliary usage in the headless projection and unchanged `numTurns`.

No live provider was called. The tests exercise the durable actor and Sideband lifecycle boundaries with controlled responses and cancellation; they do not measure upstream billing accuracy when a provider supplies no trustworthy usage.
