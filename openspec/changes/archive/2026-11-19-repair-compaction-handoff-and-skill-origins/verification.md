## Incident evidence

The reported session was recorded on v2.6.16. At #17679 a full Surface repair invalidated the range frozen by the still-running background summary. The old scheduler signaled cancellation and tried to start its replacement before the first transaction's terminal. With a blocked HTTP summary, the regression reproduced the same `compaction … already active` error on v2.6.17.

The same recording crossed its 80% preventive threshold (380,000) while its next request still fit the 475,000-token model. Before the fix, a real `handle_prompt` test with an explicit output reserve reproduced the turn failure without a primary provider call.

At #17679 the repaired branch has two pending loads, at offsets 856 and 857. After settling those at #17682 the fixed projection has no pending loads. At #17699, with 76,000 retained tokens and a 5,000 minimum, the actual planner selects items 326 through 677 inclusive, covering 141,960 source tokens. Formerly five consumed skills were incorrectly pinned and the planner returned no range.

The committed overbroad second settlement at #17701 remains historical evidence. Full replay through all 18,758 events and restoration from a #17682 checkpoint plus the suffix produced the same 1,013-item final Surface. The load probes use isolated copies of the session.

The separate 18:09 HTTP 409 contains a bridge error. Original request evidence shows the `wait_subagent` tool-use ID and its returned ID match. A same-route auxiliary info request occurred in between. The bridge's active-call state is outside this journal.

## Storage and client verification

The original production resume entry successfully opened a schema-52 copy via its deferred migration path. A separate eager index rebuild exposed a pre-existing ContextRecall validation error at #11937, frozen through #11936: the validated reader omitted diagnostic observations, but recall required the retained vector length to equal the original sequence frontier. After the fix, the eager writer opened and published schema 53. A fresh production resume on that migrated copy returned an indexed checkpoint with 10 retained events and the same final Surface as dense replay. The entire 80,441,705-byte canonical journal matched the original on both copies.

Unit coverage includes the response-confirmed and fresh skill cohort in both dense and checkpoint projections, unchanged ordinary load results, repeated settlement, explicit rejection of invented historical removals, and recall rejection of bad coordinates/revision/frontier. A mock HTTP summary overlaps actual full-Surface integrity repair. The writer test holds the old terminal acknowledgement and exercises successful and failed ownership transfer. A fitting primary request with explicit output reserve sends exactly once, while hard overflow, provider rejection and manual compaction keep their existing admission rules.

## Automated checks

Pinned Rust 1.93.1 with the locked workspace dependencies. Full library tests passed for Shell (4,883 passed, 14 ignored), ChatState (679 passed, 3 ignored) and Pager (7,747 passed, 15 ignored). The default CLI passed `cargo check --locked -p cli --bin grow`. Rustfmt and `git diff --check` passed. The real-data test harness, isolated copies and its temporary executable were removed after verification.
