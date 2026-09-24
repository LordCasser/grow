## Why

The passive coordination row changed from a terminal boolean to a typed phase, but the Minimal commit frontier and its fixture still access the removed boolean. The CLI cannot build, and Minimal's print-once frontier must continue to hold received and approved rows until terminal.

## What changes

- Read the typed terminal phase at Minimal's commit frontier.
- Update the fixture to cover all three phases, then build the CLI and run focused tests.

## Impact

Minimal Pager coordination rendering only. No storage or wire format changes.
