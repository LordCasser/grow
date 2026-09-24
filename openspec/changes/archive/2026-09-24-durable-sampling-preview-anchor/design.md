# Design

Reuse the existing ACP event envelope with an empty text chunk and candidate identity metadata as a storage-only anchor. The first candidate's event ID is its exact-append key; the candidate body never enters this envelope. At most one anchor is sent per attempt. The persistence actor appends ordinary untagged notifications before/after that anchor as they arrive and retains only an attempt key and sticky append failure, not notification bodies.

Before `push_response_durably_with_identity`, flush the actor replay buffer, then send an ordered persistence barrier. The barrier confirms durable writes up to the terminal candidate or returns a sticky error. The turn fails closed before canonical admission if it cannot confirm the marker or any independent update. Projection commit appends the projection at the physical tail; raw/typed reconciliation uses the candidate anchor to place canonical text at the first candidate position between independent updates. On discard, the storage-only anchor is suppressed in replay.

Any ordinary ACP merge across the attempt boundary must be flushed before writing the anchor. Avoid holding an unbounded merged text notification by limiting its payload size separately. Tests cover long interleaving, candidate-only flood, failed marker/untagged append, discard, and cold/resident replay ordering.
