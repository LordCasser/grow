# Design

`project_ui_notice` is the shared live/replay transcript boundary. Its existing structured `AgentMessageNotice::from_notice` validation is the authority for entering the new presentation. A valid receipt becomes `OtherToolCallBlock` with a `CommunicationBody`, concise source-derived title, `Received` status and immutable `parent-message:<receipt_id>` identity. The block is completed when inserted; it does not claim a message is being consumed or processed.

The immutable event identity is attached to the tool row so scrollback deduplication and replay equivalence continue to use the durable receipt ID even when transport event IDs differ. Cursor-tail append drops a repeated notice or parent-message receipt already present in the preserved transcript and carries forward its Minimal commit state; it does not treat ordinary tool rows as receipts. Full-replay Minimal frontier reconciliation recognizes the same identity on either the old notice or new tool-row projection, preventing already printed receipts from printing again after reload.

When a child view contains only a user prompt and a valid parent-message receipt, that receipt does not suppress the first replay of the remaining child transcript. Replay eligibility recognizes only a communication `OtherToolCallBlock` carrying the `parent-message:` identity and a structured body; an ordinary tool or outgoing communication row remains substantive and prevents a duplicate transcript replay.

Invalid structured identity follows the current raw-notice path. The implementation does not change Timeline receipt creation, reply validation, inbox consumption or passive inquiry phase rows.
