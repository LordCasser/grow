## ADDED Requirements

### Requirement: Accepted Ask answers show submitted freeform text

Pager SHALL render non-empty freeform notes from an accepted `ask_user_question` result in its expanded Ask answer row. When the answer is only the `Other` placeholder, the row SHALL show the submitted text in its place. When a selected option also has freeform notes, the row SHALL show both the option and submitted text. Answers without freeform notes SHALL continue to show their selected labels. This display projection SHALL NOT change the model-facing tool result or the persisted result text.

#### Scenario: Freeform-only answer

- **WHEN** the user submits an accepted answer using only the freeform field
- **THEN** the expanded Ask row shows the entered text as the answer, without `Other` or a stray quote.

#### Scenario: Selected option with additional text

- **WHEN** the user submits an accepted option and non-empty freeform notes
- **THEN** the expanded Ask row shows both the selected label and the entered text.

#### Scenario: Selected option without freeform text

- **WHEN** the user submits an accepted option without notes
- **THEN** the expanded Ask row shows the selected label as before.
