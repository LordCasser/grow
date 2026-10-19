## ADDED Requirements

### Requirement: Human interaction waits have durable attempt boundaries

Timeline SHALL persist typed open and resolution facts for user question, permission, and Plan approval reverse-request attempts. Each fact SHALL identify the original tool call, interaction kind, and unique attempt. The facts SHALL NOT contain question text, user answers, permission option contents, or tool arguments. Timeline SHALL reject duplicate opens, mismatched resolutions, duplicate resolutions, and resolutions without an open. Live dispatch SHALL persist Opened before sending the reverse request and persist a terminal resolution for answer, rejection, cancellation, timeout, transport failure, malformed response, or suspension. A request SHALL NOT be sent when its Opened fact is unconfirmed. Cold recovery SHALL leave an unclosed historical attempt unknown rather than assigning it a synthetic resolution time. A rebound request SHALL use a fresh attempt identity.

#### Scenario: Human question is answered

- **WHEN** a user question reverse request is sent and answered
- **THEN** Timeline contains one matching Opened/Resolved pair and exposes the interval from their recorded event times without persisting question or answer content in those facts.

#### Scenario: Permission is decided automatically

- **WHEN** local permission policy grants or denies a tool without sending a human reverse request
- **THEN** Timeline creates no human-wait interval for that decision.

#### Scenario: Request cannot be sent or resolved durably

- **WHEN** Opened cannot be confirmed, or a request returns but its matching Resolved append cannot be confirmed
- **THEN** the first case does not send the reverse request; the second remains unresolved/unknown and is not projected as a completed wait.

#### Scenario: Restore rebinds an unanswered request

- **WHEN** a suspended unanswered request is reissued after restoration
- **THEN** the old attempt is closed at the durable suspension boundary and the new reverse request uses a distinct attempt identity; the offline interval is not counted as human response time.

### Requirement: Activity excludes validated suspension offline intervals

Replay activity SHALL exclude time between a validated `TreeSuspended` event and its matching `ExecutionReleased` event for the same operation. `Resumed` alone SHALL NOT end the offline interval. A missing, mismatched, or reversed pair SHALL remain unknown and SHALL NOT be converted into confirmed idle or execution time. Other operation spans that overlap the offline interval SHALL remain protected. Recovered terminal timestamps SHALL continue to use explicit recovery-gap treatment rather than representing the full offline interval as active work. After idle, recovery, and offline ranges are split around one another, only final fragments strictly longer than 30 seconds SHALL be compressed; shorter fragments SHALL retain their recorded duration.

#### Scenario: Runtime resumes after suspension

- **WHEN** an open tool/request spans a valid tree suspension and later execution release
- **THEN** the suspension-to-release interval is excluded from protected activity while pre-suspend and post-release work remains protected.

#### Scenario: Another operation remains active

- **WHEN** one node is suspended while a separate captured operation remains active outside that suspension owner
- **THEN** that operation's known activity remains protected.

#### Scenario: Restore commit is not release

- **WHEN** `Resumed` is durable but `ExecutionReleased` is absent
- **THEN** the runtime remains treated as offline/closed through the captured frontier.

#### Scenario: Short fragments around a long gap

- **WHEN** verified offline ranges split a longer candidate gap into pieces shorter than or equal to 30 seconds
- **THEN** those short pieces remain at recorded speed, only longer final fragments are compressed, and source-to-playback mapping remains monotone.
