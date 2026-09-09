## ADDED Requirements

### Requirement: Interjection display preserves user text across resume
Mid-turn user input SHALL persist its user-facing text separately from model-only interjection instructions and skill expansion. Resume SHALL display that persisted user text without exposing the runtime envelope. Literal markup authored by the user SHALL remain intact; display SHALL NOT be recovered by parsing model prompt tags.

#### Scenario: Resume after accepted steering
- **WHEN** accepted mid-turn user input is consumed and its display update is replayed
- **THEN** the display text is the sanitized original input while model context retains its interjection envelope.

#### Scenario: Direct interjection injection
- **WHEN** an interjection without queued input identities is injected
- **THEN** the same display/model separation applies and image blocks remain available.

#### Scenario: User authors prompt-like markup
- **WHEN** the user input includes literal user_query tags
- **THEN** those tags remain part of the user text and are not parsed or removed as runtime framing.
