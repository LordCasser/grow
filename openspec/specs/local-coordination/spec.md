# local-coordination Specification

## Purpose
定义本机会话协调协议中可以识别和追踪的交互。覆盖 peer 握手字段、询问与取消的稳定标识，以及进度、结果、取消回执和错误的区分，不把协调通道等同于任意会话输入。

## Requirements

### Requirement: Peer handshake identity
本地协调连接 SHALL 使用包含 protocol version、peer identity、incarnation、bearer token 和 source session 的握手。

#### Scenario: 建立协调连接
- **WHEN** 客户端发送 ClientHello
- **THEN** 服务端通过 ServerHello 返回是否 accepted 及可能的错误。

证据：`crates/codegen/shell/src/coordination/protocol.rs` — `ClientHello`。

### Requirement: Inquiry scoped operations
协调协议 SHALL 以 inquiry_id 与 target_session_id 标识询问和取消，并区分进度、结果、取消回执与错误。

#### Scenario: 取消询问
- **WHEN** 客户端对已知 inquiry_id 发送 Cancel
- **THEN** 响应保留 inquiry_id 和 accepted 状态，不将取消回执伪装成询问答案。

证据：`crates/codegen/shell/src/coordination/protocol.rs` — `Request`。

### Requirement: Parent-child inquiries use isolated Sideband execution
A running child SHALL be able to ask its immediate parent a question, and a primary agent SHALL be able to ask a directly-owned running child. Runtime-derived identities SHALL authorize the relationship. Questions SHALL use asynchronous tool-free Sideband execution without entering or interrupting the target foreground context. Children SHALL NOT gain cross-session discovery/inquiry or upward intervention.

#### Scenario: Child clarification during parent work
- **WHEN** a child asks its parent while the parent foreground is busy or waiting for that child
- **THEN** a Sideband answers from frozen parent context, returns the answer to the child, and the main view shows one correlated inquiry row through receipt and completion.

#### Scenario: Parent asks child
- **WHEN** the parent asks its running child a question
- **THEN** the child answers via Sideband without changing its foreground task or requiring cross-workspace approval for its delegated worktree.

#### Scenario: Invalid route or cancellation
- **WHEN** a caller targets an unrelated, sibling, terminated or Workflow-owned hidden child, or cancels an in-flight inquiry
- **THEN** a typed local failure/cancellation is returned without injecting user input, leaking another session's context or pausing the Goal.

### Requirement: Parent intervention has explicit delivery timing
A primary agent SHALL be able to send a message to a directly-owned running child with an explicit immediate-interrupt or queued-next-step option. The message SHALL be attributed to the parent and durably received before acknowledgement. Retries and restoration SHALL preserve exactly-once inbox consumption.

#### Scenario: Queued intervention
- **WHEN** the parent sends a non-interrupting message
- **THEN** the child completes its current work boundary and includes the message at the next step's sampling without cancelling the active request.

#### Scenario: Immediate intervention
- **WHEN** the parent requests immediate interruption
- **THEN** the child safely preempts the current model request or interruptible wait and resamples with the message, preserving non-interruptible tool outcomes, Timeline integrity and the current Goal lifecycle.

#### Scenario: Restore or retry
- **WHEN** an acknowledged intervention is retried or its receiving session restores
- **THEN** the same receipt is not duplicated and unconsumed context remains available without impersonating human input.

### Requirement: Windows peer publication coexists with manifest readers
Windows peer heartbeat publication SHALL atomically replace the private manifest while an already-open discovery reader retains the previous file. Owner-only access, no-reparse validation and long-path support SHALL remain enforced; publication failure SHALL preserve the prior manifest.

#### Scenario: Reader spans heartbeat replacement
- **WHEN** a discovery reader holds a peer manifest open while a new heartbeat is published
- **THEN** that reader can finish reading the previous complete manifest, new readers see the replacement, and publication does not first delete the destination or relax its ACL.
