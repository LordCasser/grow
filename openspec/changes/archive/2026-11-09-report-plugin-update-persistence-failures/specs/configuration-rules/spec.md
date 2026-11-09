## ADDED Requirements

### Requirement: Plugin update success requires its registry publication

Shared CLI and ACP plugin updates SHALL propagate existing registry load failures before repository side effects. An ordinary Git update SHALL not report Updated when its resulting registry publication fails; the error SHALL identify known changed repositories and incomplete persistence without claiming that checkout or other successfully completed updates were rolled back. Marketplace transactions SHALL retain their existing save and rollback ownership. No-change updates SHALL not introduce a new registry publication requirement.

#### Scenario: Ordinary update save fails
- **WHEN** a Git checkout has advanced but its resulting registry save fails
- **THEN** CLI returns an error and ACP returns InternalError without requesting reload or restart, describing the known checkout change and incomplete registry state.

#### Scenario: Registry cannot be loaded
- **WHEN** update encounters an existing registry read or parse failure
- **THEN** it fails before changing any repository rather than treating the registry as empty.

#### Scenario: Explicit retry after persistence recovers
- **WHEN** the user explicitly retries the incomplete update after storage recovers
- **THEN** actual checkout/registry facts determine adoption, the correct commit and plugin map can converge, and a subsequent same-tip update remains a no-op.

#### Scenario: No ordinary registry change
- **WHEN** selected repositories are unchanged, pinned, live-local or handled by their existing transactional publication
- **THEN** their original outcomes and publication ownership remain, without an unnecessary ordinary registry save.
