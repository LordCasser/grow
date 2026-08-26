//! Public wire types (DTOs) for the ACP session actor.
//!
//! These are the request/response structs exchanged between the agent layer
//! and the session actor. They were extracted from the actor module to keep
//! that file focused on behaviour while giving downstream crates a lightweight
//! import path for data types.

use crate::util::config::DEFAULT_AUTO_COMPACT_THRESHOLD_PERCENT;

// ── Compaction ──────────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactConversationRequest {
    pub session_id: String,
    #[serde(default)]
    pub user_context: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactConversationStatus {
    Completed,
    Scheduled,
    AlreadyRunning,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CompactConversationResponse {
    pub status: CompactConversationStatus,
}

// ── Citations / comments ────────────────────────────────────────────────

/// A reference to a range of lines in a file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Citation {
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
}

/// Request to record an inline comment on a prompt turn.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentRequest {
    pub session_id: String,
    /// 0-indexed prompt turn this comment is associated with
    pub prompt_index: u32,
    pub comment: String,
    pub citation: Citation,
}

/// Response from recording a comment
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentResponse {
    pub comment_id: String,
    pub recorded: bool,
}

/// Request to delete a previously recorded comment.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentDeleteRequest {
    pub session_id: String,
    pub comment_id: String,
}

/// Response from deleting a comment
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentDeleteResponse {
    pub comment_id: String,
    pub deleted: bool,
}

// ── Rewind ──────────────────────────────────────────────────────────────

/// What to rewind: conversation, files, or both.
/// Clients must specify the mode explicitly — there is no default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RewindMode {
    /// Roll back both conversation and files (full time-travel).
    All,
    /// Roll back conversation only; leave files untouched.
    /// Use when the agent went in the wrong direction but the code is fine.
    ConversationOnly,
    /// Roll back files only; leave conversation untouched.
    /// Use when the files went wrong but the conversation context is valuable.
    FilesOnly,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RewindRequest {
    /// Target prompt index to rewind to (0-based).
    /// Semantics: "restore state before prompt N ran" — prompts 0..N-1 are kept.
    pub target_prompt_index: usize,
    /// Whether to force rewind even with conflicts
    pub force: bool,
    /// What to rewind. Clients must specify this explicitly.
    pub mode: RewindMode,
}

/// Response from a rewind operation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RewindResponse {
    /// Whether the rewind was successful
    pub success: bool,
    /// The prompt index we rewound to
    pub target_prompt_index: usize,
    /// Which mode was executed
    pub mode: RewindMode,
    /// List of file paths that were reverted (only populated on success with All or FilesOnly)
    pub reverted_files: Vec<String>,
    /// List of file paths that can be cleanly reverted (no conflicts)
    pub clean_files: Vec<String>,
    /// List of conflicts that were encountered (if force=false and conflicts exist, success=false)
    pub conflicts: Vec<RewindConflictInfo>,
    /// The original prompt text at target_prompt_index, for pre-filling the input field.
    /// Populated on successful conversation rewind (All or ConversationOnly).
    pub prompt_text: Option<String>,
    /// Optional error message
    pub error: Option<String>,
}

/// Info about a conflict during rewind
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RewindConflictInfo {
    pub path: String,
    pub conflict_type: String, // "missing_file", "extra_file", "content_mismatch"
}

/// Request to get available rewind points for the session
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RewindPointsRequest {}

/// Response with available rewind points
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RewindPointsResponse {
    pub rewind_points: Vec<RewindPointInfo>,
}

/// Info about a single rewind point
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RewindPointInfo {
    pub prompt_index: usize,
    pub created_at: String,
    pub num_file_snapshots: usize,
    /// Whether this prompt has file snapshots that can be reverted.
    /// When false, only conversation rewind is available for this checkpoint.
    pub has_file_changes: bool,
    /// Preview of the user prompt text (truncated)
    pub prompt_preview: Option<String>,
}

// ── Session info ────────────────────────────────────────────────────────

