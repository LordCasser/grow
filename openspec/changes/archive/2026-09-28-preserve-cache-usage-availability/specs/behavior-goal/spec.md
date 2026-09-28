## MODIFIED Requirements

### Requirement: Goal usage exposes provider-style components
Goal SHALL durably accumulate full input, output and available cache-hit/cache-miss classifications for admitted model attempts, including descendants and sidebands, using the existing acknowledged settlement boundary. Cache-miss input SHALL mean full input minus cache-hit input for an attempt with both counts known, including cache-write input; it SHALL NOT require a separately reported miss field. When cache read is unknown, that attempt's input classification SHALL remain unknown independently of known total consumption. Known failed attempts SHALL retain usage; repeated settlement SHALL NOT charge twice. The token budget and displayed total SHALL both count full input plus output, including cache-hit input. Reasoning tokens SHALL NOT be added again to output.

#### Scenario: Cache-inclusive budget
- **WHEN** an attempt reports 500 input tokens including 200 cache hits and 80 output tokens
- **THEN** Goal total and budget consumption increase by 580 and the three components increase by 200, 300 and 80 respectively.

#### Scenario: Cache-only request
- **WHEN** a request reports only cache-hit input
- **THEN** it increases total usage and consumes the token budget.

#### Scenario: Restore and continued accounting
- **WHEN** a classified Goal is resumed
- **THEN** counters, classification availability and their recorded coverage persist, and further usage adds once under the original Goal owner.

#### Scenario: Missing usage or historical categories
- **WHEN** a provider omits usage or a restored Goal has only historical aggregate consumption
- **THEN** unavailable categories are identified without fabricated values; total is a lower bound only when total consumption itself is incomplete. Existing incomplete-usage rules prevent exact budget enforcement only for unknown total consumption, while unbudgeted work continues.

#### Scenario: Settlement fails
- **WHEN** durable Goal settlement fails
- **THEN** total and classified usage roll back together and retry cannot bypass the persistence boundary.

#### Scenario: Cache details are missing but token budget is exact
- **WHEN** Goal attempt 的 full input=500、output=80 可确认，cache read/write 没有报告
- **THEN** Goal 总量和预算消费增加 580，缓存分类保留未知，不能把 500 全部计为已知 cache miss；不因分类缺失单独关闭预算准入。
