## ADDED Requirements

### Requirement: Image description provider work respects recovery deadline
Auxiliary image-description provider polling SHALL use an absolute deadline no later than the existing image recovery deadline. Time spent preparing a group SHALL NOT extend that deadline. Durable preparation and terminal persistence are outside this provider-time guarantee.

#### Scenario: Preparation consumes the remaining recovery budget
- **WHEN** group preparation finishes after the recovery deadline
- **THEN** its provider request is not polled and recovery follows its timeout failure path without installing an incomplete image shadow

#### Scenario: Preparation consumes part of the remaining budget
- **WHEN** a group reaches provider entry before the recovery deadline
- **THEN** provider polling is bounded by the earlier of that deadline and its per-call timeout
