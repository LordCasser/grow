## Why

An accepted `ask_user_question` freeform answer reaches the model as `"Other" user notes: <text>`, but Pager's expanded Ask row strips the notes and shows `Other"`. The user's submitted answer is therefore hidden in the conversation view even though the tool result retains it.

## What Changes

- Render accepted freeform notes as the visible answer. A freeform-only `Other` answer shows the typed text instead of the placeholder; a selected option with additional notes shows both.
- Preserve the existing answer wire format, model-facing tool result, and other Ask outcomes.
- Add focused parser and expanded-row regression coverage.

The Plan-mode partial-answer path also discards freeform notes before formatting. That separate behavior is recorded in the backlog, not changed here.
