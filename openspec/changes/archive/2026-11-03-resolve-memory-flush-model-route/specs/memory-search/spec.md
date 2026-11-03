## ADDED Requirements

### Requirement: Explicit memory flush models resolve their complete transport

An explicit flush_model SHALL resolve through the existing auxiliary model configuration resolver before creating its client. Provider endpoint, backend, credentials, actual request model and image capability identity SHALL come from the same resolved configuration. Session-local attribution SHALL retain the existing auxiliary stamping rules. An unavailable configured model SHALL fail without falling back to the primary route or writing memory. With no override, one prepared primary configuration SHALL remain authoritative.

#### Scenario: Flush model belongs to a different provider
- **WHEN** a visual primary uses one provider and the configured text-only flush model resolves to another endpoint or backend
- **THEN** the flush goes to the resolved flush transport, image rejection learns only that transport, neutral recovery uses its configured vision route, and the text retry can persist memory without altering the original images.

#### Scenario: Flush model cannot be resolved
- **WHEN** an explicit flush model alias is unavailable
- **THEN** flush fails without a provider request, primary-route fallback or memory write.
