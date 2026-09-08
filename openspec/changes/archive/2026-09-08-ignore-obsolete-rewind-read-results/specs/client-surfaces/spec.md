## ADDED Requirements

### Requirement: Rewind reads belong to the requesting interaction
The client SHALL apply points and preview results only to their current requesting interaction, session and phase. Dismissal SHALL invalidate the pending read. Starting a new read SHALL supersede the previous read. Execution results SHALL retain their existing reconciliation behavior.

#### Scenario: Dismissed read completes
- **WHEN** points or preview completes or fails after its interaction was dismissed
- **THEN** it does not reopen an overlay, alter the draft or show a failure toast.

#### Scenario: New interaction supersedes a read
- **WHEN** a new read starts before a previous result arrives
- **THEN** the previous success or failure does not alter the new interaction.

#### Scenario: Session ownership
- **WHEN** a result belongs to a session no longer attached to that agent
- **THEN** the result is ignored.

#### Scenario: Another view is active
- **WHEN** the owning agent and session still await the read while another view is active
- **THEN** the result updates its owning agent normally.

#### Scenario: Current points read fails
- **WHEN** the current points request fails
- **THEN** the interaction closes, restores its stashed draft and reports the failure.
