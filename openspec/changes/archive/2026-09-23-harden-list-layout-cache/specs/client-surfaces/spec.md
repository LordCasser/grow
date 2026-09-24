## ADDED Requirements

### Requirement: List layout queries respect cached item bounds
The Pager list layout cache SHALL report per-item geometry only for indexes represented by the cache, and SHALL support appending items to either fixed-height or variable-height layouts without panicking.

#### Scenario: Query outside an empty or populated cache
- **WHEN** a caller requests an item's virtual y or height at an index greater than or equal to the cached item count
- **THEN** the query returns no geometry instead of fabricating a position or height.

#### Scenario: Append to either layout representation
- **WHEN** incremental item heights are appended to a fixed-height cache
- **THEN** the cache count grows by the number of appended items and each appended item has height one.
- **WHEN** incremental item heights are appended to a variable-height cache
- **THEN** each height and its corresponding prefix sum are added consistently.
