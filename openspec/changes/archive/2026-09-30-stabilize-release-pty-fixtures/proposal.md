# Proposal: stabilize release PTY fixtures

## Why

The release PTY checklist exposed a fixture that sends `/new` after only the head of an 80-line mocked turn reaches native scrollback. The turn may still be streaming, so the command does not test the intended idle new-session path. This creates a false failure in the release gate.

## What Changes

Synchronize the selected PTY fixture with the complete first turn before asserting `/new` behavior. If the same release checklist exposes other fixture-only setup or synchronization errors, record and repair them here without changing application behavior. Runtime defects require a separate behavior change.

This is a test-only change; `.openspec.yaml` sets `skip_specs: true` because the archived product contract is unchanged.
