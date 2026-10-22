# Reuse validated descendant recovery

## Why

P1 removed most unneeded Markdown parsing, but complete load finalization still spends about242ms rereading the root Timeline in a2048-turn text fixture. Descendant recovery additionally reads each parent once for ownership/enrichment and again for its history projection.

## What Changes

Reuse ownership facts from the same already-validated root load frontier, and derive each descendant's ownership and ordered lifecycle/control projection from one validated read. Bind reuse to exact session and load boundary; do not infer ownership from UI rows or retain an ambient cross-load cache. Preserve durable validation, provenance, history order, TUI state and the existing complete-load ready barrier. No SQLite, checkpoints, pagination or early readiness.

## Verification

Measure fresh release before/after with the same modern long root history and separate valid child-tree fixtures; retain failure evidence, review adversarial paths, archive independently and remove only the dedicated build target.
