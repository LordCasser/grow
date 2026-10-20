## Why

ConfigFileWatcher can unregister paths, but production sends only new cwds and never releases them. A long-lived runtime accumulates dynamic project watches after sessions are removed or idle-unloaded. Releasing one session must not revoke a shared cwd, repository ancestor, global/explicit source or the runtime bootstrap cwd.

## What Changes

Bind one config-watch registration to the existing ResidentResources lifetime: preserve it across actor rebuild, release it on removal, idle-unload and agent drop. Send ordered session-keyed updates through the existing local channel; the watcher reconciles current cwd ownership and invokes its existing distinct-cwd watch/unwatch paths. Same-session repeats and unknown releases are idempotent. Preserve normalized acquisition identity when a directory or alias disappears. Both leader and in-process runtimes consume the same updates. Close a failed watcher's unused registration receiver rather than leaving an unconsumed queue.

## Capabilities

### Modified Capabilities
- configuration-rules: dynamic project watches follow resident ownership; shared and independent source owners survive.

## Impact

Existing shell configuration watcher, MvpAgent resident resources and both runtime wiring sites, with targeted lifecycle/ownership tests and docs. No MCP SDK/protocol/catalog-policy change, remote-control work, cache-policy change, version bump or published artifact mutation.
