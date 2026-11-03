# Passive message-delivery model

Run from the repository root with Python 3 and no third-party packages:

```sh
python3 openspec/changes/archive/2026-11-03-separate-execution-control-and-context-delivery/demo/model.py
```

The command exits nonzero if an invariant, replayed scenario, or mutation guard fails. It writes the measured report to `results.json` beside this file.

Latest run: 144 enumerated starting configurations, depth 7, 24,658 states, 98,953 transitions, and 40 reached action classes. Sixteen named traces and ten mutated-transition counterexamples are recorded in the JSON report.

The model freezes both canonical terminal obligations and Received IDs at each authorized boundary. Dispatch waits for every frozen obligation to reach durable Receipt, Consumed, and confirmed acknowledgement. The report also records the distinction between tentative final-candidate snapshot and irreversible close, including the trace where m1 authorizes a successor and later m2 joins the next safe boundary.

This bounded model does not prove Rust behavior, persistence integration, provider observation, multi-writer linearizability, or infinite scheduler liveness. It abstracts payload bytes, timing, filesystem behavior and provider internals. One explicit fair finishing trace witnesses drain only when an eligible boundary is scheduled.
