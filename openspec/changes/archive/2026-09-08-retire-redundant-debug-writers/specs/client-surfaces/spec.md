## ADDED Requirements

### Requirement: Debug routing retains only selected writer guards
For concurrent first writes to a debug routing sink, only the selected writer guard SHALL be parked for process lifetime. Unselected writer guards SHALL be dropped outside the routing mutex so accepted first lines can flush and redundant workers retire. File opening SHALL remain outside that mutex.

#### Scenario: Concurrent first writes
- **WHEN** several threads first write to the same session sink or fallback sink
- **THEN** one guard per sink remains parked after those writes return, and successfully queued lines remain available after flushing.