/// Itemized token usage for one context category, shown as an
/// informational row in `/context`, e.g. the skills listing or the
/// MCP server listing.
///
/// Token counts come from rendering the current state (the skill set, the
/// connected servers), never from parsing conversation text. Once
/// injected, these rows overlap [`ContextInfo::message_tokens`]; a fresh
/// session can show rows before the reminders are injected. Neither
/// estimate counts the `<system-reminder>` wrapper added on injection.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TokenUsageCategory {
    /// Display label, e.g. `"Skills"` or `"MCP servers"`.
    pub label: String,
    /// Estimated tokens this category costs in context.
    pub tokens: u64,
    /// Short supporting detail. By convention a count followed by a
    /// noun, e.g. `"21 skills"`; the pager right-aligns the leading count
    /// across rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl TokenUsageCategory {
    /// Row for the skills listing. `text` is the canonical render from
    /// `SkillManager::listing_snapshot`.
    pub fn skills_listing(text: &str, skill_count: usize) -> Self {
        Self {
            label: "Skills".to_string(),
            tokens: token_estimation::estimate_tokens(text),
            detail: Some(count_detail(skill_count as u64, "skill")),
        }
    }

    /// Row for the MCP server announcement. `text` is the full reminder
    /// body for the current server set.
    pub fn mcp_servers(text: &str, server_count: usize) -> Self {
        Self {
            label: "MCP servers".to_string(),
            tokens: token_estimation::estimate_tokens(text),
            detail: Some(count_detail(server_count as u64, "server")),
        }
    }
}

/// Formats a count with a naively pluralized noun: `"1 skill"`, `"21 skills"`.
pub fn count_detail(count: u64, noun: &str) -> String {
    let suffix = if count == 1 { "" } else { "s" };
    format!("{count} {noun}{suffix}")
}

/// Context usage breakdown for session info.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextInfo {
    pub used: u64,
    pub total: u64,
    pub system_prompt_tokens: u64,
    pub tool_definitions_count: u64,
    pub tool_definitions_tokens: u64,
    pub compaction_count: u64,
    pub turn_count: u64,
    pub tool_call_count: u64,
    /// Total conversation items (system + user + assistant + tool responses).
    pub message_count: u64,
    /// Bytes/4 estimate of all non-system conversation items.
    pub message_tokens: u64,
    pub free_tokens: u64,
    pub usage_pct: u8,
    /// The resolved auto-compact threshold percent (0-100) for the active model
    /// at the time this snapshot was captured. Comes from the 6-tier resolution
    /// (env > user per-model > user global > GB per-model > GB global > 85).
    /// Used by the TUI `/context` view so the displayed “Auto-compact at X%”
    /// always matches the actual trigger (e.g. 65 for grow-build in remote settings).
    pub auto_compact_threshold_percent: u8,
    /// Itemized usage rows (skills listing, MCP server listing). Empty on
    /// partial snapshots.
    pub usage_categories: Vec<TokenUsageCategory>,
}

impl ContextInfo {
    /// Partial snapshot from a notification carrying only used + total.
    /// Breakdown fields default to zero until the next full ContextInfo update.
    pub fn from_notification(used: u64, total: u64) -> Self {
        Self {
            used,
            total,
            usage_pct: token_estimation::usage_percentage_u8(used, total),
            free_tokens: token_estimation::free_tokens(total, used),
            auto_compact_threshold_percent: DEFAULT_AUTO_COMPACT_THRESHOLD_PERCENT,
            ..Self::default()
        }
    }
}

/// Unified session info data returned by GetSessionInfo.
/// One query, all the fields needed for /session-info and /context.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfoData {
    /// Agent definition name for this session (e.g. `grow-build`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_display_name: Option<String>,
    pub resolved_model_id: Option<String>,
    pub model_fingerprint: Option<String>,
    /// Catalog opt-in to display the served-checkpoint fingerprint for this model.
    #[serde(default)]
    pub show_model_fingerprint: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_backend: Option<String>,
    pub turns: u64,
    /// Current turn (0-based).
    /// Matches the `turn_number` used in TurnStarted events, traces, and rewinds.
    #[serde(default)]
    pub turn_index: u64,
    pub context: ContextInfo,
}

/// Whether this model slug supports showing checkpoint identity (resolved model ID, fingerprint).
pub fn is_coding_model_slug(model: &str) -> bool {
    matches!(model, "grow-build" | "grow-4.5")
}

/// Display gate for the model fingerprint: server/catalog opt-in OR the built-in coding-slug default.
pub fn should_show_model_fingerprint(catalog_flag: bool, model_slug: &str) -> bool {
    catalog_flag || is_coding_model_slug(model_slug)
}

