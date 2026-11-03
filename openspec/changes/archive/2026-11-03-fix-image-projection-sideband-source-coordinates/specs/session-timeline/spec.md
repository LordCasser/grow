## ADDED Requirements

### Requirement: Image projection ownership covers allocated Surface coordinates

Timeline source ownership SHALL recognize every Surface coordinate allocated by an accepted `ImageProjection`: one coordinate per shadow, one per projected tool-call shadow, and one per carrier source. Coordinates at or beyond the number of allocated slots SHALL NOT be considered owned.

#### Scenario: Projected tool and carrier coordinates remain valid sources

- **WHEN** a Sideband selection refers to a tool-call or carrier Surface coordinate allocated by an acknowledged ImageProjection
- **THEN** Timeline recognizes that coordinate as owned, including after replay, while rejecting coordinates beyond the projection's allocated slots.

证据入口：`crates/codegen/chat-state/src/timeline.rs` — `Timeline::owns_surface_id`、ImageProjection live apply、`fold_branch_provenance`；回归为 `image_projection_scrubs_response_carrier_paths_from_compaction_summary` 和 `canonical_surface_coordinate_producers_are_complete`。
