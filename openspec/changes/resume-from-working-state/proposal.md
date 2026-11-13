## Why

Confirmed exit still copies closed history to determine suspension state. Executable resume reads the complete journal, checks historical sampling evidence, revisits settled child results, and waits for full presentation. These operations put history growth on both critical paths.

## What Changes

- Establish the initial checkpoint on session creation. Advance the recoverable working state and durable indexes during ordinary commits, then hydrate that state and apply committed tail facts on resume.
- Resolve suspension and recovery from current obligations and exact indexed identities.
- Make the latest history page available independently of older history. Keep display pagination independent of execution and live delivery.
- Compare the original and revised product paths with equivalent data at multiple closed-history sizes, including the first resume of sessions written through the normal commit path. Exercise recovery after process termination at publication boundaries and measure pre-existing unindexed journal migration separately.

## Capabilities

### Modified Capabilities

- `session-timeline`: working state, append continuity, and integrity verification boundaries.
- `client-surfaces`: readiness, ordered history pagination, and complete-history operations.
