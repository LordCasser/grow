## ADDED Requirements

### Requirement: Answering sessions consume consultation evidence durably
A new-contract successful incoming inquiry's canonical terminal SHALL establish one context-synchronization obligation in its original answering Session. Its structured proof SHALL bind the asking Session, original target Session, inquiry, actual Sideband and its frozen source. Only that original target's runtime SHALL materialize the obligation through the existing Timeline notification receipt/consumption path, validating the Sideband owner against the proof. Receipt identity SHALL be derived from canonical inquiry identity, and source version SHALL bind the successful terminal. Consumption and its exact model input SHALL be committed together before a request may include that input. Historical Answered observations without this new proof and UI presentation SHALL NOT establish obligations.

#### Scenario: Completed consultation is consumed
- **WHEN** an answering Session reaches an eligible sampling boundary after a successful incoming inquiry
- **THEN** it commits one consumed receipt with the exact attributed question and answer before building the request, without synthesizing a human permission or model-authored tool call.

#### Scenario: Consultation consumption omits model input
- **WHEN** live append or strict replay encounters a Consumed event selecting an InquiryAnswered receipt with no materialized input
- **THEN** it rejects the event rather than clearing the obligation without making consultation context visible; other sources retain their existing tool-result input-none rules.

#### Scenario: Success is durable but receipt is missing
- **WHEN** live reconciliation, same-identity retry or replacement-writer recovery finds a successful canonical inquiry terminal without its derived receipt
- **THEN** it reconstructs only the missing receipt from that exact terminal/Sideband evidence, without running the inquiry again or replacing Answered with Failed; the obligation is coordinated before subsequent foreground sampling.

#### Scenario: Consumption acknowledgement is unavailable
- **WHEN** consumption cannot be durably confirmed
- **THEN** no request claims to contain the unconfirmed input, the runtime follows existing Timeline ownership/reconciliation rules, and a confirmed historical consumption is not appended a second time.

#### Scenario: Receipt identity conflicts
- **WHEN** an existing inquiry-derived receipt is presented with conflicting source, terminal or payload
- **THEN** the conflict is rejected without rewriting the original receipt or guessing a new identity from question text.

#### Scenario: Historical success has no new-contract proof
- **WHEN** a stored Answered observation predates the structured synchronization proof, even if its UI shows the full question and answer
- **THEN** it remains historical consultation evidence and does not create a new synchronization obligation or trigger a backfill from presentation data.

#### Scenario: Read-only observation encounters an incomplete synchronization
- **WHEN** a session list, full/light observation, trajectory viewer or UI replay reads a valid success with a missing derived receipt
- **THEN** it remains read-only without acquiring writer ownership or appending the missing receipt; reconciliation belongs to the legitimate runtime writer.

### Requirement: Required notification consumption failures block dependent requests
The shared sampling-boundary notification path SHALL distinguish an empty eligible set from failure to read required payloads or confirm their consumption. A first request, successor Step or final-response continuation that depends on that boundary SHALL NOT dispatch after such failure. The existing fatal storage-boundary error path SHALL preserve the cause rather than treating the failure as ordinary completion recovery. This applies to existing notification sources using the same required-consumption path as well as consultation evidence.

#### Scenario: Required payload cannot be read
- **WHEN** a selected notification payload is unavailable while preparing the first or a subsequent model request
- **THEN** the request is not dispatched and the actual boundary error remains observable; the missing payload is not treated as an empty inbox.

#### Scenario: Final-response consumption fails
- **WHEN** a captured candidate requires consumption to continue the same Turn but the durable consumption fails or its acknowledgement is unavailable
- **THEN** no dependent continuation request is dispatched, the failure is not interpreted as a successful empty drain, and canonical committed facts are preserved.

### Requirement: Context-only receipts remain passive across execution boundaries
A context-only receipt SHALL NOT open an idle Turn or abort active execution. It SHALL remain pending across Turn cancellation, idle time and suspension until an otherwise authorized Turn/Step consumes it. Final-response settlement SHALL capture exact candidate receipt identities at an actor-owned boundary; only those candidates may justify a further Step while the original owner, budget and Behavior still authorize continuation. Later arrivals SHALL NOT be back-inserted into that candidate decision. If a successor Step is admitted, later arrivals may join its next normal boundary; if final closure wins, they SHALL wait for a later authorized Turn. Receipt ownership SHALL NOT move to a fork as actionable pending work.

#### Scenario: Idle Session receives only consultation context
- **WHEN** the answering Session is idle and its only new receipt is context-only
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
