## ADDED Requirements

### Requirement: A cancelled Turn retires its sampling continuation

After the session owner durably terminates a Turn through cancellation, it SHALL retire that Turn's transient sampling continuation before admitting a different Turn. A new user Turn SHALL receive a fresh logical sampling allowance and deadline. Retiring the continuation SHALL NOT rewrite prior attempt identities, erase evidence, cancel settlement ownership or convert unknown usage into known usage. An unconfirmed cancellation or a suspension that preserves the same Turn SHALL NOT constitute this terminal retirement.

#### Scenario: New prompt retries after cancellation
- **WHEN** an admitted sampling attempt is cancelled and its Turn terminal is confirmed, then a new user prompt in the same resident session receives one transient failure followed by success
- **THEN** the new prompt records attempts 1 and 2 under its own RequestId and has its configured allowance, while the cancelled attempt retains its original identity and settlement.

#### Scenario: Repeated cancelled Turns
- **WHEN** several user Turns each begin sampling and then durably terminate through cancellation
- **THEN** a subsequent new Turn starts at attempt 1 rather than accumulating the cancelled Turns' attempts or deadlines.

#### Scenario: Same Turn is suspended instead
- **WHEN** suspension preserves a Turn and its unfinished model step for restoration
- **THEN** restoration retains that step's consumed attempt allowance, absolute deadline and replay restrictions.

#### Scenario: Terminal acknowledgement is unavailable
- **WHEN** cancellation cannot confirm the Turn terminal
- **THEN** the owner reports the existing terminal failure and does not admit new work as though successful cancellation had established a fresh sampling allowance.
