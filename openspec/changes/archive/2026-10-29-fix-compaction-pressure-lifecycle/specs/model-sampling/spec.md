## ADDED Requirements
### Requirement: Context mutations use effective request estimates
ChatState SHALL compute context pressure from the effective current request input estimate plus the current route's provider calibration overhead. Raw Surface content omitted from replay SHALL NOT be subtracted from provider-visible pressure. Surface mutation, request assembly, native reset and learned reasoning changes SHALL preserve this coordinate; repeated request projection SHALL be idempotent. Route replacement and rewind SHALL reset calibration; other replacements SHALL retain valid same-route overhead. Failed durable writes SHALL NOT publish changed pressure. Lifetime usage SHALL remain independent.

#### Scenario: Compaction removes mostly omitted reasoning
- **WHEN** raw removed history exceeds the current pressure while portable retained history is nonempty
- **THEN** pressure reflects retained effective content and overhead without an intermediate saturated subtraction, and a later request cannot recover phantom tokens.

#### Scenario: Explicit request projection changes
- **WHEN** an image route invalidates cached selection, native continuation resets, or portable reasoning replay changes
- **THEN** context pressure is recomputed immediately and a changed count is published before another request is built.

#### Scenario: Projection and mutation boundaries
- **WHEN** append, pruning, image projection, repair, schema changes or repeated builds alter the effective request
- **THEN** changed content is counted once, omitted history stays omitted, and valid provider overhead is preserved.

### Requirement: Known input overflow cannot bypass provider admission
Before ordinary provider dispatch the Session SHALL check the current request pressure against its current configured window independently of auto-compaction suppression and task output budgeting. Model-switch compaction errors SHALL propagate through existing cancellation and failure boundaries rather than authorize sampling. Automatic recovery MAY attempt existing bounded compaction; a still oversized request SHALL fail without provider dispatch.

#### Scenario: Smaller model and failed compaction
- **WHEN** 600K context on a 1M model switches to a 500K model and compaction fails or leaves a known overflow
- **THEN** the turn fails without sending the ordinary oversized request, even if compaction suppression is active.

#### Scenario: Suppression or child budget disables automatic compaction
- **WHEN** a fully projected ordinary request exceeds its configured window and automatic compaction is unavailable
- **THEN** provider dispatch is rejected while existing history and usage remain intact.

## MODIFIED Requirements

### Requirement: Cold context pressure follows portable replay

When cold restoration starts a new native continuation lane, ChatState SHALL initialize context pressure from the provider-neutral portable history that this lane can replay, without counting omitted historical reasoning. It SHALL retain effective input and request-envelope estimates in the same coordinate. The first real request SHALL add its current request envelope through the existing projection path, and later provider observations SHALL remain authoritative anchors. Initialization SHALL NOT delete history, start provider work, fabricate usage, or clamp genuine overflow to the model window.

#### Scenario: Resume retains reasoning that is omitted from replay

- **WHEN** a restored Timeline contains enough historical reasoning to make its raw estimate exceed the window, while portable message content fits
- **THEN** context pressure reflects the portable message estimate before the first prompt, and the raw reasoning remains available in Timeline.

#### Scenario: A new message follows restoration

- **WHEN** a committed Surface append and a normal request build follow cold initialization
- **THEN** the new content is counted once, and request projection replaces the prior envelope adjustment without reintroducing omitted reasoning or subtracting it twice.

#### Scenario: Portable content is genuinely too large

- **WHEN** portable replay itself exceeds the configured model window
- **THEN** the restored context count remains above the window rather than being clipped for display.

### Requirement: Rewind and route replacement rebase context pressure to portable replay

After a durable conversation rewind or a sampling route replacement, ChatState SHALL initialize context pressure and request-estimate baselines from the new continuation lane's portable history, using any portable reasoning rule retained by that lane. It SHALL publish the resulting pressure before a new provider request. Raw retained reasoning omitted by replay and the previous branch or route's provider anchor SHALL NOT inflate the new count. Subsequent mutations and request envelopes SHALL use effective input estimates with same-route provider calibration, without raw Surface delta adjustments.

This initialization SHALL NOT change cumulative consumption or durable conversation content, start provider work, or clamp genuine portable overflow. Failed rewind SHALL NOT reset pressure. Ordinary sampling parameter updates and non-rewind compaction SHALL retain their existing provider-anchor behavior.

#### Scenario: Rewind crosses compaction before switching models

- **WHEN** rewind restores pre-compaction history with large reasoning omitted by portable replay and the user then replaces the model route
- **THEN** rewind and route replacement each publish portable context pressure, preserving original history, and the first and repeated requests do not reintroduce the omitted reasoning.

#### Scenario: Route replacement invalidates the previous provider anchor

- **WHEN** the selected provider, model or backend route is replaced after a provider context observation
- **THEN** the counter immediately reflects portable history before the first new request, and later provider observations calibrate it normally.

#### Scenario: Portable content really exceeds the new window

- **WHEN** portable history itself exceeds the new model's context window
- **THEN** the count remains above that window rather than being capped for display.

#### Scenario: Failed rewind and ordinary configuration updates

- **WHEN** rewind is rejected or cannot durably commit, or only same-route sampling parameters change
- **THEN** the current context pressure and provider calibration remain intact.
