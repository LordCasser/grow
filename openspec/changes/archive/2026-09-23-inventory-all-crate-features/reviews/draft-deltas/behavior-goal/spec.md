## ADDED Requirements

### Requirement: Behavior wire and display identities
BehaviorId SHALL 包含 Normal/Clarify/Plan/Workflow/Goal，默认 Normal；as_id/Display 与 try_from_id 使用 normal/ask/plan/workflow/goal，serde 使用 snake_case 枚举名。

#### Scenario: Clarify 双表示
- **WHEN** 序列化 Clarify 或解析外部 ID
- **THEN** serde 值为 clarify，外部 ID 为 ask；try_from_id 不接受 clarify 或未知值。

#### Scenario: 运行时分类与投影
- **WHEN** 调用 owns_special_runtime 或 availability.choice
- **THEN** 只有 Plan/Goal 分类为特殊 runtime；choice 返回首个匹配 entry，entry 含 supported、available/confirmation_required/unavailable 和可选 reason；本包只定义投影，不执行状态转换或授权。

证据：`crates/common/tool-types/src/behavior.rs` — `BehaviorId`；`crates/common/tool-types/src/behavior.rs` — `BehaviorAvailability`。



### Requirement: Shell Goal notification projection and delivery
GoalNotifySender SHALL 在tracker无snapshot时不发送，存在时取goal字段、调用方tokens_used及tracker elapsed_ms构建GoalUpdated，不在此重新计算或截断tokens_used。状态映射active/paused/blocked/budget_limited/complete。send_update先添加event ID meta并尝试JSON编码，再无条件尝试入队普通Grow Update，忽略入队失败；编码成功则独立fire-and-forget发送grow/session_notification，不等待持久化确认，也没有gateway_enabled门控。编码失败只跳过gateway发送，仍尝试持久化。cleared投影使用空ID/objective/时间串、status=cleared、无budget/message、tokens及elapsed为0、usage_incomplete=false。format_elapsed向下取整到秒，有小时输出h与m并省略秒，否则输出m/s或s，不四舍五入。

#### Scenario: Persistence unavailable
- **WHEN** 通知编码成功但持久化channel关闭
- **THEN** 仍尝试gateway发送，不能认为客户端可见代表已保存。

#### Scenario: Cleared projection
- **WHEN** 调用build_goal_cleared
- **THEN** 返回专用cleared哨兵载荷，不复用Complete状态。

#### Scenario: Elapsed hour formatting
- **WHEN** elapsed为3661000毫秒
- **THEN** 显示1h 1m，省略秒。

源码证据：
- `crates/codegen/shell/src/session/goal_notification.rs` — `impl GoalNotifySender`。
- `crates/codegen/shell/src/session/goal_notification.rs` — `pub(crate) fn build_goal_updated`。
- `crates/codegen/shell/src/session/goal_notification.rs` — `pub(crate) fn build_goal_cleared`。
- `crates/codegen/shell/src/session/goal_notification.rs` — `pub(crate) fn format_elapsed`。
### Requirement: Pager plan slash idempotent selection and deferred prompt

PlanCommand SHALL 接受可选description、标记session_scoped且offered_when_session_less=true。参数trim后为空时总返回SetBehaviorMode(Plan)，不因快照已是Plan而短路；非空时返回SetBehaviorThenPrompt{mode:Plan,prompt:Some(trim文本)}，要求下游先完成behavior切换再发送prompt。run不检查session、不读取当前behavior、不验证prompt长度或附件；dashboard可利用session-less offering暂存下一session的Plan选择。

#### Scenario: Already in plan
- **WHEN** 当前快照behavior已为Plan且参数为空
- **THEN** 仍产生SetBehaviorMode(Plan)。

#### Scenario: Plan with description
- **WHEN** 参数为两端带空白的任务描述
- **THEN** 产生trim后description的SetBehaviorThenPrompt，不直接返回普通prompt。

证据：`crates/codegen/pager/src/slash/commands/plan.rs` — `PlanCommand::session_scoped / offered_when_session_less / run`。

### Requirement: Pager behavior slash availability rows and selection gates

available_modes SHALL 固定按Normal、Clarify、Plan开头，仅在AppCtx.workflows_available时追加Workflow、goal_available时再追加Goal。behavior_items可先加入Keep current行，其insert_text为prefix trim或空；mode行按当前BehaviorId追加(current)，match_text含label/wire id/description，prefix存在时insert_text为trim prefix、单空格和wire id，否则仅wire id。BehaviorCommand接受可选参数、session_scoped且offered_when_session_less；建议忽略query并使用AppCtx列表。run空参数打开behavior picker；非空trim并ASCII小写，normal、ask/clarify、plan、workflow、goal映射对应BehaviorId，default拒绝。Workflow与Goal再分别检查PagerLocalSnapshot的可用性，其他模式无gate；建议与执行使用不同快照。Normal/Clarify shortcut均session_scoped且可session-less offered，run不检查参数、当前mode或可用性，直接SetBehaviorMode。

