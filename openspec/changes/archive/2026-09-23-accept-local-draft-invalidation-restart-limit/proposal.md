# Proposal: Accept the local draft invalidation restart boundary

## Why

Local draft invalidation retries are held in Pager process memory. If storage
continues failing until Pager exits, a stale draft can remain on disk and be
restored by a later process. The existing contract and developer guide already
limit the guarantee to a running Pager.

## Decision

Accept this as a low-frequency best-effort limitation. Making deletion intent
durable would require another durable authority and conflict/version rules with
new drafts; synchronously blocking ACP prompt admission on successful storage
would change the prompt ownership boundary. Neither is justified for this
cleanup item. A stale draft may reappear after restart, but recovery only fills
the local composer and never submits it automatically.

## What Changes

Record the decision and remove the corresponding backlog entry. No runtime or
contract behavior changes.
