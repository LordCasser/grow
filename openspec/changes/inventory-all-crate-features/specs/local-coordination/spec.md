## ADDED Requirements

### Requirement: Pager turn cancellation queue mutation and session control effects

The pager root executor SHALL encode cancel policy in CancelNotification metadata, send plan-mode and queue mutations through their grow extension notifications, and map behavior, model/effort and agent switches to control-correlated completion results. A control response status of superseded SHALL map to Superseded; every other or absent status SHALL mean AuthoritativeUpdatePending. Control failures SHALL preserve whether a terminal update was already published and sanitize their message. Notification transport failures SHALL be warned but still collapse to CancelComplete. This file does not prove queue optimistic-version enforcement, cancel completion, control terminal delivery, mode/model/agent validity or reducer reconciliation.

#### Scenario: Cancel metadata
- **WHEN** optional pause, trigger or pristine rewind flags are enabled
- **THEN** their camelCase keys are added alongside cancelSubagents.

#### Scenario: Queue mutation
- **WHEN** remove, reorder, clear, edit, hold, release or interject is requested
- **THEN** the corresponding grow/queue notification includes its session and mutation-specific fields.

#### Scenario: Superseded control
- **WHEN** the control response reports status superseded
- **THEN** the completion result is Superseded; all other statuses await the authoritative update.

#### Scenario: Effort patch
- **WHEN** SwitchModel is an effort-only patch
- **THEN** it targets the reasoning effort config and requires a selected effort; otherwise it targets the model config.

#### Scenario: Notification send failure
- **WHEN** cancel, plan or queue notification delivery fails
- **THEN** the executor warns and returns CancelComplete without a failure payload.

证据：`crates/codegen/pager/src/app/root/effects/mod.rs`。

### Requirement: Pager background task subagent scheduler and terminal control effects

The pager root executor SHALL send background-task kill, subagent cancel, scheduled-task delete and terminal-background requests using their respective grow extension methods. Background kill SHALL parse the response into a typed outcome and preserve an ACP transport failure separately; subagent cancel SHALL collapse transport failure to RpcFailed. Scheduler delete and terminal demotion SHALL warn on transport failure and still return CancelComplete. This file does not prove that a remote process stops, scheduled state is deleted, a terminal detaches, late lifecycle events are reconciled or identifiers belong to the supplied session.

#### Scenario: Background kill response
- **WHEN** grow/task/kill succeeds
- **THEN** BgTaskKilled carries the parsed outcome for the session and task id.

#### Scenario: Subagent cancel failure
- **WHEN** grow/subagent/cancel fails
- **THEN** KillSubagentComplete carries RpcFailed rather than an error string.

#### Scenario: Best-effort scheduler control
- **WHEN** scheduler delete or terminal background send fails
- **THEN** the failure is only warned and the task completes as CancelComplete.

证据：`crates/codegen/pager/src/app/root/effects/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/coordination/inquiry.rs peer coordination and inquiry protocol contract

crates/codegen/shell/src/coordination/inquiry.rs SHALL 维护 peer coordination and inquiry protocol 的入口 MAX_QUESTION_BYTES, MAX_QUEUED_INQUIRIES, APPROVAL_TIMEOUT, INQUIRY_DEADLINE, TERMINAL_CACHE_TTL, InquiryPhase, InquiryStatus, InquiryOutcome, answered, terminal, InquiryState, InquiryAudit, InquiryEvent, timeline_kind, from_timeline, notice, IncomingInquiryAudit, received (plus 11 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** MAX_QUESTION_BYTES, MAX_QUEUED_INQUIRIES, APPROVAL_TIMEOUT, INQUIRY_DEADLINE, TERMINAL_CACHE_TTL, InquiryPhase, InquiryStatus, InquiryOutcome, answered, terminal, InquiryState, InquiryAudit, InquiryEvent, timeline_kind, from_timeline, notice, IncomingInquiryAudit, received (plus 11 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** MAX_QUESTION_BYTES, MAX_QUEUED_INQUIRIES, APPROVAL_TIMEOUT, INQUIRY_DEADLINE, TERMINAL_CACHE_TTL, InquiryPhase, InquiryStatus, InquiryOutcome, answered, terminal, InquiryState, InquiryAudit, InquiryEvent, timeline_kind, from_timeline, notice, IncomingInquiryAudit, received (plus 11 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/coordination/inquiry.rs`。

