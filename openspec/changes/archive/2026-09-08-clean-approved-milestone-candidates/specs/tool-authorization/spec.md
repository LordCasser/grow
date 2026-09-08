## ADDED Requirements

### Requirement: Question output uses the active answer formatter
AskUserQuestion SHALL retain the active label-based answer formatter and SHALL NOT expose an internal unused alternate ID-keyed format selector. Question and option IDs, notes and validation SHALL remain available.

#### Scenario: Selected answer with notes
- **WHEN** a question response contains selected labels and notes
- **THEN** the active formatter preserves both without consulting a format selector.

### Requirement: Legacy unregistered Skill IO is removed
ToolInput and ToolOutput SHALL NOT expose the unregistered legacy Skill variants. Registered tools, dynamic ToolPack calls and skill prompt expansion SHALL remain available.

#### Scenario: Built-in skill capability
- **WHEN** the built-in registry is assembled
- **THEN** skills continue through discovery and prompt expansion without a legacy Skill tool IO variant.
