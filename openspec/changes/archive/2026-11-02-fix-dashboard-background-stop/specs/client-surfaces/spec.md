## ADDED Requirements

### Requirement: Dashboard stop covers the selected session activity

Dashboard Ctrl+X on a busy local root SHALL target the activity that makes that row busy, including a foreground-idle root with running ordinary children, an active Goal or a manageable active Workflow. It SHALL cancel ordinary children through the existing session cancellation path, pause an active Goal, and stop active manageable Workflows through their existing host control keyed by run identity. It SHALL preserve unrelated roots and drafts, SHALL NOT enter control text into the model prompt queue, and SHALL wait for authoritative activity updates before allowing deletion. Unavailable controls SHALL NOT be reported as successfully stopped.

#### Scenario: Only background children remain
- **WHEN** the selected root foreground is idle but ordinary children are running and the user presses Ctrl+X
- **THEN** the existing cancellation path targets that root and its ordinary children, without arming deletion.

#### Scenario: Active Goal at an idle boundary
- **WHEN** only the selected root's Goal keeps the row Working
- **THEN** Ctrl+X sends its existing pause control and retains the row until authoritative state changes.

#### Scenario: Workflow owns the background work
- **WHEN** a selected root has an active manageable Workflow and an idle foreground
- **THEN** Ctrl+X stops that exact run through host control, preserving the composer and unrelated runs in other roots.

#### Scenario: Work has not settled yet
- **WHEN** cancellation or pause/stop has been requested but the row remains Working
- **THEN** repeated stop cannot confirm deletion or manufacture an idle state.
