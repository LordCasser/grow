//! Subagent business types.
//!
//! Tracking state for spawned child sessions. [`SubagentInfo`] is the single
//! source of truth — used by both the subagent pane (display) and the
//! permission view (provenance labels).
use shell::session::storage::{ReplayEmission, stream_replay_updates_at};
use std::sync::Arc;
use std::time::Instant;
/// Enriched subagent tracking info.
///
/// Keyed by `child_session_id` in `AgentSession::subagent_sessions`.
/// Populated from `SubagentSpawned` notifications, updated by
/// `SubagentProgress` and `SubagentFinished`.
#[derive(Debug, Clone)]
pub struct SubagentInfo {
    pub subagent_id: Arc<str>,
    pub child_session_id: Arc<str>,
    pub description: Arc<str>,
    pub subagent_type: Arc<str>,
    pub model: Option<Arc<str>>,
    /// "new" or "resumed".
    pub context_source: Option<Arc<str>>,
    pub resumed_from: Option<Arc<str>>,
    /// "read-only", "read-write", "execute", or "all".
    pub capability_mode: Option<Arc<str>>,
    /// Requested child decision route (`ask`, `auto`, `always-approve`, or
    /// `follow`). `follow` is projected against the live parent mode.
    pub permission_mode: Option<Arc<str>>,
    /// Effective mode at spawn after managed-policy clamping.
    pub effective_permission_mode: Option<Arc<str>>,
    pub workflow_run_id: Option<Arc<str>>,
    /// Whether the context was normalized into `<background_context>`.
    pub context_normalized: bool,
    pub parent_prompt_id: Option<Arc<str>>,
    pub started_at: Instant,
    /// Wall-clock time of the most recent `SubagentProgress` /
    /// `SubagentFinished` update. For
    /// running subagents this is the "last activity" timestamp the
    /// dashboard uses for sort + age display; for finished subagents
    /// this is the finish time, not the start.
    ///
    /// Initialised to `started_at` so that brand-new subagents with
    /// no progress notifications yet still sort correctly.
    pub last_progress_at: Instant,
    pub finished: bool,
    /// "completed", "failed", or "cancelled".
    pub status: Option<Arc<str>>,
    pub error: Option<Arc<str>>,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: Option<u64>,
    pub tool_calls: Option<u32>,
    pub turns: Option<u32>,
    pub turn_count: Option<u32>,
    pub tool_call_count: Option<u32>,
    pub tokens_used: Option<u64>,
    pub context_window_tokens: Option<u64>,
    /// 0-100.
    pub context_usage_pct: Option<u8>,
    pub tools_used: Vec<Arc<str>>,
    pub error_count: Option<u32>,
    /// Live activity label ("Thinking", "Running: cargo build") mirroring
    /// the scrollback block's field; feeds the tasks pane row and the
    /// dashboard activity column. Cleared on `SubagentFinished`.
    pub activity_label: Option<String>,
    /// Affects scrollback rendering (background shows "started:"/"completed:").
    pub is_background: bool,
    /// Set on kill request, cleared on `SubagentFinished`.
    pub pending_kill: bool,
    /// When the kill request was sent. Used to auto-clear `pending_kill`
    /// after a timeout so the user can retry if the notification is lost.
    pub kill_requested_at: Option<Instant>,
    /// Set on spawn, updated on finish.
    pub scrollback_entry_id: Option<crate::scrollback::entry::EntryId>,
    pub prompt: Option<Arc<str>>,
    pub child_cwd: Option<Arc<str>>,
    pub worktree_path: Option<Arc<str>>,
    /// Set after the first `replay_inherited_updates` attempt (spawn or open).
    /// Prevents duplicate replay when scrollback is prompt-only after spawn.
    pub child_updates_replayed: bool,
}
impl SubagentInfo {
    /// Whether the subagent is currently running (not finished).
    pub fn is_running(&self) -> bool {
        !self.finished
    }
    /// Elapsed time since spawn.
    pub fn elapsed(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }
    /// Display-ready elapsed duration.
    /// Uses authoritative `duration_ms` from SubagentFinished when available,
    /// falls back to live wall-clock elapsed for running subagents.
    pub fn display_elapsed(&self) -> std::time::Duration {
        if self.finished {
            self.duration_ms
                .map(std::time::Duration::from_millis)
                .unwrap_or_else(|| self.elapsed())
        } else {
            self.elapsed()
        }
    }

