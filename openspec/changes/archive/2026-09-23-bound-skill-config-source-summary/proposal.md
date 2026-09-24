## Why

`grow/skills/config` reloads skills in a bounded worker, then synchronously discovers display source directories on the async request thread. That second pass uses Git discovery, filesystem metadata, and path canonicalization, so a blocked filesystem call can still stall the request runtime after the bounded reload finishes.

## What changes

Run the source summary under the existing discovery worker's single-permit and deadline boundary. Return a request error on timeout or worker failure instead of an incomplete successful summary. Leave skill inventory and source counting semantics unchanged.

## Impact

Shell skill configuration request only. Other agent/workflow and TUI discovery callers remain separate backlog work.
