# tool-authorization Specification

## Purpose
定义实际工具调用与可见工具目录之间的授权边界。覆盖显式 deny 优先、冻结 shell 命令的保守 RWX 投影，以及子 Agent 向后代委派时不可扩大的能力与 MCP identity 上限。

## Requirements

### Requirement: Deny before automatic approval
权限管理 SHALL 在 always-approve 快速放行前执行显式 deny 策略。

#### Scenario: 显式禁止命令
- **WHEN** 调用匹配 deny 规则且会话为 always-approve
- **THEN** 调用被拒绝。

证据：`crates/codegen/workspace/src/permission/manager.rs` — `policy_deny`。

### Requirement: Conservative shell access
shell 调用 SHALL 从冻结命令投影所需 RWX；无法解析或未知可执行程序按 All 处理。

#### Scenario: 未知程序
- **WHEN** 命令无法证明属于已知只读观察命令
- **THEN** shell_required_access 返回 All；已知无外发的观察命令可返回 ReadExecute。

证据：`crates/codegen/shell/src/session/actor/tool/authorization.rs` — `shell_required_access`。

### Requirement: Immutable delegated ceiling
子 Agent SHALL 将后代请求能力与创建时上限取交集，并绑定继承 MCP 的服务端及 client identity。

#### Scenario: 后代请求更大权限
- **WHEN** 子 Agent 请求创建权限更大的后代
- **THEN** constrain_mode 返回交集；不匹配初始 client_id 的 MCP binding 不可委派。

证据：`crates/codegen/shell/src/session/subagent_capability.rs` — `DelegableCapabilityCeiling`。

### Requirement: Web allowlist preserves path identity
web_fetch 静态允许列表 SHALL 仅对主机执行大小写、www 前缀和尾点规范化；路径匹配 SHALL 保留大小写和末尾点，并以完整路径段作为前缀边界。

#### Scenario: 不同路径不共享静态许可
- **WHEN** 配置 example.com/Docs 或 example.com/docs.
- **THEN** 对应路径及其子路径匹配，但 /docs 不因路径规范化而获得该条目的静态许可。

#### Scenario: 带路径主机规范化
- **WHEN** 配置 WWW.Example.COM./Docs
- **THEN** example.com/Docs 匹配，example.com/docs 不匹配。

### Requirement: Redirect targets require a new authorized call
web_fetch SHALL 不自动请求 HTTP 跳转目标，而返回 RedirectRequired、原始 URL 与解析后的目标 URL，要求新的工具调用经过正常授权；同主机也不例外。

#### Scenario: 同主机路径或端口变化
- **WHEN** 已授权 URL 返回指向不同路径或端口的 Location
- **THEN** 返回目标提示且不发送目标请求；目标被单独调用时经过普通验证与授权路径。

#### Scenario: 无效跳转目标
- **WHEN** Location 不可解析、包含凭据或使用不支持的 scheme
- **THEN** 返回跳转错误，不请求目标。

#### Scenario: 客户端展示
- **WHEN** 工具返回 RedirectRequired
- **THEN** 模型提示包含目标 URL 和新调用说明，ACP 将该次未取得内容的调用标记 Failed。

### Requirement: Question request cancellation preserves the worker
ask_user_question coordinator SHALL 在通知 Hook 阶段当前接收端关闭时结束该请求并继续服务后续问题，释放当前 pending guard；服务关闭与 Hook 失败仍遵从既有退出规则。

#### Scenario: 取消问题后再次提问
- **WHEN** 第一个问题在通知 Hook 等待前或等待中失去接收端，随后另一个有效问题到达
- **THEN** 后一个问题仍可经 ACP 返回响应，不因前一个请求取消而失去 coordinator。

### Requirement: Question option labels are unambiguous
ask_user_question SHALL 在发送请求前拒绝同一道题内完全相同的选项 label，并返回参数错误；不同题之间相同 label SHALL 允许。选项 ID 或描述差异 SHALL 不使重复 label 合法。

#### Scenario: 同题重复选项
- **WHEN** 同一问题的两个选项 label 相同
- **THEN** 工具返回参数错误，不向 coordinator 发送请求。

#### Scenario: 不同题共用选项文本
- **WHEN** 不同问题均有同名选项，但各题内部 label 唯一
- **THEN** 不因跨题重复拒绝请求。

