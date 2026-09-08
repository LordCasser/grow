## ADDED Requirements

### Requirement: Retired inert configuration surfaces are not exposed
Shell SHALL NOT expose the empty unstable Cargo feature or the unconsumed ZDR-access setting/resolver. Removing these inert surfaces SHALL NOT introduce a new access policy.

#### Scenario: Shell feature and setting inventory
- **WHEN** inspecting supported Shell build features and remote setting types
- **THEN** the empty unstable feature and unused zdr_access_enabled field are absent.
