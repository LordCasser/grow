## MODIFIED Requirements

### Requirement: Canonical cancelled subagent lifecycles are resumable

A cancelled subagent SHALL be a valid durable resume source when its parent Timeline contains matching Spawned and Ended facts, its child identity and seed match that spawn, and the terminal result reference resolves to an exactly matching validated child SubagentResult. Resume eligibility SHALL be based on this canonical linkage and the existing security/workspace checks, not on requiring a successful or completed outcome. Resuming SHALL preserve the source agent, context and worktree semantics and SHALL NOT relabel the cancelled source as successful. Model route and reasoning effort SHALL retain the source's actual selection unless independently overridden by explicit ordinary resume parameters under the current model-selection constraints. The derived Spawn and child route baseline SHALL record the final effective selection while the source lifecycle remains immutable. Original-identity checkpoint restoration SHALL retain its existing route constraints.

#### Scenario: Goal stop cancels a child using another model

- **WHEN** a root Goal stop cancels a child whose model route differs from the root, and Spawned, Ended(cancelled), SubagentSeed and SubagentResult(cancelled) form a valid canonical link
- **THEN** a later explicit resume accepts that child as its source and, without a model override, pins the original child route instead of rejecting it because its outcome is cancelled or silently replacing it with a fresh spawn.

#### Scenario: Cancelled result has an exact canonical link

- **WHEN** parent and child identities, spawn/seed coordinates, outcome, duration, tool/turn counts, usage, error and result reference all match and both Timelines validate
- **THEN** resume uses the child as a durable source under the existing security and workspace constraints.

#### Scenario: Cancelled lifecycle is incomplete or inconsistent

- **WHEN** the source lacks a spawn or terminal, the child identity/seed does not match, the result reference is missing or invalid, or either Timeline fails validation
- **THEN** resume fails closed without starting a child, provider request or worktree mutation and without treating the source as a valid canonical lifecycle.

#### Scenario: Override records only the derived selection

- **WHEN** a valid ordinary resume supplies an admitted model or effort override
- **THEN** the derived Spawn and child baseline record the final choice, its context retains source facts, and the cancelled source Spawn/result/outcome remain unchanged.

### Requirement: Subagent resume admission is fail-closed and retry-safe

Subagent resume SHALL reject a source while its original runtime remains live, even if terminal facts are already visible. After durable source validation, agent/model/transport/effort, workspace, context and derived-child admission failures SHALL stop only the requested resume epoch, preserve the immutable source lifecycle and return the failing stage. Explicit ordinary resume overrides SHALL be validated against their final selected model and current authority; omitted selections SHALL retain the source constraints. No valid resume request SHALL silently become a fresh child. A later retry SHALL revalidate the source and current environment.

#### Scenario: Durable terminal is visible while the source remains live

- **WHEN** the parent and child contain an exact terminal link but the coordinator still owns the source as pending, active or settling
- **THEN** resume reports that the source is still running or settling and does not start an overlapping child epoch.

#### Scenario: Historical non-worktree cwd is missing

- **WHEN** a validated non-worktree source names a cwd that no longer exists and the current parent workspace is valid
- **THEN** resume uses the current parent workspace and preserves the source transcript and selected route; an existing non-directory, unverifiable path or canonical path outside the parent workspace remains rejected.

#### Scenario: Source route or context is incompatible

- **WHEN** the source agent type, retained model/effort/transport or explicit replacement is unavailable or invalid, its transcript cannot be validated or fit safely, or required prompt artifacts cannot be loaded
- **THEN** resume identifies the incompatible stage, starts no fresh replacement and does not change the source lifecycle or guess a substitute route, effort, context or artifact.

#### Scenario: Current child System head cannot be rendered

- **WHEN** a validated resume source already contains its stable System head but the current child-audience head renderer is unavailable or invalid
- **THEN** resume preserves the inherited head and does not invoke the current renderer; a new or normalized child without a renderable head still fails before persistence.

#### Scenario: Historical completion output artifact is missing

- **WHEN** a validated source has an exact terminal/result link and materializable Timeline Surface but its optional completion output artifact is absent
- **THEN** resume remains eligible because the display artifact is not context authority; any immutable prompt blob directly referenced by the Surface remains required.

#### Scenario: Workflow route authority no longer exists

- **WHEN** a workflow-owned resume source is canonical but its owning Workflow Run or frozen runtime route is no longer registered
- **THEN** resume rejects the derived epoch and does not replace the route with the current global agent definition.

#### Scenario: Isolated source workspace cannot be restored

- **WHEN** a source worktree is absent without a snapshot or snapshot rehydration cannot produce the exact recorded target
- **THEN** the derived resume epoch fails without discarding the snapshot reference or claiming an empty workspace continuation.

#### Scenario: Derived child admission fails after source validation

- **WHEN** parent spawn persistence, child storage, session startup, catalog convergence, promotion or first-prompt admission fails
- **THEN** the requested derived epoch is failed or left for existing canonical recovery as appropriate, while the original source remains unchanged and eligible for a later fully revalidated retry.

#### Scenario: First prompt is not durably admitted

- **WHEN** child control publication, Goal snapshot mailbox delivery, QueuePrompt delivery or the durable prompt persistence acknowledgment fails
- **THEN** the derived epoch reports the exact launch stage, admits no further provider work, settles any prompt that may already have started before committing its child result, and leaves the original resume source unchanged.

证据与验证入口：`crates/codegen/shell/src/agent/subagent/{mod.rs,handle_request.rs,resolution.rs}`、既有 canonical resume 与 checkpoint restore 测试。
