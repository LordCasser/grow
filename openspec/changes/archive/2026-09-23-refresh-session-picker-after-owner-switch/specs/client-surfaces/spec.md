## ADDED Requirements

### Requirement: Pending session pickers recover after owner switches
When a visible session picker remains loading after another view supersedes its list request, Pager SHALL issue a new request for the visible picker. A late result from the former owner SHALL NOT update the new picker.

#### Scenario: Switch between pending Agent pickers
- **WHEN** Agent A and Agent B have pending picker modals and focus switches between them
- **THEN** each newly visible pending modal receives a fresh list request bound to its own cwd; late results from the other modal do not replace its entries.

#### Scenario: Switch to a loaded picker
- **WHEN** focus moves to a picker whose list has already loaded
- **THEN** its entries remain available without an unnecessary refetch.
