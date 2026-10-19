## ADDED Requirements

### Requirement: Live Workflow clears obey revision ordering
The live Workflow projection SHALL reject a positive-revision update whose revision is not newer than the accepted revision for that same run, including `cleared` updates. A revisionless clear SHALL retain the existing compatibility behavior; a revisionless non-clear update SHALL NOT resurrect a cleared run.

#### Scenario: Stale positive-revision clear
- **WHEN** a newer Workflow revision for a run has been accepted and an older positive-revision clear arrives
- **THEN** the clear is ignored without changing the run, revision high-water mark, tombstone, or running block.

#### Scenario: Newer positive-revision clear
- **WHEN** a clear arrives with a positive revision newer than the accepted revision
- **THEN** the run is removed and its clear tombstone and revision are recorded.

#### Scenario: Revisionless clear compatibility and stale replay
- **WHEN** a revisionless clear arrives, followed by a revisionless snapshot for the same run
- **THEN** the clear applies under the existing compatibility rule and the later snapshot cannot resurrect the run.
