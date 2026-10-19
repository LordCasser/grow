## ADDED Requirements

### Requirement: Configuration watch delivery retains a live source owner
Configuration watcher callbacks SHALL check source ownership when handling a debounced path. A project config path whose last project owner has unregistered SHALL NOT publish a new change or reattach its directory watch. Global and explicitly configured source directories SHALL retain their independent watches when project registrations are released. Changes already delivered to the receiver queue need not be retracted.

#### Scenario: Project event arrives from the debouncer after unwatch
- **WHEN** a queued project .grow or config.toml path is handled after its final project registration has been released
- **THEN** the callback drops it without publishing a project change or reviving the directory watch.

#### Scenario: Global or explicit config shares a project directory
- **WHEN** a project registration is released while its directory is also watched by a global or explicit configuration source
- **THEN** the independent source watch remains and subsequent configuration writes still produce their normal events.

#### Scenario: Another cwd still owns a shared ancestor
- **WHEN** one cwd unregisters while another registration retains the same repository ancestor
- **THEN** ancestor configuration writes remain observable for the remaining owner.
