## ADDED Requirements
### Requirement: Displayed recap coverage survives cold load
Successful displayed recaps SHALL persist their frozen real-user turn count. Existing actors SHALL restore the latest explicitly recorded count without deriving it from notification arrival position. Fresh and seeded actors SHALL not inherit it. Historical replay SHALL remain display-only and manual recap SHALL remain repeatable.

#### Scenario: Unchanged conversation is loaded again
- **WHEN** a displayed recap has explicit coverage and the same conversation is cold loaded
- **THEN** automatic recap does not repeat until the existing new-turn gate permits it.

#### Scenario: New prompt precedes recap persistence
- **WHEN** a new prompt arrives after recap commit but before its notification is durably appended
- **THEN** restored coverage excludes that new prompt.

#### Scenario: Historical recap has no explicit coverage
- **WHEN** legacy recap history lacks the frozen count
- **THEN** restoration does not guess coverage from presentation order or Sideband completion.
