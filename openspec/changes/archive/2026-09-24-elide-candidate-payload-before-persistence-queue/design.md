# Design

At the actor's existing ACP persist/broadcast fork, inspect the stamped notification metadata for `samplingRequestId` and `samplingAttempt`. A tagged candidate sends a compact `SamplingCandidate` marker containing those two fields to the persistence actor, while the live gateway still receives the original notification. Untagged ACP updates continue through the normal persistence path. The same FIFO channel preserves marker order relative to untagged updates and the durable projection command.

The persistence actor validates active/terminal attempt ownership for the marker and sets only the first candidate insertion index in its existing pending window. Candidate notifications arriving via its generic Update path are no longer a normal producer path; route tests through the marker. The worker still drops any tagged Update defensively if one is sent internally.
