## ADDED Requirements

### Requirement: Rewind and route replacement rebase context pressure to portable replay

After a durable conversation rewind or a sampling route replacement, ChatState SHALL initialize context pressure and request-estimate baselines from the new continuation lane's portable history, using any portable reasoning rule retained by that lane. It SHALL publish the resulting pressure before a new provider request. Raw retained reasoning omitted by replay and the previous branch or route's provider anchor SHALL NOT inflate the new count. The raw Surface estimate SHALL remain the signed mutation baseline; subsequent request envelopes and provider observations SHALL use the existing projection and calibration path.

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
