## ADDED Requirements

### Requirement: Automatic recap is suppressed during reconnect
Pager SHALL NOT dispatch automatic recap requests while reconnect initialization or session reload is pending. Suppressed poll attempts SHALL remain no-ops and SHALL NOT record automatic retry backoff. A focus-return event during reconnect SHALL drop that away-period recap opportunity under the existing best-effort behavior. After reconnect completes, recap availability SHALL follow the replacement shell's refreshed capability; manual requests and a later away period SHALL use the normal admission path.

#### Scenario: Replacement disables recap while session reload is delayed
- **WHEN** the previous shell advertised recap, a replacement shell disables it, session reload is still pending, and the automatic recap poll fires
- **THEN** Pager sends no recap request and records no automatic retry attempt; after reload completes, the refreshed disabled capability remains authoritative

#### Scenario: Focus returns during reconnect
- **WHEN** focus returns after the recap threshold while reconnect is pending
- **THEN** Pager sends no automatic recap request and drops only that away-period opportunity; after reconnect, manual requests and a later away period follow the refreshed capability normally
