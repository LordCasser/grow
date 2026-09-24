## ADDED Requirements

### Requirement: Usage surfaces include auxiliary model consumption

`/usage` SHALL display the owning session's known main, child and Sideband model consumption in lifetime and resume segments, grouped by the frozen provider/model route. Full input plus output includes cache-hit input. Incomplete Sideband attempts SHALL preserve the lower-bound marker and prevent unknown cost from appearing exact. Sideband calls SHALL NOT increment the public main-loop turn count. Existing aggregate UI and headless usage shapes remain unchanged.

#### Scenario: Auxiliary calls across a resume

- **WHEN** a session's main loop, recap and memory Sidebands consume known tokens across two incarnations
- **THEN** `/usage` and the ordinary status projection include all three charges once, each in its proper segment and route group, while `numTurns` counts only main-loop rounds

#### Scenario: Auxiliary call with unknown usage

- **WHEN** a Sideband provider request completes or fails without trustworthy usage
- **THEN** `/usage` and headless reporting show recorded totals as an incomplete lower bound and do not present unknown cost as exact
