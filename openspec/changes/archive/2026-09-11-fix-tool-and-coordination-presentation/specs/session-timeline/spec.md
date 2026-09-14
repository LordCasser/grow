## ADDED Requirements

### Requirement: Tool preflight failures retain their actual category

Tool preflight SHALL distinguish an unregistered tool name from arguments that cannot be parsed for a registered tool. An unregistered name SHALL NOT be dispatched and SHALL produce exactly one failed tool result that states the tool was unavailable and not executed. Invalid arguments for a registered tool SHALL retain the argument-parse diagnostic and original-argument recovery context.

#### Scenario: Unknown name with valid JSON
- **WHEN** an admitted model response calls an unregistered tool name with syntactically valid JSON
- **THEN** preflight records the call as a non-existing invalid tool, emits one failed result, and performs no tool dispatch.

#### Scenario: Registered tool has invalid arguments
- **WHEN** an admitted model response calls a registered tool with arguments its parser rejects
- **THEN** the existing argument-parse result remains available and is not relabeled as an unknown tool.
