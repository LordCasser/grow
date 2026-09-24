## ADDED Requirements

### Requirement: Published user-image assets follow durable Timeline references
The session SHALL reclaim recognized published user-image batches and crash-left staging directories that have no committed physical User message reference. Reclamation SHALL derive roots from a validated committed Timeline under the session writer epoch and SHALL be serialized with live publication and user-message admission. If the committed Timeline cannot be established, reclamation SHALL leave assets unchanged.

#### Scenario: Admission fails before a durable User message
- **WHEN** an image batch was published but its user message was not committed, and reconciliation reads the committed Timeline
- **THEN** the unreferenced batch is removed without touching another referenced batch.

#### Scenario: Acknowledgement is lost after commit
- **WHEN** the user message was committed but its acknowledgement is unavailable
- **THEN** reconciliation retains the referenced batch regardless of the caller's error result.

#### Scenario: Crash leaves a staging directory
- **WHEN** the writer restarts with an unpublished image staging directory
- **THEN** reconciliation removes that recognized staging directory after establishing the committed Timeline.

#### Scenario: Timeline is unreadable
- **WHEN** the committed Timeline cannot be validated
- **THEN** no image asset is deleted.
