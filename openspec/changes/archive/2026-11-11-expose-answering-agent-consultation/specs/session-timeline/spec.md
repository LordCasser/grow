## MODIFIED Requirements

### Requirement: Answering sessions consume consultation evidence durably
A new-contract successful incoming inquiry's canonical terminal SHALL establish one context-synchronization obligation in its original answering Session. Its structured proof SHALL bind the asking Session, original target Session, inquiry, actual Sideband and its frozen source. Only that original target's runtime SHALL materialize the obligation through the existing Timeline notification receipt/consumption path, validating the Sideband owner against the proof. Receipt identity SHALL be derived from canonical inquiry identity, and source version SHALL bind the successful terminal. Consumption and its exact model input SHALL be committed together before a request may include that input. New consumption SHALL use the runtime AgentMessage context item, with its receipt ID, requester and answering Session IDs, inquiry ID, and exact receipt body containing the complete question, answer and frozen Sideband result reference. The context projection SHALL preserve the receipt payload and confer no message reply authority. Historical Answered observations without this new proof and UI presentation SHALL NOT establish obligations.

#### Scenario: Completed consultation is consumed
- **WHEN** an answering Session reaches an eligible sampling boundary after a successful incoming inquiry
- **THEN** it commits the receipt and exact attributed AgentMessage together before building the request. The host projection carries the question and answer as context without synthesizing human permission or a model-authored tool call.

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

#### Scenario: Previously consumed notification text is replayed
- **WHEN** Timeline restores an inquiry already consumed as notification text before typed consultation context was introduced
- **THEN** it retains that historical model input and creates no second receipt, consumption or Sideband execution.
