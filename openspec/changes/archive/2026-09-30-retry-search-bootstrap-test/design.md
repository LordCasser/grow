# Design

The bootstrap lease can return `BootstrapOutcome::RunAgain` when its observed cache epoch becomes stale. The production job schedules another bootstrap attempt for this outcome. The test currently bypasses that job loop and incorrectly treats one transient outcome as terminal.

The test will make at most eight direct launch-bootstrap attempts, yielding between `RunAgain` results. It will then require `Done` and the completed marker before proceeding with the existing failure-preservation and repair checks. This confines the change to test synchronization and makes exhaustion explicit.