    pub fn display_elapsed_at(&self, now: Instant) -> std::time::Duration {
        if self.finished {
            self.duration_ms
                .map(std::time::Duration::from_millis)
                .unwrap_or_else(|| now.saturating_duration_since(self.started_at))
        } else {
            now.saturating_duration_since(self.started_at)
        }
    }
}
/// Resolve the immediate durable children owned by `parent_session_id`.
/// Ownership comes exclusively from the identity-bound parent Timeline.
fn durable_child_session_ids(
    grow_home: &std::path::Path,
    parent_session_id: &str,
) -> std::collections::BTreeSet<String> {
    let Ok(Some(timeline)) =
        shell::session::storage::load_timeline_by_id_at(parent_session_id, grow_home)
    else {
        return std::collections::BTreeSet::new();
    };
    let mut children = std::collections::BTreeMap::new();
    for event in timeline.events() {
        let chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(spawn)) =
            &event.kind
        else {
            continue;
        };
        children.insert(spawn.child_session_id.clone(), ());
    }
    tracing::trace!(
        parent_session_id,
        children = children.len(),
        "projected durable child ownership from Timeline"
    );
    children.into_keys().collect()
}
/// Grow home for the replay path. In production this is just `grow_home()`; the
/// whole test override below is `#[cfg(test)]`, so no thread-local or dead
/// always-false branch ships in release.
#[cfg(not(test))]
fn effective_grow_home() -> std::path::PathBuf {
    shell::util::grow_home::grow_home()
}
#[cfg(test)]
thread_local! {
    static REPLAY_GROW_HOME: std::cell::RefCell<Option<std::path::PathBuf>> =
        const { std::cell::RefCell::new(None) };
}
/// Override grow home for disk-replay unit tests (thread-local).
#[cfg(test)]
pub(crate) fn set_replay_grow_home_for_tests(home: Option<std::path::PathBuf>) {
    REPLAY_GROW_HOME.with(|h| *h.borrow_mut() = home);
}
#[cfg(test)]
fn effective_grow_home() -> std::path::PathBuf {
    if let Some(home) = REPLAY_GROW_HOME.with(|h| h.borrow().clone()) {
        return home;
    }
    shell::util::grow_home::grow_home()
}
/// Best-effort enrichment from the parent's canonical spawn fact.
pub(crate) fn enrich_from_timeline(
    info: &mut SubagentInfo,
    _parent_cwd: &std::path::Path,
    parent_session_id: &str,
) {
    enrich_from_timeline_with_home(info, &effective_grow_home(), parent_session_id);
}
fn enrich_from_timeline_with_home(
    info: &mut SubagentInfo,
    grow_home: &std::path::Path,
    parent_session_id: &str,
) {
    let Ok(Some(timeline)) =
        shell::session::storage::load_timeline_by_id_at(parent_session_id, grow_home)
    else {
        return;
    };
    let Some(spawn) =
        timeline
            .events()
            .iter()
            .find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) if spawn.subagent_id == info.subagent_id.as_ref() => Some(spawn),
                _ => None,
            })
    else {
        return;
    };
    info.prompt = Some(Arc::from(spawn.prompt.as_str()));
    info.child_cwd = Some(Arc::from(spawn.child_cwd.as_str()));
    info.worktree_path = spawn.worktree_path.as_deref().map(Arc::from);
}
/// Best-effort replay of a child's inherited conversation, streamed one typed
/// update at a time so a large inherited transcript is not materialized as a
/// full `Vec` of typed structs (peak stays near the file size rather than
/// several multiples of it). No-ops when the child session or file is missing.
pub(crate) fn replay_inherited_updates(
    child_view: &mut crate::app::agent_view::AgentView,
    child_session_id: &str,
) {
    let home = effective_grow_home();
    let replay_meta = crate::acp::meta::NotificationMeta {
        is_replay: true,
        ..Default::default()
    };
    let outcome = match stream_replay_updates_at(child_session_id, &home, |update| {
        child_view
            .session
            .handle_update(update, &replay_meta, &mut child_view.scrollback);
    }) {
        Ok(outcome) => outcome,
        Err(e) => {
            tracing::warn!(session_id = %child_session_id, error = %e, "failed to read updates for replay");
            return;
        }
    };
    if outcome == ReplayEmission::Emitted {
        crate::memory_release::release_retained_memory_with("subagent-replay");
    }
}

