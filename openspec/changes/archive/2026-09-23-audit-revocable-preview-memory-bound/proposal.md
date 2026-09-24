# Audit revocable sampling preview memory bounds

## Why

`openspec/backlog.md` asks whether one unaccepted sampling candidate has a practical memory ceiling across provider streaming, Shell's leader-side staging, and the persisted replay projection. Existing byte caps name different representations, so source tracing is needed before deciding whether the debt is resolved.

## What Changes

- Record the proven per-representation limits and candidate copy points.
- Keep the backlog item open because current limits do not establish a total live-memory ceiling.
- Add no runtime code, tests, or specification delta.

## Scope

Trace the configured session sampling path from provider bytes through candidate notifications, leader persistence staging, response projection construction, and durable storage. Record exact configured bounds and copy points. Do not change runtime behavior or claim a measured process RSS value without an allocation/RSS measurement.

This is a source-level audit only. It does not change the specification or product behavior, so this change uses `skip_specs: true`.
