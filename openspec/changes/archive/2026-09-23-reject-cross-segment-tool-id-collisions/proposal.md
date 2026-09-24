# Change: Reject tool ID collisions across portable and native exchanges

## Why

Portable history rejects duplicate neutral call IDs, but request segmentation checks that region separately from retained native spans. An older neutral call and a distinct native call can reuse the same original ID and both enter the provider request, making their results ambiguous. Messages ID encoding deliberately preserves native IDs and therefore cannot repair that ambiguity at the wire encoder.

## What Changes

- Detect when a native tool call ID is also owned by an assistant call outside its native span.
- Fail closed to one fully portable projection for that request. The existing duplicate-call filter then omits the ambiguous tool protocol while retaining ordinary conversation facts; the native opaque continuation is not sent.
- Preserve a native call whose neutral mirror is inside its own span and whose result follows it outside the span.

## Impact

Provider request projection for Chat Completions, Responses and Messages; no Timeline mutation or tool re-execution.
