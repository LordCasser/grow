- [x] Make ordinary invalidation signal-only for unfinished workers and keep the original owner reachable; explicitly drain at promotion completion, manual compaction, suspension, and teardown barriers.
- [x] Carry the runtime suspension shared deadline into the drain and retain ownership on timeout; cover both dropped drain futures and an actual expired deadline.
- [x] Keep the root Goal accounting dispatcher active while shutdown drains compaction/child owners; add a same-inbox promoted incomplete-usage regression.
- [x] Add regressions proving cancelled drain futures retain and settle the original Sideband/Goal owner without Surface commit or detached settlement.
- [x] Record focused validation evidence; do not claim full-suite validation until the parent runs it.

Validation: asynchronous compaction focus75 18/0, owner/deadline cases and full Shell82 4219/0/5 ignored, fmt86; exact names and scope in verification.md.
