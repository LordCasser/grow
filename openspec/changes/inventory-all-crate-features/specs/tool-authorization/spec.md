## ADDED Requirements

### Requirement: RWX authority algebra
ToolAccess SHALL 表示 None、Read、Write、Execute 及其并集，提供 union、covers 和单项测试；未知访问类型的默认值为 All。

#### Scenario: 能力包含
- **WHEN** ReadWrite 检查所需 Execute
- **THEN** covers 返回 false；All 可覆盖所有 RWX 组合。

#### Scenario: 观察类型
- **WHEN** 调用 is_observation_only
- **THEN** 仅 None 与 Read 返回 true；None 是控制平面标记，不自动授予工具 identity。

证据：`crates/common/tool-protocol/src/capabilities.rs` — `ToolAccess`。

### Requirement: Native capability declaration
ToolCapabilities SHALL 严格解析 streaming、supports_cancel、max_concurrency、hooks、max_frame_bytes、timeout_ms 与 max_access；StreamingSpec 声明 subkind 与可选 delta cap。

#### Scenario: 缺省声明
- **WHEN** 使用 ToolCapabilities::default
- **THEN** streaming=None、supports_cancel=false、max_concurrency=None、hooks 为空、frame/timeout=None、max_access=All；并发 None 的含义为不设上限。

#### Scenario: 未知能力字段
- **WHEN** 反序列化 capability 包含未声明字段
- **THEN** 因 deny_unknown_fields 拒绝；声明本身不执行取消、超时或权限策略。

证据：`crates/common/tool-protocol/src/capabilities.rs` — `ToolCapabilities`；`crates/common/tool-protocol/src/capabilities.rs` — `StreamingSpec`。

### Requirement: Validated local invocation identifiers
ToolCallId SHALL 是非空 opaque 字符串且可生成 UUID v7；ToolId SHALL 限于 name 或 namespace:name，每段非空且只含 ASCII 字母数字、下划线或连字符。

#### Scenario: 反序列化无效 ID
- **WHEN** ToolId 有多重冒号、空段或非法字符，或任一 ID 为空
- **THEN** 返回校验错误；ToolCallId 不额外要求输入必须是 UUID。

证据：`crates/common/tool-protocol/src/ids.rs` — `validate_tool_id`；`crates/common/tool-protocol/src/ids.rs` — `ToolCallId`。

### Requirement: Subagent capability meet
SubagentCapabilityMode SHALL 序列化为 read-only/read-write/execute/all，默认 ReadWrite；intersection 按能力交集收窄，All 为单位，ReadOnly 为下界。

#### Scenario: 不可比较分支
- **WHEN** ReadWrite 与 Execute 取交集
- **THEN** 返回 ReadOnly，既不扩大也不在本层报错；isolation enum 为 none/worktree，默认 None。

证据：`crates/common/tool-types/src/task.rs` — `SubagentCapabilityMode`；`crates/common/tool-types/src/task.rs` — `SubagentIsolationMode`。
### Requirement: Pager permission slash suggestions and runtime gate

permission_items SHALL 总按Ask、可选Auto、Always Approve顺序生成；Auto只在AppCtx.auto_permission_available时加入，当前项仅以id与current_permission精确字符串相等标记`(current)`，match_text连接id/label/description，insert_text为id。PermissionCommand接受可选参数、session_scoped且offered_when_session_less；suggest_args忽略query并返回完整列表。空参数打开command=permission且空query的OpenCommandPicker；非空先trim并ASCII lowercase，ask与always-approve直接映射SetPermissionMode，auto另要求CommandExecCtx.pager_state.auto_mode_gate，否则与未知值共同返回Unknown or unavailable错误。AskCommand同样session_scoped且可在无session时offered，run直接设Ask。建议可用性与执行gate来自两个快照，本文件不证明两者时序一致，也不持久化permission。

#### Scenario: Auto shown but execution gate closed
- **WHEN** 建议阶段AppCtx允许Auto，执行阶段pager_state.auto_mode_gate为false
- **THEN** 列表可含Auto，但运行/permission auto返回不可用错误。

#### Scenario: Bare permission command
- **WHEN** 参数trim后为空
- **THEN** 打开permission参数picker，不立即切换模式。

证据：`crates/codegen/pager/src/slash/commands/permission.rs` — `permission_items / PermissionCommand::offered_when_session_less / suggest_args / run / AskCommand::run`。

### Requirement: Pager direct permission slash shortcuts

AlwaysApproveCommand与AutoCommand SHALL 都标记session_scoped且offered_when_session_less=true，并分别无条件返回SetPermissionMode(AlwaysApprove)与SetPermissionMode(Auto)；run不读取当前permission，因此重复选择仍产生同一Action，也不检查参数。AutoCommand本文件内不读取auto_mode_gate，其隐藏由SlashController的外部可用性设置负责；与会在run中复查gate的`/permission auto`路径不同。AlwaysApprove没有额外feature gate。两者只产生Action，持久化、rollback与toast由dispatch层负责。

#### Scenario: Idempotent direct selection
- **WHEN** 当前已经是目标permission mode
- **THEN** 仍返回相同SetPermissionMode Action，不在命令层短路。

#### Scenario: Direct auto run without local check
- **WHEN** 测试或调用方直接调用AutoCommand::run
- **THEN** 不读取pager_state.auto_mode_gate并生成Auto Action，可用性约束依赖外围层。

证据：`crates/codegen/pager/src/slash/commands/always_approve.rs` — `AlwaysApproveCommand::session_scoped / offered_when_session_less / run`；`crates/codegen/pager/src/slash/commands/auto.rs` — `AutoCommand::session_scoped / offered_when_session_less / run`。
### Requirement: Pager ask-user reverse request session routing and replacement

The ask-user reverse-request handler SHALL parse the typed payload or reply with ACP -32602, route its session through the shared root/child matcher, and drop the response sender when no local concrete view exists so transport failure completes the request. On a valid target it cancels any existing question response, adds that question's open duration to paused time, restores its stashed prompt, emits a cancellation notice for displaced local question kinds, then stashes the current prompt, installs the new question with response sender and mode, clears prompt text and stamps last_active_at. Every successfully opened question returns true even off-screen so dashboard and parent status can repaint. It does not immediately answer the new request, preserve a displaced local directive payload, or distinguish active-view redraw in its return value.

