## Existing owners

ConfigFileWatcher already tracks distinct cwd chains and protects shared ancestors plus global/explicit directories. Its startup cwd is a separate runtime bootstrap owner; it stays until the runtime watcher is dropped. Dynamic sessions are not independent owners of that bootstrap registration. Do not conflate removing a resident with ending those independent sources.

Use the existing per-session ResidentResources map, whose entries survive actor rebuild but drop on remove/idle-unload and agent destruction. The existing SessionLifecycleGuard also releases the watch when a failed/cancelled rebuild ends without publishing a resident replacement. This prevents the otherwise intentionally retained resource entry from leaking a registration. A small registration guard sends Register(session_id,cwd) and sends Release(session_id) on Drop. Reuse a guard across rebuild and update its cwd without creating a second same-id guard. Watcher-side session-id -> normalized cwd entries provide idempotent replacement and shared-cwd accounting; existing project_roots still retain shared ancestors. Acquire the replacement path before releasing the old path. No separate service, generation registry or timer sweeper.

Normalize the path once when accepted. Release the previously recorded key, not a possibly changed symlink's new target; public direct watch/unwatch retain their existing path resolution. Statement/event ownership checks from the preceding late-event fix remain authoritative. Disabled/unavailable watching does not queue unused registrations; global/explicit reload behavior stays unchanged.

## Verification

Exercise the real production guard and watcher: two same-cwd/alias owners, different cwd siblings sharing a repo ancestor, replacement cwd, duplicate release, last release, bootstrap/global/explicit owners, and late .grow/config events after release. Agent tests verify successful registration, remove, idle-unload's existing resource-drop boundary, rebuild retention and whole-agent drop; no watcher registration before successful actor setup. Existing live filesystem late-directory/recreation tests remain.

Run focused shell watcher/agent regressions and compile both runtime wiring sites using the current Rust toolchain/cache. Keep tests deterministic at the callback boundary for ownership assertions; existing real OS tests cover notification delivery. Do not mix MCP feature changes or the previous uncommitted sqlite-vec fix.
