## Why

Matched Shell and PTY synthetic resume runs show that terminal-visible history takes substantially longer than the Shell `session/load` round trip, but those are different harnesses and cannot isolate Pager cost by subtraction. The existing instrumentation log can time Shell and Pager work inside one instrumented PTY process.

## What Changes

- Add opt-in instrumentation timers around Pager frame render, terminal diff/queue, writer-thread I/O, and replay batch finalization.
- Extend the ignored PTY resume probe to enable the existing instrumentation log and summarize those phases together with Shell load timing on the same run.
- Use the phase evidence to choose a focused production optimization or close the performance item if it is no longer actionable.

No UI behavior, protocol, or persistence contract changes. The timers are inert when instrumentation is disabled, so this measurement-only change sets `skip_specs: true`.
