## Tool and preset checks

- The Tools `subagent` unit-test selection passed 45 cases. Coverage includes omitted list scope, active and all-state observations, timed-out and unauthorized observations, repeated single-child result retrieval, and active children returning their state without waiting.
- The Agent preset test confirmed that Grow Build exposes `get_subagent_result`. The coordination tool gate test confirmed that primary session discovery is available to the primary Agent and excluded from subagent toolsets. The built-in registry taxonomy test confirmed the new result lookup is a read tool.

## Canonical evidence checks

The two Shell history-observation tests, cold authorized lookup test and live peer discovery test passed. The invalid historical link remains visible as invalid evidence in all-history scope. Active scope uses the admitted parent Ended fact to filter it without checking the child link. Peer discovery returned the other online primary session and excluded the source.
