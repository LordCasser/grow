## MODIFIED Requirements

### Requirement: Memory flush retains bounded completed tool evidence

A memory-flush Sideband SHALL derive its provider request from one frozen Timeline Surface. It SHALL retain the result body and attachments of completed, unambiguous tool exchanges in the selected recent context, even when no Assistant message follows the result. It SHALL omit incomplete or ambiguous tool protocol, select only complete User turns within an explicit input budget, and fail without writing a summary when the newest turn cannot fit. Tool output SHALL be treated as untrusted data and SHALL NOT enter permission authority. Snapshot capture SHALL wait for an in-progress primary image conversion and its durable ImageProjection acknowledgement, observe session Sideband cancellation, and select every available acknowledged image description on the request copy before budgeting. Image evidence with a description SHALL be represented by that text without raw media, independently of negative capability. A supplied pre-compaction snapshot SHALL retain its original source IDs, revision and input range even if live context advances. The client, explicit request model and image capability key SHALL use the same prepared configuration, including the resolved explicit flush-model transport. Only the final selected window SHALL be considered for image recovery. Known rejecting routes SHALL recover neutral descriptions before provider admission. Unknown routes retain their first optimistic image request; explicit image capability rejection SHALL record that actual route and permit one recovered text attempt from the same snapshot, within the admitted budget. Missing descriptions or exceeded budget SHALL fail without a memory write. Flush SHALL NOT mutate the primary Surface and SHALL release the image projection gate before answer sampling.

#### Scenario: Tool result is the only completion evidence

- **WHEN** a User says deployment has not happened, an Assistant plans and calls a deployment tool, and the completed result reports a revision without a subsequent Assistant reply
- **THEN** the memory-flush provider request includes the completed call, its result and attachments or their acknowledged image descriptions, and the Sideband attempt identifies the frozen source revision and selected Surface coordinates

#### Scenario: Selection would split an exchange or exceed the budget

- **WHEN** the recent frozen turn contains an incomplete or ambiguous call, or its complete request exceeds the memory-flush input or attachment budget
- **THEN** incomplete protocol is omitted and older whole turns are dropped as needed; if the latest turn still exceeds the budget, the flush fails without persisting a summary

#### Scenario: Images already have acknowledged descriptions

- **WHEN** a flush snapshot contains described user or tool images and the flush request uses a text-only model
- **THEN** budgeting and the provider request use those descriptions without their raw media, preserving tool evidence and the immutable main image facts

#### Scenario: Main conversion is pending

- **WHEN** a flush snapshot is requested during primary image conversion or its pending durable ACK
- **THEN** capture waits for acknowledgement or Sideband cancellation; selected unresolved images on a known text-only route use neutral recovery, failing without a memory write when recovery is unavailable

#### Scenario: Compaction or later input changes live context

- **WHEN** a valid pre-compaction flush snapshot is captured and the live Surface subsequently changes
- **THEN** the flush uses only the supplied snapshot's content, selected source IDs, revision and input range without rebasing to the newer Surface

#### Scenario: Unresolved images are outside the selected window

- **WHEN** old unresolved images fall outside the bounded recent User turns and the retained request fits a known text-only route
- **THEN** those unsent images do not block the flush or trigger conversion

#### Scenario: Flush model override changes image capability identity

- **WHEN** an explicit flush model differs from the prepared primary model or transport
- **THEN** unresolved image validation uses the actual flush model's transport identity and does not inherit a different primary model's negative capability


#### Scenario: Frozen images are first rejected during flush

- **WHEN** the explicit flush route first rejects unresolved images after pre-compaction capture and live history advances
- **THEN** recovery and its single text retry use the old snapshot IDs, revision and input range only; accepted text is written to memory and the durable ledger reloads with those original sources.
