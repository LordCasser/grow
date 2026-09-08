## ADDED Requirements

### Requirement: Input diagnostic dumps describe the input owner
Input diagnostic recording and dumping SHALL describe the agent view that owns the inspected input surface, including child views and dashboard-attached sessions. Diagnostic routing SHALL not change the business action produced by the key event and SHALL preserve printable-character redaction.

#### Scenario: Child composer handles input
- **WHEN** a key is delegated to a child view and a dump is requested from that surface
- **THEN** diagnostic session and textarea metadata correspond to that child rather than the parent composer

#### Scenario: Parent intercepts input
- **WHEN** a parent overlay or close action consumes input before child delegation
- **THEN** its diagnostic observation is not attributed to an unchanged child textarea

#### Scenario: Dashboard attached surface
- **WHEN** an input dump action targets a dashboard-attached session
- **THEN** a nonempty diagnostic snapshot can be produced for that surface instead of silently returning because the top-level view is the dashboard
