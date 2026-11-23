## ADDED Requirements

### Requirement: Context recall verifies the frozen journal frontier on indexed reload
ContextRecall parent validation SHALL reconstruct the frozen branch at the request's canonical sequence frontier when validated diagnostics are omitted from an indexed parent.

#### Scenario: The journal reader omits diagnostics during index migration
- **WHEN** the reader has validated a parent journal through the frozen frontier and removes diagnostic observations from its in-memory branch
- **THEN** recall validates the same revision, live coordinates and unloaded transcript leaves as dense replay. Unavailable frontiers, invalid coordinates and mismatched manifests still fail validation.
