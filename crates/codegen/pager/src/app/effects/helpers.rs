#![cfg_attr(rustfmt, rustfmt::skip)]
use std::path::Path;
use agent_client_protocol as acp;
use tokio::task::JoinSet;
use acp_transport::{AcpAgentTx, acp_send};
use super::actions::{PermissionModePersist, SubagentKillOutcome, TaskResult};
use super::agent::AgentId;
use crate::unified_log as ulog;
use shell::sampling::error::{
    RATE_LIMITED_ERROR_CODE, error_detail_from_data, format_rate_limited_user_message,
};
use shell::session::ExtMethodResult;
use shell::session::unified_list::ListScope;
pub(super) fn log_prompt_result(
    session_id: &acp::SessionId,
    result: &Result<acp::PromptResponse, acp::Error>,
) {
    let sid = &session_id.0;
    match result {
        Ok(_) => ulog::info("agent response complete", Some(sid), None),
        Err(e) => {
            ulog::error(
                "agent response failed",
                Some(sid),
                Some(serde_json::json!({"error": e.to_string()})),
            )
        }
    }
}
/// Delay between post-install MCP-list re-probes (`Effect::RetryPluginCtaMcps`).
pub(super) const CTA_MCP_RETRY_DELAY_MS: u64 = 1000;
/// How long the CTA shows its "installed" confirmation before auto-dismissing.
pub(super) const CTA_INSTALLED_DISMISS_MS: u64 = 4000;
/// Upper bound on the off-thread clipboard-attachment probe. A wedged osascript
/// read must not pin `paste_probe_in_flight` and silently stash every later send.
pub(super) const CLIPBOARD_PROBE_TIMEOUT_SECS: u64 = 10;
/// Picker search debounce ([`Effect::DebounceSessionSearch`]):
/// long enough to coalesce a typing burst, short enough to feel live.
pub(super) const SESSION_SEARCH_DEBOUNCE_MS: u64 = 250;
/// Run the post-CTA-install uncached `grow/mcp/list` read and map it into a
/// `TaskResult::PluginCtaMcpsLoaded`. Shared by the immediate fetch and the
/// delayed re-probe.
pub(super) async fn fetch_plugin_cta_mcps(
    agent_id: AgentId,
    session_id: acp::SessionId,
    plugin_name: String,
    tx: AcpAgentTx,
) -> TaskResult {
    let params = serde_json::json!({
        "sessionId": session_id.0.to_string(),
        "cache": false,
    });
    let req = acp::ExtRequest::new(
        "grow/mcp/list",
        serde_json::value::to_raw_value(&params)
            .expect("serialize mcp/list params")
            .into(),
    );
    let result = match acp_send(req, &tx).await {
        Ok(resp) => {
            let wrapper: serde_json::Value = serde_json::from_str(resp.0.get())
                .unwrap_or_default();
            let inner = wrapper.get("result").unwrap_or(&wrapper);
            serde_json::from_value::<
                crate::views::mcps_modal::McpsListResponse,
            >(inner.clone())
                .map(crate::views::mcps_modal::convert_list_response)
                .map_err(|_| "couldn't load server list".to_string())
        }
        Err(e) => Err(sanitize_user_error(&format!(
            "couldn't load server list: {e}"
        ))),
    };
    TaskResult::PluginCtaMcpsLoaded {
        agent_id,
        plugin_name,
        result,
    }
}
/// Convert an ACP error to a user-friendly string for display.
/// Rate-limit errors preserve provider detail and use a provider-neutral
/// fallback when the response has no detail.
/// All other errors are sanitized to remove internal service names and jargon.
pub(super) fn format_acp_error(err: &acp::Error) -> String {
    if i32::from(err.code) == RATE_LIMITED_ERROR_CODE {
        let detail = err.data.as_ref().and_then(error_detail_from_data);
        return sanitize_user_error(&format_rate_limited_user_message(detail.as_deref()));
    }
    if err.code == acp::ErrorCode::InvalidParams && let Some(data) = &err.data
        && let Some(msg) = error_detail_from_data(data) && !msg.is_empty()
    {
        return msg;
    }
    sanitize_user_error(&err.to_string())
}
/// CANONICAL wire parser for the worktree resume response. Any other code
/// consuming the `codeRestored` / `restoreSummary` / `restoreDegree` shape
/// MUST go through this function — do not re-implement.
pub(super) fn parse_worktree_restore_payload(
    result_obj: &serde_json::Value,
) -> (bool, Option<String>, Option<workspace::session::git::RestoreDegree>) {
    let code_restored = result_obj
        .get("codeRestored")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let restore_summary = result_obj
        .get("restoreSummary")
        .and_then(|v| v.as_str())
        .map(String::from);
    let restore_degree = result_obj
        .get("restoreDegree")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok());
    (code_restored, restore_summary, restore_degree)
}
/// CANONICAL wire parser for `LoadSessionResponse._meta.codeRestore`. Any
/// other code consuming this shape MUST go through this function — do not
/// re-implement.
pub(super) fn parse_session_load_restore_meta(
    resp_meta: Option<&acp::Meta>,
) -> (bool, Option<String>, Option<workspace::session::git::RestoreDegree>) {
    let code_restore = resp_meta.and_then(|m| m.get("codeRestore"));
    let code_restored = code_restore
        .and_then(|r| r.get("restored"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let restore_summary = code_restore
        .and_then(|r| r.get("summary"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let restore_degree = code_restore
        .and_then(|r| r.get("degree"))
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok());
    (code_restored, restore_summary, restore_degree)
}
/// Canonical parser for the structured regular foreground snapshot carried by
/// `session/load`. Missing or malformed snapshots are idle; prompt ids are
/// never inspected to recover origin or lifecycle kind.
pub(crate) fn parse_session_load_foreground(
    resp_meta: Option<&acp::Meta>,
) -> Option<crate::app::prompt_queue::ForegroundSnapshot> {
    resp_meta
        .and_then(|m| m.get("grow/foreground"))
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}
/// Whether `raw` is (or wraps) a disk-full / ENOSPC failure.
fn is_disk_full_error(raw: &str) -> bool {
    raw.contains(fast_worktree::OUT_OF_DISK_CONTEXT)
        || raw.contains(fast_worktree::ENOSPC_OS_MESSAGE)
        || raw.contains("Disk quota exceeded") || raw.contains("Out of disk space")
}
/// Sanitize an error string before showing it to the user.
///
/// Strips protocol jargon (ACP, JSON-RPC) and other technical noise that would
/// be meaningless in a toast, and collapses known disk-full markers.
pub(crate) fn sanitize_user_error(raw: &str) -> String {
    if is_disk_full_error(raw) {
        return fast_worktree::ENOSPC_OS_MESSAGE.to_string();
    }
    static REPLACEMENTS: &[(&str, &str)] = &[
        ("cli-chat-proxy", "server"),
        ("cli_chat_proxy", "server"),
        ("inference-api", "server"),
        ("inference_api", "server"),
        ("research-api", "server"),
        ("research_api", "server"),
        ("grow-code-backend", "server"),
        ("ACP error:", "error:"),
        ("ACP request failed:", "request failed:"),
        ("JSON-RPC error", "request error"),
        ("acp_send", "request"),
        ("ExtRequest", "request"),
        ("ExtNotification", "notification"),
        ("Authentication required: ", ""),
        ("Authentication failed: ", ""),
    ];
    let mut result = raw.to_string();
    for (pattern, replacement) in REPLACEMENTS {
        result = result.replace(pattern, replacement);
    }
    if result.chars().count() > 200 {
        let truncated: String = result.chars().take(180).collect();
        result = format!("{truncated}...");
    }
    result
}
/// Session creation flags passed from CLI → AppView → effects.
///
/// Plan is delivered independently through ACP `SetSessionMode`, and subagent
/// availability is fixed by the existing process/session capability path.
/// Neither belongs in session-creation metadata. `askUserQuestion: false`
/// remains an independent final tool clamp.
#[derive(Debug, Clone, Default)]
pub(crate) struct SessionFlags {
    pub ask_user: bool,
    /// Restore code state on resume (`--restore-code`).
    /// Injected as `grow/restore_code` into `LoadSession` meta, or passed
    /// as `restoreCode` in the `resume_session` ACP payload for worktrees.
    pub restore_code: Option<bool>,
    pub agent_override: Option<serde_json::Value>,
    /// Canonical session permission mode (`_meta.permissionMode`).
    pub permission_mode: shell::util::config::PermissionMode,
    /// Effective screen mode label (`ScreenMode::meta_label`), stamped into
    /// every `PromptRequest._meta.screenMode` for minimal-vs-regular usage
    /// diagnostics. `None` (key omitted) only under `Default` in tests; real
    /// launches always know their mode.
    pub screen_mode_label: Option<&'static str>,
    /// Startup resume target deferred to the worktree handler after missing
    /// local id/title resolution. Worktree failure messages append the
    /// no-match hint only when the failing target equals this value.
    pub resume_local_miss: Option<String>,
}
impl SessionFlags {
    /// Build the `_meta` JSON value for ACP `NewSessionRequest` / `LoadSessionRequest`.
    ///
    /// In practice always `Some`: `permissionMode` is emitted unconditionally.
    /// `--no-ask-user` always forces
    /// `askUserQuestion: false` into the meta, even when paired with
    /// `GROW_AGENT` — the env var chooses the *agent*, but the tool-strip is
    /// independent.
    pub(super) fn to_meta(&self) -> Option<acp::Meta> {
        let mut meta = serde_json::Map::new();
        if let Some(ref profile) = self.agent_override {
            meta.insert("agentProfile".into(), profile.clone());
        }
        if !self.ask_user {
            meta.insert("askUserQuestion".into(), serde_json::json!(false));
        }
        meta.insert(
            "permissionMode".into(),
            serde_json::json!(shell::util::config::permission_mode_canonical_str(
                self.permission_mode,
            )),
        );
        if meta.is_empty() { None } else { Some(meta) }
    }
}
#[derive(Default)]
pub(crate) struct EffectMeta;
/// Extract the first branch-local prompt from the canonical Timeline.
pub(super) fn extract_first_user_prompt(
    info: &shell::session::info::Info,
) -> Option<String> {
    load_timeline(info)?
        .prompt_records()
        .into_iter()
        .next()
        .and_then(|record| {
            record
                .text
                .lines()
                .next()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
        })
}

pub(super) fn count_timeline_stats(info: &shell::session::info::Info) -> (usize, usize) {
    let Some(timeline) = load_timeline(info) else {
        return (0, 0);
    };
    let turns = timeline
        .events()
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { .. })
            )
        })
        .count();
    let tools = timeline
        .events()
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                chat_state::TimelineEventKind::Tool(chat_state::ToolEvent::Started { .. })
            )
        })
        .count();
    (turns, tools)
}

