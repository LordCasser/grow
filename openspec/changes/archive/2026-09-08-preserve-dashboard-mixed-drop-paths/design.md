## Evidence
handle_bracketed_paste, handle_paste_key_deferred and complete_clipboard_attachment_paste all use try_read_images_from_paste. The shared try_read_dropped_paths already represents ordered Image/NonImage entries and enforces whole-paste classification plus batch budget.
## Design
Add one private Dashboard method that classifies and inserts the ordered entries into dispatch or peek. Reuse attach_pasted_image/attach_peek_pasted_image and insert_pasted_caption, mapping insertion results to existing completion variants. Continue processing ordinary paths even if an image insertion fails at its cap. Any successful insertion yields Handled, matching the existing multi-image completion behavior. Callers retain question/target admission guards.
## Boundaries
Do not change the question-mode image detection used solely for discard feedback. No changes to agent prompt, clipboard source enum, or classification policy. The classifier may return empty on prose or aggregate overflow; those cases keep existing caller fallback.

The old Dashboard test expected FullMiss for a missing explicit file URL. That conflicts with the shared classifier's intentional NonImage result for explicit URLs, and with preserving recognized paths. Dashboard now inserts that decoded path as text (Handled, no image). The regression is retained with this explicit new expectation; truly unclassified input is still outside this change.
