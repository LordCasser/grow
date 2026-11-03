## ADDED Requirements

### Requirement: Suspended restore admission follows execution dependencies
Restore admission SHALL verify original operation/writer ownership, Agent selector and plugin identity, actual workspace directory, persisted model destination, valid Workflow frozen sources and child birth ceilings. Effective durable role and project instructions SHALL govern the resumed model. Unrelated display metadata, current rule files, inactive Hook registry entries and numeric limit equality SHALL NOT block the whole tree. Aggregate source fingerprints SHALL remain diagnostic facts and SHALL NOT replace these checks or grant authority. Pending calls SHALL retain their identity and final arguments and revalidate current tool/transport/permission authority; unknown effects SHALL NOT be replayed.

#### Scenario: Presentation or unrelated configuration changes
- **WHEN** color, description, current rules or unrelated Hook registry/limit configuration changes while a task is suspended
- **THEN** valid execution dependencies allow restoration under its effective durable role/rules and current authorization, without rewriting its checkpoint or replenishing usage.

#### Scenario: Workspace aliases
- **WHEN** current and recorded cwd spellings resolve to directory capabilities with the same entity identity
- **THEN** restoration uses that workspace and preserves the durable session namespace; a different or unavailable directory is rejected.

#### Scenario: Catalog alias and actual route
- **WHEN** a persisted catalog alias is replaced by a selectable alias with the exact same recorded provider/model and backend/endpoint/query transport identity
- **THEN** restoration may rebind the alias through the existing route observation; a different data destination/model is rejected before execution.

#### Scenario: Original Agent cannot be resolved
- **WHEN** the persisted selector/plugin cannot be reconstructed as that formal identity
- **THEN** restore reports that missing dependency and never runs a same-name or global default substitute.

#### Scenario: Source digest changes without an execution dependency change
- **WHEN** the diagnostic aggregate digest differs but the required independent dependencies remain valid
- **THEN** the original operation may continue with current authority; a matching digest cannot compensate for a missing birth or frozen-source fact.

#### Scenario: Completion recovery policy changes
- **WHEN** an ordinary Agent's optional completion recovery configuration changes offline
- **THEN** restored execution uses the current harness recovery policy while retaining original Goal/TODO gates, output schema and committed tool prefix; Workflow execution retains its frozen policy.

#### Scenario: A restored child has switched away from its birth Agent
- **WHEN** an existing child's durable Agent control selected a different formal Agent before suspension and its birth profile is later removed
- **THEN** exact restore reconstructs the persisted active Agent and retains birth RWX/MCP ceilings without treating the old birth profile as a new delegation; fresh child admission still uses current delegation filters.

#### Scenario: A new seeded child records its own model route
- **WHEN** a child starts a new lineage from a new, forked or resumed Surface seed
- **THEN** it durably records its actual initial provider route before execution; inherited conversation content does not substitute for the child's route authority.

## MODIFIED Requirements

### Requirement: Cold model route changes continue durable selection
Fresh sessions SHALL durably record a secret-free baseline for the selected catalog model, reasoning effort, provider-facing model, and backend/endpoint transport identity before admitting prompts. Ordinary cold load without a suspended execution continuation SHALL compare the last durable model route with the currently resolved catalog route and, when they differ, durably append an exact from/to transition before publishing the actor or admitting sampling. Suspended execution restore SHALL instead retain the checkpoint frontier's actual model destination and effort; it MAY rebind a catalog alias only to that exact destination. A failed append SHALL prevent publication. A historical Timeline without a route observation SHALL receive an explicit current-route baseline only for ordinary cold load, without inventing an earlier route. Resident reconnect SHALL retain its live route. Stored discontinuous transitions SHALL still be rejected.

#### Scenario: Catalog route changes across restart
- **WHEN** an ordinary cold load without suspended execution resolves the same catalog model ID to a different backend, endpoint, query route, or wire model
- **THEN** the replacement writer records a transition from the last durable route to the selected current route before the resumed actor can sample, and a later reload validates the continuous chain.

#### Scenario: Catalog route is unchanged
- **WHEN** a cold load resolves the same model, effort, provider model, and transport identity as the latest durable observation
- **THEN** it does not append a redundant transition.

#### Scenario: Stored history is already discontinuous
- **WHEN** two existing model observations disagree on the exact preceding route
- **THEN** load rejects the invalid history rather than bridging or rewriting it.

#### Scenario: The original key is repointed while an exact alias remains
- **WHEN** a suspended checkpoint's catalog key now names another destination but a current selectable alias retains its exact original transport and effort
- **THEN** restore selects the exact alias, records that alias rebind, and never executes the repointed destination.
