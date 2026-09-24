## Why

When a Workflow sidecar is missing or invalid, restore falls back to the Timeline Spawn seed. That seed has no later phase, agent rows, or usage progress. A completed run can therefore appear to have used zero agents with `agent_usage_incomplete = false`, presenting an estimate as a fact.

## What Changes

- Mark agent usage incomplete whenever restore uses the Timeline seed rather than a valid progress sidecar.
- Keep Timeline lifecycle and journal replay as the authorities for execution and budget admission. Do not duplicate every mutable progress update into Timeline solely to reproduce lost UI rows.

## Scope

Workflow restore projection and its documented degraded-recovery contract only.
