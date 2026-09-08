## ADDED Requirements

### Requirement: Unified writer identity belongs to the opened handle
Unified log writers SHALL capture their initial identity from the opened descriptor rather than a subsequent path lookup. On Unix, maintenance SHALL compare this captured device/inode with the current path identity, allowing recovery when replacement or removal occurs between open and writer construction.

#### Scenario: Replacement during open initialization
- **WHEN** a log path is replaced or removed after its descriptor opens but before writer initialization finishes
- **THEN** the writer tracks the old descriptor identity and the next due maintenance can reopen the live path for visible writes.
