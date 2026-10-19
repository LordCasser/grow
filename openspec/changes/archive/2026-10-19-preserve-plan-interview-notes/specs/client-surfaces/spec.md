## ADDED Requirements

### Requirement: Plan interview partial actions preserve freeform notes
Plan-mode `Chat about this` and `Skip interview` responses SHALL preserve non-empty selected freeform text in the existing per-question `annotations.notes` field. Their `partial_answers` SHALL continue to contain selected option labels, or `Other` when the answer has only freeform text. The tool result shown to the model SHALL include both the partial answer and its notes. Empty or unselected freeform input SHALL NOT be sent.

#### Scenario: Chat about this includes a freeform-only answer
- **WHEN** the user enters non-empty freeform text for a Plan question and chooses “Chat about this”
- **THEN** the response carries `Other` in `partial_answers` and the exact text in that question's `annotations.notes`, and the model-facing tool result includes the text.

#### Scenario: Skip interview includes option and freeform notes
- **WHEN** the user selects an option, enters non-empty freeform text, and chooses “Skip interview”
- **THEN** the response preserves the option label and exact text separately, and the model-facing tool result includes both.

#### Scenario: Unselected freeform input is omitted
- **WHEN** freeform input is empty or not selected when either partial action is submitted
- **THEN** no notes annotation is emitted for that question.
