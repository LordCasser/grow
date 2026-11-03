//! Read-only session preview data and rendering.
//!
//! The overview displays session facts; input belongs to the session view.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::state::DashboardRowId;
use crate::app::agent_view::AgentView;
use crate::theme::Theme;

/// Args for painting a dense bottom-pinned live tail in the peek middle.
pub struct PeekLiveTailArgs<'a> {
    pub scrollback: &'a crate::scrollback::state::ScrollbackState,
}

/// Read-only projection of one session for the overview preview.
#[derive(Debug, Clone)]
pub struct PeekFields {
    pub label: String,
    pub time_ago: String,
    /// Type of the most recent agent response (e.g. `"Thinking"`,
    /// `"Response"`, `"Edit"`), shown as the header's left label.
    pub response_type: String,
    pub last_user_message: Option<String>,
    pub question: Option<String>,
    pub options: Vec<(String, String)>,
    pub request_id: Option<usize>,
    /// Stable AskUserQuestion interaction identity. The row identifies the
    /// source session; this identifies the concrete tool call within it.
    pub question_id: Option<String>,
    /// Index into `options` of the `RejectOnce` ("No") option, when the
    /// request has one. Selecting it lets the user type a free-text
    /// feedback message (mirrors the chat permission panel).
    pub reject_option: Option<usize>,
}

/// Read-only snapshot of the selected session; it owns no editor or answers.
#[derive(Debug, Clone)]
pub struct PeekPanelState {
    pub row: DashboardRowId,
    pub label: String,
    pub time_ago: String,
    pub response_type: String,
    pub last_user_message: Option<String>,
    pub question: Option<String>,
    pub options: Vec<(String, String)>,
    pub request_id: Option<usize>,
    pub question_id: Option<String>,
    pub reject_option: Option<usize>,
}
impl PeekPanelState {
    pub fn new(row: DashboardRowId, fields: PeekFields) -> Self {
        Self {
            row,
            label: fields.label,
            time_ago: fields.time_ago,
            response_type: fields.response_type,
            last_user_message: fields.last_user_message,
            question: fields.question,
            options: fields.options,
            request_id: fields.request_id,
            question_id: fields.question_id,
            reject_option: fields.reject_option,
        }
    }
    /// Returns whether the row or pending interaction identity changed.
    pub fn apply_fields(&mut self, row: DashboardRowId, fields: PeekFields) -> bool {
        let changed = self.row != row
            || self.request_id != fields.request_id
            || self.question_id != fields.question_id;
        *self = Self::new(row, fields);
        changed
    }
    pub fn is_ask_question(&self) -> bool {
        self.question.is_some() && self.question_id.is_some()
    }
}

/// Pending interaction facts for a session; answers remain in AgentView.
type PendingInteractionPeek = (
    Option<String>,
    Vec<(String, String)>,
    Option<usize>,
    Option<String>,
    Option<usize>,
);

fn pending_interaction_peek(
    agent: &AgentView,
    session_id: &str,
    include_local_question: bool,
    questions_enabled: bool,
) -> PendingInteractionPeek {
    use crate::views::session_title::sanitize_display_text;

    if let Some(p) = agent
        .permission_queue
        .iter()
        .find(|p| p.request.request.session_id.0.as_ref() == session_id)
    {
        let mut q = sanitize_display_text(&p.title).into_owned();
        let detail = p
            .description
            .iter()
            .map(|line| sanitize_display_text(line).into_owned())
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        if !detail.is_empty() {
            q.push_str(" — ");
            q.push_str(&detail);
        }
        let selected_words: Option<String> = p
            .bash_highlights
            .as_ref()
            .filter(|_| p.bash_selection_count > 0)
            .map(|h| h.highlighted_words[..p.bash_selection_count].join(" "));
        let options = p
            .options
            .iter()
            .map(|option| {
                let name = crate::views::permission_view::option_label_for_selection(
                    option,
                    selected_words.as_deref(),
                    p.mcp_scope.as_ref(),
                );
                (
                    option.option_id.0.to_string(),
                    sanitize_display_text(&name).into_owned(),
                )
            })
            .collect::<Vec<_>>();
        let reject = p.options.iter().position(|option| {
            option.kind == agent_client_protocol::schema::v1::PermissionOptionKind::RejectOnce
        });
        return (Some(q), options, Some(p.id), None, reject);
    }

    if !questions_enabled {
        // Subagent rows: the peek never surfaces child questions. The
        // single answering entry is the child fullscreen view, so the
        // panel falls through to the plain reply box.
        return (None, Vec::new(), None, None, None);
    }

    let Some((qv, question)) = agent.question_view.as_ref().and_then(|qv| {
        let belongs_to_session = qv.source_session_id.as_deref() == Some(session_id)
            || (include_local_question && qv.source_session_id.is_none());
        let supported = belongs_to_session
            && qv.local_kind.is_none()
            && !qv.questions.is_empty()
            && qv
                .questions
                .iter()
                .all(|question| !question.multi_select.unwrap_or(false));
        supported
            .then(|| {
                qv.questions
                    .get(qv.active_tab)
                    .map(|question| (qv, question))
            })
            .flatten()
    }) else {
        return (None, Vec::new(), None, None, None);
    };
    let mut options: Vec<(String, String)> = question
        .options
        .iter()
        .map(|option| {
            let label = sanitize_display_text(&option.label).into_owned();
            (label.clone(), label)
        })
        .collect();
    let reject = if qv.no_freeform {
        None
    } else {
        options.push(("__other__".to_string(), "Other".to_string()));
        Some(options.len() - 1)
    };
    let n = qv.questions.len();
    let text = if n > 1 {
        format!("({}/{}) {}", qv.active_tab + 1, n, question.question)
    } else {
        question.question.clone()
    };
    (
        Some(sanitize_display_text(&text).into_owned()),
        options,
        None,
        Some(qv.tool_call_id.clone()),
        reject,
    )
}

