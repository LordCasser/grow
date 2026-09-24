## ADDED Requirements

### Requirement: Independent notifications bypass retractable candidate buffering

The leader SHALL retain only attempt-owned provisional notifications in its mixed-client retractable candidate buffer. Untagged independent ACP and Grow notifications SHALL continue through the normal live route to observers that cannot retract provisional output, even while a candidate is active. A discarded candidate SHALL remain hidden from those observers; an accepted candidate SHALL be delivered after admission. Durable replay SHALL retain its canonical ordering.

#### Scenario: Independent update during an active candidate

- **WHEN** an untagged independent ACP or Grow notification arrives between provisional candidate fragments
- **THEN** a non-retracting observer receives that independent notification live, while the candidate remains withheld until its terminal boundary.

#### Scenario: Candidate is discarded after independent update

- **WHEN** an independent notification has been delivered during a candidate that is later discarded
- **THEN** the independent notification remains visible exactly once and no candidate fragment is delivered to a non-retracting observer.
