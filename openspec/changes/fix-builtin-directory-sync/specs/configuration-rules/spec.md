## ADDED Requirements

### Requirement: Builtin publication syncs a readable directory
On Unix, builtin extraction SHALL synchronize renamed managed files through a readable descriptor opened relative to the existing pinned directory capability. The generation marker SHALL remain the last published file and failures SHALL remain observable to the transaction caller.

#### Scenario: First extraction on Linux
- **WHEN** a fresh Grow home receives one builtin extraction transaction on Linux
- **THEN** all managed files and the matching version marker are published successfully without requiring repeated startup attempts to advance the generation.

#### Scenario: Managed parent is invalid
- **WHEN** a managed parent is a file or a symlink
- **THEN** extraction fails without publishing a successful generation marker or writing through the symlink.
