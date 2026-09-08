## ADDED Requirements

### Requirement: Discoverable historical conversation search
Pager SHALL expose `/history [query]` through the existing session picker and session content index, including assistant results and content snippets. `/history --prompts` SHALL retain prompt recall and composer insertion. No separate conversation index SHALL be introduced.

#### Scenario: Search historical answer
- **WHEN** the user runs `/history deployment error`
- **THEN** the session picker opens with that query and requests content results immediately, showing matching snippets through the existing picker and allowing the selected session to be resumed.

#### Scenario: Browse before searching
- **WHEN** the user runs `/history` without a query
- **THEN** the picker opens with recent sessions and focused search input; editing the query uses the existing debounced content search.

#### Scenario: Recall a prompt
- **WHEN** the user runs `/history --prompts`
- **THEN** the existing prompt-history overlay opens and accepting a match inserts its text into the composer.

### Requirement: Conversation search failures and stale completions
Pager SHALL show a diagnostic for the current content search when the index is disabled, the request fails or times out. A failed search SHALL NOT be represented solely as an empty successful result. Completions from a closed picker or superseded query SHALL NOT mutate results or show diagnostics in the current picker.

#### Scenario: Index disabled
- **WHEN** the current content search returns the index-disabled diagnostic
- **THEN** loading ends and the user sees the diagnostic while the picker remains usable.

#### Scenario: Old search completes after reopen
- **WHEN** a picker is closed and reopened before an older request finishes
- **THEN** the old results and errors are ignored even if the reopened picker has the same query.
