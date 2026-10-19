## ADDED Requirements

### Requirement: Received agent messages use communication tool rows

Pager SHALL project an agent message receipt with valid structured identity as an ordinary communication tool row in the receiving Session. The row SHALL use the durable receipt ID for deduplication, show `Message from <source>` or `Reply from <source>` from structured facts, and display `Received` as its terminal status. A durable receipt SHALL NOT be shown as processing, consumed, read, executed, or completed work. The row SHALL preserve the complete message body and inspectable receipt/source/reply metadata. Notices without valid structured message identity SHALL remain finite notices.

#### Scenario: A parent message is durably received
- **WHEN** the receiving Session projects a valid parent-message receipt
- **THEN** its history contains one `Message from <source>` tool-style row with `Received` status, the original message body and inspectable receipt data; the row does not claim the child consumed or acted on the message.

#### Scenario: A reply is durably received
- **WHEN** a Session projects a valid agent-reply receipt with a structured `reply_to`
- **THEN** its history contains one `Reply from <source>` tool-style row with `Received` status and preserves the reply reference and original body in details.

#### Scenario: Duplicate delivery or replay uses a different transport event ID
- **WHEN** the same receipt is projected live, on replay, or after reconnect with different transport event IDs
- **THEN** the durable receipt identity keeps one row, and a previously committed Minimal receipt is not printed again.

#### Scenario: Receipt belongs to a child Session
- **WHEN** a child Session receives a valid message while another view is selected
- **THEN** the row belongs to that child's history without switching the selected view or entering the parent's history.

#### Scenario: A received row does not suppress the child's first transcript replay
- **WHEN** a child view contains only a user prompt and a valid receipt before its remaining updates have been replayed
- **THEN** replay restores the remaining child history after the receipt without duplicating it; an ordinary tool row still suppresses replay when history is already present.

#### Scenario: Message body is unavailable or notice identity is invalid
- **WHEN** a valid receipt has no recoverable body, or a notice lacks valid structured receipt identity
- **THEN** the valid receipt row retains its identity and shows `Message unavailable`; the invalid notice remains a bounded finite notice and does not enter a running-row lifecycle.
