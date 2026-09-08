## ADDED Requirements

### Requirement: Replayed recaps do not settle current feedback
Pager SHALL restore historical recap blocks without marking the current away period satisfied or clearing current manual recap progress. Live recap notifications SHALL retain their existing feedback behavior.

#### Scenario: Historical recap restored
- **WHEN** an admitted replay contains an automatic or manual recap
- **THEN** its block is restored while current automatic eligibility and manual progress remain unchanged

#### Scenario: Live recap received
- **WHEN** a live recap is displayed
- **THEN** it marks the away period satisfied and clears manual progress only for a manual recap
