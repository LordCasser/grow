## ADDED Requirements

### Requirement: Skill identity and terminal status survive width changes and history paging
Skill rows SHALL preserve the requested identity, resource and terminal outcome in live and fully reconstructed history. At narrow widths, the row SHALL reserve room for the outcome and show a distinguishing resource suffix when space permits. Detailed failures SHALL expose the returned reason.

#### Scenario: Long paths on a narrow terminal
- **WHEN** a 64-column display shows two resources with a long common prefix
- **THEN** the collapsed rows show their distinct file names or suffixes and their success or failure status.

#### Scenario: Large tool output is reconstructed from fragments
- **WHEN** a skill tool output exceeds the inline history limit and all its fragments are restored
- **THEN** the reconstructed tool retains the original structured kind, requested identity, resource and outcome.

#### Scenario: Qualified names and failed or unpublished loads
- **WHEN** two namespaces provide the same short skill name, or the loader rejects a request or cancels an unpublished group member
- **THEN** terminal rows retain their distinct requested identities, and the failed or cancelled load exposes the corresponding outcome and reason.