#### Scenario: Clarify wire identifier
- **WHEN** 建议Clarify或运行参数ask/clarify
- **THEN** 展示label为Clarify，insert wire id为ask，两种输入都选择BehaviorId::Clarify。

#### Scenario: Goal suggestion becomes stale
- **WHEN** AppCtx建议阶段goal可用，但执行快照goal_available为false
- **THEN** 选择运行时返回Unknown or unavailable错误。

证据：`crates/codegen/pager/src/slash/commands/behavior.rs` — `available_modes / behavior_items / BehaviorCommand::suggest_args / run / BehaviorShortcutCommand`。

### Requirement: Pager ACP goal command state aware subcommand suggestions

AcpSlashCommand::suggest_args SHALL 仅对name按ASCII不区分大小写等于goal时提供特殊候选，其他ACP命令返回None。args_query只trim_start；subcommand识别本身区分大小写并只把普通space视为`edit `或`set `前缀。edit阶段若尚未键入非空objective且current_goal_objective存在并trim非空，返回唯一一行，以objective为display、`edit <objective>`为match/insert；已有任意typed objective或没有可用objective则None。set及`set `后任意文本始终None以开放自由输入。其他query返回完整的状态合法候选集供外层fuzzy：没有unfinished objective且behavior不是Goal时仅set；只要current_goal_objective为Some即视为unfinished，即使字符串为空，也按固定顺序给edit、budget、status、pause、restart、clear并隐藏set；Goal behavior且objective为None返回Some(empty)。set/edit/budget insert尾随空格，其他leaf不尾随。该层只建议，不验证Shell接受subcommand或执行goal变更。

#### Scenario: Outside Goal without objective
- **WHEN** behavior为Normal且current_goal_objective=None
- **THEN** 候选只有set并以`set `插入。

#### Scenario: Edit current objective
- **WHEN** unfinished objective为修复登录流程且query为edit或edit空格
- **THEN** 唯一候选预填`edit 修复登录流程`；用户开始键新目标后停止建议。

#### Scenario: Goal behavior without objective
- **WHEN** behavior为Goal但current_goal_objective=None
- **THEN** 返回空候选集合，不重新建议set。

源码证据：
- `crates/codegen/pager/src/slash/acp_command.rs` — `AcpSlashCommand::suggest_args`。
### Requirement: Pager goal update cleared parsed state and elapsed floor

GoalUpdated status `cleared` SHALL clear the goal. Other statuses must parse before a full display state is applied with received_at now and elapsed_floor_ms equal to supplied elapsed_ms. Invalid status warns and returns false; budget and timestamp relationships are not validated here.

#### Scenario: Cleared
- **WHEN** status is cleared
- **THEN** the current goal clears.

#### Scenario: Valid
- **WHEN** status parses
- **THEN** the full display projection is applied.

#### Scenario: Invalid
- **WHEN** status is unknown
- **THEN** it is warned and ignored.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `GoalUpdated`。

### Requirement: Pager authoritative Behavior mode plan phase and confirmation projection

CurrentModeUpdate SHALL be the sole Behavior state source. Supported Normal/Clarify/Plan/Workflow/Goal update identity, plan state/phase, workflow visibility and confirmation warnings; unknown or unsupported ids are ignored. Missing remainingMs defaults to one, applied/rejected clear confirmation, and valid updates return true even unchanged.

#### Scenario: Supported
- **WHEN** a supported mode arrives
- **THEN** behavior and plan projections update.

#### Scenario: Confirmation
- **WHEN** metadata requires confirmation for a valid target
- **THEN** a timed warning appears.

#### Scenario: Terminal
- **WHEN** status is applied or rejected
- **THEN** confirmation clears.

#### Scenario: Unsupported
- **WHEN** id is unknown or unsupported
- **THEN** it is warned and ignored.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `detect_plan_mode_change`。

### Requirement: Pager Behavior control resolution and explicit target extraction

Supported CurrentModeUpdate without status or with applied SHALL resolve Applied; confirmation_required and rejected map distinctly; other mode/status values return none. Target extraction independently parses behaviorChange.target for delayed-result correlation.

#### Scenario: Plain
- **WHEN** supported mode has no status
- **THEN** Applied is returned.

#### Scenario: Non-applied
- **WHEN** status is confirmation_required or rejected
- **THEN** its distinct resolution is returned.

#### Scenario: Target
- **WHEN** metadata names a parseable target
- **THEN** its BehaviorId is extracted independently.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `behavior_mode_update_resolution`、`behavior_mode_update_target`。
### Requirement: Tools crates/codegen/tools/src/slash_commands.rs tools crate module boundary contract
crates/codegen/tools/src/slash_commands.rs SHALL implement the tools crate module boundary boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols SCHEDULER_CREATE_TOOL_NAME, loop_usage_message, str, loop_schedule_instruction, UPDATE_GOAL_TOOL_NAME, WORKFLOW_TOOL_NAME, GOAL_COMMAND_NAME, GOAL_RESERVED_SUBCOMMANDS, goal_usage_message, goal_instruction, instruction_carries_args_and_contract_tokens, fire_instruction_describes_detached_runtime, goal_instruction_carries_objective_and_contract_tokens, usage_message_has_no_default_claim follow explicit markers explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、image/PDF/media processing、scheduler generation/journal state; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/slash_commands.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

