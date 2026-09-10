# Why

The native process harness expects a resident reload to re-emit the original `incoming inquiry` receipt. However, the model request has already started, and `pending_coordination_notices` deliberately folds Timeline facts to the latest nonterminal audit. Same-workspace approval is durably recorded before inference, so the correct snapshot is `inquiry approval` with the approved decision and no outcome. This stale test assertion predates the reviewed changes.

# What changes

Update only the fixture assertion to require one transient snapshot with the current approved state, expected source/question and no terminal result. Wait for asynchronous notification delivery before reading the snapshot. Retain the no-duplicate-inference assertion and every subsequent crash/reload scenario.

# Capabilities

No production behavior or spec changes; `skip_specs: true` for regression-harness maintenance. The authority remains the archived local-coordination and session-timeline contracts; this test checks the existing latest-fact projection rather than inventing a receipt-only contract.

# Impact

`scripts/test_local_coordination.py` and this evidence record. Run the complete native process harness, then the existing cross-platform workflow.
