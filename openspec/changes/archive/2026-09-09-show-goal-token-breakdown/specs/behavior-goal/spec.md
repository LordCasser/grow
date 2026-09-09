## ADDED Requirements

### Requirement: Goal usage exposes provider-style components
Goal SHALL durably accumulate cache-hit input, cache-miss input and output for admitted model attempts, including descendants and sidebands, using the existing acknowledged settlement boundary. Known failed attempts SHALL retain usage; repeated settlement SHALL NOT charge twice. The token budget and displayed total SHALL both count full input plus output, including cache-hit input. Reasoning tokens SHALL NOT be added again to output.

#### Scenario: Cache-inclusive budget
- **WHEN** an attempt reports 500 input tokens including 200 cache hits and 80 output tokens
- **THEN** Goal total and budget consumption increase by 580 and the three components increase by 200, 300 and 80 respectively.

#### Scenario: Cache-only request
- **WHEN** a request reports only cache-hit input
- **THEN** it increases total usage and consumes the token budget.

#### Scenario: Restore and continued accounting
- **WHEN** a classified Goal is resumed
- **THEN** counters persist and further usage adds once under the original Goal owner.

#### Scenario: Missing usage or historical categories
- **WHEN** a provider omits usage or a restored Goal has only historical aggregate consumption
- **THEN** total is a lower bound and unavailable historical categories are identified without fabricated values; existing incomplete-usage rules prevent exact budget enforcement while unbudgeted work continues.

#### Scenario: Settlement fails
- **WHEN** durable Goal settlement fails
- **THEN** total and classified usage roll back together and retry cannot bypass the persistence boundary.
