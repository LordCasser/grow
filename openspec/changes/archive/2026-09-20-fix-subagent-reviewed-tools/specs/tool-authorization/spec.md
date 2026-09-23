## ADDED Requirements

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
