## Why
Primary-only cross-session inquiries exist, but delegated agents cannot ask their parent for clarification and a parent cannot inquire about or steer a running child.

## What Changes
Reuse the existing asynchronous tool-free Sideband inquiry and single correlated UI row for child-to-parent and parent-to-child questions. Separately expose parent-to-child intervention with immediate soft interruption or queued next-step delivery. Children cannot send intervention messages upward. Cross-session inquiry remains primary-only. Route through the existing coordinator's immediate-parent authority, not arbitrary session IDs. Preserve durable evidence, cancellation, Goal ownership and replay.