证据：`crates/codegen/tools/src/slash_commands.rs` — `SCHEDULER_CREATE_TOOL_NAME`；`crates/codegen/tools/src/slash_commands.rs` — `loop_usage_message`；`crates/codegen/tools/src/slash_commands.rs` — `str`；`crates/codegen/tools/src/slash_commands.rs` — `loop_schedule_instruction`；`crates/codegen/tools/src/slash_commands.rs` — `UPDATE_GOAL_TOOL_NAME`；`crates/codegen/tools/src/slash_commands.rs` — `WORKFLOW_TOOL_NAME`；`crates/codegen/tools/src/slash_commands.rs` — `GOAL_COMMAND_NAME`；`crates/codegen/tools/src/slash_commands.rs` — `GOAL_RESERVED_SUBCOMMANDS`；`crates/codegen/tools/src/slash_commands.rs` — `goal_usage_message`；`crates/codegen/tools/src/slash_commands.rs` — `goal_instruction`；`crates/codegen/tools/src/slash_commands.rs` — `instruction_carries_args_and_contract_tokens`；`crates/codegen/tools/src/slash_commands.rs` — `fire_instruction_describes_detached_runtime`；`crates/codegen/tools/src/slash_commands.rs` — `goal_instruction_carries_objective_and_contract_tokens`；`crates/codegen/tools/src/slash_commands.rs` — `usage_message_has_no_default_claim`。
### Requirement: Pager task-result test: behavior_transport_failure_keeps_first_prompt_local_and_retryable
A failed SetBehaviorThenPrompt transport request SHALL preserve the first prompt locally, retain the deferred behavior mode, and allow an explicit behavior retry.

#### Scenario: Behavior transport failure
- **WHEN** the serialized behavior control fails before acknowledgement
- **THEN** no effect is emitted, the prompt remains queued, and retrying the mode still produces a SwitchBehavior effect.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `behavior_transport_failure_keeps_first_prompt_local_and_retryable`。

### Requirement: Pager task-result test: superseded_behavior_rpc_clears_only_the_matching_local_correlation
A superseded behavior RPC SHALL clear only its matching local control correlation and produce no local scrollback feedback.

#### Scenario: Superseded behavior control
- **WHEN** the completion carries the active control token and Superseded outcome
- **THEN** pending controls are cleared without appending a notice.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `superseded_behavior_rpc_clears_only_the_matching_local_correlation`。
### Requirement: Pager agent input test: ctrl_x_b_opens_behavior_picker
Ctrl-X then B SHALL open the behavior picker only when workflow/goal behavior commands are available.

#### Scenario: Leader behavior picker
- **WHEN** behavior support and goal command metadata are available
- **THEN** OpenCommandPicker targets behavior.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `ctrl_x_b_opens_behavior_picker`。

### Requirement: Pager agent input test: ctrl_x_b_in_child_view_does_not_open_parent_behavior_control
A child Agent view SHALL not open the parent behavior control from Ctrl-X then B; it SHALL consume the sequence and show ownership feedback.

#### Scenario: Child behavior boundary
- **WHEN** the same leader sequence is entered in a subagent view
- **THEN** no behavior picker modal opens.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `ctrl_x_b_in_child_view_does_not_open_parent_behavior_control`。

### Requirement: Pager agent input test: behavior_switch_confirmation_enter_preserves_draft
Confirming a behavior switch with bare Enter SHALL request the target mode without mutating the composer draft or committed behavior mode locally.

#### Scenario: Behavior confirmation
- **WHEN** Enter is pressed while a switch warning is active
- **THEN** SetBehaviorMode is returned, draft is preserved, and the warning is cleared.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `behavior_switch_confirmation_enter_preserves_draft`。

### Requirement: Pager agent input test: behavior_switch_confirmation_other_input_cancels_without_editing
Any non-confirming key, paste, or modified Enter during behavior confirmation SHALL cancel back to the current mode without editing the draft or emitting effects.

#### Scenario: Behavior cancellation
- **WHEN** a non-confirming event arrives during the warning
- **THEN** the current mode is reselected and the draft remains unchanged.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `behavior_switch_confirmation_other_input_cancels_without_editing`。

### Requirement: Pager agent input test: behavior_switch_confirmation_expires_without_switching
An expired behavior switch banner SHALL clear its target and banner without switching mode.

#### Scenario: Behavior confirmation expiry
- **WHEN** the maintenance deadline is reached
- **THEN** the target and banner are removed.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `behavior_switch_confirmation_expires_without_switching`。

### Requirement: Pager agent input test: behavior_switch_confirmation_ignores_key_release
A key-release event SHALL not confirm or cancel behavior switching.

#### Scenario: Behavior release event
- **WHEN** Enter arrives as a key release while the warning is active
- **THEN** the warning remains pending and the outcome is Unchanged.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `behavior_switch_confirmation_ignores_key_release`。
