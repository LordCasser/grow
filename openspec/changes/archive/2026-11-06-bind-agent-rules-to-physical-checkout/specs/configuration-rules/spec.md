## ADDED Requirements

### Requirement: Project Agent rules follow physical checkout ancestry

Project Agent rule discovery SHALL derive cwd ancestry and the Git checkout boundary from the same physical path identity. It SHALL include checkout root through the actual cwd in root-to-cwd order and SHALL NOT add an external alias's lexical ancestors. Project rule source attribution SHALL identify the physical checkout paths. Explicit user roots, canonical candidate deduplication, and existing no-checkout and home-as-repository policies SHALL remain intact.

#### Scenario: External alias points inside a checkout

- **WHEN** the supplied cwd is an external directory alias pointing at a checkout's internal subdirectory
- **THEN** discovery includes the actual checkout root and internal rules in order and excludes AGENTS.md and rule directories belonging to external alias ancestors.

#### Scenario: Alias points at a linked worktree

- **WHEN** the supplied cwd resolves inside a linked worktree
- **THEN** discovery stops at that worktree's physical checkout boundary without importing main-checkout or alias-parent rules.

#### Scenario: Explicit workspace user roots remain authorized

- **WHEN** a configured workspace user rule root participates alongside the physical checkout chain
- **THEN** it retains its established insertion order and contributes each canonical rule file once.
