## ADDED Requirements

### Requirement: Native sampling control is bound to the caller

The native `set_session_model` tool SHALL obtain its target from the host-bound calling session and SHALL NOT accept a model-authored target session or client control intent. Its invocation SHALL traverse ordinary tool eligibility, frozen arguments, Hook, permission and one-shot permit validation. Its model and effort selection SHALL obey the same selectable-model and allowed-model constraints as user controls, and Workflow sessions SHALL remain within their frozen Run route. Changing sampling selection SHALL NOT expand tool capability, workspace, MCP identity or permission authority.

#### Scenario: Model supplies another session

- **WHEN** a call includes a target session ID or attempts to forge user control metadata
- **THEN** argument validation rejects it and no other session changes.

#### Scenario: Tool execution is denied

- **WHEN** an applicable tool policy, capability ceiling, Hook or exact-call permit rejects the invocation
- **THEN** no sampling control is admitted, even if the requested model is selectable.

#### Scenario: Workflow requests an external route

- **WHEN** the current session belongs to a Workflow Run and the tool requests a model outside its frozen route authority
- **THEN** the request fails without consulting a replacement global route or changing that Run's sampling selection.

#### Scenario: Selection changes without execution authority changes

- **WHEN** a calling session successfully changes its model or effort
- **THEN** its existing tool eligibility, delegated ceiling, workspace and permissions remain in force.

证据与验证入口：tools 原生工具注册、`crates/codegen/shell/src/session/actor/tool/{preparation,dispatch}.rs`、`crates/codegen/shell/src/agent/handlers/model_switch.rs` 及实际工具入口的拒绝测试。
