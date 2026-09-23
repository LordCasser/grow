//! Capabilities used by the in-process tool runtime.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Per-tool capabilities. Defaults conservatively (no
/// progress, no cancel, single concurrency, no hooks).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCapabilities {
    /// Streaming declaration. `None` — the default for every tool today —
    /// means the tool never emits partial-result progress.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub streaming: Option<StreamingSpec>,

    /// Tool honours `hook { Cancel }`.
    #[serde(default)]
    pub supports_cancel: bool,

    /// Maximum concurrent invocations the tool will accept. `None` is
    /// unlimited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrency: Option<u32>,

    /// Lifecycle hooks the tool opts in to receive.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hooks: Vec<HookKind>,

    /// Per-tool override for the progress-frame size cap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_frame_bytes: Option<u32>,

    /// Per-call timeout override (defaults to 60_000ms when omitted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,

    /// Maximum native authority that any invocation of this tool may require.
    ///
    /// This is the descriptor-owned eligibility ceiling, not the permission
    /// requirement of every call. Call normalization projects frozen arguments
    /// to the exact requirement (for example Workflow inspect is `Read`) and
    /// must prove that requirement is covered by this ceiling. Unknown tools
    /// default to [`ToolAccess::All`] and fail closed until a trusted projector
    /// and actor eligibility are both present.
    #[serde(default)]
    pub max_access: ToolAccess,

    /// Whether a child agent must send every invocation through its configured
    /// permission gate even when the child's initial RWX covers the call.
    ///
    /// `Required` is also an explicit, descriptor-owned admission for a
    /// runtime-injected native tool to enter child hard eligibility. It never
    /// grants the call: final tool/policy filtering, the permission decision,
    /// and the call-bound permit remain authoritative.
    #[serde(default)]
    pub subagent_review: SubagentReviewPolicy,
}

/// Child-specific review policy declared by a concrete native tool.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentReviewPolicy {
    /// Use authored exact identity and the child's initial RWX normally.
    #[default]
    Inherit,
    /// Admit the finalized identity only through an exact-call permission gate.
    Required,
}

/// How a tool streams partial results. Declared once in
/// [`ToolCapabilities::streaming`] and consumed at the source to stamp a
/// self-describing progress envelope; downstream layers dispatch on that
/// envelope rather than the tool's identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamingSpec {
    /// Stable snake_case discriminator the tool stamps on its
    /// `ToolProgress::Custom.subkind` (e.g. `"bash_output_chunk"`).
    pub subkind: String,

    /// Per-frame `delta` byte cap (UTF-8-safe). Unset falls back to the
    /// runtime's 16 KiB default. Independent of
    /// [`ToolCapabilities::max_frame_bytes`], which caps whole frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_delta_bytes: Option<u32>,
}

/// Lifecycle hook a tool may opt in to receive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookKind {
    OnSessionOpen,
    OnSessionClose,
    OnToolCallStart,
    OnToolCallResult,
    OnCancel,
    OnNotification,
}

/// Coarse read/write/execute authority required by a tool call.
///
/// `Read` observes user reality, `Write` changes durable state or emits into
/// it, and `Execute` starts or controls an execution unit in that reality.
/// `None` is reserved for framework control whose exact tool identity remains
/// authoritative and whose downstream work is independently gated (for
/// example, spawning a child actor or cancelling a session-owned resource).
/// Combined tools declare the union. This vocabulary is intentionally closed
/// and replaces the old read/write-only `ToolScope` classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolAccess {
    /// Framework control-plane operation outside user-reality RWX.
    None,
    Read,
    Write,
    Execute,
    ReadWrite,
    ReadExecute,
    WriteExecute,
    All,
}

impl Default for ToolAccess {
    /// Unknown tools fail closed until their descriptor declares an exact
    /// requirement.
    fn default() -> Self {
        Self::All
    }
}

impl ToolAccess {
    const READ: u8 = 0b001;
    const WRITE: u8 = 0b010;
    const EXECUTE: u8 = 0b100;

    const fn bits(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Read => Self::READ,
            Self::Write => Self::WRITE,
            Self::Execute => Self::EXECUTE,
            Self::ReadWrite => Self::READ | Self::WRITE,
            Self::ReadExecute => Self::READ | Self::EXECUTE,
            Self::WriteExecute => Self::WRITE | Self::EXECUTE,
            Self::All => Self::READ | Self::WRITE | Self::EXECUTE,
        }
    }

    const fn from_bits(bits: u8) -> Self {
        match bits & (Self::READ | Self::WRITE | Self::EXECUTE) {
            0 => Self::None,
            Self::READ => Self::Read,
            Self::WRITE => Self::Write,
            Self::EXECUTE => Self::Execute,
            bits if bits == Self::READ | Self::WRITE => Self::ReadWrite,
            bits if bits == Self::READ | Self::EXECUTE => Self::ReadExecute,
            bits if bits == Self::WRITE | Self::EXECUTE => Self::WriteExecute,
            _ => Self::All,
        }
    }

    pub const fn union(self, other: Self) -> Self {
        Self::from_bits(self.bits() | other.bits())
    }

    pub const fn covers(self, required: Self) -> bool {
        self.bits() & required.bits() == required.bits()
    }

    pub const fn requires_write(self) -> bool {
        self.bits() & Self::WRITE != 0
    }

    pub const fn requires_read(self) -> bool {
        self.bits() & Self::READ != 0
    }

    pub const fn requires_execute(self) -> bool {
        self.bits() & Self::EXECUTE != 0
    }

    pub const fn is_observation_only(self) -> bool {
        matches!(self, Self::None | Self::Read)
    }
}

#[cfg(test)]
mod tests {
    use super::{SubagentReviewPolicy, ToolAccess, ToolCapabilities};

    #[test]
    fn access_union_and_coverage_form_the_rwx_lattice() {
        assert_eq!(
            ToolAccess::Write.union(ToolAccess::Execute),
            ToolAccess::WriteExecute
        );
        assert!(ToolAccess::All.covers(ToolAccess::WriteExecute));
        assert!(ToolAccess::ReadWrite.covers(ToolAccess::Write));
        assert!(!ToolAccess::ReadWrite.covers(ToolAccess::Execute));
        assert!(!ToolAccess::Execute.covers(ToolAccess::Write));
    }

    #[test]
    fn unknown_capabilities_fail_closed() {
        assert_eq!(ToolAccess::default(), ToolAccess::All);
    }

    #[test]
    fn subagent_review_defaults_to_inherited_behavior() {
        let default = ToolCapabilities::default();
        assert_eq!(default.subagent_review, SubagentReviewPolicy::Inherit);

        let decoded: ToolCapabilities = serde_json::from_value(serde_json::json!({
            "max_access": "write",
            "subagent_review": "required"
        }))
        .unwrap();
        assert_eq!(decoded.subagent_review, SubagentReviewPolicy::Required);
    }
}
