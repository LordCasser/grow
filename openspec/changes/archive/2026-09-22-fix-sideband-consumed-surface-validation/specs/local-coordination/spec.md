## ADDED Requirements

### Requirement: Valid prior Sidebands do not disable later inquiries

A session whose prior Sideband selected valid canonical Surface coordinates SHALL remain able to durably receive and answer peer and direct parent-child inquiries. Foreground activity and active background subagents SHALL NOT be used as an inquiry admission gate.

#### Scenario: Inquiry after compaction while a child remains active

- **WHEN** a target completed compaction over consumed user or notification input, its foreground is idle or busy, and a background subagent remains active
- **THEN** a peer or directly related agent can durably record the receipt, obtain the tool-free Sideband answer and record the terminal audit without a Surface-selection `audit_failure`.
