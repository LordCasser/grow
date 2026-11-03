## ADDED Requirements

### Requirement: Session deletion reconciles runtime and local views on every outcome

Delete SHALL preflight canonical eligibility inside the existing session lifecycle transaction before retiring a resident runtime, then revalidate under the existing writer/quarantine transaction. A predictable refusal SHALL preserve the runtime. Errors SHALL distinguish a preserved binding from one invalidated by teardown; absence of a trustworthy response SHALL remain unknown. Pager SHALL retain the original binding only when preservation is confirmed; invalidated/unknown outcomes SHALL release that stale binding while retaining undeleted history and recoverable drafts, so explicit resume performs a real load. Success SHALL require an affirmative server acknowledgement and retire matching local views for every after policy, including Stay. Unrelated views, drafts and picker state SHALL be preserved; navigation SHALL never remain on a removed view. Frontend delete SHALL NOT issue independent cancellation before backend preflight.

#### Scenario: Predictable deletion refusal
- **WHEN** canonical references or invalid target evidence reject preflight
- **THEN** no Cancel/Shutdown is issued and the original bound runtime remains usable with the refusal visible.

#### Scenario: New facts cause refusal after teardown
- **WHEN** facts committed during shutdown make final deletion ineligible
- **THEN** history remains, the response identifies binding invalidation, and Pager permits explicit resume rather than focusing the old dead binding.

#### Scenario: Delete transport outcome is unknown
- **WHEN** delete has no trustworthy acknowledgement or preservation fact
- **THEN** Pager does not claim deletion or keep presenting the old binding as live; it preserves recoverable local content and can reconcile through resume.

#### Scenario: Stay deletion matches a local view
- **WHEN** successful after=Stay deletion targets a locally open session
- **THEN** matching views retire, unrelated active views and picker state remain, and an active deleted view gets a valid remaining surface.

#### Scenario: Deletion response arrives after replacement
- **WHEN** a deletion response arrives after the original AgentId/binding epoch has been replaced, including a fresh binding to the same logical session id
- **THEN** that response cannot retire the replacement page or unregister its active identity.
