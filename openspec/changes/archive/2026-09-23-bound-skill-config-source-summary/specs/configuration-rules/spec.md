## ADDED Requirements

### Requirement: Skill configuration source summary uses the bounded discovery worker

`grow/skills/config` SHALL compute its automatic source summary off the asynchronous request thread under a bounded worker permit and deadline. Timeout or worker failure SHALL fail the request rather than return a partial successful summary; a timed-out worker SHALL retain its permit until it exits.

#### Scenario: Source filesystem scan blocks
- **WHEN** the source summary's filesystem scan remains blocked beyond its deadline
- **THEN** the request returns an error while the worker retains the shared discovery permit, and later requests cannot create unlimited replacement scans.

#### Scenario: Source scan completes
- **WHEN** the source summary scan finishes within the deadline
- **THEN** the response includes the existing source counts and full skill list.