fn load_timeline(info: &shell::session::info::Info) -> Option<chat_state::Timeline> {
    shell::session::storage::load_timeline_by_id(info.id.0.as_ref())
        .ok()
        .flatten()
}
/// Reads `_meta["grow/listScope"]` from a session-list payload.
pub(super) fn parse_session_list_scope(payload: &serde_json::Value) -> ListScope {
    match payload
        .get("_meta")
        .and_then(|m| m.get("grow/listScope"))
        .and_then(|v| v.as_str())
    {
        Some("repo") => ListScope::Repo,
        Some("all") => ListScope::All,
        _ => ListScope::Cwd,
    }
}
/// Parse the `grow/session/list` response payload (the unwrapped
/// `{ "sessions": [...] }` object) into [`SessionPickerEntry`] rows.
///
/// Shared by the resume picker ([`Effect::FetchSessionList`]) and the
/// dashboard's non-leader idle-session fallback
/// ([`Effect::FetchDashboardSessions`]) so both produce identical labels.
/// Sessions older than 30 days are dropped. The current protocol requires one
/// canonical `title`; no `summary`, `firstPrompt`, or snake_case wire fallback
/// is accepted here.
pub(super) fn parse_session_picker_entries(
    payload: &serde_json::Value,
) -> Vec<crate::app::app_view::SessionPickerEntry> {
    use crate::app::app_view::SessionPickerEntry;
    let entries: Vec<serde_json::Value> = payload
        .get("sessions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let now = chrono::Utc::now();
    let cutoff = now - chrono::Duration::days(30);
    entries
        .into_iter()
        .filter_map(|v| {
            let id = v
                .get("sessionId")
                .and_then(|s| s.as_str())?
                .to_string();
            let title = v.get("title").and_then(|s| s.as_str())?.to_string();
            let parsed_updated: Option<chrono::DateTime<chrono::Utc>> = v
                .get("updatedAt")
                .and_then(|s| s.as_str())
                .and_then(|s| s.parse().ok());
            let parsed_created: Option<chrono::DateTime<chrono::Utc>> = v
                .get("createdAt")
                .and_then(|s| s.as_str())
                .and_then(|s| s.parse().ok());
            let updated_at: chrono::DateTime<chrono::Utc> = match parsed_updated {
                Some(ts) => {
                    if ts < cutoff {
                        return None;
                    }
                    ts
                }
                None => return None,
            };
            use tools::implementations::skills::skill::extract_skill_display_text;
            let display = extract_skill_display_text(&title).unwrap_or(title);
            let created_at: chrono::DateTime<chrono::Utc> = parsed_created
                .unwrap_or(updated_at);
            let cwd_str = v
                .get("cwd")
                .and_then(|s| s.as_str())
                .unwrap_or_default()
                .to_string();
            let hostname = v.get("hostname").and_then(|s| s.as_str()).map(String::from);
            let model_id = v
                .get("modelId")
                .and_then(|s| s.as_str())
                .map(String::from);
            let num_messages = v
                .get("numMessages")
                .and_then(|n| n.as_u64())
                .unwrap_or(0) as usize;
            let last_active_at: Option<chrono::DateTime<chrono::Utc>> = v
                .get("lastActiveAt")
                .and_then(|s| s.as_str())
                .and_then(|s| s.parse().ok());
            let branch = v.get("branch").and_then(|s| s.as_str()).map(String::from);
            let worktree_label = v
                .get("worktreeLabel")
                .and_then(|s| s.as_str())
                .map(String::from);
            let repo_name = crate::views::session_picker::repo_name_from_cwd(&cwd_str);
            Some(SessionPickerEntry {
                id,
                summary: display,
                updated_at,
                created_at,
                cwd: cwd_str,
                hostname,
                model_id,
                num_messages,
                last_active_at,
                branch,
                repo_name,
                worktree_label,
                card_detail: None,
            })
        })
        .filter(|e| !e.summary.is_empty())
        .collect()
}
/// Convert a resume-picker session into a dormant dashboard roster row.
///
/// Used by the non-leader dashboard fallback: local on-disk sessions have no
/// live activity signal, so they map to [`RosterActivity::Dormant`] and render
/// in the dashboard's **Inactive** group. The label, cwd, model, and worktree
/// badge all come straight from the picker entry.
pub(super) fn session_picker_entry_to_roster(
    e: &crate::app::app_view::SessionPickerEntry,
) -> crate::app::roster::RosterEntry {
    use crate::app::roster::{RosterActivity, RosterEntry, RosterOrigin};
    let last_change = e.last_active_at.unwrap_or(e.updated_at);
    RosterEntry {
        session_id: e.id.clone(),
        title: Some(e.summary.clone()).filter(|s| !s.trim().is_empty()),
        cwd: e.cwd.clone(),
        is_worktree: e.worktree_label.is_some(),
        model_id: e.model_id.clone(),
        permission_mode: diagnostics::enums::PermissionMode::Ask,
        activity: RosterActivity::Dormant,
        resident: false,
        last_change_unix_ms: last_change.timestamp_millis(),
        origin: RosterOrigin {
            kind: "local".into(),
            host: e.hostname.clone(),
        },
    }
}
/// Translate a settings-registry key + value into the matching shell
/// helper call. Type mismatches return an error (not panic) so a
/// spawned task doesn't crash the pager. Unknown keys also return
/// a descriptive error.
pub(crate) async fn persist_setting(
    key: crate::settings::SettingKey,
    value: crate::settings::SettingValue,
) -> Result<(), String> {
    use crate::settings::SettingValue;
    fn kind_mismatch(key: &str, expected: &str, got: &SettingValue) -> String {
        format!("persist_setting({key}) expected {expected}, got {got:?}")
    }
    match key {
        "compact_mode" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("compact_mode", "Bool", &value));
            };
            shell::util::config::set_compact_mode(b)
                .await
                .map_err(|e| e.to_string())
        }
        "show_timestamps" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("show_timestamps", "Bool", &value));
            };
            shell::util::config::set_show_timestamps(b)
                .await
                .map_err(|e| e.to_string())
        }
        "page_flip_on_send" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("page_flip_on_send", "Bool", &value));
            };
            shell::util::config::set_page_flip_on_send(b)
                .await
                .map_err(|e| e.to_string())
        }
        "combine_queued_prompts" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("combine_queued_prompts", "Bool", &value));
            };
            shell::util::config::set_combine_queued_prompts(b)
                .await
                .map_err(|e| e.to_string())
        }
        "show_timeline" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("show_timeline", "Bool", &value));
            };
            shell::util::config::set_show_timeline(b)
                .await
                .map_err(|e| e.to_string())
        }
        "simple_mode" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("simple_mode", "Bool", &value));
            };
            shell::util::config::set_simple_mode(b)
                .await
                .map_err(|e| e.to_string())
        }
        "contextual_hints.undo" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("contextual_hints.undo", "Bool", &value));
            };
            shell::util::config::set_contextual_hint_undo(b)
                .await
                .map_err(|e| e.to_string())
        }
        "contextual_hints.plan_mode" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("contextual_hints.plan_mode", "Bool", &value));
            };
            shell::util::config::set_contextual_hint_plan_mode(b)
                .await
                .map_err(|e| e.to_string())
        }
        "contextual_hints.image_input" => {
            let SettingValue::Bool(b) = value else {
                return Err(
                    kind_mismatch("contextual_hints.image_input", "Bool", &value),
                );
            };
            shell::util::config::set_contextual_hint_image_input(b)
                .await
                .map_err(|e| e.to_string())
        }
        "contextual_hints.send_now" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("contextual_hints.send_now", "Bool", &value));
            };
            shell::util::config::set_contextual_hint_send_now(b)
                .await
                .map_err(|e| e.to_string())
        }
        "contextual_hints.small_screen" => {
            let SettingValue::Bool(b) = value else {
                return Err(
                    kind_mismatch("contextual_hints.small_screen", "Bool", &value),
                );
            };
            shell::util::config::set_contextual_hint_small_screen(b)
                .await
                .map_err(|e| e.to_string())
        }
        "contextual_hints.word_select" => {
            let SettingValue::Bool(b) = value else {
                return Err(
                    kind_mismatch("contextual_hints.word_select", "Bool", &value),
                );
            };
            shell::util::config::set_contextual_hint_word_select(b)
                .await
                .map_err(|e| e.to_string())
        }
        "contextual_hints.ssh_wrap" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("contextual_hints.ssh_wrap", "Bool", &value));
            };
            shell::util::config::set_contextual_hint_ssh_wrap(b)
                .await
                .map_err(|e| e.to_string())
        }
        "theme" => {
            let SettingValue::Enum(s) = value else {
                return Err(kind_mismatch("theme", "Enum", &value));
            };
            shell::util::config::set_theme(s.to_string())
                .await
                .map_err(|e| e.to_string())
        }
        "auto_dark_theme" => {
            let SettingValue::Enum(s) = value else {
                return Err(kind_mismatch("auto_dark_theme", "Enum", &value));
            };
            shell::util::config::set_auto_dark_theme(s.to_string())
                .await
                .map_err(|e| e.to_string())
        }
        "auto_light_theme" => {
            let SettingValue::Enum(s) = value else {
                return Err(kind_mismatch("auto_light_theme", "Enum", &value));
            };
            shell::util::config::set_auto_light_theme(s.to_string())
                .await
                .map_err(|e| e.to_string())
        }
        "default_model" => {
            let SettingValue::String(s) = value else {
                return Err(kind_mismatch("default_model", "String", &value));
            };
            shell::util::config::set_default_model(s)
                .await
                .map_err(|e| e.to_string())
        }
        "scroll_speed" => {
            let SettingValue::Int(i) = value else {
                return Err(kind_mismatch("scroll_speed", "Int", &value));
            };
            shell::util::config::set_scroll_speed(i)
                .await
                .map_err(|e| e.to_string())
        }
        "scroll_mode" => {
            let SettingValue::Enum(s) = value else {
                return Err(kind_mismatch("scroll_mode", "Enum", &value));
            };
            shell::util::config::set_scroll_mode(s.to_string())
                .await
                .map_err(|e| e.to_string())
        }
        "invert_scroll" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("invert_scroll", "Bool", &value));
            };
            shell::util::config::set_invert_scroll(b)
                .await
                .map_err(|e| e.to_string())
        }
        "display_refresh_auto_cadence" => {
            let SettingValue::Bool(b) = value else {
                return Err(
                    kind_mismatch("display_refresh_auto_cadence", "Bool", &value),
                );
            };
            shell::util::config::set_display_refresh_auto_cadence(b)
                .await
                .map_err(|e| e.to_string())
        }
        "scroll_lines" => {
            let SettingValue::Int(i) = value else {
                return Err(kind_mismatch("scroll_lines", "Int", &value));
            };
            shell::util::config::set_scroll_lines(i)
                .await
                .map_err(|e| e.to_string())
        }
        "default_selected_permission" => {
            let SettingValue::Enum(s) = value else {
                return Err(kind_mismatch("default_selected_permission", "Enum", &value));
            };
            shell::util::config::set_default_selected_permission(s.to_string())
                .await
                .map_err(|e| e.to_string())
        }
        "cancel_subagents_on_turn_cancel" => {
            let SettingValue::Enum(s) = value else {
                return Err(
                    kind_mismatch("cancel_subagents_on_turn_cancel", "Enum", &value),
                );
            };
            shell::util::config::set_cancel_subagents_on_turn_cancel(
                    s.to_string(),
                )
                .await
                .map_err(|e| e.to_string())
        }
        "vim_mode" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("vim_mode", "Bool", &value));
            };
            shell::util::config::set_vim_mode(b)
                .await
                .map_err(|e| e.to_string())
        }
        "remember_tool_approvals" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("remember_tool_approvals", "Bool", &value));
            };
            shell::util::config::set_remember_tool_approvals(b)
                .await
                .map_err(|e| e.to_string())
        }
        "toolset.ask_user_question.timeout_enabled" => {
            let SettingValue::Bool(b) = value else {
                return Err(
                    kind_mismatch(
                        "toolset.ask_user_question.timeout_enabled",
                        "Bool",
                        &value,
                    ),
                );
            };
            shell::util::config::set_ask_user_question_timeout_enabled(b)
                .await
                .map_err(|e| e.to_string())
        }
        "show_thinking_blocks" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("show_thinking_blocks", "Bool", &value));
            };
            shell::util::config::set_show_thinking_blocks(b)
                .await
                .map_err(|e| e.to_string())
        }
        "group_tool_verbs" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("group_tool_verbs", "Bool", &value));
            };
            shell::util::config::set_group_tool_verbs(b)
                .await
                .map_err(|e| e.to_string())
        }
        "prompt_suggestions" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("prompt_suggestions", "Bool", &value));
            };
            shell::util::config::set_prompt_suggestions(b)
                .await
                .map_err(|e| e.to_string())
        }
        "keep_text_selection" => {
            let SettingValue::Enum(s) = value else {
                return Err(kind_mismatch("keep_text_selection", "Enum", &value));
            };
            shell::util::config::set_keep_text_selection(s.to_string())
                .await
                .map_err(|e| e.to_string())
        }
        "respect_manual_folds" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("respect_manual_folds", "Bool", &value));
            };
            tokio::task::spawn_blocking(move || crate::appearance::persist_respect_manual_folds(
                    b,
                ))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())
        }
        "render_mermaid" => {
            let SettingValue::Enum(s) = value else {
                return Err(kind_mismatch("render_mermaid", "Enum", &value));
            };
            shell::util::config::set_render_mermaid(s.to_string())
                .await
                .map_err(|e| e.to_string())
        }
        "hunk_tracker_mode" => {
            let SettingValue::Enum(s) = value else {
                return Err(kind_mismatch("hunk_tracker_mode", "Enum", &value));
            };
            shell::util::config::set_hunk_tracker_mode(s.to_string())
                .await
                .map_err(|e| e.to_string())
        }
        "screen_mode" => {
            let SettingValue::Enum(s) = value else {
                return Err(kind_mismatch("screen_mode", "Enum", &value));
            };
            shell::util::config::set_screen_mode(s.to_string())
                .await
                .map_err(|e| e.to_string())
        }
        "max_thoughts_width" => {
            let SettingValue::Int(i) = value else {
                return Err(kind_mismatch("max_thoughts_width", "Int", &value));
            };
            shell::util::config::set_max_thoughts_width(i)
                .await
                .map_err(|e| e.to_string())
        }
        "show_tips" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("show_tips", "Bool", &value));
            };
            shell::util::config::set_show_tips(b)
                .await
                .map_err(|e| e.to_string())
        }
        "auto_update" => {
            let SettingValue::Bool(b) = value else {
                return Err(kind_mismatch("auto_update", "Bool", &value));
            };
            shell::util::config::set_auto_update(b)
                .await
                .map_err(|e| e.to_string())
        }
        "fork_secondary_model" => {
            let SettingValue::String(s) = value else {
                return Err(kind_mismatch("fork_secondary_model", "String", &value));
            };
            shell::util::config::set_fork_secondary_model(s)
                .await
                .map_err(|e| e.to_string())
        }
        other => Err(format!("unknown setting key for persist: `{other}`")),
    }
}
/// Body for `Effect::PersistPermissionMode`. Factored out for testability.
///
/// 1. Persist `ui.permission_mode` to disk.
/// 2. Fire ACP `grow/permission_mode_changed` (gated on disk success for
///    `WithRollback`; always for `BestEffort`).
/// 3. Return the matching `TaskResult`.
pub(crate) async fn persist_permission_mode_and_notify(
    canonical: &'static str,
    session_id: Option<acp::SessionId>,
    persist: PermissionModePersist,
    tx: AcpAgentTx,
) -> TaskResult {
    let config_str: &'static str = canonical;
    let disk_result = shell::util::config::update_config(|cfg| {
            cfg.ui.permission_mode = Some(config_str.to_string());
        })
        .await;
    let disk_outcome: Result<(), String> = disk_result.map_err(|e| e.to_string());
    if should_send_permission_mode_notification(&disk_outcome, persist)
        && let Some(session_id) = session_id
    {
        let params = serde_json::json!({
            "sessionId": session_id,
            "permissionMode": config_str,
        });
        let notification = acp::ExtNotification::new(
            "grow/permission_mode_changed",
            serde_json::value::to_raw_value(&params)
                .expect("serialize permission_mode_changed params")
                .into(),
        );
        if let Err(e) = acp_send(notification, &tx).await {
            tracing::warn!("Failed to send permission_mode_changed notification: {e}");
        }
    }
    route_permission_mode_result(disk_outcome, persist, config_str)
}
/// Whether to fire the ACP `grow/permission_mode_changed` notification.
/// `WithRollback` suppresses on disk failure (agent must not see the
/// optimistic value). `BestEffort` always fires.
pub(super) fn should_send_permission_mode_notification(
    disk_outcome: &Result<(), String>,
    persist: PermissionModePersist,
) -> bool {
    match (disk_outcome, persist) {
        (_, PermissionModePersist::BestEffort) => true,
        (Ok(()), PermissionModePersist::WithRollback(_)) => true,
        (Err(_), PermissionModePersist::WithRollback(_)) => false,
    }
}
pub(super) fn marketplace_outcome_succeeded(
    outcome: &extension_types::ActionOutcome,
) -> bool {
    outcome.status == extension_types::OutcomeStatus::Success
}
/// Extract the typed kill outcome from an `grow/task/kill` ext response.
///
/// The agent serializes `ExtMethodResult<KillTaskResponse>`, so the outcome
/// lives at `result.outcome` (`{"result":{"taskId":..,"outcome":
/// "not_found"}}`). Deserializes through the same wire DTOs the agent
/// serializes (`shell::extensions::task::KillTaskResponse` +
/// `shell::session::result::ExtMethodResult`) so the contract stays
/// typed end-to-end. Returns `None` — which the dispatcher treats as "clear
/// pending state, keep the row" — for error envelopes (`result: null`) or
/// unparseable payloads. Probing the top level with untyped JSON here was
/// why the tasks-pane ✗ never removed stale (`not_found`) rows after a
/// session resume.
pub(super) fn parse_kill_outcome(
    resp: &str,
) -> Option<tools::types::KillOutcome> {
    use shell::extensions::task::KillTaskResponse;
    use shell::session::result::ExtMethodResult;
    serde_json::from_str::<ExtMethodResult<KillTaskResponse>>(resp)
        .ok()
        .and_then(|envelope| envelope.result)
        .map(|payload| payload.outcome)
}
/// Map an `grow/subagent/cancel` response (payload under `result`) to a kill
/// outcome. An error or invalid payload is `RpcFailed` because the subagent may
/// still be running, so the caller must leave the row alone.
pub(super) fn parse_subagent_kill_outcome(resp: &str) -> SubagentKillOutcome {
    use shell::extensions::task::{
        CancelSubagentResponse, SubagentCancelOutcomeDto,
    };
    let Some(payload) = serde_json::from_str::<
        ExtMethodResult<CancelSubagentResponse>,
    >(resp)
        .ok()
        .and_then(|envelope| envelope.result) else {
        return SubagentKillOutcome::RpcFailed;
    };
    match payload.outcome {
        SubagentCancelOutcomeDto::Cancelled => SubagentKillOutcome::StoppedLive,
        SubagentCancelOutcomeDto::AlreadyFinished { status } => {
            SubagentKillOutcome::NothingLive {
                status: Some(status),
            }
        }
        SubagentCancelOutcomeDto::NotFound => {
            SubagentKillOutcome::NothingLive {
                status: None,
            }
        }
    }
}
/// Map disk-write outcome + persist variant to the correct `TaskResult`.
pub(super) fn route_permission_mode_result(
    disk_outcome: Result<(), String>,
    persist: PermissionModePersist,
    config_str: &'static str,
) -> TaskResult {
    match (disk_outcome, persist) {
        (Ok(()), _) => {
            TaskResult::SettingPersisted {
                key: "permission_mode",
                value: crate::settings::SettingValue::Enum(config_str),
            }
        }
        (Err(e), PermissionModePersist::WithRollback(prev_canonical)) => {
            tracing::warn!("failed to save permission mode preference: {e} — rolling back");
            TaskResult::SettingPersistFailed {
                key: "permission_mode",
                rollback_value: crate::settings::SettingValue::Enum(prev_canonical),
                error: e,
            }
        }
        (Err(e), PermissionModePersist::BestEffort) => {
            tracing::warn!("failed to save permission mode preference (best-effort): {e}");
            TaskResult::SettingPersistFailedBestEffort {
                key: "permission_mode",
                error: e,
            }
        }
    }
}
/// Fire-and-forget blocking write of one `[hints]` value to config.toml.
/// `what` names the preference for log messages.
pub(super) fn persist_hint(
    tasks: &mut JoinSet<TaskResult>,
    key: &'static str,
    value: impl Into<toml_edit::Value> + Send + 'static,
    what: &'static str,
) {
    tasks
        .spawn(async move {
            match tokio::task::spawn_blocking(move || crate::config_toml_edit::set_hint(
                    key,
                    value,
                ))
                .await
            {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::warn!("failed to persist {what}: {e}"),
                Err(e) => tracing::warn!("failed to persist {what} (join error): {e}"),
            }
            TaskResult::CancelComplete
        });
}
/// A blocking flock on the shared, possibly-network `~/.grow` lock must never
/// stall the event-loop thread (and would hang exit on `/quit`); the registry
/// is best-effort, so skip on contention.
pub(super) fn unregister_active_session_best_effort(session_id: &acp::SessionId) {
    unregister_active_session_best_effort_in(
        &shell::util::grow_home::grow_home(),
        session_id,
    );
}
pub(super) fn unregister_active_session_best_effort_in(
    root: &Path,
    session_id: &acp::SessionId,
) {
    match shell::active_sessions::try_unregister_in(root, session_id) {
        Ok(true) => {}
        Ok(false) => {
            tracing::debug!(
            session_id = %session_id.0,
            "Skipped active-session unregister under lock contention; \
             reaped by collect_crashed on next launch"
        )
        }
        Err(e) => tracing::warn!(?e, "Failed to unregister active session"),
    }
}
