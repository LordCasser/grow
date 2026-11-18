## Why

A Responses provider omitted `annotations` from `output_text` in part-done and item-done frames. The saved exchange also contains a completed response with the same omission. The existing decoder only handles an empty initial part, so ordinary text can fail after HTTP 200 with a serialization error.

## What Changes

- Extend the decode-only annotation default to text parts in the observed close and terminal paths.
- Compose it with the existing terminal usage-detail completion. Preserve the original response bytes and all other SDK and protocol validation.
