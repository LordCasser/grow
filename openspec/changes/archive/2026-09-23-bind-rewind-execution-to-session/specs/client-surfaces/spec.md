## ADDED Requirements

### Requirement: Rewind execution feedback belongs to its source session binding
The client SHALL carry the source session ID and binding epoch with an executing rewind. A result for the same binding SHALL reconcile its committed success or explicit rejection with that Agent view even when another view is active. A result for an obsolete binding SHALL NOT mutate the replacement transcript, composer, inline editor or rewind overlay; it SHALL visibly report the previous session's confirmed success, explicit rejection or unknown transport outcome. A confirmed success outside its original binding SHALL direct the user to reload that session for an authoritative projection.

#### Scenario: Source binding remains active
- **WHEN** an execution result arrives after the user switches to another view without unbinding its source session
- **THEN** the result reconciles against the owning Agent's unchanged binding.

#### Scenario: Source binding is replaced
- **WHEN** an execution result arrives after its Agent unbinds or binds another session
- **THEN** the replacement view's transcript, overlay and draft remain unchanged and a previous-session notice describes the result.

#### Scenario: Same session ID is rebound
- **WHEN** the source session ID is unbound and later rebound before an execution result arrives
- **THEN** the old binding's result does not mutate the new binding and the user is told to reload or verify the session.

#### Scenario: Session changes during inline resubmit
- **WHEN** a binding change interrupts an executing inline-edit rewind
- **THEN** the old pending resubmit cannot be sent into the replacement binding, the old rewind overlay cannot appear there, and unsent draft/edit text remains in the local composer.

#### Scenario: Execution response is lost
- **WHEN** transport or response parsing fails after an execution was sent
- **THEN** the client reports an unknown outcome and directs verification rather than asserting that no rewind committed.

## MODIFIED Requirements

### Requirement: Rewind reads belong to the requesting interaction
The client SHALL apply points and preview results only to their current requesting interaction, session and phase. Dismissal SHALL invalidate the pending read. Starting a new read SHALL supersede the previous read. Execution results SHALL follow the separate session-binding reconciliation requirement.

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

#### Scenario: Session ID is reused after unbinding
- **WHEN** a read was issued before unbinding and the same session ID is subsequently rebound
- **THEN** its result is ignored because it belongs to an earlier binding lifetime.

#### Scenario: Same binding is reiterated
- **WHEN** the current session ID is bound again without an intervening unbind or identity change
- **THEN** a pending matching read remains valid.