/// Compute the live display fields for a dashboard row.
///
/// Returns `None` when the row's owning agent (or subagent) no longer
/// exists, signalling the caller to close the peek. Extracted from the
/// dashboard dispatcher so both the initial open and the per-frame
/// refresh share one source of truth.
///
pub fn compute_peek_fields(
    row: &DashboardRowId,
    agents: &indexmap::IndexMap<crate::app::session::AgentId, AgentView>,
    now: std::time::Instant,
) -> Option<PeekFields> {
    use crate::views::session_title::{entry_title, sanitize_display_text};
    match row {
        DashboardRowId::TopLevel(id) => {
            let agent = agents.get(id)?;
            let label = sanitize_display_text(&entry_title(agent)).into_owned();
            let response_type = extract_last_response_type(agent);
            let last_user_message = extract_last_user_message(agent);
            let time_ago = agent
                .session
                .last_active_at
                .map(|t| crate::util::format_time_ago(now.saturating_duration_since(t)))
                .unwrap_or_default();
            let root_session_id = agent.session.session_id.as_ref()?.0.as_ref();
            let (question, options, request_id, question_id, reject_option) =
                pending_interaction_peek(agent, root_session_id, true, true);
            Some(PeekFields {
                label,
                time_ago,
                response_type,
                last_user_message,
                question,
                options,
                request_id,
                question_id,
                reject_option,
            })
        }
        DashboardRowId::Subagent {
            parent,
            child_session_id,
        } => {
            let parent_agent = agents.get(parent)?;
            let info = parent_agent
                .session
                .subagent_sessions
                .get(child_session_id)?;
            let label = {
                let (l, _) = crate::app::subagent::format_subagent_label(info);
                sanitize_display_text(&l).into_owned()
            };
            // `subagent_views` holds `Box<AgentView>`; the closures let
            // deref coercion turn `&Box<AgentView>` into `&AgentView`.
            let child = parent_agent.subagent_views.get(child_session_id);
            let response_type = child
                .map(|c| extract_last_response_type(c))
                .unwrap_or_else(|| "Subagent".to_string());
            let last_user_message = child.and_then(|c| extract_last_user_message(c));
            let time_ago =
                crate::util::format_time_ago(now.saturating_duration_since(info.last_progress_at));
            let (question, options, request_id, question_id, reject_option) = child
                .map(|child| pending_interaction_peek(child, child_session_id, false, false))
                .unwrap_or_else(|| (None, Vec::new(), None, None, None));
            Some(PeekFields {
                label,
                time_ago,
                response_type,
                last_user_message,
                question,
                options,
                request_id,
                question_id,
                reject_option,
            })
        }
        // Roster-only rows are not locally hosted — there is no local
        // `AgentView` to peek into.
        DashboardRowId::Roster { .. } => None,
    }
}

