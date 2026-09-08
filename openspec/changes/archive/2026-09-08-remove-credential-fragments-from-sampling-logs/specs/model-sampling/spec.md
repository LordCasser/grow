## ADDED Requirements

### Requirement: Sampling authentication logs omit credential fragments
Sampling client construction/request events and sampling request spans SHALL describe authentication using type and presence metadata without raw credential prefixes, suffixes or complete values. This SHALL NOT alter outgoing authentication headers or the independent 401 attribution callback.

#### Scenario: Short bearer or API key
- **WHEN** a request is built with a short bearer token or x-api-key
- **THEN** sampling event/span logs contain no credential value while the built request retains the configured authentication header.
