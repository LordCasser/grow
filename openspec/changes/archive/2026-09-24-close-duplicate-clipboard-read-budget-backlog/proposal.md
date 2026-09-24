# Change: Close duplicate clipboard read-budget backlog item

## Why

The clipboard read/decode backlog entry groups limits that are now documented in the client-surfaces contract with a separate unbounded native-call concern already tracked by the native clipboard deadline item. Its remaining byte cap is enforced for temporary image files, and header inspection does not perform full pixel decoding. Keeping the broad item would duplicate those records and imply an independently cancellable local-file read deadline that the current synchronous file owner cannot provide.

## What Changes

- Remove only the duplicate “剪贴板其他读取与解码预算” entry from `openspec/backlog.md`.
- Record the evidence and scope boundary for that decision.

## Impact

Documentation-only. No runtime behavior or accepted contract changes; `skip_specs: true` applies.
