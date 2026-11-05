## ADDED Requirements

### Requirement: Subagent tool calls can select reasoning effort

The model-facing subagent spawn tool SHALL expose optional `effort` alongside optional `model`. Fresh task calls SHALL forward a supplied effort through the existing subagent runtime policy, with precedence over Agent/model defaults. The resolved model's existing effort validation, sampling configuration and durable spawn projection SHALL remain authoritative. Omission or JSON null SHALL preserve existing default resolution. The canonical `"none"` token SHALL remain an explicit effort value. Resume calls SHALL retain the source model and effort, ignoring tool overrides as with the existing model override.

#### Scenario: Select model and effort together
- **WHEN** a fresh subagent tool call supplies a selectable model and an effort offered by that model
- **THEN** the child uses that model and effort through the existing runtime and provider request path.

#### Scenario: Select effort on the resolved model
- **WHEN** a fresh tool call supplies effort without model
- **THEN** the existing model resolution is retained and that model's offered-effort validation applies to the explicit effort.

#### Scenario: Omit effort
- **WHEN** the tool call omits effort or supplies JSON null
- **THEN** existing Agent/model effort defaults apply without an explicit runtime policy.

#### Scenario: Explicit none
- **WHEN** a fresh tool call supplies `effort: "none"`
- **THEN** it remains an explicit policy for runtime/model validation rather than being discarded as an absent argument.

#### Scenario: Invalid or unsupported effort
- **WHEN** the supplied token is invalid or the resolved model does not offer it
- **THEN** the existing child runtime returns its effort failure and does not issue a child provider request using a substituted effort.

#### Scenario: Resume retains the source selection
- **WHEN** a resume call also supplies model or effort overrides
- **THEN** the resumed child retains its validated source model and effort, including when those ignored tool overrides are invalid.
