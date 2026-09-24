# Preserve memory flush tool evidence

## Why

Memory flush currently prepares its Sideband input through the simplified compaction projector. That projector removes every tool result. A completed action whose only confirmation is a tool response can therefore be stored as an unexecuted plan or omitted from cross-session memory.

## What changes

- Freeze the current Timeline Surface and its identities before building a memory-flush request.
- Project completed, unambiguous tool exchanges with their result text and attachments into the read-only memory-flush request.
- Select a recent whole-turn window under an explicit request and attachment budget, and record the frozen source revision and selected Surface coordinates in the Sideband attempt.
- Treat tool output as untrusted historical data; memory flush does not supply tools or turn its output into authorization evidence.

This change does not alter generic compaction's simplified projection.
