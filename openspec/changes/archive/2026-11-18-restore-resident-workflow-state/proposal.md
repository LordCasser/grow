## Why

A Workflow owner can publish a new live revision after its last durable history row. A second client attaching to that resident session currently sees the historical revision and missed running child state.

## What Changes

Publish the resident Workflow tracker's current state to the loading client after history replay. Adopt it in the existing Workflow revision reducer without adding a transcript row.