/// Paint a preview without an editor, answer controls or caret.
pub fn render_readonly_peek(
    buf: &mut Buffer,
    area: Rect,
    panel: &PeekPanelState,
    theme: &Theme,
    live_tail: Option<PeekLiveTailArgs<'_>>,
) {
    use ratatui::widgets::{Block, BorderType, Borders, Widget};
    if area.height < 3 || area.width < 4 {
        return;
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.prompt_border).bg(theme.bg_base));
    let inner = block.inner(area);
    block.render(area, buf);
    let title = format!("Preview · {} · read-only", panel.label);
    buf.set_stringn(
        inner.x,
        inner.y,
        title,
        inner.width as usize,
        Style::default().fg(theme.text_secondary).bg(theme.bg_base),
    );
    let body = Rect::new(
        inner.x,
        inner.y.saturating_add(1),
        inner.width,
        inner.height.saturating_sub(1),
    );
    if let Some(question) = panel.question.as_deref() {
        let text = format!(
            "Pending: {} · Enter to open and handle",
            question.lines().next().unwrap_or(question)
        );
        if body.height > 0 {
            buf.set_stringn(
                body.x,
                body.y,
                text,
                body.width as usize,
                Style::default().fg(theme.warning).bg(theme.bg_base),
            );
        }
    } else if let Some(tail) = live_tail {
        super::peek_tail::paint_peek_live_tail(tail.scrollback, body, buf);
    } else if body.height > 0 {
        buf.set_stringn(
            body.x,
            body.y,
            "Enter to open session",
            body.width as usize,
            Style::default().fg(theme.gray_dim).bg(theme.bg_base),
        );
    }
}
/// The header label for the peek panel, e.g. `"Thinking"` / `"Thought"`,
/// `"Response"`, `"Edit"`, `"Read"`, `"Bash"`, `"Working"`, …
///
/// While the turn is RUNNING the label follows the live turn activity
/// (`Thinking` / `Responding` / a running tool / `Working` when waiting),
/// mirroring the agent view's turn-status line so the peek never dwells on a
/// stale completed response while the agent has actually moved on.
///
/// When IDLE it scans the scrollback newest-first and returns a label for the
/// first agent-produced block (a `Thinking` block reads `"Thought"` once
/// done). Scanning stops at the user's latest prompt / interjection (anything
/// before it belongs to a previous turn), falling back to `"Idle"`.
pub fn extract_last_response_type(agent: &AgentView) -> String {
    use crate::scrollback::block::RenderBlock;
    use crate::scrollback::blocks::ToolCallBlock;

    use crate::acp::tracker::TurnActivity;

    let running = !agent.session.state.is_idle();
    // While the turn is running, the live activity is the ground truth for
    // what the agent is doing RIGHT NOW — mirrors the agent view's turn-status
    // line. Driving the status from this (instead of only the scrollback scan)
    // is what keeps the peek from dwelling on the previous, now-stale
    // "Response" once the agent has moved past its last message into tool
    // execution or waiting for results.
    if running {
        match agent.resolve_turn_activity() {
            Some(TurnActivity::Thinking) => return "Thinking".to_string(),
            Some(TurnActivity::Responding) => return "Response".to_string(),
            Some(TurnActivity::AutoCompacting) => return "Compacting".to_string(),
            Some(TurnActivity::Retrying { .. }) => return "Retrying".to_string(),
            // A tool is executing: fall through to the scan to recover its
            // specific label (Bash/Read/…); a missing/stale block yields the
            // generic "Working" fallback below.
            Some(TurnActivity::ToolRunning { .. }) => {}
            // Blocked on a suppressed tool (task output / wait / sleep) → keep
            // the compact "Working" the peek showed before this was surfaced.
            Some(TurnActivity::Waiting(_)) => return "Working".to_string(),
            // Turn running but no live activity (e.g. just granted a
            // permission and waiting for tool results / the next inference) →
            // "Working", never a stale response.
            None => return "Working".to_string(),
        }
    }
    let len = agent.scrollback.len();
    for idx in (0..len).rev() {
        let Some(entry) = agent.scrollback.entry(idx) else {
            break;
        };
        match &entry.block {
            RenderBlock::AgentMessage(_) => {
                // Idle → the last message is the result ("Response"). While
                // running we only reach here in the ToolRunning case (other
                // activities returned above): a message is then stale, so skip
                // it and fall through to "Working" / a newer tool label.
                if running {
                    break;
                }
                return "Response".to_string();
            }
            RenderBlock::Thinking(_) => {
                return if running { "Thinking" } else { "Thought" }.to_string();
            }
            RenderBlock::ToolCall(tc) => {
                let label = match tc {
                    ToolCallBlock::Execute(_) => Some("Bash"),
                    ToolCallBlock::Read(_) => Some("Read"),
                    ToolCallBlock::Edit(_) => Some("Edit"),
                    ToolCallBlock::ListDir(_) => Some("List"),
                    ToolCallBlock::Search(_) => Some("Search"),
                    ToolCallBlock::WebFetch(_) => Some("Fetch"),
                    ToolCallBlock::IntegrationSearch(_) => Some("Tool search"),
                    ToolCallBlock::UseTool(_) => Some("Tool"),
                    ToolCallBlock::MemorySearch(_) => Some("Memory"),
                    ToolCallBlock::Skill(_) => Some("Skill"),
                    ToolCallBlock::Other(_) => Some("Tool"),
                    // Lifecycle events aren't real tool calls — keep scanning.
                    ToolCallBlock::Lifecycle(_) => None,
                };
                if let Some(label) = label {
                    return label.to_string();
                }
            }
            RenderBlock::Subagent(_) => return "Subagent".to_string(),
            RenderBlock::Workflow(_) => return "Workflow".to_string(),
            RenderBlock::BgTask(_) => return "Task".to_string(),
            RenderBlock::Btw(_) => return "Btw".to_string(),
            RenderBlock::ContextInfo(_) => return "Context".to_string(),
            RenderBlock::ImageRecovery(_) => return "Image".to_string(),
            // The user's latest input marks the turn boundary — there's
            // no agent response after it yet.
            RenderBlock::UserPrompt(_) => break,
            // Structural blocks carry no response type — keep scanning.
            RenderBlock::Notice(_)
            | RenderBlock::SessionEvent(_)
            | RenderBlock::SubagentPermission(_)
            | RenderBlock::Stub(_) => {}
        }
    }
    if running {
        "Working".to_string()
    } else {
        "Idle".to_string()
    }
}