#### Scenario: Invalid params
- **WHEN** typed request parsing fails
- **THEN** an Invalid params ACP error is sent and false returned.

#### Scenario: Unknown view
- **WHEN** session matching or concrete child resolution fails
- **THEN** the response sender is dropped and false returned.

#### Scenario: Replacement
- **WHEN** the target already has a question
- **THEN** the old response is Cancelled, its prompt is restored and local kinds receive a notice.

#### Scenario: Open
- **WHEN** a valid target accepts the question
- **THEN** prompt is stashed and cleared, question state and activity time are installed, and true is returned.

#### Scenario: Deferred response
- **WHEN** the overlay opens
- **THEN** the new response sender remains stored until later user action or replacement.

证据：`crates/codegen/pager/src/app/acp_handler/interactions.rs` — `handle_ask_user_question`。

### Requirement: Pager plan approval reverse request overlay ownership

The plan-approval reverse-request handler SHALL parse typed params, reject blank trimmed plan content with ACP -32602, route to the concrete root or child view, and drop the response sender on unresolved ownership. Replacing an existing approval sends its stale cancellation, restores its prompt and clears the line viewer; active modal and block viewer are dismissed, an in-progress casual comment restores its earlier prompt, and current plan comments and edit ranges reset. The new approval stashes and clears the prompt, attempts a plan preview, marks an available plan viewer feedback-active or focuses the approval prompt, and returns true only when the concrete target view is visible. It does not dismiss a question overlay in this function, preserve comments across replacement, or send an approval response before user action.

#### Scenario: Invalid plan
- **WHEN** JSON is malformed or planContent trims empty
- **THEN** ACP -32602 is sent and false returned.

#### Scenario: Unknown target
- **WHEN** the addressed concrete view cannot be resolved
- **THEN** the response sender is dropped and false returned.

#### Scenario: Replacement
- **WHEN** an approval is already active
- **THEN** it receives stale cancellation and its stashed prompt is restored before new state is installed.

#### Scenario: Overlay ownership
- **WHEN** the new approval opens
- **THEN** modal, block viewer, casual edit and prior plan-comment state are cleared as specified.

#### Scenario: Visibility
- **WHEN** the target is parked off-screen
- **THEN** state opens but false is returned; a visible target returns true.

证据：`crates/codegen/pager/src/app/acp_handler/interactions.rs` — `handle_plan_approval`。
### Requirement: Pager permission owner routing root auto-approval and notification

Permission handling SHALL match the request session to a root or child owner and cancel unknown or vanished owners. A root whose owning session is always-approve auto-selects the first AllowOnce option and returns false without notification or redraw; it does not select AllowAlways when AllowOnce is absent, and Child requests never inherit the root UI switch. Otherwise one ApprovalRequired notification is emitted only when the notification service's permission suppression is false, the request is FIFO-enqueued on the parent root interaction layer, and redraw is returned only for an active owning parent. Notification can occur before a later parent lookup fails and cancels the request.

#### Scenario: Unknown session
- **WHEN** shared session matching returns none
- **THEN** Cancelled is sent and false returned.

#### Scenario: Root always approve
- **WHEN** the root owner is always-approve and offers AllowOnce
- **THEN** that option is selected immediately without redraw.

#### Scenario: Missing AllowOnce
- **WHEN** always-approve offers only other allow kinds
- **THEN** the request falls through to interactive enqueue.

#### Scenario: Child request
- **WHEN** a child of an always-approve root asks
- **THEN** the root switch is not inherited and the request is queued.

#### Scenario: Notification gate
- **WHEN** the permission notification is not suppressed
- **THEN** one ApprovalRequired event is sent and the service is marked notified before enqueue.

证据：`crates/codegen/pager/src/app/acp_handler/permissions.rs` — `handle_permission_request`、`cancel_permission`。

### Requirement: Pager permission FIFO state prompt and pane transition

Permission enqueue SHALL tolerantly parse Bash highlights from request meta, derive the default always-allow word count, parse MCP tool scope only from the `allow-always-mcp` option meta, build provenance and display fields, assign and increment the agent-local request id, and resolve the initial option cursor. On the empty-to-nonempty transition it stashes and clears the prompt; if Scrollback owns the pane it also remembers that pane and forces Prompt. It clones request options, pushes a PermissionViewState without replacing older requests, stamps session last_active_at and returns true. Later enqueues do not restash prompt or pane and malformed optional metadata silently removes the corresponding enhancement.

#### Scenario: First request
- **WHEN** the permission queue and stash are empty
- **THEN** prompt is stashed and cleared before FIFO push.

#### Scenario: Scrollback transition
- **WHEN** the first request arrives while Scrollback is active
- **THEN** Scrollback is remembered and Prompt is forced.

#### Scenario: Concurrent request
- **WHEN** the queue is already nonempty
- **THEN** the request is appended with a new id without replacing or restashing.

#### Scenario: Optional metadata
- **WHEN** Bash or MCP metadata fails typed parsing
- **THEN** enqueue continues without that parsed state.

#### Scenario: Activity
- **WHEN** the state is pushed
- **THEN** last_active_at is reset to the current Instant and true is returned.

证据：`crates/codegen/pager/src/app/acp_handler/permissions.rs` — `enqueue_permission`。

### Requirement: Pager permission subagent provenance confidence tiers

Permission provenance SHALL be absent when the request session exactly equals the root session id. A non-root id found in `subagent_sessions` is labelled `Subagent <canonical formatted title>:`; any other non-root id is labelled `Child session (untracked):`. When the root session id itself is absent, every supplied id is treated as non-root and therefore tracked or untracked child provenance.

#### Scenario: Root
- **WHEN** the root session id exists and equals the request id
- **THEN** no provenance label is returned.

#### Scenario: Tracked child
- **WHEN** subagent_sessions contains the non-root id
- **THEN** the canonical formatted subagent title is prefixed and suffixed as a label.

#### Scenario: Opaque child
- **WHEN** the non-root id has no tracked record
- **THEN** the untracked child label is returned.

