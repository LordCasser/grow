## ADDED Requirements

### Requirement: Session setup and Agent catalogs do not require Git configuration

Session creation/loading, Agent catalog discovery and configuration inspection SHALL NOT open Git configuration solely to locate the checkout or Git directory. An unavailable Git config SHALL NOT prevent those discovery steps. Operations that actually consume Git contents SHALL retain their own errors rather than declaring the repository healthy from a path-only result.

#### Scenario: Checkout config is a cloud placeholder
- **WHEN** session setup or Agent catalog discovery runs in a valid checkout whose Git config cannot be read
- **THEN** root-only discovery completes without triggering config reads, allowing session setup and catalog publication to continue.

#### Scenario: Actual Git content is unavailable
- **WHEN** a Git content operation needs configuration or other unavailable repository data
- **THEN** the existing Git operation reports its failure; path discovery does not synthesize content or suppress that failure.