/// Calculate and format the model name for display.
pub fn model_display_name(
    name: Option<&str>,
    model: &str,
    resolved: Option<&str>,
    show_resolved: bool,
) -> String {
    // If the catalogue entry has a name, that's the displayed model.
    if let Some(n) = name {
        return n.to_string();
    }

    // For displaying the resolved model slug from the API response.
    if show_resolved {
        return match resolved.filter(|r| *r != model) {
            Some(r) => format!("{model} ({r})"),
            None => model.to_string(),
        };
    }

    // There's no resolved model slug, we display the request model slug.
    model.to_string()
}

/// Full wire response for `grow/session/info`.
///
/// Wraps `SessionInfoData` with session-level fields (`session_id`, `cwd`)
/// that come from the agent layer rather than the session actor.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfoResponse {
    pub session_id: String,
    pub cwd: String,
    #[serde(flatten)]
    pub data: SessionInfoData,
}

// ── Startup hints ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupHints {
    #[serde(default)]
    pub non_interactive: bool,
    #[serde(default)]
    pub skip_git_status: bool,
    /// Leading conversation items to preserve verbatim across compaction (the
    /// immutable head): spawn-injected items for a fresh subagent, or just the
    /// System head for a `resume_from` subagent so the resumed body stays compactable.
    #[serde(default)]
    pub inherited_prefix_len: Option<usize>,
    /// Whether this session is a subagent child.
    #[serde(default)]
    pub is_subagent: bool,
    /// Parent session id when this session is a subagent child. Emitted as
    /// `parent_agent_id` on the turn span for trace attribution.
    #[serde(default)]
    pub parent_session_id: Option<String>,
    /// The task's `subagent_type` when this session is a subagent child, used for hook
    /// payload attribution so it matches the `SubagentStart`/`SubagentStop` events the
    /// parent emits (which also key off the task type, not the resolved agent name).
    #[serde(default)]
    pub subagent_type: Option<String>,
    /// Workflow Run whose immutable execution route owns this child.
    #[serde(skip)]
    pub workflow_run_id: Option<String>,
    /// Process-local copy of the same Run-owned route for actor execution.
    /// The durable source remains the root Workflow tracker; this avoids a
    /// child consulting its unrelated local Workflow manager.
    #[serde(skip)]
    pub(crate) workflow_runtime_route:
        Option<crate::session::workflow::tracker::WorkflowRuntimeRoute>,
    /// Whether this child owns an immutable delegated Goal view. Agent
    /// rebuilds must preserve its read-only lifecycle boundary.
    #[serde(skip)]
    pub(crate) delegated_goal: bool,
    /// Root Goal activity/accounting route inherited by descendant sessions.
    /// It is process-local runtime state and is never serialized.
    #[serde(skip)]
    pub(crate) goal_usage_window: Option<crate::session::actor::goal_support::GoalUsageWindow>,
    /// Internal-only permission route for child tool requests. The primary
    /// session also carries the configured value so it can host the child's
    /// auto-classifier side query before any child exists.
    #[serde(skip)]
    pub subagent_permission_mode: Option<workspace::permission::types::RequestPermissionMode>,
    /// Child assignment shown to the permission classifier and ask UI.
    #[serde(skip)]
    pub subagent_description: Option<String>,
    /// Set for a verbatim mirror-fork whose Timeline seed already contains the
    /// exact parent prefix. Runtime prefix materialization must preserve that
    /// System head and must not inject a fresh project-instructions item into
    /// the inherited cache prefix.
    #[serde(default)]
    pub preserve_inherited_system: bool,
}

