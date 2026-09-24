# Change: Close online Timeline append cost observation

## Why

The backlog recorded measured lifecycle-fold cloning cost without an existing product latency budget or representative session lifecycle distribution. The archived measurement found scaling with retained request identities, but did not establish a user-visible performance defect.

## What Changes

- Remove only this conditional performance observation from `openspec/backlog.md`.
- Preserve the measurements and the closure rationale in this change's verification record, linked to the archived benchmark.

This is documentation and backlog maintenance only. It changes no runtime behavior or accepted contract, so `skip_specs: true` is intentional.