### Requirement: Stable per-client permission cache keys
Remembered permission storage SHALL derive per-client filenames from the SHA-256 digest of the exact UTF-8 client identifier. An absent identifier SHALL use permission.toml; a missing per-client file SHALL retain the shared-file fallback. Legacy sanitized per-client names SHALL NOT be probed as a fallback.

#### Scenario: Previously colliding identifiers
- **WHEN** clients foo/bar, foo\\bar and foo_bar persist distinct grants
- **THEN** each client reloads its own grants without overwriting the others.

#### Scenario: Empty and long identifiers
- **WHEN** a client identifier is empty or contains a long Unicode string
- **THEN** it maps to a fixed-length filename distinct from the shared filename.

### Requirement: Permission read errors do not import shared grants
Permission state loading SHALL allow shared-file fallback only when the client-specific read reports NotFound. Other read errors, including invalid UTF-8, SHALL use default remembered state and SHALL NOT rewrite the failed source. This does not override independent configured policy or permission modes.

#### Scenario: Invalid UTF-8 client cache
- **WHEN** a shared cache contains grants and a client-specific cache contains invalid UTF-8
- **THEN** loading that client returns default remembered state and preserves the invalid source bytes.

#### Scenario: Client cache is a directory
- **WHEN** a client cache path is a directory and its read fails
- **THEN** loading returns default remembered state without importing shared grants or altering the directory.

### Requirement: Permission cache IO is bounded
Permission cache reads SHALL admit only ordinary files of at most 1 MiB, checking metadata on the opened handle and limiting actual bytes before UTF-8 and TOML parsing. Rejected reads SHALL preserve the source and use default remembered state without shared fallback. Unix opening SHALL not wait for a FIFO writer. Serialized writes over 1 MiB SHALL fail before atomic publication, preserving the previous destination.

#### Scenario: Exact and excessive bytes
- **WHEN** valid serialized state is exactly 1 MiB or a source exceeds that limit
- **THEN** the exact-boundary state loads and the excessive source is rejected; actual read consumes at most limit+1 bytes.

#### Scenario: Special source
- **WHEN** a Unix permission cache is a FIFO without a writer
- **THEN** reading rejects it without waiting for a writer and preserves the FIFO.

#### Scenario: Oversized write
- **WHEN** a serialized permission snapshot exceeds 1 MiB
- **THEN** writing returns an error and leaves the prior destination bytes unchanged.

### Requirement: Question output uses the active answer formatter
AskUserQuestion SHALL retain the active label-based answer formatter and SHALL NOT expose an internal unused alternate ID-keyed format selector. Question and option IDs, notes and validation SHALL remain available.

#### Scenario: Selected answer with notes
- **WHEN** a question response contains selected labels and notes
- **THEN** the active formatter preserves both without consulting a format selector.

### Requirement: Legacy unregistered Skill IO is removed
ToolInput and ToolOutput SHALL NOT expose the unregistered legacy Skill variants. Registered tools, dynamic ToolPack calls and skill prompt expansion SHALL remain available.

#### Scenario: Built-in skill capability
- **WHEN** the built-in registry is assembled
- **THEN** skills continue through discovery and prompt expansion without a legacy Skill tool IO variant.

### Requirement: Descriptor-owned child review is call-bound
Native tool descriptors SHALL be able to require the ordinary permission Gate for every child invocation independently of the child's initial RWX. A review-required descriptor that survives final tool and policy filtering SHALL be hard-eligible for an exact call and model-visible as approval-required, including when the implementation was injected after authored tool resolution; it SHALL NOT enter the initial-RWX fast path. Tools without this declaration SHALL retain authored-identity and RWX behavior. Hard-ineligible identities SHALL remain non-reviewable.

Evidence targets: `crates/common/tool-protocol/src/capabilities.rs` — `ToolCapabilities`; `crates/codegen/shell/src/session/subagent_capability.rs` — `SubagentCapabilityState`; `crates/codegen/shell/src/session/actor/tool/preparation.rs` — tool-call preflight.

#### Scenario: Whole-file write requires an exact-call decision
- **WHEN** a child with ReadWrite or All initial RWX invokes a finalized `write` tool whose descriptor requires child review
- **THEN** the capability catalog identifies it as approval-required and the frozen invocation enters the configured permission Gate instead of executing through the initial-RWX fast path.

#### Scenario: Ordinary matching edit keeps its fast path
- **WHEN** the same child invokes the authored `search_replace` tool within its initial ReadWrite authority and no independent rule requires confirmation
- **THEN** the call follows the ordinary in-fence permission path without spending a primary-context judgment.

