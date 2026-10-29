## ADDED Requirements

### Requirement: Pager precise navigation follows visible sticky geometry
Pager scrollback entry-top and center navigation SHALL account for sticky prompt rows only when the renderer displays sticky headers: sticky headers are enabled and compact prompt mode is off.

#### Scenario: Top navigation with sticky headers disabled
- **WHEN** Pager navigates an entry to the viewport top while sticky headers are disabled
- **THEN** the entry is positioned at the viewport top without reserving sticky-header rows

#### Scenario: Center navigation with sticky headers disabled
- **WHEN** Pager centers an entry while sticky headers are disabled
- **THEN** the entry is centered against the full viewport without reserving sticky-header rows

#### Scenario: Precise navigation with visible sticky headers
- **WHEN** Pager navigates an entry with sticky headers enabled and compact prompt mode off
- **THEN** top navigation places it at the sticky content area's top and center navigation accounts for the displayed header

#### Scenario: Precise navigation with compact prompts
- **WHEN** Pager navigates an entry while compact prompt mode is active
- **THEN** top and center navigation use the full viewport because the renderer displays no sticky header
