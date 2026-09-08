## Why
Rewind points and preview tasks outlive Esc dismissal. Their results unconditionally rebuild UI state, or overwrite a newer request for the same agent.

## What Changes
Bind each read to a unique request and its issuing session. Accept the result only for the current read in its matching phase; dismiss invalidates ownership. Preserve execute-result reconciliation.
