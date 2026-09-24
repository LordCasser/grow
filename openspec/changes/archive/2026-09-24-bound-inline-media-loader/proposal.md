## Why

The Pager currently creates a thread for each distinct inline-media path and reads each file without an input limit. Prepared image bytes can also accumulate in per-view completion mailboxes before the 64 MiB CPU cache admits or rejects them. Bound worker concurrency, pending requests per view, and per-file input/prepared bytes while keeping filesystem work off the UI thread. Saturation remains retryable on a later render pass; genuine read/prepare failures keep the existing retry window and sticky failure behavior.

## What Changes

- Admit at most two inline-media workers process-wide and two pending paths per AgentView.
- Read at most 16 MiB per source and retain at most 16 MiB of prepared bytes per completion. Preparation's existing transient conversion-output limit remains 100 MB per worker.
- Preserve session reset mailbox detachment and the existing bounded rename-race retry.
- Update the Pager behavior contract, developer guidance, and remove the resolved loader item from the backlog.

## Out of scope

This does not bound terminal-side image memory, the separate modal image viewer, or total bytes cached across all AgentViews.