#### Scenario: Removed or forbidden tool cannot request approval
- **WHEN** final filtering removes a tool or a visible native identity has neither authored eligibility nor a trusted review-required descriptor
- **THEN** the child cannot obtain a permit for it and no permission request is opened; a still-visible forbidden identity is described as non-approvable for this child and directs a genuine dependency to `ask_parent` for parent handling or task reassignment rather than implying that the reply grants authority.

### Requirement: Child review preserves the frozen call identity
A child permission request SHALL carry the actual model-facing tool identity, the canonical hash of the frozen arguments, and a bounded operation summary sufficient to distinguish whole-file replacement from matching edits. A whole-file content summary SHALL retain the target path, original byte length, content digest and an explicitly bounded preview. The primary-context judgment and permission audit SHALL use the actual tool identity rather than re-deriving a generic name from the coarse access kind. The permit SHALL bind the same exact tool identity and canonical arguments and SHALL be consumed at most once.

Evidence targets: `crates/codegen/workspace/src/permission/types.rs` — permission request context; `crates/codegen/workspace/src/permission/auto_mode.rs` — `PermissionJudgmentRequest`; `crates/codegen/shell/src/session/actor/tool/authorization.rs` and `dispatch.rs` — permit issue/validation.

#### Scenario: Main agent reviews a bounded write summary
- **WHEN** a child Auto request proposes `write` with content larger than the review preview budget
- **THEN** the primary-context request names `write`, identifies whole-file replacement, includes the target, total bytes, digest, canonical argument hash and marked truncated preview, without treating omitted content as reviewed plaintext.

#### Scenario: Approved call changes before dispatch
- **WHEN** the tool name, canonical arguments, cwd, child authorization epoch or applicable transport generation differs after approval
- **THEN** dispatch rejects the stale permit and does not execute the tool.

#### Scenario: Repeated write needs another permit
- **WHEN** an approved `write` call has consumed its permit and the child proposes another call, including the same target or arguments
- **THEN** the prior permit cannot authorize the new call and the configured Gate decides the new invocation independently.

### Requirement: Receipt-bound replies grant only communication authority

正式 agent 意见回复 SHALL 由 runtime-owned 收件证据授权。回复调用的权限投影 SHALL 不要求或授予文件/进程 RWX；初始 parent-to-child 消息仍保留既有 Write 投影。工具 eligibility、冻结参数和目标关系校验 SHALL 继续生效。

#### Scenario: Read-only child replies to received guidance
- **WHEN** read-only child 对实际收到的消息调用 send_subagent_message(reply_to)
- **THEN** 它可沿原参与方关系提交非 interrupt 意见，其文件/进程权限和父任务权限均不改变。

#### Scenario: Reply arguments cannot bypass routing
- **WHEN** 调用方同时提供 target 和 reply_to、请求 reply interrupt，或引用没有收件证据的消息
- **THEN** runtime 拒绝该精确调用，即使回复的 RWX 投影为空，也不能向第三方写入消息。

### Requirement: Search-replace creation requires confirmed absence

When `search_replace` receives an empty old string, it SHALL treat a failed read as evidence of absence only if the filesystem explicitly reports NotFound. Other read failures SHALL stop before any write or file-written notification. A confirmed missing target SHALL be created only through a filesystem operation that atomically refuses to replace an existing target; if that operation is unsupported or another writer creates the target first, the tool SHALL report failure without a file-written notification. Existing empty-file updates remain allowed; this requirement does not claim cross-writer conflict protection for those updates or ordinary replacement.

#### Scenario: Existing file cannot be read
- **WHEN** the target read fails with PermissionDenied or an error without a preserved NotFound kind
- **THEN** the tool returns a failure that identifies the read error, without attempting a write or publishing a file-written notification.

#### Scenario: Target is absent
- **WHEN** the target read explicitly reports NotFound and the filesystem supports exclusive creation
- **THEN** the full new content is committed to the target without exposing a partial result, and a file-written notification is published after success.

#### Scenario: Another writer creates the target after the read
- **WHEN** the target read explicitly reports NotFound but a competing process creates the final path before the exclusive commit
- **THEN** `search_replace` reports a conflict without replacing the competitor's bytes or publishing a file-written notification.

#### Scenario: Filesystem cannot create exclusively
- **WHEN** the target read explicitly reports NotFound but its filesystem lacks a no-replace creation operation
- **THEN** `search_replace` reports that limitation before any ordinary write or file-written notification.

