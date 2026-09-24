## ADDED Requirements

### Requirement: Tool bridge Grow projections share preview credits

Background task completion Grow projections SHALL omit copied task output while preserving full output through the task file and model-facing notification. The tool bridge SHALL reserve session preview credits for queued TaskBackgrounded, TaskCompleted, and ScheduledTaskCreated Grow persistence and live gateway copies, and for ScheduledTaskFired and MonitorEvent live copies, holding each reservation until its consumer completes. A failed reservation or append during an active sampling attempt SHALL prevent canonical response admission. Durable task-completion acknowledgement SHALL remain a prerequisite for acknowledged UI projection publication. Monitor events SHALL retain their model notification.

#### Scenario: Completed task has a large output file

- **WHEN** a background task completes with a large retained output file during an active attempt
- **THEN** the Grow completion snapshot contains status metadata but no copied output, the model notification retains its existing truncated text and output-file pointer, and queued Grow copies stay within preview credits.

#### Scenario: Monitor events outrun a slow client

- **WHEN** monitor events arrive while the gateway has not completed prior Grow delivery
- **THEN** additional live Grow events cannot exceed the session preview budget, while model notification commands retain their normal admission path.
