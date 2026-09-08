## ADDED Requirements

### Requirement: Pager log flush has a bounded delivery wait
An initialized pager log flush SHALL stop waiting for its current batch after two seconds if ACP acknowledgement has not completed. An acknowledged or failed send SHALL finish without waiting out that deadline. Timeout SHALL not retry the batch or claim cancellation of already enqueued remote processing.

#### Scenario: Peer retains acknowledgement
- **WHEN** the peer holds a log notification without acknowledging it
- **THEN** the current-batch flush releases its local wait after the two-second budget

#### Scenario: Peer acknowledges or disconnects
- **WHEN** the current log batch is acknowledged or its channel fails
- **THEN** flush completes without waiting for the deadline
