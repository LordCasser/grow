## MODIFIED Requirements

### Requirement: Skill publication uses projected request context pressure

User and model skill publication SHALL check existing context using ChatState's current provider-anchored request projection, including committed Surface deltas and the current request envelope adjustment. The commit SHALL add the complete provider-visible contribution of the new user, result and skill items and recheck the current Surface revision and reserved headroom. Raw history omitted by provider projection SHALL NOT independently cause rejection; known provider overhead SHALL NOT be discarded. The dependent request's final projection and budget validation SHALL remain required.

#### Scenario: Raw history exceeds projected request size

- **WHEN** raw history includes reasoning omitted by the request projection and the current projected context plus a complete skill group fits the reserved budget
- **THEN** the group may publish atomically without rejecting solely because the raw Surface estimate is larger.

#### Scenario: Provider accounting exhausts available space

- **WHEN** the current provider anchor plus complete new skill content exceeds reserved headroom, even without a Surface revision change
- **THEN** user publication rejects without consuming or exposing any part of the group, and model publication uses its existing paired rejection result without publishing skill guidance.

#### Scenario: Model and user loads have different wire representations
- **WHEN** a model skill has both a typed fact and its tool result, including after tool-result pruning, or the user explicitly loads a skill
- **THEN** publication headroom and the live and portable estimates count the remaining model tool result once and include the user load. The durable facts remain available.

### Requirement: Stateful sampling transports distinguish independent conversations

Grow SHALL provide an opaque request-scoped conversation identity in the X-Session-Id HTTP header for primary sampling with an explicit conversation owner and for owned Sideband sampling, unless the route already configures that header. Chat Completions, Responses and Messages SHALL carry this identity independently of body formatting and evidence recording.

#### Scenario: Two sessions have the same opening

- **WHEN** two sessions with identical system instructions, first user message, working directory and route sample concurrently
- **THEN** their requests carry different identities. Appends and transport retries on one lineage retain its identity, and a different fork or rewind lineage uses its own key.

#### Scenario: Independent auxiliary work

- **WHEN** a Sideband operation samples while another Sideband or the primary conversation is active
- **THEN** its attempts carry its own stable operation identity and cannot inherit the primary or sibling identity.

#### Scenario: Explicit transport configuration and unowned requests

- **WHEN** the route supplies X-Session-Id explicitly, or a caller has no conversation or Sideband identity
- **THEN** the explicit value is preserved as a single header, and an unowned request without a configured value omits the header.

#### Scenario: Requests reuse a cache key
- **WHEN** separate conversations share a prefix-cache key or an unowned request supplies a cache key
- **THEN** independently owned requests use their own transport identities, an unowned request gains no transport identity from its cache key, and Responses can still send the caller's cache key.

#### Scenario: Cold restoration starts a new continuation lane
- **WHEN** a cold actor reconstructs a committed conversation
- **THEN** it sends an independently identified request on its new lane containing committed portable history and its retained tail. Normal appends and retries of this lane keep their identity.
