## ADDED Requirements

### Requirement: Cold context pressure follows portable replay

When cold restoration starts a new native continuation lane, ChatState SHALL initialize context pressure from the provider-neutral portable history that this lane can replay, without counting omitted historical reasoning. It SHALL retain the raw Surface estimate separately as the signed mutation baseline. The first real request SHALL add its current request envelope through the existing projection path, and later provider observations SHALL remain authoritative anchors. Initialization SHALL NOT delete history, start provider work, fabricate usage, or clamp genuine overflow to the model window.

#### Scenario: Resume retains reasoning that is omitted from replay

- **WHEN** a restored Timeline contains enough historical reasoning to make its raw estimate exceed the window, while portable message content fits
- **THEN** context pressure reflects the portable message estimate before the first prompt, and the raw reasoning remains available in Timeline.

#### Scenario: A new message follows restoration

- **WHEN** a committed Surface append and a normal request build follow cold initialization
- **THEN** the new content is counted once, and request projection replaces the prior envelope adjustment without reintroducing omitted reasoning or subtracting it twice.

#### Scenario: Portable content is genuinely too large

- **WHEN** portable replay itself exceeds the configured model window
- **THEN** the restored context count remains above the window rather than being clipped for display.
