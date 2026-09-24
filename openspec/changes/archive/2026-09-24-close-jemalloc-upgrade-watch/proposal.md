# Close tikv jemallocator upgrade watch

## Why

The archived `upgrade-tikv-jemalloc-0-7` audit made a no-go decision: upstream issue #166 reports a 0.7-series x86_64 Linux musl build failure, and Grow ships both x86_64 and aarch64 musl targets with jemalloc enabled by default. There is no candidate upgrade to implement or verify while the upstream issue remains open and 0.7.0 is still the latest release.

The backlog entry only asks someone to watch for a future upstream release or successful candidate CI run. Those are external events, not current engineering work. The archived audit preserves the evidence and the criteria for restarting review, so this optional upgrade does not need active backlog tracking.

## What Changes

- Remove only the tikv jemallocator 0.7 watch from `openspec/backlog.md`.
- Preserve the archived no-go decision and document that a future fixed upstream release or successful Grow musl release-dist build can justify a new, separately scoped upgrade change.

## Impact

Documentation-only. No runtime behavior or accepted contract changes; `.openspec.yaml` uses `skip_specs: true`.
