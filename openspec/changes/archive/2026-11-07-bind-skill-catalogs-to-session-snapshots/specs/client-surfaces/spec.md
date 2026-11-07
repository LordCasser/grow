## ADDED Requirements

### Requirement: Skills management responses belong to their requesting session binding

Pager skill-list and toggle operations SHALL send the exact sessionId and preserve the originating session binding epoch through asynchronous completion. Before changing catalog data, errors or pending-action state, Pager SHALL confirm both the current session and its binding epoch. Toggle SHALL retain the existing global disabled configuration and baseline refresh semantics.

#### Scenario: Session view is rebound before completion

- **WHEN** a list or toggle response arrives after its view changes from session A to B, or from A to B and back to A
- **THEN** the stale response does not change the current view's skill data, error or pending-action state.

#### Scenario: Same binding receives its catalog

- **WHEN** a skill-list or toggle operation completes under the same sessionId and binding epoch that initiated it
- **THEN** Pager applies the returned management catalog normally and retains the existing refresh behavior.
