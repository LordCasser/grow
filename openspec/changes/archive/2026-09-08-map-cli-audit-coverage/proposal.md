## Why
Archive counts mix fixes, tests and audits and cannot answer feature coverage. Establish a source-backed inventory for the CLI command surface before assigning coverage.

## What Changes
Map every current top-level Command variant to dispatch and a concrete next verification boundary. Record hidden/gated surfaces and exclusions. No runtime changes; skip_specs true for audit-only work.
