# Change: Pin permission cancellation during requester close

## Why

Pager session teardown already cancels queued permission requests. A focused regression is needed to prove that a prompt pending when its requester closes cannot later authorize the tool or enable session-wide always-approve.

## What Changes

- Add a regression that closes the requester while its permission prompt is pending, then exercises a stale allow action and verifies it has no authorization side effect.

## Scope

Test-only maintenance. The existing cancellation behavior and product contract do not change, so this change uses `skip_specs: true` and adds no delta spec.
