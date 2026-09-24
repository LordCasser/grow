## ADDED Requirements

### Requirement: Session picker list scope follows the visible owner
Pager SHALL request an Agent picker list using that visible Agent or child session's cwd. A list result SHALL update only the same visible picker and session binding that requested it. Relaxed-scope notices SHALL use the request cwd.

#### Scenario: Agent cwd differs from launch cwd
- **WHEN** a picker opens in an Agent whose session cwd differs from the app launch cwd
- **THEN** the list request uses the Agent session cwd, and the result's selection anchor and relaxed-scope notice use that same cwd.

#### Scenario: View or session binding changes before result
- **WHEN** the user switches Agent or child view, changes cwd, or rebinds the session while a list fetch is pending
- **THEN** the old result does not update the new visible picker, loading state, or toast.
