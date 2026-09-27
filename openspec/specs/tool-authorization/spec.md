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

### Requirement: Auto model permission judgments obey one end-to-end deadline

主会话和子 Agent 的 Auto 模型权限裁决 SHALL 从请求提交给主会话分类通道开始，共享一个有界总期限；该期限 SHALL 覆盖派发、准备、模型尝试与结果结算。首次尝试 SHALL 可使用全部剩余期限，不得仅为预留重试时间而提前取消仍在运行的请求。实际返回无效结构或可恢复 provider 错误后 SHALL 最多重试一次且不得延长总期限。超过期限或请求方已离开后 SHALL 不采纳允许结果。

#### Scenario: 较慢的首次结果仍在总期限内
- **WHEN** 首次模型尝试超过旧的平均半额但在当前总期限内返回有效裁决
- **THEN** 裁决被使用，不因预留第二次尝试预算而提前取消。

#### Scenario: 排队或准备耗尽期限
- **WHEN** 分类请求在派发或 Sideband 准备期间耗尽总期限
- **THEN** 该精确调用不获得允许；迟到响应不能改变结果或写入许可。

#### Scenario: 子 Agent 与主会话超时后续
- **WHEN** 子 Agent 的 Auto 模型裁决超时
- **THEN** 当前工具调用失败且不打开人工提示，子 Agent 可继续其他调用；主会话自己的 Auto 分类超时仍进入既有的人工提示后续路径。

### Requirement: Permission waits are request-local

主会话和子 Agent 的权限请求 SHALL 独立等待模型或人工回应；等待中的请求 SHALL NOT 阻塞其他请求的本地策略检查、独立模型裁决或权限控制命令。同一权限域的状态提交 SHALL 有序合并到当前状态，不得以旧快照覆盖其他请求的授权、拒绝计数或历史。根权限文件写入 SHALL 串行保存最新状态，子权限域 SHALL 不落入根权限文件。

#### Scenario: One child judgment is slow
- **WHEN** 子 Agent A 的模型裁决已发出但未结束，而 B 或主会话提交独立权限请求
- **THEN** 后者可以完成本地决策或发出自己的 Sideband，无需等待 A 的模型终态。

#### Scenario: Human permission is pending
- **WHEN** 一个请求等待人工回应，另一个请求可通过本地规则或 Auto 判断处理
- **THEN** 后者继续处理，不受前者的人工交互期限约束。

#### Scenario: Concurrent decisions update one scope
- **WHEN** 同一权限域内多个独立请求完成并提交计数、历史或 remembered grant
- **THEN** 所有当前有效更新按完成顺序合并，主/子和不同子域之间不共享授权。

### Requirement: Permission controls revoke stale pending requests

ResetState SHALL 撤销全部旧权限请求并清空状态；ReleaseChild SHALL 撤销对应 child 请求并释放域状态；主权限模式改变 SHALL 撤销旧 primary 请求而不改变 child mode。控制调用完成 SHALL 表示 actor 已提交对应状态转换与撤销；在此后完成的旧裁决 SHALL NOT 授权调用或写入 remembered grant。新增 remembered deny SHALL 撤销同域旧判断。调用方离开后迟到的模型/人工允许 SHALL NOT 授权调用。Shutdown SHALL 取消并等待全部已接纳请求及状态写入，再关闭权限 audit 流。

#### Scenario: Control overlaps an old classifier completion
- **WHEN** classifier 在同一轮调度中触发 ResetState、ReleaseChild 或主 mode change，并准备返回允许
- **THEN** 控制完成前 actor 已撤销对应旧请求，该允许不能成为请求结果。

#### Scenario: Reset or child release during judgment
- **WHEN** 请求等待模型或人工回应时发生对应 ResetState 或 ReleaseChild
- **THEN** 当前调用结束为取消，迟到允许不能重新创建授权状态。

#### Scenario: Primary mode changes while a child waits
- **WHEN** 一个 child 正在等裁决且主 mode 被更新
- **THEN** 更新完成后对新的 primary 请求生效，旧 primary 请求被撤销，child 沿冻结的独立模式继续。

#### Scenario: Child release is isolated
- **WHEN** 一个 child 被释放而另一个 child 或主会话正在裁决
- **THEN** 仅被释放 child 的旧请求被撤销，其他权限域继续运行。

#### Scenario: Control completion precedes new request
- **WHEN** 模式切换或状态重置完成后提交新请求
- **THEN** 新请求使用已提交的模式或清空后的状态。

#### Scenario: Shutdown with multiple pending decisions
- **WHEN** 多个裁决或提示在途时关闭权限系统
- **THEN** 所有已接纳请求被取消，最终审计事件与写入处理结束后才确认关闭。

