## ADDED Requirements

### Requirement: History search accepts only current request results
History search SHALL identify results by the originating request and expose only results matching the latest submitted request. Submitting a query or item refresh SHALL invalidate previously selectable results immediately.

#### Scenario: Reopen or change query before completion
- **WHEN** an older request completes after reopening or submitting a newer query
- **THEN** its results cannot be displayed or selected, while the current request result can be applied once

#### Scenario: Item refresh retains navigation intent
- **WHEN** items refresh and the same query is resubmitted after manual navigation
- **THEN** stale results are hidden while waiting and the retained selection index is clamped to current results without resetting the existing navigation intent
