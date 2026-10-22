## ADDED Requirements

### Requirement: Historical Markdown defers only derived presentation work

Pager SHALL complete every delivered historical assistant/thinking source, including pending normalizer bytes and existing terminal lifecycle/timing, before crossing the existing full-history load barrier. It MAY defer rebuildable final Markdown presentation for offscreen content. Raw and pretty copy, hyperlinks, source maps, diagrams and visible layout SHALL return complete results equivalent to eager finalization before those results are used. Image-reference filesystem observation SHALL remain at the existing message completion boundary. Offscreen estimates SHALL NOT depend on cache warmth; exact viewport layout and source/entry anchors SHALL remain consistent after materialization, resize and raw/pretty changes. This SHALL NOT introduce pagination, early readiness or background history restoration.

#### Scenario: Offscreen source completes before first drawing
- **WHEN** historical source ends with buffered normalization bytes and its message remains offscreen
- **THEN** source/timing/order are complete at the existing load barrier, while first pretty copy or drawing materializes the complete final presentation without missing text or stale maps.

#### Scenario: Media and diagram observations survive deferred presentation
- **WHEN** a historical message references an image or contains a Mermaid fence, and image files later change before its first drawing
- **THEN** message-completion image reference discovery/validation retains its existing observation, and Mermaid source/affordance rows match eager output; deferred pure presentation does not postpone filesystem observation.

#### Scenario: Materialization preserves display and source anchors
- **WHEN** a user scrolls to a formerly offscreen historical block, copies it, changes width/raw mode or later receives another chunk
- **THEN** rendered content, links, source maps and logical selection/entry anchors stay equivalent to eager processing; estimates do not change merely because a cache was warmed, and normal input remains governed by the existing full-history load boundary.
