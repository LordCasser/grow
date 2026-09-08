## ADDED Requirements

### Requirement: Export path completion bounds directory enumeration
Export path completion SHALL inspect at most1,000 directory iterator results per request, counting hidden entries and read errors toward that limit, and SHALL return at most100 visible suggestions.

#### Scenario: Directory contains mostly hidden entries or errors
- **WHEN** directory iteration yields hidden names or failed entries before visible files
- **THEN** those results consume the enumeration budget and the iterator is not advanced beyond1,000 results

#### Scenario: Normal visible entries
- **WHEN** visible files and directories occur within the budget
- **THEN** existing prefix insertion, directory suffixes and directory-first sorting remain intact
