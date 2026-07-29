//! Cross-ecosystem error type for tool execution.
//!
//! `ToolError` is a struct with a `kind` discriminator and a tool-provided
//! `detail` string. The `detail` is the model-facing message — tools MUST
//! provide a human-readable explanation of what went wrong, since this text
//! is sent back to the model to inform its next action.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use xai_tool_protocol::ToolId;

/// Discriminator for tool errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolErrorKind {
    /// The tool has no implementation for the requested operation.
    NotImplemented,
    /// Inputs failed validation.
    InvalidArguments,
    /// No tool registered under the given id.
    NotFound,
    /// Caller lacks required permissions (403-shaped).
    PermissionDenied,
    /// Authentication failed (401-shaped).
    Unauthorized,
    /// The tool ran past its time budget.
    Timeout,
    /// The caller cancelled the tool call.
    Cancelled,
    /// Rate limit exceeded.
    RateLimited,
    /// Upstream service unavailable.
    ServiceUnavailable,
    /// Network-level failure.
    NetworkError,
    /// Tool body returned an error.
    Execution,
    /// Requested behavior version not supported.
    BehaviorVersionUnsupported,
    /// Render-card budget exceeded.
    RenderLimited,
    /// Terminal subprocess failure.
    TerminalError,
    /// Forward-compat catch-all.
    Custom,
}

impl ToolErrorKind {
    /// Snake-case identifier for metrics / logs.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotImplemented => "not_implemented",
            Self::InvalidArguments => "invalid_arguments",
            Self::NotFound => "not_found",
            Self::PermissionDenied => "permission_denied",
            Self::Unauthorized => "unauthorized",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::RateLimited => "rate_limited",
            Self::ServiceUnavailable => "service_unavailable",
            Self::NetworkError => "network_error",
            Self::Execution => "execution",
            Self::BehaviorVersionUnsupported => "behavior_version_unsupported",
            Self::RenderLimited => "render_limited",
            Self::TerminalError => "terminal_error",
            Self::Custom => "custom",
        }
    }
}

/// Cross-ecosystem error type for tool execution.
///
/// Every error carries:
/// - `kind` — the machine-readable discriminator
/// - `detail` — the model/user-facing message that tools MUST provide
/// - `source` — optional causal chain for debugging (not sent to the model)
/// - `details` — optional structured metadata (JSON Schema validation
///   report, retry_after hints, etc.)
#[derive(Serialize, Deserialize)]
pub struct ToolError {
    pub kind: ToolErrorKind,
    /// Human-readable message provided by the tool. This is sent back to
    /// the model so it can understand what went wrong and adjust its next
    /// action. Tools MUST make this specific and actionable.
    pub detail: String,
    /// Optional causal chain for developer debugging. NOT sent to the model.
    #[serde(skip)]
    source: Option<anyhow::Error>,
    /// Optional structured metadata (e.g. per-field validation errors,
    /// `retry_after` hints, `tool_id`, `card_id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl fmt::Debug for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("ToolError");
        d.field("kind", &self.kind);
        d.field("detail", &self.detail);
        if let Some(ref source) = self.source {
            d.field("source", &format!("{source:#}"));
        }
        if let Some(ref details) = self.details {
            d.field("details", details);
        }
        d.finish()
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.detail)
    }
}

impl std::error::Error for ToolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(|e| e.as_ref() as &_)
    }
}

// ---------------------------------------------------------------------------
// Constructors — one per kind for ergonomic tool code
// ---------------------------------------------------------------------------

impl ToolError {
    /// Core constructor. All other constructors delegate here.
    pub fn new(kind: ToolErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
            source: None,
            details: None,
        }
    }

    /// Attach structured metadata.
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    /// Attach a causal error chain (for developer logs, not sent to model).
    pub fn with_source(mut self, source: impl Into<anyhow::Error>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn not_implemented(detail: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::NotImplemented, detail)
    }

    pub fn invalid_arguments(detail: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::InvalidArguments, detail)
    }

    pub fn not_found(tool_id: ToolId, detail: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::NotFound, detail)
            .with_details(serde_json::json!({ "tool_id": tool_id.as_str() }))
    }

    pub fn permission_denied(detail: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::PermissionDenied, detail)
    }

    pub fn unauthorized(detail: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::Unauthorized, detail)
    }

    pub fn timeout(tool_id: ToolId, detail: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::Timeout, detail)
            .with_details(serde_json::json!({ "tool_id": tool_id.as_str() }))
    }

    pub fn cancelled(tool_id: ToolId, detail: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::Cancelled, detail)
            .with_details(serde_json::json!({ "tool_id": tool_id.as_str() }))
    }

    pub fn rate_limited(detail: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::RateLimited, detail)
    }

    pub fn service_unavailable(detail: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::ServiceUnavailable, detail)
    }

    pub fn network_error(detail: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::NetworkError, detail)
    }

    pub fn execution(tool_id: ToolId, detail: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::Execution, detail)
            .with_details(serde_json::json!({ "tool_id": tool_id.as_str() }))
    }

    pub fn terminal_error(tool_id: ToolId, detail: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::TerminalError, detail)
            .with_details(serde_json::json!({ "tool_id": tool_id.as_str() }))
    }

    pub fn custom(code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(ToolErrorKind::Custom, detail)
            .with_details(serde_json::json!({ "code": code.into() }))
    }

    /// Snake-case identifier for the kind. Delegates to
    /// [`ToolErrorKind::as_str`].
    pub fn variant_name(&self) -> &'static str {
        self.kind.as_str()
    }
}

// ---------------------------------------------------------------------------
// From impls
// ---------------------------------------------------------------------------

impl From<serde_json::Error> for ToolError {
    fn from(value: serde_json::Error) -> Self {
        Self::invalid_arguments(value.to_string())
    }
}
