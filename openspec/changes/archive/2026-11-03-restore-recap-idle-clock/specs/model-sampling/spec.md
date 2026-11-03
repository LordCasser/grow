## ADDED Requirements

### Requirement: Restored automatic recap uses durable primary API activity

An existing session actor SHALL initialize its recap idle clock from the most recent primary Request fact in its validated Timeline. Later Sideband, UI or load activity SHALL NOT replace that primary timestamp. A fresh or newly seeded task, or history without primary Request evidence, SHALL retain the no-evidence state. Runtime primary sampling SHALL continue to update the same field. Restoration SHALL NOT bypass the existing three-minute idle interval, minimum main turns, new-turn, away identity or cancellation gates.

#### Scenario: Cold load precedes return from away
- **WHEN** a session with at least three real main turns and durable primary requests is loaded into a new process, and its main API activity is older than the idle threshold when recap is requested
- **THEN** automatic recap may pass the idle gate using the durable request time and recover its frozen historical images without requiring another main prompt first.

#### Scenario: Auxiliary work follows the last main request
- **WHEN** later Sideband or UI facts follow the most recent primary Request
- **THEN** they do not delay the restored main API idle clock.

#### Scenario: No primary activity is proven
- **WHEN** the actor is fresh, is a newly seeded task or has no primary Request facts
- **THEN** restoration does not invent an API timestamp or bypass the existing automatic recap gate.
