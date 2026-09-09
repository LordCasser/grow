## ADDED Requirements

### Requirement: Shared budget windows do not confer delegated Goal ownership
A non-Goal child SHALL NOT acquire Goal turn ownership merely because its shared budget admission window names an active Goal. Child turn owner inference SHALL respect the delegated_goal provenance stamped from SubagentOwner. Root turn inference and shared provider admission enforcement SHALL remain intact.

#### Scenario: Ordinary Task child observes a parent Goal
- **WHEN** a non-Goal child with no inherited Goal context starts a turn while the shared window names an active Goal
- **THEN** it commits an unowned turn identity instead of an incomplete Goal owner.

#### Scenario: Goal-owned child starts
- **WHEN** delegated_goal is true
- **THEN** its turn retains matching Goal id and revision validation and cannot bypass invalid ownership as an ordinary Task.
