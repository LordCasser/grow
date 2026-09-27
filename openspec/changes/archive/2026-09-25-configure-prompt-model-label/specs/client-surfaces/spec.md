## ADDED Requirements

### Requirement: Prompt model label optionally includes provider

Pager SHALL offer a persistent Settings choice between a compact `model (effort)` prompt footer label and the canonical `provider/model (effort)` label. Compact SHALL be the default. The choice SHALL apply to active session, child and Dashboard composers on the next render without changing the selected model or effort. The effort suffix SHALL appear only when an effective effort exists.

#### Scenario: Compact default

- **WHEN** the provider preference is unset or disabled and the active route is `bigmodel/glm-5.3` with display name `GLM-5.3` and effort `max`
- **THEN** the prompt footer displays `GLM-5.3 (max)`.

#### Scenario: Show provider

- **WHEN** the user enables the preference in Settings for that route
- **THEN** the prompt footer displays `bigmodel/glm-5.3 (max)` on the next render and future launches retain the choice.

#### Scenario: Dashboard and child composer

- **WHEN** a Dashboard selection or opened child has its own model and effort
- **THEN** its prompt footer applies the same preference to that view's selected route and effective effort.

#### Scenario: Persistence fails

- **WHEN** writing the preference fails after a live Settings change
- **THEN** Pager restores the prior display choice and reports the setting failure through its existing path.
