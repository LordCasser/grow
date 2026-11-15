## MODIFIED Requirements

### Requirement: Context-only receipts remain passive across execution boundaries
A passive context-only receipt SHALL NOT open an idle Turn or abort active execution. Live delivery of a verified successful consultation MAY persist idle continuation eligibility on its canonical receipt when the original answering actor is eligible for idle injection. Only that unconsumed eligible receipt may justify one internal continuation through idle arbitration. User FIFO, foreground ownership, Goal admission and lifecycle gates take precedence. It SHALL remain pending across Turn cancellation, idle time and suspension until an otherwise authorized Turn/Step consumes it. Final-response settlement SHALL capture exact candidate receipt identities at an actor-owned boundary; only those candidates may justify a further Step while the original owner, budget and Behavior still authorize continuation. Later arrivals SHALL NOT be back-inserted into that candidate decision. If a successor Step is admitted, later arrivals may join its next normal boundary; if final closure wins, they SHALL wait for a later authorized Turn. Receipt ownership SHALL NOT move to a fork as actionable pending work.

#### Scenario: Idle Session receives only consultation context
- **WHEN** the answering Session is idle and its only new receipt is passive context-only
- **THEN** no Turn or provider request starts; a later otherwise authorized Turn can include it at its first sampling boundary.

#### Scenario: Consultation races the final response
- **WHEN** context-only receipt admission races the Turn's final-close cutoff
- **THEN** only the captured candidates may justify a next Step when continuation is otherwise authorized; later arrivals wait for a subsequent authorized boundary, and if final closure wins they wait for a later Turn. Neither outcome cancels the in-flight request, overrides a budget/termination gate or adds input to an ended Turn.

#### Scenario: A later receipt joins an already-admitted successor
- **WHEN** one receipt is captured before the final-response candidate snapshot, a second arrives afterward, and the first legitimately causes a successor Step in the same Turn
- **THEN** the second does not change the old candidate decision but may be consumed at the successor's normal safe boundary, without a new Turn or a new durable minimum-Turn route.

#### Scenario: Passive delivery races loss of continuation authority
- **WHEN** a passive receipt exists before final-close but cancellation, exhausted budget or a committed control transition closes the original Turn's continuation authority
- **THEN** the receipt remains pending for later authorized consumption and cannot reopen that Turn, change the Goal state or force a provider request.

#### Scenario: Consumption commits before a request can be sent
- **WHEN** a receipt's Consumed fact is durable but the process stops before sending the intended model request
- **THEN** recovery reconstructs the committed Surface without consuming again or claiming the model observed it; subsequent authorized requests use normal context projection and admission.

#### Scenario: Cancel or suspend occurs before consumption
- **WHEN** the owning Turn is cancelled or the Session is suspended while consultation context is pending
- **THEN** the original Session retains the unconsumed receipt and no passive delivery wakes it; restoring the Session does not create a synthetic Turn solely for that receipt.

#### Scenario: Fork inherits history
- **WHEN** a Session is forked with already-consumed consultation history and other pending receipts
- **THEN** its inherited context follows existing fork history/provenance rules without inheriting the original Session's pending delivery or consumption authority.

#### Scenario: Fork inherits an unanswered synchronization obligation
- **WHEN** a fork contains historical evidence of a successful consultation owned by the source Session but no derived local receipt
- **THEN** the fork does not reconstruct a local pending receipt from that inherited evidence; only the original answering Session may fulfill the obligation.

#### Scenario: An eligible live consultation is retried

- **WHEN** a live idle consultation is received and the same canonical inquiry or receipt is retried
- **THEN** the first permitted internal or otherwise authorized Turn consumes its exact context once, and the retry cannot start a second continuation.

#### Scenario: Live idle admission loses to existing work or a closed gate

- **WHEN** a real input takes precedence, the actor has foreground work, or automatic admission is closed
- **THEN** the receipt follows the next authorized context boundary and does not preempt or override the owner or gate.
