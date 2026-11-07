## ADDED Requirements

### Requirement: ACP plugin updates adopt their resulting install state

A successful plugin update performed through `grow/plugins/action` SHALL adopt the update result into the existing install registry through the shared plugin application owner. When a mutable Git branch advances without an operation or persistence failure, the recorded commit, update timestamp and discovered plugin map SHALL describe the resulting checkout. Marketplace registrations SHALL retain their existing provenance-aware update path. The action SHALL retain its existing selection and reload semantics without duplicating the registry or updating Git state independently of the shared application owner.

#### Scenario: An installed branch adds a plugin

- **WHEN** the remote branch advances to a new commit containing a new plugin and a session performs an ACP Update successfully
- **THEN** the checkout and saved install entry use that commit, the saved plugin map reflects the discovered plugins, and the action requests the existing reload path.

#### Scenario: The updated branch is already at its tip

- **WHEN** a second ACP Update targets the same already adopted remote tip
- **THEN** it reports no update, does not request a reload, and does not manufacture a new update timestamp or commit change.

#### Scenario: Selection and nonmutable entries

- **WHEN** an ACP Update selects one installed plugin or encounters a pinned or live local registration
- **THEN** the existing selected-repository scope and pinned/local policy remain in effect, and only actual adopted updates request reload.
