## ADDED Requirements

### Requirement: Unchanged Agent and Sampling selections do not claim a switch

Pager SHALL show an applied success notice for an Agent or Sampling selection only when applying it changes the effective selection. Agent no-op comparison SHALL continue to use the fully built runtime-equivalent profile and role prompt. Sampling model identity SHALL use the provider-qualified `ModelId`; reasoning effort is a separate selection attribute, and changing effort SHALL count as a real Sampling change even when the provider/model identity is unchanged. An unchanged selection SHALL still retain its authoritative terminal receipt, but it SHALL NOT synthesize a switch notice during terminal receipt repair or replay. While a control is Pending or Applying, the compact progress status SHALL omit a transition label when its current and desired targets are equal, without changing control admission or lifecycle state.

#### Scenario: The active Agent is selected again

- **WHEN** a requested Agent builds to a profile runtime-equivalent to the active Agent, including its role prompt
- **THEN** the control receives its normal applied receipt without an “Agent switched” success notice

#### Scenario: The active provider/model and effort are selected again

- **WHEN** a requested Sampling target has the active provider-qualified model ID and effective reasoning effort
- **THEN** the control receives its normal applied receipt without a “Sampling switched” success notice

#### Scenario: Effort changes on the active provider/model

- **WHEN** the provider-qualified model ID is unchanged but the requested reasoning effort differs
- **THEN** the change remains an applied Sampling change and retains its success notice

#### Scenario: An unchanged target is pending or applying

- **WHEN** a Pending or Applying control projection has equal current and desired targets
- **THEN** the compact progress status omits a transition label while the control remains in its normal lifecycle

#### Scenario: Same Agent identity has a changed profile

- **WHEN** the requested Agent has the same selector identity but its built profile is not runtime-equivalent to the active profile
- **THEN** the Agent runtime is replaced and the applied success notice remains visible

#### Scenario: A no-op receipt needs repair

- **WHEN** an applied no-op terminal projection is retried or repaired after its durable receipt was recorded
- **THEN** the receipt is restored without synthesizing a switch notice