/// Rebuild the flat descendant index after a root session load. Root replay
/// contains direct-child lifecycle records, while each nested spawn is durable
/// in its immediate parent's session. Walk those child logs breadth-first and
/// feed only lifecycle records through the normal notification projection so
/// pending grandchild interactions can route immediately after load.
pub(crate) fn restore_descendant_lifecycle(
    app: &mut crate::app::root::AppView,
    root_agent_id: crate::app::session::AgentId,
) {
    use shell::extensions::notification::SessionUpdate;

    let Some(root) = app.agents.get(&root_agent_id) else {
        return;
    };
    let grow_home = effective_grow_home();
    let Some(root_session_id) = root.session.session_id.as_ref().map(|id| id.0.to_string()) else {
        return;
    };
    let replayed_direct_children = root
        .session
        .subagent_sessions
        .keys()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let mut queue = durable_child_session_ids(&grow_home, &root_session_id)
        .into_iter()
        .filter(|child_session_id| replayed_direct_children.contains(child_session_id))
        .collect::<std::collections::VecDeque<_>>();
    let mut visited = std::collections::HashSet::new();
    let previous_loading_replay = root.session.loading_replay;
    if let Some(root) = app.agents.get_mut(&root_agent_id) {
        // Reuse the lifecycle projection's restoration semantics: spawn does
        // not eagerly replay a whole child transcript and finish does not add
        // a second live footer. The descendant-specific handler bypasses the
        // network "unexpected replay" gate while retaining source-local
        // highwaters.
        root.session.loading_replay = true;
    }

    while let Some(parent_session_id) = queue.pop_front() {
        if !visited.insert(parent_session_id.clone()) {
            continue;
        }
        let mut durable_children = durable_child_session_ids(&grow_home, &parent_session_id);
        let mut lifecycle = Vec::new();
        if let Err(error) = shell::session::storage::stream_replay_grow_notifications_at(
            &parent_session_id,
            &grow_home,
            |notification| {
                if matches!(
                    &notification.update,
                    SessionUpdate::SubagentSpawned { .. }
                        | SessionUpdate::SubagentProgress { .. }
                        | SessionUpdate::SubagentFinished { .. }
                ) {
                    lifecycle.push(notification);
                }
            },
        ) {
            tracing::debug!(
                session_id = parent_session_id,
                ?error,
                "failed to replay descendant lifecycle"
            );
            continue;
        }

        for mut notification in lifecycle {
            let discovered_child = match &notification.update {
                SessionUpdate::SubagentSpawned {
                    child_session_id, ..
                } => Some(child_session_id.clone()),
                _ => None,
            };
            notification.session_id =
                agent_client_protocol::SessionId::new(parent_session_id.clone());
            let mut meta = notification
                .meta
                .take()
                .and_then(|value| value.as_object().cloned())
                .unwrap_or_default();
            meta.insert("isReplay".into(), serde_json::Value::Bool(true));
            notification.meta = Some(serde_json::Value::Object(meta));
            let Ok(raw) = serde_json::value::to_raw_value(&notification) else {
                continue;
            };
            let ext = agent_client_protocol::ExtNotification::new(
                "grow/session_notification",
                std::sync::Arc::from(raw),
            );
            crate::app::acp_handler::handle_descendant_lifecycle_replay(&ext, app, root_agent_id);
            if let Some(child) = discovered_child
                && durable_children.remove(&child)
            {
                queue.push_back(child);
            }
        }
    }
    if let Some(root) = app.agents.get_mut(&root_agent_id) {
        root.session.loading_replay = previous_loading_replay;
    }
}
/// True when the child scrollback has no substantive replay content yet.
fn subagent_child_needs_replay(child_view: &crate::app::agent_view::AgentView) -> bool {
    let len = child_view.scrollback.len();
    if len == 0 {
        return true;
    }
    for i in 0..len {
        let Some(entry) = child_view.scrollback.entry(i) else {
            continue;
        };
        match &entry.block {
            crate::scrollback::block::RenderBlock::UserPrompt(_) => {}
            _ => return false,
        }
    }
    true
}
/// Replay child `updates.jsonl` when opening fullscreen if spawn-time replay
/// has not run yet and scrollback only has the injected task prompt (or is empty).
pub(crate) fn ensure_subagent_child_replayed(
    parent: &mut crate::app::agent_view::AgentView,
    child_sid: &str,
) {
    let should_replay = parent
        .session
        .subagent_sessions
        .get(child_sid)
        .is_some_and(|info| !info.child_updates_replayed)
        && parent
            .subagent_views
            .get(child_sid)
            .is_some_and(|v| subagent_child_needs_replay(v.as_ref()));
    if !should_replay {
        return;
    }
    let finished_elapsed = parent
        .session
        .subagent_sessions
        .get(child_sid)
        .filter(|info| info.finished)
        .and_then(|info| info.duration_ms)
        .map(std::time::Duration::from_millis);
    let parent_turn_running =
        parent.session.state.is_turn_running() || parent.session.state.is_cancelling();
    if let Some(child_view) = parent.subagent_views.get_mut(child_sid) {
        replay_inherited_updates(child_view, child_sid);
        if let Some(elapsed) = finished_elapsed {
            finalize_finished_child_view(child_view, elapsed);
        } else if !parent_turn_running {
            child_view.scrollback.finish_all_running();
        }
    }
    if let Some(info) = parent.session.subagent_sessions.get_mut(child_sid) {
        info.child_updates_replayed = true;
    }
}
/// Finalize a finished child view: end the turn and append the `TurnCompleted`
/// footer. Shared by the live `SubagentFinished` path and the deferred resume path.
pub(crate) fn finalize_finished_child_view(
    child_view: &mut crate::app::agent_view::AgentView,
    elapsed: std::time::Duration,
) {
    child_view
        .session
        .tracker
        .finish_turn(&mut child_view.scrollback);
    child_view.scrollback.finish_all_running();
    child_view
        .scrollback
        .push_block(crate::scrollback::block::RenderBlock::session_event(
            crate::scrollback::blocks::SessionEvent::TurnCompleted {
                elapsed: Some(elapsed),
            },
        ));
}
fn join_meta_parts(parts: &[Option<&str>]) -> String {
    let non_empty: Vec<&str> = parts.iter().copied().flatten().collect();
    if non_empty.is_empty() {
        String::new()
    } else {
        non_empty.join(" \u{00b7} ")
    }
}
pub(crate) fn format_type_label(subagent_type: &str) -> &str {
    match subagent_type {
        "general-purpose" => "general",
        other => other,
    }
}
pub(crate) fn format_context_badge(info: &SubagentInfo) -> &str {
    match info.context_source.as_deref() {
        Some("resumed") => "resumed",
        Some("forked") => "forked",
        _ => "",
    }
}
/// Parse a leading `[tag]` prefix from a description.
///
/// Returns `(Some(tag), rest_after_close_bracket)` if the description begins
/// with `[<non-empty>]`, otherwise `(None, description)` unchanged.
fn parse_tag_prefix(description: &str) -> (Option<&str>, &str) {
    if let Some(rest) = description.strip_prefix('[')
        && let Some(close) = rest.find(']')
    {
        let tag = rest[..close].trim();
        if !tag.is_empty() {
            return (Some(tag), rest[close + 1..].trim_start());
        }
    }
    (None, description)
}
/// Single consolidated label + display description for a subagent row.
///
/// Precedence for the label:
/// 1. `subagent_type` (only when **not** `general-purpose`) — `explore`,
///    `plan`, or any custom type carries real signal.
/// 2. `[tag]` parsed from the description — fallback when nothing above
///    identifies the agent and `subagent_type` is the meaningless default.
/// 3. `"general"` — final fallback.
///
/// The returned label has its first character capitalized for display
/// (e.g. `explore` → `Explore`).
///
/// The returned description always has any leading `[tag]` prefix stripped,
/// regardless of whether the tag was used as the label, so callers never
/// render `[tag]` bracket noise inline.
pub(crate) fn format_subagent_label(info: &SubagentInfo) -> (String, String) {
    let (tag, clean_desc) = parse_tag_prefix(&info.description);
    let raw_label = if info.subagent_type.as_ref() != "general-purpose" {
        format_type_label(&info.subagent_type).to_string()
    } else if let Some(tag) = tag {
        tag.to_string()
    } else {
        "general".to_string()
    };
    let mut chars = raw_label.chars();
    let label = match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => raw_label,
    };
    (label, clean_desc.to_string())
}

