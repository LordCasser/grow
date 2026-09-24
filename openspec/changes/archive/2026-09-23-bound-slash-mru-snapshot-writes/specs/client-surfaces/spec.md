## ADDED Requirements

### Requirement: Slash MRU snapshot writes obey the encoded input allowance
Slash MRU persistence SHALL reject encoded snapshots larger than 1,048,576 bytes before background handoff and before filesystem publication. Rejected snapshots SHALL leave the destination unchanged; a rejected handoff SHALL be reported to the controller so its store remains dirty for retry. Snapshots at or below the allowance SHALL retain atomic publication behavior.

#### Scenario: Snapshot fits the encoded allowance
- **WHEN** a serialized Slash MRU snapshot is at most 1,048,576 bytes
- **THEN** it may be handed to the background writer and is published as a complete atomic replacement

#### Scenario: Snapshot exceeds the encoded allowance
- **WHEN** a serialized Slash MRU snapshot is larger than 1,048,576 bytes
- **THEN** background handoff and filesystem publication reject it, preserve any existing destination, and the controller retains dirty state after the handoff failure

Evidence: `crates/codegen/pager/src/slash/mru.rs` (`MAX_STORE_BYTES`, `MruSnapshot::write`, `persist_async`, `snapshot_write_enforces_the_reader_byte_allowance_before_publication`) and `crates/codegen/pager/src/slash/mod.rs` (`SlashController::record_command_use`, exercised by `controller_rejects_oversized_snapshot_and_retains_dirty_state`).
