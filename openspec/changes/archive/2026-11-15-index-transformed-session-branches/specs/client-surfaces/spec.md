## ADDED Requirements

### Requirement: Transformed branches page selected canonical history

Interactive history pages after a published rewind or image projection SHALL seek the selected branch at a captured cut. They SHALL preserve original display order, exact identities, and response and Tool continuation coordinates with bounded request work.

#### Scenario: Earlier preview and later canonical admission cross the selected cut
- **WHEN** a rewind preserves or discards a response with a distinct physical candidate anchor
- **THEN** recent and older pages apply the same response ownership and display selection as full history, keeping the original anchor for surviving rows.

#### Scenario: An older page arrives after another branch selection
- **WHEN** the response belongs to a retired branch cut
- **THEN** it does not update the new view, execution readiness or live notification cursor.

#### Scenario: An image projection has replaced a model-visible image
- **WHEN** the user requests an older page of that session
- **THEN** the page applies the existing historical display policy without using display evidence to restore a removed image to the current model Surface.