/// Canonical visible identity shared by the Subagents pane and any secondary
/// surface that refers to the same live child.
pub(crate) fn format_subagent_title(info: &SubagentInfo) -> String {
    let (label, description) = format_subagent_label(info);
    if description.is_empty() {
        label
    } else {
        format!("{label} {description}")
    }
}
pub(crate) fn format_subagent_meta(model: Option<&str>) -> String {
    let bare = join_meta_parts(&[model]);
    if bare.is_empty() {
        bare
    } else {
        format!(" ({bare})")
    }
}
/// Format a [`TurnActivity`] into a concise display label.
///
/// Used in the subagent scrollback block and the fullscreen title bar.
/// Callers handle the `None` activity / "Waiting" case separately.
pub(crate) fn format_activity_label(activity: &crate::acp::tracker::TurnActivity) -> String {
    use crate::acp::tracker::TurnActivity;
    match activity {
        TurnActivity::Thinking => "Thinking".to_string(),
        TurnActivity::Responding => "Responding".to_string(),
        TurnActivity::ToolRunning { title, description } => {
            if let Some(desc) = description
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                crate::acp::tracker::format_waiting_for_subject(desc)
            } else if title.is_empty() {
                "Running tool".to_string()
            } else {
                let first_line = title.lines().next().unwrap_or(title);
                let max_len = crate::acp::tracker::MAX_ACTIVITY_SUBJECT_CHARS;
                if first_line.len() <= max_len {
                    format!("Running: {first_line}")
                } else {
                    let char_count = first_line.chars().count();
                    if char_count <= max_len {
                        format!("Running: {first_line}")
                    } else {
                        let truncated: String = first_line.chars().take(max_len).collect();
                        format!("Running: {truncated}\u{2026}")
                    }
                }
            }
        }
        TurnActivity::AutoCompacting => "Compacting".to_string(),
        TurnActivity::Retrying {
            attempt,
            max_retries,
            ..
        } => {
            format!("Retrying ({attempt}/{max_retries})")
        }
        TurnActivity::Waiting(reason) => reason.label(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::meta::NotificationMeta;
    use crate::acp::model_state::ModelState;
    use crate::app::agent_view::AgentView;
    use crate::app::session::{AgentId, AgentSession};
    use crate::scrollback::block::RenderBlock;
    use crate::scrollback::state::ScrollbackState;
    use agent_client_protocol as acp;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Instant;
    fn make_info() -> SubagentInfo {
        SubagentInfo {
            subagent_id: "sa-1".into(),
            child_session_id: "cs-1".into(),
            description: "test task".into(),
            subagent_type: "explore".into(),
            model: None,
            context_source: None,
            resumed_from: None,
            capability_mode: None,
            permission_mode: None,
            effective_permission_mode: None,
            workflow_run_id: None,
            context_normalized: false,
            parent_prompt_id: None,
            started_at: Instant::now(),
            last_progress_at: Instant::now(),
            finished: false,
            status: None,
            error: None,
            duration_ms: None,
            tool_calls: None,
            turns: None,
            turn_count: None,
            tool_call_count: None,
            tokens_used: None,
            context_window_tokens: None,
            context_usage_pct: None,
            tools_used: Vec::new(),
            error_count: None,
            activity_label: None,
            is_background: false,
            pending_kill: false,
            kill_requested_at: None,
            scrollback_entry_id: None,
            prompt: None,
            child_cwd: None,
            worktree_path: None,
            child_updates_replayed: false,
        }
    }
    fn make_min_child_view() -> AgentView {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let session = {
            let session = AgentSession::new(
                AgentId(0),
                tx,
                Some(acp::SessionId::new(Arc::from("child"))),
                ModelState::default(),
                PathBuf::from("/tmp"),
                shell::util::config::PermissionMode::Ask,
            );
            session
        };
        AgentView::new(session, ScrollbackState::new())
    }
    fn seed_tool_call(view: &mut AgentView) {
        view.session.tracker.handle_update(
            acp::SessionUpdate::ToolCall(
                acp::ToolCall::new(acp::ToolCallId::new(Arc::from("tc1")), "Read foo")
                    .kind(acp::ToolKind::Other)
                    .status(acp::ToolCallStatus::Pending)
                    .content(vec![])
                    .locations(vec![]),
            ),
            &NotificationMeta::default(),
            &mut view.scrollback,
        );
    }
    #[test]
    fn subagent_child_needs_replay_empty_scrollback() {
        let view = make_min_child_view();
        assert!(subagent_child_needs_replay(&view));
    }
    #[test]
    fn subagent_child_needs_replay_prompt_only() {
        let mut view = make_min_child_view();
        view.scrollback
            .push_block(RenderBlock::user_prompt("scan src/"));
        assert!(subagent_child_needs_replay(&view));
    }
    #[test]
    fn subagent_child_needs_replay_false_when_tool_call_present() {
        let mut view = make_min_child_view();
        seed_tool_call(&mut view);
        assert!(!subagent_child_needs_replay(&view));
    }
    #[test]
    fn subagent_child_needs_replay_false_when_prompt_and_tool_call() {
        let mut view = make_min_child_view();
        view.scrollback
            .push_block(RenderBlock::user_prompt("scan src/"));
        seed_tool_call(&mut view);
        assert!(!subagent_child_needs_replay(&view));
    }
    #[test]
    fn ensure_subagent_child_replayed_skips_when_spawn_flag_set() {
        let mut parent = make_min_child_view();
        let child_sid = "child-skip";
        let mut child = make_min_child_view();
        child
            .scrollback
            .push_block(RenderBlock::user_prompt("task only"));
        parent
            .subagent_views
            .insert(child_sid.to_string(), Box::new(child));
        let mut info = make_info();
        info.child_session_id = child_sid.into();
        info.child_updates_replayed = true;
        parent
            .session
            .subagent_sessions
            .insert(child_sid.to_string(), info);
        ensure_subagent_child_replayed(&mut parent, child_sid);
        let child = parent.subagent_views.get(child_sid).unwrap();
        assert_eq!(child.scrollback.len(), 1);
        assert!(matches!(
            child.scrollback.entry(0).unwrap().block,
            RenderBlock::UserPrompt(_)
        ));
    }
    /// The child-transcript replay purges exactly once when it actually
    /// parsed an `updates.jsonl` transient — and never when the load no-ops
    /// (missing file) or the open takes the already-replayed skip path. The
    /// purge lives inside `replay_inherited_updates` so BOTH producers (the
    /// eager live-spawn path and this deferred first-open path) are covered.
    #[test]
    fn ensure_subagent_child_replayed_releases_retained_memory_once() {
        use crate::memory_release::test_support;
        test_support::install_counting_hook();
        let child_sid = "child-purge-real";
        let home = tempfile::tempdir().unwrap();
        let session_dir = home
            .path()
            .join("sessions")
            .join(shell::util::grow_home::encode_cwd_dirname("/tmp"))
            .join(child_sid);
        std::fs::create_dir_all(&session_dir).unwrap();
        let info = shell::session::info::Info {
            id: acp::SessionId::new(child_sid),
            cwd: "/tmp".into(),
        };
        let mut summary =
            shell::session::persistence::Summary::new(&info, acp::ModelId::new("test-model"))
                .unwrap();
        summary.session_kind = Some("subagent".into());
        std::fs::write(
            session_dir.join("summary.json"),
            serde_json::to_vec(&summary).unwrap(),
        )
        .unwrap();
        let tool_line = format!(
            r#"{{"method":"session/update","params":{{"sessionId":"{child_sid}","update":{{"sessionUpdate":"tool_call","toolCallId":"tc1","title":"Read foo","kind":"read","locations":[{{"path":"/tmp/foo"}}]}}}}}}"#
        );
        std::fs::write(session_dir.join("updates.jsonl"), tool_line + "\n").unwrap();
        set_replay_grow_home_for_tests(Some(home.path().to_path_buf()));
        let mut parent = make_min_child_view();
        parent
            .subagent_views
            .insert(child_sid.to_string(), Box::new(make_min_child_view()));
        let mut info = make_info();
        info.child_session_id = child_sid.into();
        parent
            .session
            .subagent_sessions
            .insert(child_sid.to_string(), info);
        let before = test_support::calls();
        ensure_subagent_child_replayed(&mut parent, child_sid);
        assert_eq!(
            test_support::calls(),
            before + 1,
            "a real replay must purge after the parsed transient drops"
        );
        assert!(
            parent.session.subagent_sessions[child_sid].child_updates_replayed,
            "fixture sanity: the replay attempt must mark the child replayed"
        );
        let before = test_support::calls();
        ensure_subagent_child_replayed(&mut parent, child_sid);
        assert_eq!(
            test_support::calls(),
            before,
            "the skip path allocates nothing and must not purge"
        );
        let ghost_sid = "child-purge-ghost";
        parent
            .subagent_views
            .insert(ghost_sid.to_string(), Box::new(make_min_child_view()));
        let mut ghost = make_info();
        ghost.child_session_id = ghost_sid.into();
        parent
            .session
            .subagent_sessions
            .insert(ghost_sid.to_string(), ghost);
        let before = test_support::calls();
        ensure_subagent_child_replayed(&mut parent, ghost_sid);
        assert_eq!(
            test_support::calls(),
            before,
            "a no-op replay (missing transcript) must not purge"
        );
        assert!(parent.session.subagent_sessions[ghost_sid].child_updates_replayed);
        let empty_sid = "child-purge-empty";
        let empty_dir = home
            .path()
            .join("sessions")
            .join(urlencoding::encode("/tmp").as_ref())
            .join(empty_sid);
        std::fs::create_dir_all(&empty_dir).unwrap();
        std::fs::write(empty_dir.join("summary.json"), "{}").unwrap();
        std::fs::write(empty_dir.join("updates.jsonl"), "").unwrap();
        parent
            .subagent_views
            .insert(empty_sid.to_string(), Box::new(make_min_child_view()));
        let mut empty = make_info();
        empty.child_session_id = empty_sid.into();
        parent
            .session
            .subagent_sessions
            .insert(empty_sid.to_string(), empty);
        let before = test_support::calls();
        ensure_subagent_child_replayed(&mut parent, empty_sid);
        assert_eq!(
            test_support::calls(),
            before,
            "an empty replay (zero updates parsed) must not purge"
        );
        assert!(parent.session.subagent_sessions[empty_sid].child_updates_replayed);
        set_replay_grow_home_for_tests(None);
    }
    #[test]
    fn subagent_meta_empty() {
        assert_eq!(format_subagent_meta(None), "");
    }
    #[test]
    fn subagent_meta_model() {
        assert_eq!(format_subagent_meta(Some("grow-3")), " (grow-3)");
    }
    #[test]
    fn type_label_abbreviates_general_purpose() {
        assert_eq!(format_type_label("general-purpose"), "general");
    }
    #[test]
    fn type_label_passes_through_known_types() {
        assert_eq!(format_type_label("explore"), "explore");
        assert_eq!(format_type_label("plan"), "plan");
    }
    #[test]
    fn type_label_passes_through_unknown() {
        assert_eq!(format_type_label("custom-agent"), "custom-agent");
    }
    #[test]
    fn context_badge_resumed() {
        let mut info = make_info();
        info.context_source = Some("resumed".into());
        assert_eq!(format_context_badge(&info), "resumed");
    }
    #[test]
    fn context_badge_forked() {
        let mut info = make_info();
        info.context_source = Some("forked".into());
        assert_eq!(format_context_badge(&info), "forked");
    }
    #[test]
    fn context_badge_new_returns_empty() {
        let mut info = make_info();
        info.context_source = Some("new".into());
        assert_eq!(format_context_badge(&info), "");
    }
    #[test]
    fn context_badge_none_returns_empty() {
        assert_eq!(format_context_badge(&make_info()), "");
    }
    #[test]
    fn label_uses_subagent_type_when_meaningful() {
        let mut info = make_info();
        info.subagent_type = "explore".into();
        info.description = "[deep-dive] find auth code".into();
        let (label, desc) = format_subagent_label(&info);
        assert_eq!(label, "Explore");
        assert_eq!(desc, "find auth code");
    }
    #[test]
    fn label_falls_back_to_tag_when_general_purpose() {
        let mut info = make_info();
        info.subagent_type = "general-purpose".into();
        info.description = "[security-fix] patch XSS".into();
        let (label, desc) = format_subagent_label(&info);
        assert_eq!(label, "Security-fix");
        assert_eq!(desc, "patch XSS");
    }
    #[test]
    fn label_final_fallback_general() {
        let mut info = make_info();
        info.subagent_type = "general-purpose".into();
        info.description = "do a thing".into();
        let (label, desc) = format_subagent_label(&info);
        assert_eq!(label, "General");
        assert_eq!(desc, "do a thing");
    }
    #[test]
    fn label_strips_tag_prefix_even_when_unused() {
        let mut info = make_info();
        info.subagent_type = "general-purpose".into();
        info.description = "[review] check the diff".into();
        let (label, desc) = format_subagent_label(&info);
        assert_eq!(label, "Review");
        assert_eq!(desc, "check the diff");
    }
    #[test]
    fn label_treats_empty_tag_as_absent() {
        let mut info = make_info();
        info.subagent_type = "general-purpose".into();
        info.description = "[] do something".into();
        let (label, desc) = format_subagent_label(&info);
        assert_eq!(label, "General");
        assert_eq!(desc, "[] do something");
    }
    #[test]
    fn label_unclosed_bracket_leaves_description_alone() {
        let mut info = make_info();
        info.subagent_type = "general-purpose".into();
        info.description = "[broken description".into();
        let (label, desc) = format_subagent_label(&info);
        assert_eq!(label, "General");
        assert_eq!(desc, "[broken description");
    }
    #[test]
    fn label_custom_subagent_type_passes_through_with_capitalization() {
        let mut info = make_info();
        info.subagent_type = "custom-agent".into();
        let (label, _) = format_subagent_label(&info);
        assert_eq!(label, "Custom-agent");
    }
    fn write_spawn_timeline(
        dir: &std::path::Path,
        subagent_id: &str,
        child_session_id: &str,
        prompt: &str,
        child_cwd: &str,
        worktree_path: Option<&str>,
    ) {
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Spawned(chat_state::SubagentSpawnEvent {
                    subagent_id: subagent_id.into(),
                    child_session_id: child_session_id.into(),
                    subagent_type: "general-purpose".into(),
                    description: "task".into(),
                    prompt: prompt.into(),
                    context_source: chat_state::SubagentContextSource::New,
                    source_ref: None,
                    context_normalized: false,
                    resumed_from: None,
                    parent_prompt_id: None,
                    capability_mode: None,
                    permission_mode: None,
                    effective_permission_mode: None,
                    workflow_run_id: None,
                    goal_id: None,
                    surface_completion: true,
                    child_cwd: child_cwd.into(),
                    worktree_path: worktree_path.map(str::to_owned),
                    effective_model_id: "grow-3".into(),
                }),
            ))
            .unwrap();
        let mut bytes = Vec::new();
        for event in timeline.events() {
            serde_json::to_writer(&mut bytes, event).unwrap();
            bytes.push(b'\n');
        }
        std::fs::write(dir.join("timeline.jsonl"), bytes).unwrap();
    }
    /// Build a session dir matching the canonical session path formula.
    fn setup_enrichment_dir(
        grow_home: &std::path::Path,
        cwd: &std::path::Path,
        session_id: &str,
    ) -> std::path::PathBuf {
        let sessions_dir = grow_home
            .join("sessions")
            .join(shell::util::grow_home::encode_cwd_dirname(
                cwd.to_string_lossy().as_ref(),
            ))
            .join(session_id);
        std::fs::create_dir_all(&sessions_dir).unwrap();
        let info = shell::session::info::Info {
            id: agent_client_protocol::SessionId::new(session_id),
            cwd: cwd.to_string_lossy().into_owned(),
        };
        let summary = shell::session::persistence::Summary::new(
            &info,
            agent_client_protocol::ModelId::new("test-model"),
        )
        .unwrap();
        std::fs::write(
            sessions_dir.join("summary.json"),
            serde_json::to_vec(&summary).unwrap(),
        )
        .unwrap();
        sessions_dir
    }
    #[test]
    fn enrich_from_timeline_populates_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = std::path::Path::new("/home/user/project");
        let session_id = "sess-abc";
        let session_dir = setup_enrichment_dir(tmp.path(), cwd, session_id);
        write_spawn_timeline(
            &session_dir,
            "sa-1",
            "child-1",
            "do stuff",
            "/tmp/work",
            Some("/tmp/wt"),
        );
        let mut info = make_info();
        enrich_from_timeline_with_home(&mut info, tmp.path(), session_id);
        assert_eq!(info.prompt.as_deref(), Some("do stuff"));
        assert_eq!(info.child_cwd.as_deref(), Some("/tmp/work"));
        assert_eq!(info.worktree_path.as_deref(), Some("/tmp/wt"));
    }
    #[test]
    fn enrich_from_timeline_missing_file_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let mut info = make_info();
        enrich_from_timeline_with_home(&mut info, tmp.path(), "no-session");
        assert!(info.prompt.is_none());
        assert!(info.child_cwd.is_none());
        assert!(info.worktree_path.is_none());
    }
    #[test]
    fn enrich_from_timeline_malformed_json_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = std::path::Path::new("/home/user");
        let session_dir = setup_enrichment_dir(tmp.path(), cwd, "sess-x");
        std::fs::write(session_dir.join("timeline.jsonl"), "not json{{{\n").unwrap();
        let mut info = make_info();
        enrich_from_timeline_with_home(&mut info, tmp.path(), "sess-x");
        assert!(info.prompt.is_none());
    }
    #[test]
    fn activity_label_thinking() {
        use crate::acp::tracker::TurnActivity;
        assert_eq!(format_activity_label(&TurnActivity::Thinking), "Thinking");
    }
    #[test]
    fn activity_label_responding() {
        use crate::acp::tracker::TurnActivity;
        assert_eq!(
            format_activity_label(&TurnActivity::Responding),
            "Responding",
        );
    }
    #[test]
    fn activity_label_auto_compacting() {
        use crate::acp::tracker::TurnActivity;
        assert_eq!(
            format_activity_label(&TurnActivity::AutoCompacting),
            "Compacting",
        );
    }
    #[test]
    fn activity_label_retrying() {
        use crate::acp::tracker::TurnActivity;
        assert_eq!(
            format_activity_label(&TurnActivity::Retrying {
                attempt: 2,
                max_retries: 5,
                reason: "rate limited".into(),
            }),
            "Retrying (2/5)",
        );
    }
    #[test]
    fn activity_label_waiting_reasons() {
        use crate::acp::tracker::{TurnActivity, WaitingReason};
        assert_eq!(
            format_activity_label(&TurnActivity::Waiting(WaitingReason::Subagent)),
            "Waiting on subagent…",
        );
        assert_eq!(
            format_activity_label(&TurnActivity::Waiting(WaitingReason::task_output())),
            "Waiting on task output…",
        );
        assert_eq!(
            format_activity_label(&TurnActivity::Waiting(WaitingReason::TaskOutput {
                task_ids: vec!["t1".into()],
                subject: Some("run tests".into()),
                waits: false,
            })),
            "run tests…",
        );
    }
    #[test]
    fn activity_label_tool_running_empty_title() {
        use crate::acp::tracker::TurnActivity;
        assert_eq!(
            format_activity_label(&TurnActivity::ToolRunning {
                title: String::new(),
                description: None
            }),
            "Running tool",
        );
    }
    #[test]
    fn activity_label_tool_running_short_title() {
        use crate::acp::tracker::TurnActivity;
        assert_eq!(
            format_activity_label(&TurnActivity::ToolRunning {
                title: "cargo build".into(),
                description: None
            }),
            "Running: cargo build",
        );
    }
    #[test]
    fn activity_label_tool_running_exactly_at_limit() {
        use crate::acp::tracker::TurnActivity;
        let title = "a".repeat(40);
        let result = format_activity_label(&TurnActivity::ToolRunning {
            title: title.clone(),
            description: None,
        });
        assert_eq!(result, format!("Running: {title}"));
        assert!(!result.contains('\u{2026}'), "no ellipsis at boundary");
    }
    #[test]
    fn activity_label_tool_running_truncates_long_title() {
        use crate::acp::tracker::TurnActivity;
        let title = "a".repeat(60);
        let result = format_activity_label(&TurnActivity::ToolRunning {
            title,
            description: None,
        });
        let expected_prefix = "Running: ".to_string() + "a".repeat(40).as_str();
        assert!(result.starts_with(&expected_prefix));
        assert!(result.ends_with('\u{2026}'), "truncated with ellipsis");
    }
    #[test]
    fn activity_label_tool_running_multibyte_under_char_limit() {
        use crate::acp::tracker::TurnActivity;
        let title: String = "\u{00e9}".repeat(35);
        assert!(title.len() > 40, "byte length exceeds threshold");
        assert!(title.chars().count() <= 40, "char count within limit");
        let result = format_activity_label(&TurnActivity::ToolRunning {
            title: title.clone(),
            description: None,
        });
        assert_eq!(result, format!("Running: {title}"));
        assert!(!result.contains('\u{2026}'), "no spurious ellipsis");
    }
    #[test]
    fn activity_label_tool_running_multibyte_over_char_limit() {
        use crate::acp::tracker::TurnActivity;
        let title: String = "\u{00e9}".repeat(45);
        let result = format_activity_label(&TurnActivity::ToolRunning {
            title,
            description: None,
        });
        assert!(result.ends_with('\u{2026}'), "truncated with ellipsis");
        let after_prefix = result.strip_prefix("Running: ").unwrap();
        let content_chars: Vec<char> = after_prefix.chars().collect();
        assert_eq!(content_chars.len(), 41);
    }
    #[test]
    fn activity_label_tool_running_multiline_uses_first_line() {
        use crate::acp::tracker::TurnActivity;
        let result = format_activity_label(&TurnActivity::ToolRunning {
            title: "first line\nsecond line".into(),
            description: None,
        });
        assert_eq!(result, "Running: first line");
    }
}
