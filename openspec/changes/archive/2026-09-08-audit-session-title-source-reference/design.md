# Findings
- Restored session actor creates a title route only when summary.display_title() is empty. Storage reconcile_session_title_projection uses canonical Timeline title events to repair stale summary projections and rejects conflicting projections.
- Manual rename validates nonempty/160-character bounds before dispatch; Timeline forbids later generated/fallback title events after a user title. Existing generated title text is not silently promoted over user intent.
- schedule_session_title(user_text) takes the one-shot route then materializes Timeline and uses input_ref.last_seq as the sole source event. Chat-state constructs last_seq from the most recent event of any kind, not the most recent direct user input.
- turn/admission commits the user item first, but can then consume notification receipts without an item and drain active notifications with a notification item before calling schedule_session_title. Both append Timeline facts. Concurrent actor commands can also append facts across awaits.
- The title request still uses original_prompt_text, so the resulting title can look correct while its Frozen input_ref identifies a notification/control fact instead of the user input it describes. This is concrete source-ordering evidence, not a live reproduction.
- consume_fifo_inputs currently erases the returned committed event through record_input_event; direct push_user_message_durably also returns only acknowledgement. Repair must identify the exact admitted input rather than just changing last_seq to an arbitrary earlier event.

# Next repair
Bind scheduling to the admitted direct-user event identity or an equivalently unambiguous materialized source identity. Preserve FIFO/direct/notification admission and one-shot/manual-title semantics. Do not bypass Sideband parent validation or assume Timeline tail equals input.
