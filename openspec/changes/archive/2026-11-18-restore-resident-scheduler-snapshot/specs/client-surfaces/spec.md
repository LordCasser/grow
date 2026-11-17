## ADDED Requirements

### Requirement: Session load restores resident scheduler state

When `session/load` attaches to a resident session, it SHALL return the current scheduler snapshot from that session's scheduler owner. The snapshot SHALL include the scheduler generation and revision and every current task's next fire time and last admitted subagent identity. Pager SHALL reconcile its scheduled-task projection from this snapshot after replay and SHALL ignore scheduler notifications older than the adopted snapshot version.

#### Scenario: Load after a recurring task fired

- **WHEN** a resident recurring scheduled task fires and a second client then loads the same session
- **THEN** the load snapshot restores the advanced `next_fire_at` and fired child identity, even when historical replay contains only the earlier creation event

#### Scenario: Scheduler notification races load snapshot adoption

- **WHEN** a live scheduler notification arrives before or after the load response with a generation/revision older than the adopted snapshot
- **THEN** Pager keeps the snapshot's task state and does not roll back its next fire time or child link

#### Scenario: Newer notification races an older multi-task snapshot
- **WHEN** a notification advances or deletes task A after snapshot version N, before Pager adopts snapshot N, while the snapshot also contains an unchanged task B
- **THEN** Pager preserves task A's newer state (or deletion) and still refreshes task B from snapshot N
- **AND** replay-only tasks absent from snapshot N are removed unless a newer notification changed that task

#### Scenario: Different task notifications arrive out of revision order
- **WHEN** task A receives revision 12 and task B's revision 11 notification arrives afterward, with a complete snapshot floor at revision 10
- **THEN** Pager applies both updates by their per-task versions
- **AND** the revision 12 update for task A does not reject task B's revision 11 update

#### Scenario: A newer scheduler generation starts before its snapshot
- **WHEN** a live notification from a newer scheduler generation arrives before that generation's complete snapshot
- **THEN** Pager accepts events from that generation without applying the prior generation's revision floor
- **AND** Pager ignores late live events from older generations

#### Scenario: An absent one-shot fires after the complete snapshot
- **WHEN** a fire notification with no next fire time arrives for a task absent from the current snapshot
- **THEN** Pager does not create a scheduled-task row for it

#### Scenario: No resident scheduler is available

- **WHEN** session load has no resident scheduler handle
- **THEN** no scheduler snapshot is included and existing replay behavior remains available
