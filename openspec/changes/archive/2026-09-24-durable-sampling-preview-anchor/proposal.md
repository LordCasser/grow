# Persist a payload-free preview anchor before canonical admission

## Why

`PendingSamplingWindow` retains every untagged ACP notification until an attempt settles. A long attempt with independent notifications can grow without limit even though candidate text was already removed from persistence traffic. The unbounded persistence sender can accumulate candidate markers as well.

## What Changes

Persist one payload-free ordering anchor at the first candidate position, stream independent untagged notifications to their own exact append, and confirm the anchor and preceding writes before canonical Timeline admission. A failed anchor or independent update blocks admission rather than retaining an unbounded retry window. Later replay reconstructs accepted content from the canonical Timeline response at the anchor; discarded attempts leave no candidate body.
