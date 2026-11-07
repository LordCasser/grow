## ADDED Requirements

### Requirement: Resident exit tree evidence reuses the canonical validated owner

Resident session-tree suspension SHALL query the existing canonical owner for a coherent runtime lifecycle and complete Spawned/Ended projection, without copying unrelated history bodies or replaying the entire already-validated Timeline solely to reconstruct child facts. The projection SHALL retain original event identity, sequence and child/result references. Cold or otherwise unvalidated storage sources SHALL retain their complete existing validation. Suspension SHALL reobserve child facts after the existing retirement frontier and SHALL preserve the shared five-second deadline, durable checkpoints and physical writer retirement requirements.

#### Scenario: Long unrelated history
- **WHEN** a resident session with long ordinary history requests confirmed exit
- **THEN** tree coordination obtains the same canonical child facts through the narrow owner projection without copying unrelated history bodies or reconstructing their Surface.

#### Scenario: Child finishes during suspension
- **WHEN** a child terminal is committed while suspension drains its owners
- **THEN** the post-retirement projection observes that original terminal and the final tree remains consistent with verified child identity and result references.

#### Scenario: Source is cold or unavailable
- **WHEN** a tree source must be loaded from storage or its canonical query fails
- **THEN** cold source validation remains required and query failure remains an unconfirmed error rather than an empty successful tree.

### Requirement: Live image maintenance uses committed owner evidence

Startup image-asset maintenance for a resident session SHALL obtain retained image references from the existing validated canonical owner, including committed historical User references no longer present in the Surface, without rereading and replaying the whole live Timeline. The existing artifact gate, contained-directory deletion and writer retirement ownership SHALL remain in force. Failed authority queries SHALL preserve all assets, and cancellation SHALL stop optional cleanup without detaching an accepted deletion or claiming an unfinished worker has retired.

#### Scenario: Immediate exit after loading long history
- **WHEN** a long-history session begins startup image maintenance and immediately requests exit
- **THEN** image maintenance does not add another full-history disk decode and replay to the service retirement barrier, and the shared five-second confirmed-exit contract remains unchanged.

#### Scenario: Historical and orphan image batches
- **WHEN** image assets include a committed historical batch removed from the current Surface and an unreferenced recognized batch
- **THEN** maintenance preserves the committed batch and may remove only the unreferenced recognized batch under the existing gate and contained directory authority.

#### Scenario: Maintenance is cancelled or authority fails
- **WHEN** cleanup is cancelled or the committed-reference query is unavailable
- **THEN** no unconfirmed reference is treated as absent, pending cleanup stops conservatively, and the exact worker ownership barrier still completes before successful exit.
