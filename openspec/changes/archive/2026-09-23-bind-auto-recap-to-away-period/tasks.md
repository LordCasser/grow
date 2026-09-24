## Implementation

- [x] Mint an away-period ID in FocusTracker and carry it through automatic recap dispatch and ACP request.
- [x] Validate/echo the ID through Shell recap command and live result.
- [x] Filter stale live automatic results before Pager display and shown-state mutation; preserve replay/manual paths.

## Verification

- [x] Add request, Shell and Pager regression coverage for the cross-period race.
- [x] Run relevant Rust tests, formatting, diff checks and OpenSpec strict validation.
- [x] Archive the change, then validate main and archived specs.
