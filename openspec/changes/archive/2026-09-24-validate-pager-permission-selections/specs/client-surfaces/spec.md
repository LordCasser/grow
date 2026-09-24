## ADDED Requirements

### Requirement: Pager validates permission selection against the queued request

Pager SHALL accept a permission selection only when its option ID belongs to the exact front request in the active Agent's permission queue. It SHALL perform this check before removing the request, sending a response, changing session permission mode, or applying any other selection side effect. An invalid selection SHALL leave the request queued and its response channel open, with no permission-mode change.

#### Scenario: Selection ID was not offered for the front request
- **WHEN** a selection names an option ID absent from the front request, including the global Always Approve ID on a child request
- **THEN** Pager leaves the queue and session mode unchanged and sends no response.

#### Scenario: Selection ID belongs to the front request
- **WHEN** a selection names an option ID offered by the front request
- **THEN** Pager handles it using the existing response, queue-transition, and applicable mode-change behavior.
