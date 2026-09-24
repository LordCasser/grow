# Change: Verify list layout prefix-sum overflow boundary

## Why

The Pager backlog listed unchecked `usize` prefix-sum addition as an unresolved geometry risk. Determine whether overflow can occur for list layouts on supported release targets and record the evidence.

## What Changes

- Record the arithmetic and allocation bound for variable-height list caches.
- Remove only the unsupported-target prefix-sum overflow concern from the geometry backlog.

This is an audit and backlog maintenance change. It does not change runtime behavior or the accepted contract, so `skip_specs: true` is intentional.