### Requirement: Auto classifier reasons remain untrusted evidence

Auto classifier 的自由文本 `reason` SHALL 不作为调用 Agent 的权限结果或权限 audit 的决策理由；拒绝结果 SHALL 使用 harness 自有的固定说明，并保留分类来源、verdict 与标准触发原因用于诊断。模型 reason 的内容不得改变拒绝、提示升级或后续工具执行规则。

#### Scenario: Model denial includes instructions
- **WHEN** Auto 模型返回 Block 且 reason 包含要求 Agent 绕过权限的指令
- **THEN** 调用 Agent 只收到 harness 自有拒绝说明，权限 audit 的理由为标准触发原因，指令不进入这两个输出。

#### Scenario: Denial escalation
- **WHEN** 连续 Block 达到既有人工提示阈值
- **THEN** 是否升级只取决于计数与策略，不取决于模型 reason 的措辞。

### Requirement: Permission request source is typed at the production boundary

生产工具授权入口 SHALL 显式携带 `PermissionRequestSource::Primary` 或 `PermissionRequestSource::Child`。子会话身份 SHALL 不由可选的 subagent 展示类型推断；缺少展示类型不得将子请求变为主会话请求。权限 manager SHALL 依据显式来源选择独立权限域和撤销边界。

#### Scenario: Child has no display type
- **WHEN** 子 Agent 的权限请求没有 `subagent_type` 展示字段，但具有显式 Child 来源和 session ID
- **THEN** 请求仍使用 child 权限域，不继承主会话的 remembered grant，ReleaseChild 可撤销它。

#### Scenario: Production caller lacks a source
- **WHEN** 生产调用方构造权限请求
- **THEN** 类型接口要求它提供 Primary 或 Child 来源，不能通过可选元数据静默推断。

### Requirement: Permission fanout has finite fail-fast admission

每个主会话共享的 child 权限请求、主 Agent 权限请求与权限 Sideband 活跃数 SHALL 有固定有限上限；达到对应上限 SHALL 立即返回不授权的超载结果，而不是把新的模型/人工等待放入无界队列。child 请求饱和 SHALL 仍为主 Agent 保留有限准入容量。已准入的模型裁决 SHALL 继续独立并行；调用方取消后，actor 丢弃该请求或完成裁决时 SHALL 释放容量。控制撤销和关闭命令 SHALL 在请求饱和时仍可处理。超载 SHALL 以不含请求参数的固定标签指标和日志可观测。

#### Scenario: Many child requests are pending
- **WHEN** 共享权限 manager 的活跃请求达到上限，另一个 child 提交工具调用
- **THEN** 新调用及时收到不授权结果；旧调用仍能独立完成或被取消，控制命令仍可执行。

#### Scenario: The primary continues while children saturate admission
- **WHEN** child 权限请求占满准入容量，主 Agent 提交本地权限决策
- **THEN** 主 Agent 的决策仍可处理，不因 child 容量满而被拒绝。

#### Scenario: The primary reserve is exhausted
- **WHEN** child 请求已占满其阈值，主 Agent 也占满保留的有限容量
- **THEN** 下一个主 Agent 权限请求及时得到不授权结果，已准入请求与控制命令仍可处理。

#### Scenario: Sideband capacity is full
- **WHEN** 权限 Sideband 已达到并发上限，另一个 Auto 裁决到达
- **THEN** 新裁决得到 typed overload，不等待已有 provider；已准入 Sideband 可各自完成，主会话沿现有不可用后续路径处理，child 不打开人工提示。

#### Scenario: Cancellation releases capacity
- **WHEN** 一个在途权限请求的调用方取消并被 actor 丢弃，或已准入 Sideband 结束
- **THEN** 对应资源槽释放，后续请求可准入。

### Requirement: Reset reports durable permission-state outcome

ResetState SHALL 在 actor 中先撤销旧请求并清空内存权限域，随后通过独立串行写入保存最新根权限状态；调用成功完成 SHALL 表示权限文件写入已确认。写入失败 SHALL 作为明确错误返回，不得报告成功；当前进程的撤销仍保持生效。其他权限请求 SHALL 不因 Reset 的文件等待而串行等待。

#### Scenario: Reset succeeds
- **WHEN** 旧 remembered grant 存在且 Reset 的权限文件写入成功
- **THEN** Reset 成功返回，重载权限文件不包含旧 grant。

#### Scenario: Reset persistence fails
- **WHEN** Reset 已撤销在途旧请求，但权限文件无法写入
- **THEN** Reset 返回错误，当前进程不恢复旧 grant，也不把旧请求的迟到允许采纳为新授权。
