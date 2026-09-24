# Change: Close the resume responsiveness watch

## Why

The tracked resume responsiveness item now has repeatable PTY evidence for ordinary and dense-tool synthetic histories. After replay paint throttling, the 128/512-turn history becomes visible in 2.35/6.11 seconds, with 47/71 render calls and 7.7/7.8 ms key-echo p95. A separate slow-writer run measured input during replay at 77.6 ms p95; the dense-tool run measured 52.9 ms p95 and preserved the draft. No current run reproduces the backlog's former input-responsiveness concern.

The remaining resident/cursor, multiple subagent, real long-text, and slow-terminal combinations are unmeasured. They remain limitations of the evidence, not a reproduced defect with an attributed failing stage or a concrete fix. Remove only this generic watch from the backlog and retain these limits in this decision record. This does not establish universal p95 ≤ 100 ms.

## What Changes

- Remove the resume responsiveness bullet and its now-empty heading from `openspec/backlog.md`, along with a previously empty Pager watch heading.
- Record the evidence and the unmeasured cases that would justify reopening a concrete investigation.

## Impact

Documentation-only. Runtime behavior and accepted contracts do not change; `skip_specs: true` applies. This closes generic follow-up tracking, not a claim that every workload meets a latency target.