### Requirement: Shell crates/codegen/shell/src/coordination/manifest.rs peer coordination and inquiry protocol contract

crates/codegen/shell/src/coordination/manifest.rs SHALL 维护 peer coordination and inquiry protocol 的入口 SCHEMA_VERSION, TRANSPORT_KIND, HEARTBEAT_INTERVAL, LEASE_DURATION, SubagentStats, LocalSessionSnapshot, PeerSession, PeerManifest, PeerDescription, from, DiscoveredSession, coordination_dir, peers_dir, ensure_private_runtime_dirs, secure_directory, write_manifest, WRITE_NONCE, lock_peer (plus 14 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** SCHEMA_VERSION, TRANSPORT_KIND, HEARTBEAT_INTERVAL, LEASE_DURATION, SubagentStats, LocalSessionSnapshot, PeerSession, PeerManifest, PeerDescription, from, DiscoveredSession, coordination_dir, peers_dir, ensure_private_runtime_dirs, secure_directory, write_manifest, WRITE_NONCE, lock_peer (plus 14 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** SCHEMA_VERSION, TRANSPORT_KIND, HEARTBEAT_INTERVAL, LEASE_DURATION, SubagentStats, LocalSessionSnapshot, PeerSession, PeerManifest, PeerDescription, from, DiscoveredSession, coordination_dir, peers_dir, ensure_private_runtime_dirs, secure_directory, write_manifest, WRITE_NONCE, lock_peer (plus 14 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/coordination/manifest.rs`。

### Requirement: Shell crates/codegen/shell/src/coordination/mod.rs peer coordination and inquiry protocol contract

crates/codegen/shell/src/coordination/mod.rs SHALL 维护 peer coordination and inquiry protocol 的入口 the file module entrypoint。实现显示该边界包含 explicit error/result paths、timeout/deadline or timing decisions、session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** the file module entrypoint 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

证据：`crates/codegen/shell/src/coordination/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/coordination/protocol.rs peer coordination and inquiry protocol contract

crates/codegen/shell/src/coordination/protocol.rs SHALL 维护 peer coordination and inquiry protocol 的入口 PROTOCOL_VERSION, ClientHello, ServerHello, Request, Response。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** PROTOCOL_VERSION, ClientHello, ServerHello, Request, Response 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

证据：`crates/codegen/shell/src/coordination/protocol.rs`。

### Requirement: Shell crates/codegen/shell/src/coordination/runtime.rs peer coordination and inquiry protocol contract

crates/codegen/shell/src/coordination/runtime.rs SHALL 维护 peer coordination and inquiry protocol 的入口 CONNECT_TIMEOUT, SETUP_TIMEOUT, RECONNECT_MIN_DELAY, RECONNECT_MAX_DELAY, CANCEL_SETTLE_GRACE, InquiryPayload, InquiryRecord, new, expired, complete, InquiryRequestError, CoordinationStartError, Shared, require_ready, publish, current_manifest, owns_session, CoordinationRuntime (plus 63 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** CONNECT_TIMEOUT, SETUP_TIMEOUT, RECONNECT_MIN_DELAY, RECONNECT_MAX_DELAY, CANCEL_SETTLE_GRACE, InquiryPayload, InquiryRecord, new, expired, complete, InquiryRequestError, CoordinationStartError, Shared, require_ready, publish, current_manifest, owns_session, CoordinationRuntime (plus 63 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** CONNECT_TIMEOUT, SETUP_TIMEOUT, RECONNECT_MIN_DELAY, RECONNECT_MAX_DELAY, CANCEL_SETTLE_GRACE, InquiryPayload, InquiryRecord, new, expired, complete, InquiryRequestError, CoordinationStartError, Shared, require_ready, publish, current_manifest, owns_session, CoordinationRuntime (plus 63 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** CONNECT_TIMEOUT, SETUP_TIMEOUT, RECONNECT_MIN_DELAY, RECONNECT_MAX_DELAY, CANCEL_SETTLE_GRACE, InquiryPayload, InquiryRecord, new, expired, complete, InquiryRequestError, CoordinationStartError, Shared, require_ready, publish, current_manifest, owns_session, CoordinationRuntime (plus 63 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/coordination/runtime.rs`。

### Requirement: Shell crates/codegen/shell/src/local_ipc/frame.rs local IPC framing, transport, and security contract

crates/codegen/shell/src/local_ipc/frame.rs SHALL 维护 local IPC framing, transport, and security 的入口 MAX_FRAME_SIZE, FrameError, read_json, write_json, Message, json_round_trip_uses_big_endian_length_prefix, oversized_inbound_frame_is_rejected_before_payload_read, oversized_outbound_frame_is_rejected。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、platform or feature-gated branches、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** MAX_FRAME_SIZE, FrameError, read_json, write_json, Message, json_round_trip_uses_big_endian_length_prefix, oversized_inbound_frame_is_rejected_before_payload_read, oversized_outbound_frame_is_rejected 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** MAX_FRAME_SIZE, FrameError, read_json, write_json, Message, json_round_trip_uses_big_endian_length_prefix, oversized_inbound_frame_is_rejected_before_payload_read, oversized_outbound_frame_is_rejected 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/local_ipc/frame.rs`。

### Requirement: Shell crates/codegen/shell/src/local_ipc/mod.rs local IPC framing, transport, and security contract

crates/codegen/shell/src/local_ipc/mod.rs SHALL 维护 local IPC framing, transport, and security 的入口 the file module entrypoint。实现显示该边界包含 platform or feature-gated branches；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/local_ipc/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/local_ipc/security.rs local IPC framing, transport, and security contract

crates/codegen/shell/src/local_ipc/security.rs SHALL 维护 local IPC framing, transport, and security 的入口 denied, sid_string, current_user_sid, UserSecurityAttributes, new, as_mut_ptr, drop, wide_path, create_private_file, open_private_file, create_private_directory, verify_private_file, private_creation_and_read_only_acl_validation, discovery_rejects_everyone_grant_without_repairing_it。实现显示该边界包含 filesystem or durable record I/O、explicit error/result paths、child process lifecycle、platform or feature-gated branches、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** denied, sid_string, current_user_sid, UserSecurityAttributes, new, as_mut_ptr, drop, wide_path, create_private_file, open_private_file, create_private_directory, verify_private_file, private_creation_and_read_only_acl_validation, discovery_rejects_everyone_grant_without_repairing_it 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** denied, sid_string, current_user_sid, UserSecurityAttributes, new, as_mut_ptr, drop, wide_path, create_private_file, open_private_file, create_private_directory, verify_private_file, private_creation_and_read_only_acl_validation, discovery_rejects_everyone_grant_without_repairing_it 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/local_ipc/security.rs`。

### Requirement: Shell crates/codegen/shell/src/local_ipc/transport.rs local IPC framing, transport, and security contract

crates/codegen/shell/src/local_ipc/transport.rs SHALL 维护 local IPC framing, transport, and security 的入口 PrivateEndpoint, new, path, validate_socket_path, LocalListener, bind, accept, private_endpoint_is_short_and_its_directory_is_owner_only, overlong_socket_path_is_rejected_before_bind, socket_is_owner_only, listener_is_ready, create_server, LocalStream, StreamInner, connect, and, are, poll_read (plus 15 additional private symbols)。实现显示该边界包含 filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、child process lifecycle、platform or feature-gated branches、timeout/deadline or timing decisions；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** PrivateEndpoint, new, path, validate_socket_path, LocalListener, bind, accept, private_endpoint_is_short_and_its_directory_is_owner_only, overlong_socket_path_is_rejected_before_bind, socket_is_owner_only, listener_is_ready, create_server, LocalStream, StreamInner, connect, and, are, poll_read (plus 15 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** PrivateEndpoint, new, path, validate_socket_path, LocalListener, bind, accept, private_endpoint_is_short_and_its_directory_is_owner_only, overlong_socket_path_is_rejected_before_bind, socket_is_owner_only, listener_is_ready, create_server, LocalStream, StreamInner, connect, and, are, poll_read (plus 15 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** PrivateEndpoint, new, path, validate_socket_path, LocalListener, bind, accept, private_endpoint_is_short_and_its_directory_is_owner_only, overlong_socket_path_is_rejected_before_bind, socket_is_owner_only, listener_is_ready, create_server, LocalStream, StreamInner, connect, and, are, poll_read (plus 15 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/local_ipc/transport.rs`。
