## ADDED Requirements

### Requirement: Root owns the shared Goal admission lifecycle
Only the root session SHALL synchronize the shared Goal provider admission window from its durable Goal tracker. Descendants SHALL consume that window for admission and usage settlement without replacing its lifecycle from their local tracker.

#### Scenario: Spawn a child during an active Goal
- **WHEN** an active root Goal spawns a descendant whose local Goal tracker is empty
- **THEN** descendant initialization preserves the root Goal identity and both sessions can admit requests under that identity.

#### Scenario: Descendant synchronizes while root admission is closed
- **WHEN** root admission is inactive, exhausted, or usage-incomplete and a descendant initializes or synchronizes local control
- **THEN** the descendant cannot reopen admission or replace its root state.

#### Scenario: Root lifecycle transition
- **WHEN** the root durably pauses, restarts or exhausts its Goal
- **THEN** its shared admission window follows that transition and stale Goal requests remain rejected.
