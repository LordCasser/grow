# Change: Keep local-draft filesystem work off Pager input and paint

## Why

`LocalDraftRuntime::sync` runs on every Pager event-loop iteration and on its timer. It can canonicalize a cwd, load/quarantine a draft, rekey it, and synchronously write or remove files with directory sync. A slow filesystem operation therefore delays both key handling and drawing. Prompt-RPC ownership transfer also removes drafts synchronously before effect execution.

## What Changes

Move local-draft path resolution and store I/O to one serialized worker. Pager captures bounded draft state, enqueues ordered updates and invalidations without waiting, and applies recovered drafts only when the originating binding and live editing state are still current. A final checkpoint waits for the worker before normal quit completes. Disk failures retain the existing retry and recovery semantics.

## Scope

This change concerns only local-draft recovery. It does not change ACP prompt ownership, draft file format, quarantine policy, or source-identity checks.
