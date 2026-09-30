## ADDED Requirements

### Requirement: Goal usage surfaces show measured cache-hit rates

Pager SHALL show a Goal-specific input cache-hit rate alongside Goal token usage in the compact status, detail overlay, and read-only Goal transcript details. It SHALL divide cached input by cached plus uncached classified input; unclassified input and historical consumption without categories SHALL NOT become zero-hit samples. The ratio SHALL be marked as measured when cache classification or total usage is incomplete. With no valid classified-input denominator, it SHALL display N/A. Goal budget consumption and token-total completeness remain independent of this ratio.

#### Scenario: Fully classified Goal input

- **WHEN** a Goal has 200 cached and 300 uncached input tokens with no unknown usage
- **THEN** its usage surfaces show a 40.00% cache-hit rate without a partial-measurement qualifier.

#### Scenario: Partially classified or incomplete Goal input

- **WHEN** a Goal has classified input plus unclassified input, historical consumption without categories, or an incomplete usage marker
- **THEN** its usage surfaces show the rate from classified input with a measured qualifier; the unknown portion does not lower the percentage.

#### Scenario: No classified input

- **WHEN** a Goal has only unclassified input, only unknown consumption, or zero input
- **THEN** its usage surfaces show N/A rather than 0.00% or an inferred cache miss.

#### Scenario: Other behavior modes without Goal

- **WHEN** Plan or Workflow is active without a Goal
- **THEN** the existing ordinary session usage status continues to show its session cache-hit rate.
