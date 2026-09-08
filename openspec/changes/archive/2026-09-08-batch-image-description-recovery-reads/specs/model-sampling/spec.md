## ADDED Requirements

### Requirement: Image description recovery batches durable lookups
A recovery pass SHALL reuse one validated durable-history load for its uncached image-description queries. It SHALL preserve exact source revision, Surface identity, prompt and completed-result provenance checks, including integrity validation of other Sidebands required by the parent Timeline. It SHALL NOT retain the full loaded history across auxiliary provider requests.

#### Scenario: Several groups need durable reuse
- **WHEN** a recovery pass has multiple uncached image groups
- **THEN** their durable lookups share one history load and each query receives only its own matching completed result

#### Scenario: Historical Sideband integrity fails
- **WHEN** required Sideband validation fails during batch lookup
- **THEN** no result is accepted from that failed snapshot and existing description failure or provider fallback handling applies

#### Scenario: Every group is cached
- **WHEN** all group descriptions are available in the session cache
- **THEN** the pass performs no durable-description history load
