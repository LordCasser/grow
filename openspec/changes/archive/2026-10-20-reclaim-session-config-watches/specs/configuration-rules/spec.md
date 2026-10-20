## ADDED Requirements

### Requirement: Dynamic project watches follow resident session ownership
Existing production session-registration paths SHALL hold one dynamic configuration-watch registration per resident session in the manager registry. The registration SHALL survive actor rebuild and release on resident removal, idle-unload or agent destruction. A cwd and its shared repository ancestors SHALL remain watched while another applicable owner exists. Global, explicit configuration sources and the runtime bootstrap cwd SHALL retain independent ownership.

#### Scenario: Last dynamic owner leaves
- **WHEN** the last resident for a dynamic cwd is removed or idle-unloaded
- **THEN** its recorded registration is released after the runtime consumes the ordered update, and unowned paths no longer publish or reattach watches; other cwd, ancestor and independent source owners remain

#### Scenario: Same cwd, alias or session is repeated
- **WHEN** sessions share the same normalized cwd or one session repeats its registration, rebuilds, or replaces its cwd
- **THEN** ownership is idempotent per session, another live owner's paths remain watched, and replacement releases the previous recorded path without re-resolving a disappeared alias

#### Scenario: Runtime or watcher is unavailable
- **WHEN** an agent is destroyed or project watching is disabled/unavailable
- **THEN** its dynamic registrations are released or no longer consumed, no unused registration queue accumulates, and independent runtime configuration sources retain their existing behavior

#### Scenario: Rebuild fails or is cancelled after retiring the old actor
- **WHEN** a session lifecycle transaction ends without a resident replacement
- **THEN** any retained watch registration is released, while a rebuild still in progress retains its owner
