## ADDED Requirements

### Requirement: Usage exposes provider-model totals and cache-hit rates
`/usage` SHALL display total token consumption and provider/model breakdowns using full input plus output including cache-hit input. It SHALL show cached input and cache-hit percentages overall and per provider/model, within the labeled ledger reporting window. The identity SHALL be captured from the selected catalog route before sampling, not from provider-returned model aliases.

#### Scenario: Switch provider or model
- **WHEN** calls use different provider/model identities, including providers serving the same wire model name
- **THEN** prior charges remain in their original groups and overall usage is their cumulative sum.

#### Scenario: Weighted total rate
- **WHEN** models have different input volumes and cache hits
- **THEN** overall rate is total cache-hit input divided by total input, rather than the arithmetic mean of model percentages; each model shows its own total, input, output, cached input and rate.

#### Scenario: Single model and empty input
- **WHEN** only one model has usage or an entry has zero input
- **THEN** the model identity is still shown and a zero denominator displays N/A rather than a fabricated percentage.

#### Scenario: Incomplete or invalid usage
- **WHEN** the ledger is incomplete or cached input exceeds reported total input
- **THEN** incomplete rates are identified as based on recorded usage and invalid ratios display N/A; no misleading exact overall percentage is asserted.

#### Scenario: Existing display paths
- **WHEN** usage is opened in fullscreen, inline or minimal mode
- **THEN** the same statistics projection is shown, and `/session-info` remains unchanged.
