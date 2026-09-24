# Bound subagent lifecycle projections

## Why

The subagent emitter sends `SubagentSpawned` and `SubagentFinished` directly to an unbounded gateway queue while also cloning them into the parent actor command queue. `SubagentFinished.output` can repeat the full child answer even though the child result artifact and completion receipt already own that content. During a parent sampling attempt this bypasses the session preview budget and can retain large redundant strings.

## What Changes

Make parent lifecycle Grow notifications metadata-only and let the parent actor persist and forward the same stamped event. The actor's existing active-attempt gateway credits and independent persistence acknowledgement then cover these events. Keep the canonical child result and completion receipt as the source of final output.