#### Scenario: Unassigned root
- **WHEN** the agent root session id is absent
- **THEN** the request id cannot be recognized as root by this helper.

证据：`crates/codegen/pager/src/app/acp_handler/permissions.rs` — `resolve_subagent_label`。

### Requirement: Pager permission title raw command and kind fallback

Permission display construction SHALL try to deserialize shared BashToolInput from raw_input, otherwise extract a raw command only from an ACP title exactly wrapped as an Execute backtick command. A request is execute-like when Bash highlights exist, kind is Execute or a raw command was recovered. Execute title prefers a nonblank trimmed Bash description, then the first highlighted word as an Allow prompt, then `Allow Execute?`; raw command is returned only for execute-like requests. Edit-like requests are detected by an AllowAlways option whose lowercased name contains `edit`, then prefer raw_input.file_path, otherwise a prettified title, otherwise `Allow Edit?`. Other titled tools use the prettified title, and untitled kinds fall back to Edit, Execute, Delete or generic Allow. Description lines are delegated separately.

#### Scenario: Typed Bash
- **WHEN** raw_input parses as BashToolInput
- **THEN** command and optional trimmed description drive execute display.

#### Scenario: Title command
- **WHEN** typed parsing fails but title has the exact Execute backtick wrapper
- **THEN** the inner command makes the request execute-like.

#### Scenario: Edit path
- **WHEN** the option-name heuristic marks edit and raw_input.file_path is a string
- **THEN** the path appears in the edit title.

#### Scenario: Named tool
- **WHEN** the request is neither execute nor edit and has a title
- **THEN** the MCP-qualified name is prettified in the title.

#### Scenario: Untitled kind
- **WHEN** no earlier branch applies
- **THEN** known kinds or the generic Allow fallback provide the title.

证据：`crates/codegen/pager/src/app/acp_handler/permissions.rs` — `build_permission_display`、`is_edit_permission`。

### Requirement: Pager permission protected edit and MCP argument description ordering

Permission description construction SHALL start with MCP planned-argument lines and, when the request is edit-like and meta parses as ProtectedEditPermission with a nonempty description, insert that protected-edit note at index zero. Missing, malformed or empty protected descriptions are omitted. Non-edit requests never show the protected description through this path even if their meta has the same shape.

#### Scenario: Protected edit
- **WHEN** edit detection succeeds and typed meta contains a nonempty description
- **THEN** the note precedes all MCP argument lines.

#### Scenario: Malformed meta
- **WHEN** ProtectedEditPermission parsing fails
- **THEN** no protected note is added.

#### Scenario: Empty note
- **WHEN** the parsed description is empty
- **THEN** it is omitted.

#### Scenario: Non-edit
- **WHEN** compatible protected meta appears on another request kind
- **THEN** the note is not inserted.

证据：`crates/codegen/pager/src/app/acp_handler/permissions.rs` — `permission_description_lines`、`protected_edit_description`。

### Requirement: Pager MCP permission planned argument formatting bounds

MCP argument display SHALL operate only when raw_input.variant is exactly UseTool or MCPTool and nonnull tool_input exists. It pretty-serializes that value as JSON, truncates each produced line after 2000 Unicode scalar positions with an ellipsis, and when more than 200 lines exist retains the first 200 plus a summary line containing the hidden count. Missing, null, nonmatching or serialization-failing inputs return no lines. The limit is scalar-count based rather than terminal display width or bytes, and output may contain 201 stored lines after the summary is appended.

#### Scenario: MCP arguments
- **WHEN** a recognized variant has nonnull serializable tool_input
- **THEN** pretty JSON lines are returned.

#### Scenario: Line bound
- **WHEN** a pretty line exceeds 2000 scalar values
- **THEN** the prefix through that boundary plus an ellipsis is stored.

#### Scenario: Count bound
- **WHEN** pretty JSON produces more than 200 lines
- **THEN** 200 lines and one hidden-count summary remain.

#### Scenario: Not MCP
- **WHEN** the variant is missing or different
- **THEN** no argument lines are returned.

#### Scenario: No payload
- **WHEN** tool_input is absent or null
- **THEN** no argument lines are returned.

证据：`crates/codegen/pager/src/app/acp_handler/permissions.rs` — `mcp_args_lines`、`MCP_ARGS_MAX_LINES`、`MCP_ARGS_MAX_LINE_CHARS`。
### Requirement: Pager subagent permission decision immutable audit row

SubagentPermissionDecision SHALL append one immutable audit row carrying child/tool/access/outcome/source/reason/classifier/latency fields and an optional formatted child title. Unknown children still render; this path neither mutates authorization state nor deduplicates independently.

#### Scenario: Known child
- **WHEN** metadata exists
- **THEN** the formatted child title accompanies the full decision audit.

#### Scenario: Unknown child
- **WHEN** metadata is absent
- **THEN** the row still renders without a title.

#### Scenario: State
- **WHEN** the audit is appended
- **THEN** no permission selection changes here.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `SubagentPermissionDecision`。
### Requirement: Pager interaction resolved root and child dismissal routing

Root InteractionResolved SHALL dismiss on the root. Child resolution first tries the owning parent because permissions are centralized, then short-circuit attempts the concrete child for other interactions. Parent success skips child dismissal; missing child preserves the parent result.

#### Scenario: Root
- **WHEN** a root interaction resolves
- **THEN** root dismissal receives session and tool identity.

#### Scenario: Central child
- **WHEN** parent dismissal succeeds
- **THEN** child dismissal is skipped.

#### Scenario: Child local
- **WHEN** parent fails and child exists
- **THEN** child dismissal is attempted.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `InteractionResolved`、`handle_child_session_notification`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tool/authorization.rs session tool authorization and dispatch contract

