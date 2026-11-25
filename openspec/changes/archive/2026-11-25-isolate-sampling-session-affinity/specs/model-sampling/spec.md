## ADDED Requirements

### Requirement: Stateful sampling transports distinguish independent conversations

Grow SHALL provide an opaque request-scoped conversation identity in the X-Session-Id HTTP header for primary sampling with a lineage key and for owned Sideband sampling, unless the route already configures that header. Chat Completions, Responses and Messages SHALL carry this identity independently of body formatting and evidence recording.

#### Scenario: Two sessions have the same opening

- **WHEN** two sessions with identical system instructions, first user message, working directory and route sample concurrently
- **THEN** their requests carry different identities. Appends and transport retries on one lineage retain its identity, and a different fork or rewind lineage uses its own key.

#### Scenario: Independent auxiliary work

- **WHEN** a Sideband operation samples while another Sideband or the primary conversation is active
- **THEN** its attempts carry its own stable operation identity and cannot inherit the primary or sibling identity.

#### Scenario: Explicit transport configuration and unowned requests

- **WHEN** the route supplies X-Session-Id explicitly, or a caller has no conversation or Sideband identity
- **THEN** the explicit value is preserved as a single header, and an unowned request without a configured value omits the header.
