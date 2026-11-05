## ADDED Requirements

### Requirement: Post-load child recovery only settles historical Workflow executions

A child reconciliation pass that runs after a Session actor starts SHALL NOT perform whole-session interrupted-work recovery on that actor. Its Workflow settlement authority SHALL be limited to the exact `(run_id, execution_epoch)` pairs captured from the validated cold-load Timeline before actor startup. The Timeline owner SHALL settle a requested execution only while that same epoch is still open, has no open subagent, and is not preserved by a runtime checkpoint. It SHALL leave the current Turn, Step, requests, tools, hooks, input reservations, skill loads and unrelated Workflow epochs unchanged. A resident reconnect SHALL grant no cold Workflow settlement authority. Settlement SHALL retain the existing durable acknowledgement boundary, interruption status and Workflow completion handoff semantics.

#### Scenario: A pending notification starts a turn during cold load

- **WHEN** the new actor starts a notification-drain Turn before post-load child reconciliation finishes
- **THEN** reconciliation leaves that Turn open, and the exact pending receipt can be durably consumed by its original Turn without an intervening recovery terminal

#### Scenario: A historical Workflow waits for its child

- **WHEN** a captured historical Workflow epoch still has an open subagent
- **THEN** Workflow settlement leaves it open until the child terminal has been confirmed through the existing canonical reconciliation path

#### Scenario: Historical child settlement completes while new work runs

- **WHEN** the last child of a captured historical Workflow epoch reaches a confirmed terminal while a new actor Turn or unrelated Workflow is active
- **THEN** settlement ends only that captured Workflow epoch as Interrupted with its completion handoff, preserving the new actor work

#### Scenario: A Workflow identity has advanced or already ended

- **WHEN** a requested Workflow is absent, already ended or closed, or its execution epoch differs from the captured epoch
- **THEN** settlement creates no terminal for that target and does not replace it with the current execution

#### Scenario: Repeated or checkpoint-owned recovery

- **WHEN** settlement is repeated, or the runtime checkpoint preserves the requested execution
- **THEN** it creates no duplicate terminal and does not consume checkpoint-owned execution rights

#### Scenario: A batch fails after one terminal was acknowledged

- **WHEN** an earlier target has been durably settled and a later target fails to persist
- **THEN** the acknowledged terminal remains canonical, the batch reports the failure without accepting an unconfirmed later terminal, and replaying the same targets from the durable prefix does not duplicate the settled execution

#### Scenario: A resident viewer reconnects

- **WHEN** an already resident Session is loaded for another view
- **THEN** child projection reconciliation does not acquire cold Workflow settlement authority or end resident work
