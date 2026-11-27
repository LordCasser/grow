## ADDED Requirements

### Requirement: An open primary session recovers from a missing runtime handle

Pager SHALL start one ordinary in-place session load for the bound primary session when a current user prompt or model selection receives Shell's pre-admission invalid-params rejection tagged `error_kind=session_not_resident`. It SHALL retain recoverable input for explicit retry and surface load failures. A stale response, other application error, or child control failure SHALL NOT initiate a replacement root load.

#### Scenario: A long-lived tab outlives its actor
- **WHEN** Shell rejects the current prompt or model request for the open primary session because its handle is absent
- **THEN** Pager loads the same session and cwd through its existing replay window, preserves the rejected draft when applicable and does not automatically submit that input again.

#### Scenario: An already recovering or changed view receives a rejection
- **WHEN** a rejection belongs to an obsolete prompt, control token or different session, or the matching view is already reloading
- **THEN** Pager does not start another load for that result.

#### Scenario: Session info is unavailable
- **WHEN** Shell returns a session-info status with no display payload
- **THEN** Pager displays that lookup outcome and the reported reason when present; malformed payloads remain errors.
