## MODIFIED Requirements

### Requirement: Native sampling control is bound to the caller

The `set_subagent_model` tool SHALL obtain the source identity from the host-bound SubagentBackend and SHALL accept only a subagent task ID as target, not a caller/session identity or client control intent. The coordinator SHALL permit only an active directly delegated non-Workflow child with open execution and SHALL forward its exact runtime context; root lifecycle visibility SHALL NOT grant control over grandchildren. Initializing, cancelled, settling, suspended and terminal children SHALL fail without restoring or rerunning. Shell SHALL retain target incarnation/cancellation checks across asynchronous preparation. Ordinary tool eligibility, frozen arguments, Hooks, permissions and exact one-shot permits SHALL precede dispatch. Model and effort choices SHALL satisfy selectable/allowed-model constraints. Sampling control SHALL NOT expand child capability, workspace, MCP identity or permission authority.

#### Scenario: Model supplies another session
- **WHEN** a call forges source/session metadata, names self, a sibling, a grandchild or unrelated task
- **THEN** parsing or direct-ownership admission rejects it without changing any session.

#### Scenario: Tool execution is denied
- **WHEN** eligibility, capability ceiling, Hook, permission or exact-call permit rejects the parent tool
- **THEN** no child sampling request is dispatched even for a selectable model.

#### Scenario: Workflow requests an external route
- **WHEN** the selected child belongs to a Workflow Run
- **THEN** native parent runtime control is rejected and its frozen Run route remains unchanged.

#### Scenario: Selection changes without execution authority changes
- **WHEN** a parent successfully controls its child's sampling
- **THEN** parent sampling and the child's existing tool/MCP/workspace/permission grants remain unchanged.

#### Scenario: Target stops during preparation
- **WHEN** target cancellation, terminal execution or actor replacement wins before target admission
- **THEN** the request fails without redirecting to a replacement actor or reporting queued without an actor receipt.