/// Pull the first line of the most recent user prompt
/// (`RenderBlock::UserPrompt`) from the
/// agent's scrollback. Sanitised + ANSI-stripped. Returns
/// `None` when the user hasn't sent any prompts yet.
pub fn extract_last_user_message(agent: &AgentView) -> Option<String> {
    crate::views::session_title::last_user_prompt_line(agent)
}

/// Pull the first line of the FIRST user prompt (`RenderBlock::UserPrompt`)
/// from the agent's scrollback, oldest-first. Sanitised + ANSI-stripped.
///
/// Used as a dashboard row title fallback: once a dashboard-dispatched
/// prompt drains out of `pending_prompts` it lives in the scrollback, so
/// this keeps showing the task as the title instead of flashing the
/// session-id fallback while the generated title is still being produced.
/// Returns `None` when the user hasn't sent any prompt yet.
pub fn extract_first_user_message(agent: &AgentView) -> Option<String> {
    use crate::scrollback::block::RenderBlock;
    use crate::views::session_title::sanitize_display_text;
    let len = agent.scrollback.len();
    for idx in 0..len {
        let entry = agent.scrollback.entry(idx)?;
        if let RenderBlock::UserPrompt(b) = &entry.block {
            let first = b.text.lines().next().unwrap_or("").trim();
            if first.is_empty() {
                continue;
            }
            let stripped = strip_ansi_escapes::strip_str(first);
            let safe = sanitize_display_text(&stripped).into_owned();
            return Some(safe.trim().to_string());
        }
    }
    None
}

