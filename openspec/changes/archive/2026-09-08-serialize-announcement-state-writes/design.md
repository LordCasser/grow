# Design
The canonical latest hidden-ID set already lives in AppView. Add only in-flight and pending booleans, rather than another snapshot queue. request method starts an effect if idle or marks pending if busy. Completion clears in-flight and issues one current-state snapshot when pending. UI visibility changes remain immediate. TaskResult is local JoinSet completion, delivered once; no remote acknowledgement identity is introduced.

All existing producers (hide/show/update pruning) must call the same method before effects can spawn. The low-level async writer remains responsible for complete snapshots and byte bounds; it is not a global ordered writer. Completion failure keeps existing logging and still advances newer pending state. With no pending change, failure is reported without a new retry policy.

# Tests and limits
Drive real actions and completion dispatch without filesystem IO, holding completion to simulate a slow earlier write. Verify no overlapping effects, final snapshot after rapid edits, failure advancement and prune participation. Existing isolated child-process effect test covers disk errors. No shutdown flush guarantee or multi-process merge/order guarantee. AppView teardown can discard pending preference state as other async UI persistence does today.