#### Scenario: Parent component is a file
- **WHEN** the target read reports that a path component is not a directory
- **THEN** the tool reports that read failure before attempting to create the target.

#### Scenario: Existing file is empty
- **WHEN** the target read succeeds with an empty byte sequence
- **THEN** the ordinary empty-file update path remains available.

### Requirement: Local search-replace commits compare the source bytes

For an existing target, `search_replace` SHALL commit an edit only when its filesystem adapter can conditionally write the exact bytes read for that edit. The local adapter SHALL serialize cooperating writers across processes on the opened file with a bounded lock acquisition, and reject a mismatch or replaced target before reporting success. An adapter without this operation SHALL fail the edit. A failed conditional commit SHALL not emit a file-written notification. This contract does not assert atomicity against an external writer that ignores advisory locks.

#### Scenario: Two Grow edits read the same version
- **WHEN** two Grow writers read the same existing file and propose different replacements
- **THEN** at most one commits against that version; the other reports a conflict without publishing a file-written notification.

#### Scenario: Existing empty file changes before commit
- **WHEN** an empty target is read for `old_string = ""` and another writer changes it before the conditional commit
- **THEN** the creation-style edit reports a conflict and preserves the newer bytes.

#### Scenario: Path no longer names the opened source
- **WHEN** a target path is replaced after the local adapter opens its source file
- **THEN** the conditional edit refuses to report a successful write to the replacement path.

#### Scenario: Adapter lacks conditional write
- **WHEN** an existing-file edit is routed through a filesystem adapter without conditional write support
- **THEN** the edit fails without an unconditional fallback or file-written notification.

#### Scenario: Target lock remains held
- **WHEN** a competing process holds the target lock past the local acquisition deadline
- **THEN** the edit fails without writing or publishing a file-written notification.

### Requirement: Permission classifier details are bounded and complete

Permission classification SHALL use the full request detail only while evaluating that request. A classifier SHALL NOT issue an inference or produce an allowing verdict from a truncated request detail. Classifier detail SHALL be bounded to 1,024 bytes for MCP calls and 2,048 bytes for other access kinds. Classifier explanation text SHALL be capped at 240 bytes. If the complete detail exceeds its budget, classification SHALL return Unavailable before heuristic or model classification; request-local Auto authorization SHALL follow its existing unavailable fail-closed behavior. A completed permission audit event SHALL NOT retain raw access detail or model-generated classifier prose.

#### Scenario: Request detail exceeds classifier budget
- **WHEN** an Auto permission request contains access detail larger than the classifier's bounded input allowance
- **THEN** the classifier returns Unavailable without sending a partial detail for inference, and the request is not automatically authorized from incomplete evidence.

#### Scenario: Permission decision completes
- **WHEN** a permission request reaches an allow, deny, timeout, cancellation, or prompt outcome
- **THEN** the emitted audit event contains no raw access detail or untrusted classifier explanation, while the active request retains the exact input needed to make that decision until it resolves.

### Requirement: Remembered permission scopes are bound to the current access

A remembered MCP or Bash permission scope SHALL be derived from, or validated against, the actual `AccessKind` for the permission request. MCP tool scope SHALL name that exact tool; MCP server scope SHALL name the server parsed from that tool's qualified identity. Bash selected command terms SHALL be a non-empty prefix of the request's primary command terms. Invalid, mismatched, or access-inappropriate selection metadata SHALL NOT create a remembered scope from the supplied metadata and SHALL fall back to the request-derived scope where one exists.

#### Scenario: MCP response names another tool
- **WHEN** an allow-always MCP response contains a well-formed tool scope naming a different tool
- **THEN** the remembered outcome is limited to the current request's MCP tool.

#### Scenario: MCP response names another server
- **WHEN** an allow-always MCP response contains a well-formed server scope that differs from the server parsed from the current tool identity
- **THEN** the remembered outcome is limited to the current request's MCP tool.

#### Scenario: Bash response selects unrelated command terms
- **WHEN** an always-allow or always-reject Bash response contains well-formed command terms that are not a non-empty prefix of the current primary command terms
- **THEN** the outcome uses the scope derived from the current command script.

#### Scenario: Request option metadata disagrees with MCP tool identity
- **WHEN** Pager receives a permission request whose MCP scope option metadata names another tool or an inconsistent server prefix
- **THEN** Pager does not expose a server scope toggle from that metadata.