/// Extract the last `count` short text descriptions from the given
/// agent view's scrollback.
///
/// No more `format!("{:?}", entry.block)`, which leaked
/// Rust Debug output (variant tags, struct field names, escaped
/// strings — and worst of all the head of bash commands containing
/// credentials).
///
/// Every projected string is run through
/// `strip_ansi_escapes::strip_str` so embedded `\x1b[...]` sequences
/// from agent output cannot reach the buffer.
///
/// Every projected string is also run
/// through `sanitize_display_text` so a maliciously crafted block
/// can't smuggle terminal escapes via this path.
///
/// The pipeline is project → first line → ANSI-strip
/// → sanitise. Splitting BEFORE sanitisation matters because
/// `sanitize_display_text` rewrites `\n` to U+FFFD (it's a control
/// character), so a `.lines().next()` AFTER sanitisation returns the
/// whole concatenated body. Doing the split first also avoids
/// allocating the entire body when only the first line is needed.
///
/// Returns newest-last (top to bottom = chronological).
pub fn extract_recent_lines(agent: &AgentView, count: usize) -> Vec<String> {
    let mut out = Vec::new();
    if count == 0 {
        return out;
    }
    let total = agent.scrollback.len();
    let mut i = total;
    while i > 0 && out.len() < count {
        i -= 1;
        if let Some(entry) = agent.scrollback.entry(i)
            && let Some(text) = block_short_text(&entry.block)
        {
            // Project → first line → ANSI-strip → sanitize → trim.
            let head = text.lines().next().unwrap_or("");
            let stripped = strip_ansi_escapes::strip_str(head);
            let safe = crate::views::session_title::sanitize_display_text(&stripped).into_owned();
            let first = safe.trim();
            if !first.is_empty() {
                out.push(first.to_string());
            }
        }
    }
    out.reverse();
    out
}

