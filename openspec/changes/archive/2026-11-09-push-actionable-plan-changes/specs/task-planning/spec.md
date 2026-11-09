## MODIFIED Requirements

### Requirement: Effective non-completion edits request replanning

An effective edit to a linked parent TODO's content, requirements or priority, or reopening a completed step SHALL record one logical objective-change responsibility per eligible directly linked unfinished child. Removal, replacement omission or cancellation SHALL instead record termination-and-wrap-up guidance for each such execution. Completion alone, progress-only transitions between pending and in_progress, repeated identical values, ordering-only changes and display/receipt/review updates SHALL remain quiet. A completed status combined with other intent edits SHALL still notify. TODO status SHALL NOT itself constitute proof of child terminal state, command termination or result acceptance.

#### Scenario: TP-06 Parent changes requirements or priority
- **WHEN** content, acceptance requirements or priority changes on a linked parent step
- **THEN** only its linked unfinished children receive Grow's fixed objective-change guidance, including the revised step and requirements. The child uses the delivered payload and reads or asks only for details it needs to make a decision.

#### Scenario: TP-07 Completion alone differs from completion with edits
- **WHEN** one step only becomes completed and another is edited while becoming completed
- **THEN** only the latter generates objective-change guidance; neither update fabricates execution termination or acceptance.

#### Scenario: TP-08 Reopen or cancel a parent step
- **WHEN** a completed step becomes pending or in_progress, or a step becomes cancelled
- **THEN** reopening notifies still eligible executions of changed intent, while cancellation issues fixed termination-and-wrap-up guidance; an execution already told to stop is not resumed by reopening.

#### Scenario: TP-09 No-op and presentation writes stay quiet
- **WHEN** effective fields are identical or only ordering, UI, receipts, usage or reviews change
- **THEN** no objective-change or stop responsibility or extra model check is created.

#### Scenario: Parent updates only progress
- **WHEN** the linked step changes between pending and in_progress with identical content, requirements and priority
- **THEN** the plan advances without creating a control or a new acknowledgement obligation.

### Requirement: Replanning is child-owned and bounded by safe execution

Grow SHALL produce fixed runtime-attributed objective-change and termination messages from committed planning facts and soft-interrupt the original child's sampling or interruptible wait after durable receipt. The child SHALL handle them at the existing execution boundary. Objective changes SHALL preserve applicable work, update only affected local TODOs and direct the child to ask its parent when intent is unclear. Termination SHALL direct it to stop advancing that objective, preserve work, perform necessary cleanup and settlement, and report results, remaining work and uncertainty. Grow SHALL NOT hard-cancel the child before it can report or claim an ongoing command already stopped. Handling acknowledgements SHALL identify the exact source through the structured plan action. Routine handling SHALL NOT require a separate prose confirmation or parent message; receiving a message or lacking a child TODO SHALL NOT discharge the responsibility.

#### Scenario: TP-10 Child changes only affected work
- **WHEN** a parent adds a test requirement after investigation was completed
- **THEN** the child preserves that investigation, changes remaining implementation or verification as needed, and acknowledges the exact change.

#### Scenario: TP-11 Existing plan still applies
- **WHEN** the child determines objective-change guidance is already covered by its current work
- **THEN** it may acknowledge with a reasoned no-change decision without inventing TODO edits; this decision cannot revoke termination guidance.

#### Scenario: Stop handling records acceptance without claiming completion
- **WHEN** a child durably consumes exact termination guidance and acknowledges it through HandleReplans
- **THEN** handling records the stopping outcome as acceptance of wrap-up, rejects an ordinary no-change decision for that stop, and leaves execution, commands, results and required delivery obligations to their existing completion owners.

#### Scenario: TP-12 Parent edit arrives during a command or wait
- **WHEN** objective-change or termination guidance arrives while an external command or interruptible wait is running
- **THEN** sampling or the wait yields at its safe interruption point, existing command/result ownership remains truthful, and the child handles the fixed instruction before advancing affected new work.

#### Scenario: TP-13 Child cannot finish the check
- **WHEN** the child cannot determine how the new objective applies
- **THEN** it keeps the exact unresolved responsibility and can ask its parent; ordinary prose or message consumption does not count as handling.

### Requirement: Planning context is constructed by Grow without user-chat insertion

Grow SHALL inject bounded planning and delegation context into the provider request using the current local plan and one initial direct-parent observation per foreground turn. It SHALL include the linked step even when it lies outside the initial summary window, its location in the overall plan, relevant child associations and access to todo_read for specific missing facts. The initial parent snapshot SHALL be presented as background and SHALL NOT instruct the child to poll, confirm or report on the plan. Ordinary change obligations SHALL be actionable from delivered guidance. A child without a local TODO SHALL still receive its delegation context. The context SHALL be counted in the effective request budget and SHALL NOT be appended to the authored task prompt, durable Messages, Surface, ordinary chat history or unsolicited user-facing notifications.

#### Scenario: First child request locates its parent step
- **WHEN** a newly admitted or resumed child makes its first provider request
- **THEN** the request contains the current delegated step and overall position, while its user prompt remains authored text and no synthetic plan chat row is published.

#### Scenario: Repeated requests refresh plan context
- **WHEN** the parent edits its plan after the child has built its first request in that foreground turn
- **THEN** subsequent request builds reuse the same parent observation without a new parent query. The child receives effective changes through typed notifications and the request still projects the current local plan. No planning MemoryContext items are accumulated and retrieved-memory persistence remains independent.

#### Scenario: Parent is unavailable
- **WHEN** a bound child cannot read the current parent owner
- **THEN** Grow identifies the unavailable source and retained delegation identity rather than presenting a fabricated empty plan.

#### Scenario: Repeated snapshots during unchanged work
- **WHEN** the child makes multiple requests without a new parent change notification
- **THEN** each snapshot provides background only. A parent progress or delivery field does not create a read, acknowledgement or reporting instruction.
