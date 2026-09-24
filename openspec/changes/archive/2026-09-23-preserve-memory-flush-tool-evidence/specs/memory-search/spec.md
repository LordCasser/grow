## ADDED Requirements

### Requirement: Memory flush retains bounded completed tool evidence

A memory-flush Sideband SHALL derive its provider request from one frozen Timeline Surface. It SHALL retain the result body and attachments of completed, unambiguous tool exchanges in the selected recent context, even when no Assistant message follows the result. It SHALL omit incomplete or ambiguous tool protocol, select only complete User turns within an explicit input budget, and fail without writing a summary when the newest turn cannot fit. Tool output SHALL be treated as untrusted data and SHALL NOT enter permission authority.

#### Scenario: Tool result is the only completion evidence

- **WHEN** a User says deployment has not happened, an Assistant plans and calls a deployment tool, and the completed result reports a revision without a subsequent Assistant reply
- **THEN** the memory-flush provider request includes the completed call, its result and attachments, and the Sideband attempt identifies the frozen source revision and selected Surface coordinates

#### Scenario: Selection would split an exchange or exceed the budget

- **WHEN** the recent frozen turn contains an incomplete or ambiguous call, or its complete request exceeds the memory-flush input or attachment budget
- **THEN** incomplete protocol is omitted and older whole turns are dropped as needed; if the latest turn still exceeds the budget, the flush fails without persisting a summary
