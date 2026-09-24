# Change: Bind session picker list fetches to their view

## Why

The previous change sequences picker requests, but a current response still has no view identity. An Agent modal fetch uses `app.cwd` while its selection anchor uses that Agent's session cwd. Switching between already-open Agent or child modals can deliver a response to the wrong view without changing the global request sequence.

## What Changes

- Capture the visible picker view, its session binding, and cwd when dispatching a list request.
- Send that cwd to `grow/session/list` and apply the response only while the same view and binding remain visible.
- Key relaxed-scope notice suppression to the actual request cwd.

## Impact

Pager picker request/result routing and scoped list notices. No Shell API or persisted format changes.
