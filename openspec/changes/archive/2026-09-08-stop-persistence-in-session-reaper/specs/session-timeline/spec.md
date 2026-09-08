## ADDED Requirements

### Requirement: Request persistence stop during final thread-owner cleanup
When the last SessionThread owner is dropped, fallback cleanup SHALL request persistence stop after its dedicated actor thread has exited, including an already-joined thread. This destructor path SHALL NOT claim that persistence has finished or that replacement admission is safe before its task completion.

#### Scenario: Live thread enters reaper
- **WHEN** the final thread owner is dropped while the dedicated thread is still running
- **THEN** the reaper joins the thread before requesting persistence stop.

#### Scenario: Joined owner is dropped
- **WHEN** the last owner is dropped after its OS thread has already been joined
- **THEN** cleanup requests persistence stop through the retained weak route.

#### Scenario: Another owner remains
- **WHEN** a SessionThread clone is dropped while another logical owner remains
- **THEN** that drop does not request persistence stop.
