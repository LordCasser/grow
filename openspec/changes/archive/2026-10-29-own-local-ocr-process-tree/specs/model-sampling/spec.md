## ADDED Requirements

### Requirement: Local OCR owns its helper process tree

Local OCR SHALL execute its helper in a process group/job whose lifetime is owned by the OCR operation. Timeout, cancellation, dropping the operation, or normal completion SHALL terminate any remaining helper descendants. The direct helper SHALL retain the existing kill-on-drop and wait/reap behavior; normal completion SHALL preserve bounded input/output behavior and recognized text.

#### Scenario: OCR operation is cancelled

- **WHEN** the caller cancels or drops local OCR while its helper is running
- **THEN** the helper process group/job is terminated, including descendants, and the direct child remains subject to the existing wait/reap behavior

#### Scenario: OCR deadline expires

- **WHEN** the local OCR deadline expires while the helper or a descendant is keeping the operation pending
- **THEN** the owned process group/job is terminated and the caller receives the existing timeout result

#### Scenario: OCR completes with a detached descendant

- **WHEN** the direct helper returns valid text while a descendant remains in the process group with its output pipes closed
- **THEN** local OCR returns the recognized text and terminates the remaining process group before releasing ownership
