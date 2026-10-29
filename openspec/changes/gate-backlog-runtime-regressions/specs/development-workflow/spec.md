## ADDED Requirements

### Requirement: Release core gates execute runtime ownership and diagnostics regressions
The immutable-commit Core release gate SHALL execute library regressions for diagnostics, pager-render, tty-utils and test-support alongside the existing Core/Pager packages. Their failures SHALL block release publication through the same existing gate.

#### Scenario: Runtime defect regression fails
- **WHEN** a Chrome timer, post-exit cleanup, process-group or owned-test-process regression fails on the pinned release commit
- **THEN** Core fails and the existing release dependency prevents publication.

#### Scenario: Dedicated PTY daemon ownership
- **WHEN** the release CLI is built and the selected interactive regressions run
- **THEN** the exact ignored leader-cluster ownership regression proves ordinary Drop and unwinding reap its isolated daemon; failure blocks the same Core gate.
