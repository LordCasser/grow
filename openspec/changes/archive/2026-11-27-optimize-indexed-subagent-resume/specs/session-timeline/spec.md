## ADDED Requirements

### Requirement: Delegated resume uses targeted lifecycle proof and indexed source recovery
Shell SHALL resolve a resume_from source by searching the owning parent's committed lifecycle facts for the requested delegation newest-first. For an indexed lifecycle lookup, unrelated published parent history SHALL stay on disk. If the committed suffix needs execution state to validate and no matching resident writer cut is available, Shell SHALL validate it through ordinary working-state recovery before looking up the delegation. The source child SHALL use the same validated working cut and canonical references as ordinary indexed resume to recover its effective model, Surface, inherited presentation and required evidence. A missing usable publication SHALL preserve canonical recovery and all existing authorization and result-link checks.

#### Scenario: Several children resume under one long-lived parent
- **WHEN** requests identify different completed delegations in a published parent, including lifecycle records in the committed suffix
- **THEN** each request resolves its exact spawn and matching terminal, checks the requesting owner, and binds the child seed and result without materializing the full published parent conversation.

#### Scenario: Source child has extensive closed history
- **WHEN** a resumed child has a usable working publication and a validated suffix
- **THEN** the derived child receives the full eligible conversation and ordered inherited display, current model route and exact result proof, without replaying unrelated published execution history.

#### Scenario: Evidence is incomplete or invalid
- **WHEN** indexed recovery cannot establish a valid source cut or the parent and child facts disagree
- **THEN** the existing canonical recovery and rejection rules apply, preserving source identity, validation errors and the original session.
