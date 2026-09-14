## ADDED Requirements

### Requirement: Generic tool failures render their terminal content once

Pager SHALL assign the terminal content of a generic successful tool call to its output presentation and the terminal content of a generic failed tool call to its error presentation. It SHALL NOT place the same failed content in both fields or render it twice.

#### Scenario: Failed generic tool has diagnostic content
- **WHEN** a generic tool call completes as Failed with nonempty text content
- **THEN** the expanded row shows that text once as the error and has no duplicate output copy.

#### Scenario: Successful generic tool has output
- **WHEN** a generic tool call completes successfully with nonempty text content
- **THEN** the expanded row retains the text as output without manufacturing an error.
