## Evidence
agent_view/paste.rs calls try_read_dropped_paths before checking PromptWidget::IMAGE_CAP. Dashboard uses try_read_images_from_paste, which filters this same result. The classifier currently collects every line's tokens into images before extending the accumulated result.
## Design
Resolve tokens sequentially and check each successful image's byte_len against remaining aggregate allowance before retaining it. Overflow returns an empty Vec, just like the existing whole-paste-or-nothing prose rule. Keep the public entry point and add a private budget-taking implementation for small deterministic tests.
## Limits
This bounds retained encoded payloads, not process RSS. One additional image can be read before admission, bounded by the existing 50 MB per-file reader; encoded payloads can therefore transiently reach 100 MB before rejection, with allocator/copy overhead additional. This does not bound decoded pixels, CPU time, or cumulative files tried and rejected. Existing prompt attachments are outside the per-paste budget.

Caller audit found separate pre-existing gaps: Dashboard image-only consumers discard mixed NonImage entries; deferred file_urls do not become fallback text when the original source has no text. These are recorded in backlog, not silently described as resolved by an empty classifier result. Existing original-text fallback remains available; no claim is made that all clipboard source variants retain text.
