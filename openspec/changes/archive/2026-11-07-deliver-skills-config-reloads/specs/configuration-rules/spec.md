## ADDED Requirements

### Requirement: Skills configuration changes reach the existing session catalog owner

Both leader and in-process runtimes SHALL deliver accepted Skills configuration updates to the existing per-session reload owner. Changes to disabled, ignore or paths SHALL refresh model-callable Skills and slash advertisement from fresh configuration and each session's own plugin snapshot. Discovery watcher ownership SHALL follow the latest configured custom paths, including updates arriving during initial registration. Watcher replacement SHALL retain the prior owner on construction failure and retire all owned registrations on cancellation. Removed roots SHALL not remain actively watched after successful replacement.

#### Scenario: Only the configuration file changes
- **WHEN** disabled or ignore is changed in the configuration without editing a skill directory
- **THEN** both runtime entry paths deliver reload and the session catalog and slash advertisement reflect that configuration.

#### Scenario: A custom root is added and removed
- **WHEN** paths adds an external root, a skill is later created or changed there, and the root is subsequently removed
- **THEN** the new root supplies discovery events while configured, the existing owner refreshes the session catalog, and successful removal releases its active watch.

#### Scenario: Configuration changes during registration
- **WHEN** paths changes before the first or replacement watcher registration completes
- **THEN** the latest pending paths are still processed rather than lost due to startup timing.

#### Scenario: macOS discovery does not wait for native stream registration
- **WHEN** a macOS runtime starts or replaces Skills/Workflow discovery watches
- **THEN** metadata polling supplies the existing create, modify and delete refresh path without waiting for FSEvents stream startup or hashing arbitrary custom-root file contents.

#### Scenario: Failed replacement or runtime cancellation
- **WHEN** replacement registration fails or runtime cancellation races registration
- **THEN** failure preserves the previous valid watcher with visible diagnostics, cancellation retires all registrations owned by that runtime, and no unconfirmed replacement is reported as active.