/// Project a `RenderBlock` to a short, user-friendly description for
/// the peek panel. Returns `None` for blocks that have no obvious
/// short text projection (we'd rather omit a row than render Rust
/// metadata).
/// For variants that own large bodies (`AgentMessage`,
/// `Thinking`), project to the first line *here* so we never allocate
/// the full body for the peek panel. UserPrompt is already first-line
/// in `b.text` because the on-disk schema collapses multi-line prompts
/// to single-line. The other variants return short fixed labels.
fn block_short_text(block: &crate::scrollback::block::RenderBlock) -> Option<String> {
    use crate::scrollback::block::RenderBlock;
    /// Read just the first non-empty line of a body owned elsewhere.
    /// `lines()` iterates without allocating; the `.to_string()` at the
    /// end is the only allocation, sized to the first line only.
    fn first_line_of(body: &str) -> String {
        body.lines().next().unwrap_or("").to_string()
    }
    match block {
        RenderBlock::UserPrompt(b) => Some(format!("\u{2771} {}", first_line_of(&b.text))),
        RenderBlock::AgentMessage(b) => Some(first_line_of(&b.text())),
        RenderBlock::Thinking(b) => Some(format!("(thinking) {}", first_line_of(&b.text()))),
        RenderBlock::Notice(_) => Some("(system event)".to_string()),
        RenderBlock::SessionEvent(_) => Some("(session event)".to_string()),
        RenderBlock::ToolCall(_) => Some("(tool call)".to_string()),
        RenderBlock::BgTask(_) => Some("(background task)".to_string()),
        RenderBlock::Subagent(_) => Some("(subagent)".to_string()),
        RenderBlock::SubagentPermission(_) => Some("(subagent permission)".to_string()),
        RenderBlock::Workflow(_) => Some("(workflow)".to_string()),
        RenderBlock::Btw(_) => Some("(btw)".to_string()),
        RenderBlock::ContextInfo(_) => Some("(context info)".to_string()),
        RenderBlock::ImageRecovery(_) => Some("(image recovery)".to_string()),
        RenderBlock::Stub(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::session::AgentId;

    /// Build a `PeekFields` for tests with sensible defaults.
    fn fields(response_type: &str) -> PeekFields {
        PeekFields {
            label: "label".to_string(),
            time_ago: "2m".to_string(),
            response_type: response_type.to_string(),
            last_user_message: None,
            question: None,
            options: Vec::new(),
            request_id: None,
            question_id: None,
            reject_option: None,
        }
    }

    #[test]
    fn peek_handles_missing_question() {
        let mut f = fields("Idle");
        f.last_user_message = Some("hello?".to_string());
        let state = PeekPanelState::new(DashboardRowId::TopLevel(AgentId(0)), f);
        assert!(state.question.is_none());
        assert_eq!(state.last_user_message.as_deref(), Some("hello?"));
    }

    /// `apply_fields` reports whether the row or pending interaction changed
    /// so the render-time refresh can clear the dashboard reply draft.
    #[test]
    fn apply_fields_reports_row_change() {
        let mut state = PeekPanelState::new(DashboardRowId::TopLevel(AgentId(0)), fields("Idle"));
        // Same row → no change reported (caller preserves the draft).
        let changed = state.apply_fields(
            DashboardRowId::TopLevel(AgentId(0)),
            fields("Running\u{2026}"),
        );
        assert!(!changed, "same row must not report a change");
        assert_eq!(state.response_type, "Running\u{2026}");
        // Different row → change reported (caller clears the draft).
        let changed = state.apply_fields(DashboardRowId::TopLevel(AgentId(1)), fields("Idle"));
        assert!(changed, "row change must be reported");
        assert_eq!(state.row, DashboardRowId::TopLevel(AgentId(1)));
    }

    #[test]
    fn apply_fields_reports_same_row_question_replacement() {
        let mut initial = fields("Awaiting your input");
        initial.question = Some("First?".into());
        initial.question_id = Some("ask-1".into());
        let mut state = PeekPanelState::new(DashboardRowId::TopLevel(AgentId(0)), initial);

        let mut replacement = fields("Awaiting your input");
        replacement.question = Some("Second?".into());
        replacement.question_id = Some("ask-2".into());
        let changed = state.apply_fields(DashboardRowId::TopLevel(AgentId(0)), replacement);

        assert!(
            changed,
            "new tool call on the same row must clear its draft"
        );
        assert_eq!(state.question_id.as_deref(), Some("ask-2"));
    }

    /// Single-entry question answering: a child question parked on the
    /// subagent's own view must NOT surface in the subagent peek (the
    /// answering entry is the child fullscreen view), while the same
    /// agent's root question still surfaces on the `TopLevel` peek.
    #[test]
    fn subagent_peek_suppresses_questions_but_root_peek_keeps_them() {
        use crate::views::question_view::QuestionViewState;
        use tools::implementations::grow_build::ask_user_question::{
            AskUserQuestionMode, Question, QuestionOption,
        };

        let single_question = |text: &str| Question {
            question: text.to_string(),
            options: vec![QuestionOption {
                label: "Yes".to_string(),
                description: String::new(),
                preview: None,
                id: None,
            }],
            multi_select: Some(false),
            id: None,
        };

        let mut parent = crate::app::agent_view::test_agent_view(Some("root-sess"), "/tmp".into());
        let mut child = crate::app::agent_view::test_agent_view(Some("child-sess"), "/tmp".into());
        let child_stashed_prompt = child.prompt.stash();
        child.replace_question_view(Some(QuestionViewState::with_response_tx(
            Some("child-sess".into()),
            "tc-child".into(),
            vec![single_question("Child choice?")],
            child_stashed_prompt,
            None,
            AskUserQuestionMode::Default,
        )));
        parent
            .subagent_views
            .insert("child-sess".into(), Box::new(child));
        parent.session.subagent_sessions.insert(
            "child-sess".into(),
            crate::app::agent_view::test_fixtures::running_subagent_info("child-sess"),
        );

        let mut agents = indexmap::IndexMap::new();
        agents.insert(AgentId(0), parent);
        let now = std::time::Instant::now();

        let sub_fields = compute_peek_fields(
            &DashboardRowId::Subagent {
                parent: AgentId(0),
                child_session_id: "child-sess".into(),
            },
            &agents,
            now,
        )
        .expect("subagent row must be peekable");
        assert_eq!(
            sub_fields.question_id, None,
            "child questions must not surface in the subagent peek"
        );
        assert!(
            sub_fields.options.is_empty(),
            "subagent peek must degrade to the plain reply box, got: {:?}",
            sub_fields.options
        );
        assert!(sub_fields.question.is_none());
        assert!(sub_fields.request_id.is_none());

        // Regression guard: the root's own question (local, no source
        // session) still surfaces on the TopLevel peek for the SAME agent.
        let root_stashed_prompt = agents.get_mut(&AgentId(0)).unwrap().prompt.stash();
        agents
            .get_mut(&AgentId(0))
            .unwrap()
            .replace_question_view(Some(QuestionViewState::with_response_tx(
                None,
                "tc-root".into(),
                vec![single_question("Root choice?")],
                root_stashed_prompt,
                None,
                AskUserQuestionMode::Default,
            )));
        let top_fields = compute_peek_fields(&DashboardRowId::TopLevel(AgentId(0)), &agents, now)
            .expect("root row must be peekable");
        assert_eq!(top_fields.question_id.as_deref(), Some("tc-root"));
        assert!(
            !top_fields.options.is_empty(),
            "root questions must keep surfacing in the top-level peek"
        );
        assert!(top_fields.question.is_some());
    }

    // Real `extract_recent_lines` coverage lives in dispatch.rs tests
    // (`extract_recent_lines_*`) because it needs a real `AgentView`
    // built via `test_app_with_agent()`.
}
