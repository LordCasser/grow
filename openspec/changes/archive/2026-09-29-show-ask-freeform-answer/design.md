## Existing path

`QuestionViewState::build_accepted_response` encodes freeform-only selection as `Other` with the actual text in `annotations[question].notes`. The active formatter preserves those notes in the tool result, and ACP forwards that text. Pager's `parse_ask_user_qa_pairs` removes the `user notes:` suffix while preparing the expanded Ask row; it also leaves the label's closing quote behind when notes follow.

## Change

Keep the existing response and formatter. Adjust only the accepted-result display parser in `pager/src/scrollback/blocks/tool/other.rs`: separate the quoted selected label from optional preview and notes, use notes as the visible answer for `Other`, and append notes to an ordinary selected label when present. Keep the existing rendering path so live and replayed tool results use the same projection. Test parser output and the expanded block with the active formatter's result shape.

Plan-mode `Chat about this` and `Skip interview` use a different, intentionally lossy partial-answer response. Leave it for a separate change.
