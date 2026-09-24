## ADDED Requirements

### Requirement: Registered tool calls retain visible identity and terminal state

Shell SHALL give every registered tool input an explicit ACP start presentation and every tool output an explicit ACP projection with the original tool-call identity. Successful completed outputs SHALL close their rows; a backgrounded Bash output MAY remain in progress until its task finishes. Adding a new closed tool input or output variant SHALL require an explicit projection decision instead of silently falling through a wildcard.

#### Scenario: LSP and dynamic tool starts
- **WHEN** an LSP call or a dynamically registered tool starts
- **THEN** the Pager receives a start update identifying the actual operation or tool name, with the original raw input; neither starts as an anonymous `Tool call`.

#### Scenario: Context recall and dynamic tool results
- **WHEN** ContextRecall or a dynamic tool returns successfully
- **THEN** the same tool-call ID receives a Completed update with its result evidence, so the row stops running before the model turn ends.
