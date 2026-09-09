# Goal token accounting audit

## Screenshot arithmetic

The supplied daily breakdown contains 1,075,974,016 cache-read input tokens, 5,795,175 uncached input tokens and 1,967,031 output tokens: 1,083,736,222 total. Excluding cache reads leaves 7,762,206. Therefore the large magnitude discrepancy is explained by comparing different accounting units; the remaining difference cannot be assigned without aligned provider request records and capture boundaries.

## Implementation

`crates/codegen/shell/src/session/goal_tracker.rs::model_usage_goal_tokens` computes `prompt_tokens.saturating_sub(cached_prompt_tokens) + completion_tokens`; reasoning is already included in output. The sideband and summary-compaction charge paths use the same basis. GoalState::tokens_used is a durable cumulative budget charge, not full provider token traffic or current context length. `crates/codegen/pager/src/views/goal_detail.rs` renders this as unqualified `Usage`, which fails to explain the excluded cache reads. `usage_incomplete` renders the lower-bound prefix ≥.

## Fixed-prefix reconciliation

Read-only source: session `01a081be-6168-7772-9e0c-dcc62a76b552`, Goal `01a081e1-153d-77a1-b6c5-c15839761b90`. Freeze Goal control deltas through Timeline seq 43889; inspect request completion records through seq 43892 to include the immediately following completion for the last charge. The running session was not modified.

| Component | Charged tokens | Evidence |
|---|---:|---|
| Main completed requests | 4,947,908 | One-to-one matching positive Goal deltas to immediately following completed request usage, same amount and within 9 sequence entries |
| Compaction 1 | 567,852 | Control seq 18414; sideband 01a08432-9079-78f2-8c16-04dc220da3bf, 562,972 input − 512 cache + 5,392 output |
| Compaction 2 | 546,390 | Control seq 31123; sideband 01a084a5-2db6-7080-b4c6-2c9ca951e7a6, 540,777 input − 512 cache + 6,125 output |
| Compaction 3 | 542,880 | Control seq 42076; sideband 01a0850a-cefe-7393-864a-1963ffbce99b, 536,734 input − 512 cache + 6,658 output |
| Failed response with known usage | 1,070 | Control seq 23557, followed by RequestFailed seq 23558 |
| Total | 6,606,100 | Exactly equals Goal tokens_used at seq 43889 |

No negative Goal delta was observed in the inspected prefix. No completed main request admitted while Goal was active was left unmatched. All remaining positive deltas are the three compactions and the one known-usage failure above. This establishes local consistency for known recorded usage; it does not prove that every provider attempt reported complete usage.

Some normal/paused-session calls are outside Goal ownership. Requests missing provider usage remain unknown, not zero; this Goal has usage_incomplete=true. Provider totals cannot be equated to this fixed Goal prefix from screenshots alone. No claim is made that the residual 7.762M versus 6.6M difference is fully reconciled with the provider.

## Validation

Source inspection and fixed-prefix arithmetic only; no Rust changes or Cargo build needed. Strict OpenSpec validation passed: 17 current specs/changes and 298 archived changes, 0 failed. Runtime display/metric changes await the user's choice of accounting and budget basis.
