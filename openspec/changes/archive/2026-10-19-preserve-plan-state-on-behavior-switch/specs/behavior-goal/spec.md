## ADDED Requirements

### Requirement: Behavior selection preserves durable Plan identity
When a successful Behavior selection leaves Plan, the durable Control snapshot SHALL reflect the same coordinator selection semantics as the live state: Plan runtime fields are cleared, while the retained candidate and `last_plan_submission_id` are preserved. A completed-plan reference SHALL still be cleared only under the existing successful manual-selection rule. The target snapshot SHALL be durably acknowledged before the live coordinator applies the selection.

#### Scenario: Retained candidate survives a manual Behavior switch
- **WHEN** a deferred Plan candidate exists and the user successfully switches to another Behavior
- **THEN** the latest acknowledged Control retains the candidate and the current submission counter, while carrying no Plan runtime residue; cold restore exposes the same candidate and counter.

#### Scenario: Completed Plan switch preserves identity and applies dismissal rule
- **WHEN** a completed Plan is followed by a successful manual Behavior selection
- **THEN** the completed-plan reference is cleared in both durable and live state, while the submission counter remains monotonic.

#### Scenario: Durable Behavior selection fails
- **WHEN** the target Control snapshot cannot be durably acknowledged
- **THEN** live Behavior selection and Plan state remain unchanged.

#### Scenario: New Plan after cold restore uses a fresh submission identity
- **WHEN** the session is restored from the latest Control after leaving Plan and submits another plan
- **THEN** the new submission identity is greater than the prior session counter and cannot reuse the retained candidate's identity.
