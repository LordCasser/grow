## ADDED Requirements

### Requirement: Session picker list results belong to the latest visible fetch
Pager SHALL apply a session picker list result only when it belongs to the latest list fetch and a picker surface is still visible. Dismissal SHALL invalidate in-flight list results.

#### Scenario: Rapid successive fetches
- **WHEN** two list requests are issued and the older request completes last
- **THEN** only the newer result remains visible.

#### Scenario: Modal closes before a response
- **WHEN** an Agent session picker modal is dismissed while its list request is pending
- **THEN** the late response does not repopulate the closed modal or the welcome picker.

#### Scenario: Welcome closes before a response
- **WHEN** the welcome picker is dismissed while its list request is pending
- **THEN** the late response does not restore its entries or loading state.
