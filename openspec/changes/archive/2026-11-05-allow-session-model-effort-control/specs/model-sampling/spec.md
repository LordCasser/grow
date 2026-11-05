## MODIFIED Requirements

### Requirement: Subagent tool calls can select reasoning effort

The model-facing subagent spawn tool SHALL expose optional `effort` alongside optional `model`. Fresh task calls SHALL forward a supplied effort through the existing subagent runtime policy, with precedence over Agent/model defaults. The resolved model's existing effort validation, sampling configuration and durable spawn projection SHALL remain authoritative. Omission or JSON null SHALL preserve existing fresh-task default resolution. The canonical `"none"` token SHALL remain an explicit effort value. Ordinary `resume_from` calls SHALL independently override supplied model and effort fields and retain the validated source's actual selection for each omitted field, including unset effort. The final model SHALL admit the final effort. An explicit model SHALL use the current selectable route; without one the source transport constraint SHALL remain in force. Original-identity checkpoint restoration SHALL retain its existing route constraints.

#### Scenario: Select model and effort together
- **WHEN** a fresh subagent tool call supplies a selectable model and an effort offered by that model
- **THEN** the child uses that model and effort through the existing runtime and provider request path.

#### Scenario: Select effort on the resolved model
- **WHEN** a fresh tool call supplies effort without model
- **THEN** the existing model resolution is retained and that model's offered-effort validation applies to the explicit effort.

#### Scenario: Omit effort
- **WHEN** a fresh tool call omits effort or supplies JSON null
- **THEN** existing Agent/model effort defaults apply without an explicit runtime policy.

#### Scenario: Explicit none
- **WHEN** a fresh or resume tool call supplies `effort: "none"`
- **THEN** it remains an explicit policy for runtime/model validation rather than being discarded as an absent argument.

#### Scenario: Invalid or unsupported effort
- **WHEN** the supplied token is invalid or the resolved model does not offer the final explicit or retained effort
- **THEN** the child runtime returns its effort failure and does not issue a child provider request using a substituted effort.

#### Scenario: Resume retains the source selection
- **WHEN** a resume call omits both model and effort, or supplies JSON null for both
- **THEN** the resumed child retains its validated source model, transport and actual effort, including unset effort.

#### Scenario: Resume overrides either or both selections
- **WHEN** a resume call supplies model, effort or both
- **THEN** each supplied field replaces the source value, omitted effort retains the source actual effort, omitted model retains the source route, and the final valid selection reaches the derived child's provider request.

#### Scenario: Resume follows the source final durable selection
- **WHEN** the source changed model or effort during its run and later completed with a validated durable route observation
- **THEN** omitted resume fields use that final durable selection, including unset effort, rather than the original Spawn selection; the source Timeline is unchanged.

#### Scenario: Explicit replacement for an unavailable source route
- **WHEN** a canonical resume source's model route is unavailable but the caller supplies a currently selectable model with an admitted final effort
- **THEN** the derived child may use the explicit replacement, while omission of model continues to fail for that unavailable source route.

证据与验证入口：`crates/common/tool-types/src/task.rs`、`crates/codegen/tools/src/implementations/grow_build/task/mod.rs::TaskTool::run`、`crates/codegen/shell/src/agent/subagent/handle_request.rs::run_shell_child_inner`。

## ADDED Requirements

### Requirement: Native model control uses the session step boundary

Grow SHALL expose a native `set_session_model` tool accepting optional model and effort for its calling session. It SHALL require at least one selection and reject unknown fields, malformed effort, unavailable or unselectable models and effort unsupported by the selected model. A model-only call SHALL follow user model-selection effort defaults. An effort-only call SHALL compose with the actor's newest desired model. The call SHALL return `queued` only after actor admission, without claiming application or waiting for its own Step to close. Accepted changes SHALL use the same closed-Step application, latest-wins ordering, durable route transition, native-continuation handling and failure boundaries as user controls. Rejected calls SHALL leave the active and desired selection unchanged.

#### Scenario: Tool switches the next request
- **WHEN** a valid model/effort tool call executes in an active Step
- **THEN** its result reports queued without deadlock, the current tool exchange closes under its original route, and the admitted next request uses the applied selection.

#### Scenario: Effort patch follows a pending user model
- **WHEN** a user model change is pending and the model tool supplies only effort
- **THEN** the effort is validated and composed with that newest desired model rather than restoring an older committed model.

#### Scenario: Invalid selection has no mutation
- **WHEN** both selections are absent, effort is malformed/unsupported, or model is unknown or not selectable
- **THEN** the tool returns an explicit failure with no model-control mutation or substituted provider request.

#### Scenario: Application fails or a later control supersedes it
- **WHEN** an admitted selection fails during application or is superseded before application
- **THEN** existing authoritative control semantics record the outcome; the earlier queued result does not become evidence of application and no partial sampler configuration is used.

证据与验证入口：`crates/codegen/shell/src/agent/handlers/model_switch.rs`、`crates/codegen/shell/src/session/actor/model_switch.rs`、新 Grow 工具及实际工具到 provider 的集成测试。