crates/codegen/shell/src/session/actor/tool/authorization.rs SHALL 维护 session tool authorization and dispatch 的入口 public_workflow_conflict, recognizable_shell_write, shell_required_access, project_call_access, hash_canonical_json, write, trajectory_meta, issue_tool_call_permit, recognizable_shell_write_paths, shell_write_path, recognizable_shell_write_under, workflow_path_write, normalized_path_under, resolve_existing_ancestors, definition_edit_path, saved_workflow_definition_write, session_workflow_definition_write, workflow_definition_write (plus 8 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、child process lifecycle、session/timeline state projection、MCP integration boundary；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** public_workflow_conflict, recognizable_shell_write, shell_required_access, project_call_access, hash_canonical_json, write, trajectory_meta, issue_tool_call_permit, recognizable_shell_write_paths, shell_write_path, recognizable_shell_write_under, workflow_path_write, normalized_path_under, resolve_existing_ancestors, definition_edit_path, saved_workflow_definition_write, session_workflow_definition_write, workflow_definition_write (plus 8 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** public_workflow_conflict, recognizable_shell_write, shell_required_access, project_call_access, hash_canonical_json, write, trajectory_meta, issue_tool_call_permit, recognizable_shell_write_paths, shell_write_path, recognizable_shell_write_under, workflow_path_write, normalized_path_under, resolve_existing_ancestors, definition_edit_path, saved_workflow_definition_write, session_workflow_definition_write, workflow_definition_write (plus 8 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

证据：`crates/codegen/shell/src/session/actor/tool/authorization.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tool/mod.rs session tool authorization and dispatch contract

crates/codegen/shell/src/session/actor/tool/mod.rs SHALL 维护 session tool authorization and dispatch 的入口 retain_batch_terminal_result, execute_tool_calls, execute_tool_calls_batch, str, request_plan_approval, finish_plan_to_default, finish_plan_to_default_if, reconcile_restored_plan_approval, reconcile_restored_plan_handoff_notification, admit_plan_handoff_notification, resume_plan_approval, preflight_terminals_keep_their_causal_outcome, terminal_task_failure_carries_its_receipt_identity, peels_redundant_session_cd_from_title, keeps_command_when_cd_not_redundant, shell_projection_distinguishes_observation_mutation_and_opaque_syntax, workflow_projection_narrows_the_all_descriptor_by_action, dynamic_and_mcp_calls_never_fall_back_to_read (plus 34 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** retain_batch_terminal_result, execute_tool_calls, execute_tool_calls_batch, str, request_plan_approval, finish_plan_to_default, finish_plan_to_default_if, reconcile_restored_plan_approval, reconcile_restored_plan_handoff_notification, admit_plan_handoff_notification, resume_plan_approval, preflight_terminals_keep_their_causal_outcome, terminal_task_failure_carries_its_receipt_identity, peels_redundant_session_cd_from_title, keeps_command_when_cd_not_redundant, shell_projection_distinguishes_observation_mutation_and_opaque_syntax, workflow_projection_narrows_the_all_descriptor_by_action, dynamic_and_mcp_calls_never_fall_back_to_read (plus 34 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** retain_batch_terminal_result, execute_tool_calls, execute_tool_calls_batch, str, request_plan_approval, finish_plan_to_default, finish_plan_to_default_if, reconcile_restored_plan_approval, reconcile_restored_plan_handoff_notification, admit_plan_handoff_notification, resume_plan_approval, preflight_terminals_keep_their_causal_outcome, terminal_task_failure_carries_its_receipt_identity, peels_redundant_session_cd_from_title, keeps_command_when_cd_not_redundant, shell_projection_distinguishes_observation_mutation_and_opaque_syntax, workflow_projection_narrows_the_all_descriptor_by_action, dynamic_and_mcp_calls_never_fall_back_to_read (plus 34 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** retain_batch_terminal_result, execute_tool_calls, execute_tool_calls_batch, str, request_plan_approval, finish_plan_to_default, finish_plan_to_default_if, reconcile_restored_plan_approval, reconcile_restored_plan_handoff_notification, admit_plan_handoff_notification, resume_plan_approval, preflight_terminals_keep_their_causal_outcome, terminal_task_failure_carries_its_receipt_identity, peels_redundant_session_cd_from_title, keeps_command_when_cd_not_redundant, shell_projection_distinguishes_observation_mutation_and_opaque_syntax, workflow_projection_narrows_the_all_descriptor_by_action, dynamic_and_mcp_calls_never_fall_back_to_read (plus 34 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/actor/tool/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tool/parse.rs session tool authorization and dispatch contract

crates/codegen/shell/src/session/actor/tool/parse.rs SHALL 维护 session tool authorization and dispatch 的入口 handle_tool_parse_error, execute_tool_call_parts。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** handle_tool_parse_error, execute_tool_call_parts 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

证据：`crates/codegen/shell/src/session/actor/tool/parse.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tool/preparation.rs session tool authorization and dispatch contract

crates/codegen/shell/src/session/actor/tool/preparation.rs SHALL 维护 session tool authorization and dispatch 的入口 stamp_tool_meta, stamp_tool_call_authority_meta, send_tool_call_start, prepare_tool_call。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、child process lifecycle、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** stamp_tool_meta, stamp_tool_call_authority_meta, send_tool_call_start, prepare_tool_call 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** stamp_tool_meta, stamp_tool_call_authority_meta, send_tool_call_start, prepare_tool_call 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

证据：`crates/codegen/shell/src/session/actor/tool/preparation.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/tool/result.rs session tool authorization and dispatch contract

crates/codegen/shell/src/session/actor/tool/result.rs SHALL 维护 session tool authorization and dispatch 的入口 is_mcp_create_pull_request, tool_execution_span, record_tool_span_outcome, consumed_completion_id_from_tool_error, undispatched_tool_outcome, str, is_interruptible_wait_tool, wait_for_pending_interjection, interrupted_wait_tool_result, interrupted_wait_tool_result_with_msg, drop_pending_items_for_consumed_completions, acknowledge_consumed_notifications, drop_pending_synthetic_items, record_git_pr_signals, record_pr_created, handle_bridge_tool_success, handle_tool_error, send_thought_chunk (plus 3 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、child process lifecycle、timeout/deadline or timing decisions、session/timeline state projection、MCP integration boundary；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** is_mcp_create_pull_request, tool_execution_span, record_tool_span_outcome, consumed_completion_id_from_tool_error, undispatched_tool_outcome, str, is_interruptible_wait_tool, wait_for_pending_interjection, interrupted_wait_tool_result, interrupted_wait_tool_result_with_msg, drop_pending_items_for_consumed_completions, acknowledge_consumed_notifications, drop_pending_synthetic_items, record_git_pr_signals, record_pr_created, handle_bridge_tool_success, handle_tool_error, send_thought_chunk (plus 3 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

证据：`crates/codegen/shell/src/session/actor/tool/result.rs`。

### Requirement: Shell crates/codegen/shell/src/tools/mod.rs shared tool runtime and notifications contract

crates/codegen/shell/src/tools/mod.rs SHALL 维护 shared tool runtime and notifications 的入口 the file module entrypoint。实现显示该边界包含 MCP integration boundary；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Primary module path
- **WHEN** 调用 the file module entrypoint 的主入口
- **THEN** 按源码声明的转换或调度路径返回结果。

证据：`crates/codegen/shell/src/tools/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/tools/notification_bridge.rs shared tool runtime and notifications contract

crates/codegen/shell/src/tools/notification_bridge.rs SHALL 维护 shared tool runtime and notifications 的入口 NotificationBridgeConfig, resolved_tool_name, stamp_event_id, stamp_scheduler_meta, durable_append_landed, handle_scheduled_task_removed, spawn_notification_bridge, emit_current_mode_update, notification_owner, handle_notification_with_ack, coordination_phase_update, handle_notification, coordination_phase_uses_standard_in_progress_with_private_meta, make_test_config, make_test_config_full, make_test_config_full_raw, make_task_snapshot, bash_task_completed_injects_bash_task_completed_source (plus 33 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** NotificationBridgeConfig, resolved_tool_name, stamp_event_id, stamp_scheduler_meta, durable_append_landed, handle_scheduled_task_removed, spawn_notification_bridge, emit_current_mode_update, notification_owner, handle_notification_with_ack, coordination_phase_update, handle_notification, coordination_phase_uses_standard_in_progress_with_private_meta, make_test_config, make_test_config_full, make_test_config_full_raw, make_task_snapshot, bash_task_completed_injects_bash_task_completed_source (plus 33 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** NotificationBridgeConfig, resolved_tool_name, stamp_event_id, stamp_scheduler_meta, durable_append_landed, handle_scheduled_task_removed, spawn_notification_bridge, emit_current_mode_update, notification_owner, handle_notification_with_ack, coordination_phase_update, handle_notification, coordination_phase_uses_standard_in_progress_with_private_meta, make_test_config, make_test_config_full, make_test_config_full_raw, make_task_snapshot, bash_task_completed_injects_bash_task_completed_source (plus 33 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** NotificationBridgeConfig, resolved_tool_name, stamp_event_id, stamp_scheduler_meta, durable_append_landed, handle_scheduled_task_removed, spawn_notification_bridge, emit_current_mode_update, notification_owner, handle_notification_with_ack, coordination_phase_update, handle_notification, coordination_phase_uses_standard_in_progress_with_private_meta, make_test_config, make_test_config_full, make_test_config_full_raw, make_task_snapshot, bash_task_completed_injects_bash_task_completed_source (plus 33 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/tools/notification_bridge.rs`。

### Requirement: Shell crates/codegen/shell/src/tools/todo.rs shared tool runtime and notifications contract

crates/codegen/shell/src/tools/todo.rs SHALL 维护 shared tool runtime and notifications 的入口 todo_item_from_plan_entry, plan_entry_from_todo_item。实现显示该边界包含 serde-backed wire/config types；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Primary module path
- **WHEN** 调用 todo_item_from_plan_entry, plan_entry_from_todo_item 的主入口
- **THEN** 按源码声明的转换或调度路径返回结果。

证据：`crates/codegen/shell/src/tools/todo.rs`。

### Requirement: Shell crates/codegen/shell/src/tools/tool_context.rs shared tool runtime and notifications contract

crates/codegen/shell/src/tools/tool_context.rs SHALL 维护 shared tool runtime and notifications 的入口 holds, is, TaskOutputTokenBudget, TaskOutputTokenBudgetState, limited, remaining, clamp_request, record_reported_output, mark_incomplete_and_exhaust, usage, is_limited, BlockingWaitState, BlockingWaitInner, new, depth, set_depth_for_test, reset, BlockingWaitGuard (plus 16 additional private symbols)。实现显示该边界包含 explicit error/result paths、channel or acknowledgement flow、child process lifecycle、platform or feature-gated branches、session/timeline state projection、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** holds, is, TaskOutputTokenBudget, TaskOutputTokenBudgetState, limited, remaining, clamp_request, record_reported_output, mark_incomplete_and_exhaust, usage, is_limited, BlockingWaitState, BlockingWaitInner, new, depth, set_depth_for_test, reset, BlockingWaitGuard (plus 16 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** holds, is, TaskOutputTokenBudget, TaskOutputTokenBudgetState, limited, remaining, clamp_request, record_reported_output, mark_incomplete_and_exhaust, usage, is_limited, BlockingWaitState, BlockingWaitInner, new, depth, set_depth_for_test, reset, BlockingWaitGuard (plus 16 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/tools/tool_context.rs`。
### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/task/backend.rs subagent task lifecycle and coordination contract
crates/codegen/tools/src/implementations/grow_build/task/backend.rs SHALL implement the subagent task lifecycle and coordination boundary through enforce subagent depth/security context, route spawn/query/cancel/validate requests, and preserve terminal completion semantics. Its source symbols abstracting, SubagentBackend, spawn, query, cancel, synchronously, validate_type, SubagentBackendResource, backend, fmt, ChannelBackend, new, for_session, parent_session_id, sender, into_resource, cancel_parent_prompt, cancel_parent_session (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、child process execution、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/task/backend.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/task/backend.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `abstracting`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `SubagentBackend`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `spawn`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `query`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `cancel`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `synchronously`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `validate_type`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `SubagentBackendResource`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `backend`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `fmt`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `ChannelBackend`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `new`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `for_session`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `parent_session_id`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `sender`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `into_resource`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `cancel_parent_prompt`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `cancel_parent_session`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `request_cancel_parent_session`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `open_spawn_admission`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `inspect`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `list_running`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `spawned_refs_for_prompt`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `registry_counts`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `spawn_with_foreground_wait`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `CancelResultReceiverOnDrop`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `drop`；`crates/codegen/tools/src/implementations/grow_build/task/backend.rs` — `VALIDATE_TYPE_TIMEOUT`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs subagent task lifecycle and coordination contract
crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs SHALL implement the subagent task lifecycle and coordination boundary through enforce subagent depth/security context, route spawn/query/cancel/validate requests, and preserve terminal completion semantics. Its source symbols recv_event, channel_backend_spawn_success, channel_backend_spawn_closed_channel, channel_backend_query_found, channel_backend_query_non_blocking_passes_through, channel_backend_query_not_found, channel_backend_cancel_success, channel_backend_cancel_closed_channel, workflow_spawn_future_drop_cancels_but_task_drop_does_not, request_for, channel_backend_spawn_result_dropped, channel_backend_query_closed_channel, channel_backend_validate_type_round_trips_outcome, channel_backend_validate_type_propagates_unknown_outcome, channel_backend_validate_type_returns_validation_unavailable_when_channel_closed, channel_backend_validate_type_returns_validation_unavailable_when_responder_dropped, channel_backend_validate_type_logs_warn_on_timeout, parse_timeout_ms_returns_none_for_unset (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、child process execution、timeout, budget, or rate limit、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `recv_event`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `channel_backend_spawn_success`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `channel_backend_spawn_closed_channel`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `channel_backend_query_found`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `channel_backend_query_non_blocking_passes_through`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `channel_backend_query_not_found`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `channel_backend_cancel_success`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `channel_backend_cancel_closed_channel`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `workflow_spawn_future_drop_cancels_but_task_drop_does_not`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `request_for`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `channel_backend_spawn_result_dropped`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `channel_backend_query_closed_channel`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `channel_backend_validate_type_round_trips_outcome`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `channel_backend_validate_type_propagates_unknown_outcome`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `channel_backend_validate_type_returns_validation_unavailable_when_channel_closed`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `channel_backend_validate_type_returns_validation_unavailable_when_responder_dropped`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `channel_backend_validate_type_logs_warn_on_timeout`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `parse_timeout_ms_returns_none_for_unset`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `parse_timeout_ms_returns_none_for_unparseable`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `parse_timeout_ms_returns_none_for_zero`；`crates/codegen/tools/src/implementations/grow_build/task/backend_tests.rs` — `parse_timeout_ms_returns_value_for_positive_integer`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/task/coordinator/query.rs subagent task lifecycle and coordination contract
crates/codegen/tools/src/implementations/grow_build/task/coordinator/query.rs SHALL implement the subagent task lifecycle and coordination boundary through enforce subagent depth/security context, route spawn/query/cancel/validate requests, and preserve terminal completion semantics. Its source symbols handle_query, handle_inspect, persisted_output, completed_snapshot_for_query, completed_inspection_for_query, ready_snapshot, handle_list_running, queue_active_progress, finish_progress, finish_list_slot follow explicit markers channel, fanout, or acknowledgement flow、child process execution、timeout, budget, or rate limit、session, prompt, goal, or subagent context、image/PDF/media processing; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/task/coordinator/query.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/grow_build/task/coordinator/query.rs` — `handle_query`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator/query.rs` — `handle_inspect`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator/query.rs` — `persisted_output`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator/query.rs` — `completed_snapshot_for_query`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator/query.rs` — `completed_inspection_for_query`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator/query.rs` — `ready_snapshot`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator/query.rs` — `handle_list_running`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator/query.rs` — `queue_active_progress`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator/query.rs` — `finish_progress`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator/query.rs` — `finish_list_slot`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs subagent task lifecycle and coordination contract
crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs SHALL implement the subagent task lifecycle and coordination boundary through enforce subagent depth/security context, route spawn/query/cancel/validate requests, and preserve terminal completion semantics. Its source symbols SubagentCoordinator, PromptScope, GoalCancelWaiter, new, run, handle_command, handle_internal, finish_child, finish_panicked_child, cancel_one, cancel_parent_prompt, prompt_scope_cancelled, teardown_session_children, cancel_parent_session, cancel_workflow_children, cancel_goal_children, resolve_goal_cancel_waiters, resolve_session_cancel_waiters (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、child process execution、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `SubagentCoordinator`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `PromptScope`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `GoalCancelWaiter`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `new`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `handle_command`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `handle_internal`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `finish_child`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `finish_panicked_child`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `cancel_one`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `cancel_parent_prompt`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `prompt_scope_cancelled`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `teardown_session_children`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `cancel_parent_session`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `cancel_workflow_children`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `cancel_goal_children`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `resolve_goal_cancel_waiters`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `resolve_session_cancel_waiters`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `session_child_ids`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `goal_child_ids`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `resolve_workflow_cancel_waiters`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `next_deadline`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `reap_abandoned_callers`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `process_deadlines`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `running_count_changed`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `cancel_all_children`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `belongs_to_session`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs` — `drop`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs subagent task lifecycle and coordination contract
crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs SHALL implement the subagent task lifecycle and coordination boundary through enforce subagent depth/security context, route spawn/query/cancel/validate requests, and preserve terminal completion semantics. Its source symbols MAX_COMPLETED_ENTRIES, LocalBoxFuture, SendBoxFuture, SubagentProgress, ChildControl, ProgressFuture, SecurityContext, progress, security_context, cancel, StartedChild, ChildRunRequest, ChildRunOutput, CompletionDisposition, ChildCompletion, ChildRunner, Control, CompletionData (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、child process execution、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `MAX_COMPLETED_ENTRIES`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `LocalBoxFuture`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `SendBoxFuture`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `SubagentProgress`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `ChildControl`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `ProgressFuture`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `SecurityContext`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `progress`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `security_context`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `cancel`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `StartedChild`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `ChildRunRequest`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `ChildRunOutput`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `CompletionDisposition`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `ChildCompletion`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `ChildRunner`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `Control`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `CompletionData`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `RunFuture`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `ValidateFuture`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `validate_type`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `on_completed`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `running_count_changed`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `persisted_output_ref`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `terminal_committed`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `load_persisted_output`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_state.rs` — `CoordinatorConfig`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs subagent task lifecycle and coordination contract
crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs SHALL implement the subagent task lifecycle and coordination boundary through enforce subagent depth/security context, route spawn/query/cancel/validate requests, and preserve terminal completion semantics. Its source symbols TestControl, ProgressFuture, SecurityContext, security_context, progress, cancel, TestRunner, Control, CompletionData, RunFuture, ValidateFuture, run, declares, validate_type, on_completed, persisted_output_ref, cancelled_result, request (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、child process execution、timeout, budget, or rate limit、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `TestControl`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `ProgressFuture`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `SecurityContext`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `security_context`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `progress`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `cancel`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `TestRunner`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `Control`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `CompletionData`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `RunFuture`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `ValidateFuture`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `declares`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `validate_type`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `on_completed`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `persisted_output_ref`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `cancelled_result`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `request`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `source_liveness_does_not_cross_sibling_security_parents`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `Harness`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `harness`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `harness_with_config`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `harness_with_options`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `parent_backend`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `loop_unit_active`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `outstanding`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `foreground_completion_is_delivered_inline`；`crates/codegen/tools/src/implementations/grow_build/task/coordinator_tests.rs` — `foreground_deadline_hands_off_without_stopping_child`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/task/mod.rs subagent task lifecycle and coordination contract
crates/codegen/tools/src/implementations/grow_build/task/mod.rs SHALL implement the subagent task lifecycle and coordination boundary through enforce subagent depth/security context, route spawn/query/cancel/validate requests, and preserve terminal completion semantics. Its source symbols MAX_SUBAGENT_DEPTH, effective_max_subagent_depth, TaskTool, kind, tool_namespace, description_template, requires_expr, Args, Output, id, description, capabilities, run, make_backend, goal_view, goal_runtime, make_backend_with_validation, make_backend_with_validation_fn (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、child process execution、timeout, budget, or rate limit、platform/feature conditional; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/task/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/task/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/task/mod.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `MAX_SUBAGENT_DEPTH`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `effective_max_subagent_depth`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `TaskTool`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `requires_expr`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `make_backend`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `goal_view`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `goal_runtime`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `make_backend_with_validation`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `make_backend_with_validation_fn`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `unwrap_spawn`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `depth_limit_exceeded`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `raised_max_depth_allows_nested_spawn`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `subagent_cannot_spawn_nested_subagent`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `missing_backend_returns_error`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `successful_subagent_returns_text_output`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `goal_owned_subagent_with_mismatched_snapshot_fails_closed`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `failed_subagent_returns_error`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `dropped_result_channel_returns_error`；`crates/codegen/tools/src/implementations/grow_build/task/mod.rs` — `auto_backgrounded_result_returns_task_id_text`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/task/types.rs subagent task lifecycle and coordination contract
crates/codegen/tools/src/implementations/grow_build/task/types.rs SHALL implement the subagent task lifecycle and coordination boundary through enforce subagent depth/security context, route spawn/query/cancel/validate requests, and preserve terminal completion semantics. Its source symbols channel, SubagentOwner, goal, workflow, workflow_run_id, goal_id, goal_definition_revision, is_workflow, SubagentRequest, SubagentSpawnRequest, Target, deref, respond_with, ModelOverrideProvenance, SubagentRuntimeOverrides, sanitize_cwd_value, is_valid_resume_id, SubagentResult (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、child process execution、timeout, budget, or rate limit、platform/feature conditional; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/task/types.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/task/types.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/task/types.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `channel`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `SubagentOwner`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `goal`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `workflow`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `workflow_run_id`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `goal_id`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `goal_definition_revision`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `is_workflow`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `SubagentRequest`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `SubagentSpawnRequest`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `Target`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `deref`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `respond_with`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `ModelOverrideProvenance`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `SubagentRuntimeOverrides`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `sanitize_cwd_value`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `is_valid_resume_id`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `SubagentResult`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `default`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `status`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `str`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `SubagentQueryRequest`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `SubagentLoopUnitActiveRequest`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `SubagentSnapshot`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `SubagentInspection`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `is_running`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `SubagentSnapshotStatus`；`crates/codegen/tools/src/implementations/grow_build/task/types.rs` — `is_terminal`。
### Requirement: Pager task-result test: kill_rpc_failure_does_not_finalize_but_nothing_live_does
A failed subagent cancel RPC SHALL leave a pending row unfinished; a NothingLive outcome SHALL finalize the orphan row.

#### Scenario: Subagent cancellation reconciliation
- **WHEN** cancel first fails and then reports no live process
- **THEN** the first result preserves the row and the second marks it finished.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `kill_rpc_failure_does_not_finalize_but_nothing_live_does`。

### Requirement: Pager task-result test: kill_nothing_live_with_status_stamps_real_terminal_status
NothingLive with a terminal status SHALL finalize the orphan subagent row with that real status instead of flattening it to cancelled.

#### Scenario: Subagent terminal status
- **WHEN** an orphan cancellation resolves with status completed
- **THEN** the row is finished and stores completed.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `kill_nothing_live_with_status_stamps_real_terminal_status`。

### Requirement: Pager task-result test: switch_agent_rpc_waits_for_authoritative_projection
A successful Agent switch acknowledgement SHALL leave the old agent projection until authoritative AgentChanged notification and SHALL not duplicate the durable control notice.

#### Scenario: Authoritative Agent projection
- **WHEN** RPC acknowledgement precedes AgentChanged
- **THEN** the target becomes current only after the notification and scrollback is unchanged.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `switch_agent_rpc_waits_for_authoritative_projection`。

### Requirement: Pager task-result test: late_session_agent_name_read_cannot_overwrite_completed_switch
A session-agent metadata read begun before a completed switch SHALL be discarded when it resolves after the authoritative switch projection.

#### Scenario: Stale agent metadata
- **WHEN** an old revision resolves after AgentChanged
- **THEN** the new agent name remains authoritative.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `late_session_agent_name_read_cannot_overwrite_completed_switch`。

### Requirement: Pager task-result test: authoritative_agent_projection_releases_the_fenced_prompt
The authoritative AgentChanged projection SHALL release a prompt fenced by the pending agent control once the control domain is committed.

#### Scenario: Agent control prompt release
- **WHEN** a queued prompt waits behind an agent switch
- **THEN** the prompt is emitted only after the authoritative projection.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `authoritative_agent_projection_releases_the_fenced_prompt`。

### Requirement: Pager task-result test: stale_switch_agent_completion_is_ignored
A switch-agent completion with an invalidated control token SHALL be ignored without changing the agent projection or adding local feedback.

#### Scenario: Stale Agent completion
- **WHEN** the control is invalidated before its RPC completion
- **THEN** no effect or scrollback notice is produced.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `stale_switch_agent_completion_is_ignored`。

### Requirement: Pager task-result test: switch_agent_complete_failure_writes_scrollback
A local Agent switch failure without a pre-published terminal notice SHALL append an error Notice naming the target agent.

#### Scenario: Agent switch failure
- **WHEN** the switch RPC fails locally
- **THEN** the failure appears once in scrollback.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `switch_agent_complete_failure_writes_scrollback`。

### Requirement: Pager task-result test: shell_owned_control_failure_does_not_duplicate_a_local_notice
A control failure marked terminal_published SHALL not append a duplicate local notice because the durable Shell rejection owns terminal feedback.

#### Scenario: Shell-owned control failure
- **WHEN** a switch failure carries terminal_published=true
- **THEN** scrollback length is unchanged.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `shell_owned_control_failure_does_not_duplicate_a_local_notice`。

### Requirement: Pager task-result test: model_and_agent_domains_dispatch_independently_but_jointly_fence_prompts
Model and Agent controls SHALL dispatch independently but jointly fence queued prompts until both authoritative projections commit.

#### Scenario: Joint control fencing
- **WHEN** model and Agent switches are pending while a prompt is queued
- **THEN** the prompt remains fenced until both ModelChanged and AgentChanged arrive.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `model_and_agent_domains_dispatch_independently_but_jointly_fence_prompts`。
### Requirement: Agent action dispatch and child-view boundary contract
Agent-level actions SHALL map cancel, background, external edit, command palette, model/agent/effort/permission/behavior pickers, settings, and registry actions to their declared outcomes; child views cannot own parent behavior control and running execute tools alone enable demotion.

#### Scenario: Agent action mapping
- **WHEN** a registered AgentScreen action is invoked
- **THEN** the declared Action, Changed, or Unchanged outcome is returned without crossing child-session ownership.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `handle_agent_action`。

### Requirement: Pager agent input test: ctrl_b_is_consumed_when_ineligible_and_demotes_when_eligible
Ctrl-B SHALL be consumed without editing or moving panes when no running execute tool exists, and SHALL demote an eligible running execute tool while preserving draft/cursor.

#### Scenario: Background demotion eligibility
- **WHEN** Ctrl-B is pressed in Prompt or Scrollback before and after a running execute tool is seeded
- **THEN** the ineligible case returns Changed and the eligible case returns DemoteToBackground.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `ctrl_b_is_consumed_when_ineligible_and_demotes_when_eligible`。

### Requirement: Pager agent input test: fullscreen_child_ctrl_b_never_demotes_child_or_parent
Ctrl-B in a fullscreen child view SHALL never demote either child or parent execute tools and SHALL keep the child view active without displaying a send-to-background affordance.

#### Scenario: Child fullscreen demotion boundary
- **WHEN** parent and child both have running execute tools and the child is fullscreen
- **THEN** the event is consumed without DemoteToBackground for either session.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `fullscreen_child_ctrl_b_never_demotes_child_or_parent`。

### Requirement: Pager agent input test: ctrl_x_a_opens_agent_picker
Ctrl-X then A SHALL open the Agent command picker.

#### Scenario: Leader Agent picker
- **WHEN** the leader sequence is entered
- **THEN** OpenCommandPicker targets agent.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `ctrl_x_a_opens_agent_picker`。

### Requirement: Pager agent input test: subagent_fullscreen_view_owns_esc
A fullscreen subagent view SHALL own Esc for child-view closure rather than advertise turn cancellation.

#### Scenario: Subagent Esc ownership
- **WHEN** a running subagent view is in Scrollback
- **THEN** the hint is false.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `subagent_fullscreen_view_owns_esc`。
### Requirement: The active permission view SHALL own keyboard input. FollowupInput SHALL support Esc back to options, Ctrl-C cancellation, multiline Enter routing, submit-to-PermissionFollowup, and prompt editing. Options SHALL support Ctrl-F argument expansion, Tab to scrollback, j/k or arrows, Enter and 1-9 option actions, RejectOnce text entry into followup mode, and left/right scope adjustment for MCP or Bash permissions.
handle_permission_key SHALL operate only on the FIFO front permission and return Changed for consumed view input. Scope arrows SHALL adjust MCP server/tool or Bash highlighted-word count; when the cursor is on a non-scoped option they SHALL move it to AllowAlways, while RejectAlways remains selected when narrowing a deny scope.

#### Scenario: Followup submit
- **WHEN** permission focus is FollowupInput and Enter submits the prompt
- **THEN** an Action::PermissionFollowup containing the current text is returned.

#### Scenario: Option shortcut
- **WHEN** permission focus is Options and Enter or a valid 1-9 key selects an option
- **THEN** an Action::PermissionSelect with that option id is returned.

#### Scenario: Scope adjustment
- **WHEN** left/right is pressed with adjustable MCP/Bash scope
- **THEN** the scope or highlighted word count changes and the row policy preserves the intended scoped row.

#### Scenario: Reject feedback
- **WHEN** the active option is RejectOnce and a non-digit text key is typed
- **THEN** focus changes to FollowupInput and the key is forwarded to the prompt.

证据：`crates/codegen/pager/src/app/agent_view/interactions.rs` — `AgentView::handle_permission_key`；`crates/codegen/pager/src/app/agent_view/interactions.rs` — `permission_scope_key_tests::scope_keys_keep_cursor_on_reject_always_row`；`crates/codegen/pager/src/app/agent_view/interactions.rs` — `permission_scope_key_tests::scope_keys_still_jump_from_neutral_row`；`crates/codegen/pager/src/app/agent_view/interactions.rs` — `permission_scope_key_tests::ctrl_f_toggles_args_expansion_when_args_present`。
