## ADDED Requirements

### Requirement: Pager permission routing requires an exact session owner

Pager SHALL admit an ACP permission request only when its session ID exactly matches a registered root session or registered child view. The startup fallback used to route ordinary notifications to an active root whose session ID has not yet been assigned SHALL NOT apply to permission requests. An unmatched request SHALL be cancelled without being queued or changing session permission mode.

#### Scenario: Stranger permission arrives during root startup
- **WHEN** a permission request carries an unmatched session ID while the active root has no assigned session ID
- **THEN** Pager cancels the request and does not queue it on the active root.

#### Scenario: Benign update arrives during root startup
- **WHEN** an ordinary session update arrives before the active root's session ID is assigned
- **THEN** Pager retains the existing startup routing behavior for that update.

#### Scenario: Exact root or registered child requests permission
- **WHEN** a permission request carries an exact registered root or child session ID
- **THEN** Pager routes it to the owning root interaction queue using the existing behavior.
