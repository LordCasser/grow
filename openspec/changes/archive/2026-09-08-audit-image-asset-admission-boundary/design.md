# Evidence
- turn/admission persists image_files assets before constructing user_chat and before consume_fifo_inputs/push_user_message_durably. input_commit errors abort without asset rollback.
- FIFO consume records InputEvent::Consumed through record_timeline_event_durably; its error becomes String. Direct push returns TimelineWriteError::AcknowledgementLost if reply is lost. Actor commits then sends result, so missing acknowledgement cannot prove absence of durable reference.
- persist_user_images creates UUID filenames each call. Repeating the same normalized bytes creates new files; persisted inbox image blobs already use a separate content-addressed directory, but normalized output may differ from those original blobs.
- Blind cleanup on input_commit Err could delete files referenced by a committed message. The previous batch-local rollback remains correct for its exclusive fresh files; extending it to shared content-addressed assets would require ownership redesign, since another reference can share the same content.

# Next repair
Make repeated preparation reuse stable assets with explicit ownership/reference semantics, and preserve potentially committed references after acknowledgement uncertainty. Do not infer garbage from missing replies or parse arbitrary prompt text as the only source of ownership. See make-image-assets-retry-safe.
