# Verification: clarify-performance-backlog-boundaries

## Scope

Docs-only change limited to the five requested performance entries in `openspec/backlog.md` and this change directory. No product code, specs, tests, or Cargo commands are part of this change.

## Evidence and decisions

- Timeline's current live `prepare`/`accept` path clones `LifecycleFold` in `validate`; the archived `accelerate-long-timeline-replay` benchmark measured restoration, not live append. The backlog now distinguishes those facts and keeps a live append benchmark.
- `skills/config`, `grow/workflows/list`, `commands/list`, inspect, and agents-modal paths contain synchronous discovery calls. The archived skill reload change already covers its bounded worker, timeout, and retained permit semantics. The backlog now requests a controlled measurement and explicit shutdown ownership observation without claiming a production stall.
- Explicit transcript file writes already use an ordered worker queue. The archived verification identifies synchronous rendering, clipboard and default-backup work as out of scope, but reports no large-document latency or memory failure. The general copy/export item is removed.
- Interactive `/transcript` synchronously renders blocks and writes a temporary snapshot. No input latency failure or threshold is recorded. The remaining item asks for a bounded size sweep and removes the issue if no existing response expectation is crossed. Crash-time temporary-file residue remains a separate policy question.
- Scroll recording synchronously writes and flushes; prior verification covers per-instance byte bounds and special-file rejection, not latency. Explicit paths open with create/write then truncate and no inter-process lock; the backlog keeps only that correctness boundary and an explicit two-writer decision.

## Checks

- Local Markdown links in `openspec/backlog.md` and this change: 5 files checked, no broken links.
- `git diff --check -- openspec/backlog.md openspec/changes/clarify-performance-backlog-boundaries`: passed.
- `openspec validate --all --strict --no-interactive`: passed, 18/18 before archive.
- `openspec archive clarify-performance-backlog-boundaries --skip-specs --yes`: completed; no specification delta was merged.
- Post-archive `openspec validate --all --strict --no-interactive`: passed, 17/17.
- `openspec validate --archived --no-interactive`: after checking the final archive task, passed, 387/387. The initial run identified that task as incomplete; it was checked only after archive and strict validation succeeded.
- No Cargo, test, or build command was run.