impl StartupHints {
    /// Primary requests have no child route. Every child request has an
    /// independent explicit route; a missing internal hint fails into the
    /// canonical child default (`Auto`), never the primary session's mode.
    pub(crate) fn permission_request_mode(
        &self,
    ) -> Option<workspace::permission::types::RequestPermissionMode> {
        self.is_subagent
            .then(|| self.subagent_permission_mode.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_show_model_fingerprint_truth_table() {
        // Catalog opt-in shows the fingerprint even for a non-coding slug.
        assert!(should_show_model_fingerprint(true, "non-coding"));
        // Coding slugs always show, even without the catalog flag.
        assert!(should_show_model_fingerprint(false, "grow-build"));
        assert!(should_show_model_fingerprint(false, "grow-4.5"));
        // Non-coding slug without the flag stays hidden.
        assert!(!should_show_model_fingerprint(false, "some-other"));
    }

    use serde_json::json;

    #[test]
    fn startup_hints_never_inherit_primary_permission_mode_for_a_child() {
        use workspace::permission::types::RequestPermissionMode;
        let mut hints = StartupHints {
            subagent_permission_mode: Some(RequestPermissionMode::AlwaysApprove),
            ..Default::default()
        };
        assert_eq!(hints.permission_request_mode(), None);

        hints.is_subagent = true;
        hints.subagent_permission_mode = None;
        assert_eq!(
            hints.permission_request_mode(),
            Some(RequestPermissionMode::Auto)
        );
        hints.subagent_permission_mode = Some(RequestPermissionMode::Ask);
        assert_eq!(
            hints.permission_request_mode(),
            Some(RequestPermissionMode::Ask)
        );
    }

    // ── RewindMode serialization ──────────────────────────────────────

    #[test]
    fn rewind_mode_serializes_to_snake_case() {
        assert_eq!(serde_json::to_value(RewindMode::All).unwrap(), json!("all"));
        assert_eq!(
            serde_json::to_value(RewindMode::ConversationOnly).unwrap(),
            json!("conversation_only")
        );
        assert_eq!(
            serde_json::to_value(RewindMode::FilesOnly).unwrap(),
            json!("files_only")
        );
    }

    #[test]
    fn rewind_mode_deserializes_from_snake_case() {
        assert_eq!(
            serde_json::from_value::<RewindMode>(json!("all")).unwrap(),
            RewindMode::All
        );
        assert_eq!(
            serde_json::from_value::<RewindMode>(json!("conversation_only")).unwrap(),
            RewindMode::ConversationOnly
        );
        assert_eq!(
            serde_json::from_value::<RewindMode>(json!("files_only")).unwrap(),
            RewindMode::FilesOnly
        );
        assert!(serde_json::from_value::<RewindMode>(json!("code_only")).is_err());
    }

    #[test]
    fn rewind_mode_rejects_unknown_variant() {
        assert!(serde_json::from_value::<RewindMode>(json!("code_only_v2")).is_err());
    }

    #[test]
    fn rewind_request_requires_mode() {
        assert!(
            serde_json::from_value::<RewindRequest>(
                json!({"targetPromptIndex": 2, "force": false})
            )
            .is_err()
        );
    }

    #[test]
    fn rewind_request_explicit_mode_is_respected() {
        let req: RewindRequest = serde_json::from_value(
            json!({"targetPromptIndex": 5, "force": true, "mode": "files_only"}),
        )
        .unwrap();
        assert_eq!(req.mode, RewindMode::FilesOnly);
        assert!(req.force);
    }

    #[test]
    fn rewind_request_roundtrip() {
        let original = RewindRequest {
            target_prompt_index: 3,
            force: false,
            mode: RewindMode::ConversationOnly,
        };
        let json = serde_json::to_value(&original).unwrap();
        let decoded: RewindRequest = serde_json::from_value(json).unwrap();
        assert_eq!(decoded.target_prompt_index, 3);
        assert_eq!(decoded.mode, RewindMode::ConversationOnly);
    }

    // ── RewindResponse fields ─────────────────────────────────────────

    #[test]
    fn rewind_response_includes_mode_and_prompt_text() {
        let resp = RewindResponse {
            success: true,
            target_prompt_index: 1,
            mode: RewindMode::ConversationOnly,
            reverted_files: vec![],
            clean_files: vec![],
            conflicts: vec![],
            prompt_text: Some("fix the bug".into()),
            error: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["mode"], json!("conversation_only"));
        assert_eq!(v["promptText"], json!("fix the bug"));
        assert_eq!(v["success"], json!(true));
    }

    #[test]
    fn rewind_response_prompt_text_null_when_none() {
        let resp = RewindResponse {
            success: true,
            target_prompt_index: 0,
            mode: RewindMode::FilesOnly,
            reverted_files: vec!["src/main.rs".into()],
            clean_files: vec![],
            conflicts: vec![],
            prompt_text: None,
            error: None,
        };
        let v = serde_json::to_value(&resp).unwrap();
        assert!(v["promptText"].is_null());
        assert_eq!(v["revertedFiles"], json!(["src/main.rs"]));
    }

    #[test]
    fn rewind_response_deserializes_current_wire_shape() {
        let v = json!({
            "success": false,
            "targetPromptIndex": 4,
            "mode": "all",
            "revertedFiles": [],
            "conflicts": [{"path": "a.rs", "conflictType": "content_mismatch"}],
            "cleanFiles": [],
            "promptText": null,
            "error": "dirty working tree"
        });
        let resp: RewindResponse = serde_json::from_value(v).unwrap();
        assert!(!resp.success);
        assert_eq!(resp.mode, RewindMode::All);
        assert!(resp.prompt_text.is_none());
        assert!(resp.clean_files.is_empty());
        assert_eq!(resp.conflicts.len(), 1);
        assert_eq!(resp.conflicts[0].path, "a.rs");
    }

    // ── RewindPointInfo.has_file_changes ──────────────────────────────

    #[test]
    fn rewind_point_info_has_file_changes_true() {
        let point = RewindPointInfo {
            prompt_index: 2,
            created_at: "2025-01-01T00:00:00Z".into(),
            num_file_snapshots: 3,
            has_file_changes: true,
            prompt_preview: Some("refactor auth".into()),
        };
        let v = serde_json::to_value(&point).unwrap();
        assert_eq!(v["hasFileChanges"], json!(true));
        assert_eq!(v["numFileSnapshots"], json!(3));
    }

    #[test]
    fn rewind_point_info_has_file_changes_false_when_no_snapshots() {
        let point = RewindPointInfo {
            prompt_index: 0,
            created_at: "2025-01-01T00:00:00Z".into(),
            num_file_snapshots: 0,
            has_file_changes: false,
            prompt_preview: None,
        };
        let v = serde_json::to_value(&point).unwrap();
        assert_eq!(v["hasFileChanges"], json!(false));
        assert_eq!(v["numFileSnapshots"], json!(0));
    }

    #[test]
    fn rewind_point_info_requires_current_shape() {
        let v = json!({
            "promptIndex": 1,
            "createdAt": "2025-01-01T00:00:00Z",
            "numFileSnapshots": 5
        });
        assert!(serde_json::from_value::<RewindPointInfo>(v).is_err());
    }

    #[test]
    fn context_info_from_notification_computes_derived_fields() {
        let c = ContextInfo::from_notification(50_000, 200_000);
        assert_eq!(c.used, 50_000);
        assert_eq!(c.total, 200_000);
        assert_eq!(c.usage_pct, 25);
        assert_eq!(c.free_tokens, 150_000);
        assert_eq!(c.system_prompt_tokens, 0);
        assert_eq!(c.message_count, 0);
        assert_eq!(c.compaction_count, 0);
    }

    #[test]
    fn context_info_from_notification_zero_total() {
        let c = ContextInfo::from_notification(100, 0);
        assert_eq!(c.usage_pct, 0);
        assert_eq!(c.free_tokens, 0);
    }

    #[test]
    fn context_info_requires_exact_current_shape() {
        assert!(serde_json::from_str::<ContextInfo>(r#"{"used":1,"total":2}"#).is_err());
        let json = serde_json::to_string(&ContextInfo::default()).unwrap();
        assert!(json.contains("usageCategories"), "{json}");
        let roundtripped: ContextInfo = serde_json::from_str(&json).unwrap();
        assert!(roundtripped.usage_categories.is_empty());

        let row: TokenUsageCategory =
            serde_json::from_str(r#"{"label":"AGENTS.md","tokens":42,"detail":null}"#).unwrap();
        assert_eq!(row.label, "AGENTS.md");
        assert!(
            serde_json::from_str::<TokenUsageCategory>(
                r#"{"kind":"agents_md","label":"AGENTS.md","tokens":42}"#
            )
            .is_err()
        );

        // Rows round-trip.
        let original = TokenUsageCategory::skills_listing("t", 2);
        let json = serde_json::to_string(&original).unwrap();
        let roundtripped: TokenUsageCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtripped, original);
    }
}
