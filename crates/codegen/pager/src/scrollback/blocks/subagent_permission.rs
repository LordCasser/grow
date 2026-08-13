//! Turn-scoped, UI-only audit group for subagent permission decisions.
//!
//! The group is a stable scrollback entity rather than a fold inferred from
//! adjacent transcript rows. Subagent status and tool updates can therefore
//! pass through without splitting it, replay reconstructs the same shape, and
//! minimal mode can keep the still-open group mutable until the primary turn
//! completes. Individual structured records remain available for the detail
//! modal and never enter model conversation state.

use std::collections::HashSet;

use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use crate::appearance::AppearanceConfig;
use crate::scrollback::block::{BlockContent, join_searchable};
use crate::scrollback::types::{AccentStyle, BlockContext, BlockLine, BlockOutput, DisplayMode};
use crate::theme::Theme;
use shell::extensions::notification::SubagentPermissionOutcome;

#[derive(Debug, Clone)]
pub struct SubagentPermissionEvent {
    pub child_session_id: String,
    /// Canonical title rendered by the Subagents pane for this live child.
    pub subagent_title: Option<String>,
    pub subagent_type: Option<String>,
    pub description: Option<String>,
    pub tool_call_id: String,
    pub tool_name: String,
    pub access_kind: String,
    pub access_summary: Option<String>,
    pub access_detail: Option<String>,
    pub outcome: SubagentPermissionOutcome,
    pub source: String,
    pub reason: Option<String>,
    pub classifier_reason: Option<String>,
    pub latency_ms: Option<u64>,
}

impl SubagentPermissionEvent {
    pub fn child_label(&self) -> String {
        if let Some(title) = self
            .subagent_title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
        {
            return title.to_owned();
        }
        match (
            self.subagent_type.as_deref().map(str::trim),
            self.description.as_deref().map(str::trim),
        ) {
            (Some(kind), Some(description)) if !kind.is_empty() && !description.is_empty() => {
                format!("{kind} {description}")
            }
            (Some(kind), _) if !kind.is_empty() => kind.to_owned(),
            (_, Some(description)) if !description.is_empty() => description.to_owned(),
            _ => "subagent".to_owned(),
        }
    }

    pub const fn outcome_label(&self) -> &'static str {
        match self.outcome {
            SubagentPermissionOutcome::Approved => "approved",
            SubagentPermissionOutcome::Denied => "denied",
            SubagentPermissionOutcome::TimedOut => "timed out → denied",
            SubagentPermissionOutcome::Unavailable => "unavailable → denied",
            SubagentPermissionOutcome::Cancelled => "cancelled",
        }
    }

    pub const fn is_approved(&self) -> bool {
        matches!(self.outcome, SubagentPermissionOutcome::Approved)
    }

    pub fn compact_text(&self) -> String {
        let access = self
            .access_summary
            .as_deref()
            .map(|summary| format!("{} [{}: {summary}]", self.tool_name, self.access_kind))
            .unwrap_or_else(|| format!("{} [{}]", self.tool_name, self.access_kind));
        format!(
            "Subagent permission · {} · {} · {access}",
            self.child_label(),
            self.outcome_label(),
        )
    }

    pub fn detail_title(&self) -> String {
        format!("Subagent permission · {}", self.child_label())
    }

    pub fn detail_text(&self) -> String {
        let mut fields = vec![
            format!("Subagent: {}", self.child_label()),
            format!("Child session: {}", self.child_session_id),
        ];
        if let Some(subagent_type) = self.subagent_type.as_deref() {
            fields.push(format!("Subagent type: {subagent_type}"));
        }
        if let Some(description) = self.description.as_deref() {
            fields.push(format!("Description: {description}"));
        }
        fields.extend([
            format!("Tool: {}", self.tool_name),
            format!("Tool call: {}", self.tool_call_id),
            format!("Access kind: {}", self.access_kind),
        ]);
        if let Some(detail) = self.access_detail.as_deref() {
            fields.push(format!("Access request:\n{detail}"));
        } else if let Some(summary) = self.access_summary.as_deref() {
            fields.push(format!("Access summary (replay-safe): {summary}"));
        }
        fields.extend([
            format!("Outcome: {}", self.outcome_label()),
            format!("Source: {}", self.source),
        ]);
        if let Some(reason) = self.reason.as_deref() {
            fields.push(format!("Reason: {reason}"));
        }
        if let Some(reason) = self.classifier_reason.as_deref() {
            fields.push(format!("Classifier reason:\n{reason}"));
        }
        if let Some(latency_ms) = self.latency_ms {
            fields.push(format!("Judgment latency: {latency_ms} ms"));
        }
        fields.join("\n")
    }

    fn compact_line(&self, theme: &Theme) -> BlockLine {
        let outcome_style = if self.is_approved() {
            theme.fg(theme.accent_success)
        } else {
            theme.fg(theme.accent_error)
        };
        let prefix = format!("Subagent permission · {} · ", self.child_label());
        let access = self
            .access_summary
            .as_deref()
            .map(|summary| format!(" · {} [{}: {summary}]", self.tool_name, self.access_kind))
            .unwrap_or_else(|| format!(" · {} [{}]", self.tool_name, self.access_kind));
        BlockLine::styled(Line::from(vec![
            Span::styled(prefix, theme.muted()),
            Span::styled(self.outcome_label(), outcome_style),
            Span::styled(access, theme.muted()),
        ]))
        .with_selection_range(Some(0))
    }
}

#[derive(Debug, Clone)]
pub struct SubagentPermissionBlock {
    epoch: u64,
    members: Vec<SubagentPermissionEvent>,
}

impl SubagentPermissionBlock {
    pub fn new(first: SubagentPermissionEvent) -> Self {
        Self::new_in_epoch(first, 0)
    }

    pub(crate) fn new_in_epoch(first: SubagentPermissionEvent, epoch: u64) -> Self {
        Self {
            epoch,
            members: vec![first],
        }
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn push(&mut self, event: SubagentPermissionEvent) {
        self.members.push(event);
    }

    pub fn extend(&mut self, events: impl IntoIterator<Item = SubagentPermissionEvent>) {
        self.members.extend(events);
    }

    pub fn members(&self) -> &[SubagentPermissionEvent] {
        &self.members
    }

    pub fn member(&self, index: usize) -> Option<&SubagentPermissionEvent> {
        self.members.get(index)
    }

    pub fn take_members(self) -> Vec<SubagentPermissionEvent> {
        self.members
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    fn header_line(&self, theme: &Theme) -> BlockLine {
        let mut approved = 0usize;
        let mut denied = 0usize;
        let mut timed_out = 0usize;
        let mut unavailable = 0usize;
        let mut cancelled = 0usize;
        let mut children = HashSet::new();
        for event in &self.members {
            children.insert(event.child_session_id.as_str());
            match event.outcome {
                SubagentPermissionOutcome::Approved => approved += 1,
                SubagentPermissionOutcome::Denied => denied += 1,
                SubagentPermissionOutcome::TimedOut => timed_out += 1,
                SubagentPermissionOutcome::Unavailable => unavailable += 1,
                SubagentPermissionOutcome::Cancelled => cancelled += 1,
            }
        }

        let title_style = theme.fg(theme.gray_bright).add_modifier(Modifier::BOLD);
        let mut spans = vec![Span::styled("◇ Subagent permissions", title_style)];
        let mut push_count = |count: usize, label: &str, style| {
            if count > 0 {
                spans.push(Span::styled(format!(" · {count} {label}"), style));
            }
        };
        push_count(approved, "approved", theme.fg(theme.accent_success));
        push_count(denied, "denied", theme.fg(theme.accent_error));
        push_count(timed_out, "timed out", theme.fg(theme.accent_error));
        push_count(unavailable, "unavailable", theme.fg(theme.accent_error));
        push_count(cancelled, "cancelled", theme.fg(theme.accent_error));
        push_count(
            children.len(),
            if children.len() == 1 {
                "subagent"
            } else {
                "subagents"
            },
            theme.muted(),
        );
        BlockLine::styled(Line::from(spans)).with_selection_range(Some(0))
    }

    pub fn searchable_text(&self) -> Option<String> {
        join_searchable(self.members.iter().flat_map(|event| {
            [
                Some(event.compact_text()),
                Some(event.child_session_id.clone()),
                event.description.clone(),
                Some(event.tool_call_id.clone()),
                Some(event.source.clone()),
                event.reason.clone(),
                event.latency_ms.map(|ms| format!("{ms} ms")),
            ]
        }))
    }
}

impl BlockContent for SubagentPermissionBlock {
    fn output(&self, ctx: &BlockContext) -> BlockOutput {
        let theme = Theme::current();
        if self.members.len() == 1 {
            return BlockOutput {
                lines: vec![self.members[0].compact_line(&theme)],
            };
        }
        let mut lines = vec![self.header_line(&theme)];
        if ctx.mode == DisplayMode::Expanded {
            lines.extend(self.members.iter().map(|event| event.compact_line(&theme)));
        }
        BlockOutput { lines }
    }

    fn accent(&self, _ctx: &BlockContext) -> Option<AccentStyle> {
        None
    }

    fn has_vpad_for(&self, _appearance: &AppearanceConfig) -> bool {
        true
    }

    fn is_foldable(&self) -> bool {
        self.members.len() > 1
    }

    fn default_display_mode(&self) -> DisplayMode {
        DisplayMode::Collapsed
    }

    fn is_groupable(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(outcome: SubagentPermissionOutcome) -> SubagentPermissionEvent {
        SubagentPermissionEvent {
            child_session_id: "019ff931-child".into(),
            subagent_title: Some("Software-engineering/coder W4-C: Approval 加固".into()),
            subagent_type: Some("software-engineering/coder".into()),
            description: Some("edit docs".into()),
            tool_call_id: "tool-7".into(),
            tool_name: "search_replace".into(),
            access_kind: "edit".into(),
            access_summary: Some("/workspace/docs/03-LM.md".into()),
            access_detail: Some("/workspace/docs/03-LM.md".into()),
            outcome,
            source: "main_agent".into(),
            reason: Some("within task scope".into()),
            classifier_reason: Some("The edit is required by the task.".into()),
            latency_ms: Some(3727),
        }
    }

    fn ctx(mode: DisplayMode) -> BlockContext {
        BlockContext {
            mode,
            is_running: false,
            width: 80,
            raw: false,
            max_lines: None,
            appearance: AppearanceConfig::default(),
            is_selected: false,
            cwd: None,
        }
    }

    #[test]
    fn compact_row_omits_verbose_audit_fields() {
        let event = event(SubagentPermissionOutcome::Approved);
        let text = event.compact_text();
        assert!(text.contains("Software-engineering/coder W4-C: Approval 加固"));
        assert!(!text.contains("019ff931"));
        assert!(text.contains("approved · search_replace [edit:"));
        assert!(!text.contains("within task scope"));
        assert!(!text.contains("3727"));
    }

    #[test]
    fn detail_contains_the_complete_audit_record() {
        let mut permission = event(SubagentPermissionOutcome::Unavailable);
        let full_request = "cd /workspace && TOKEN=visible-only-in-live-modal cargo test \
            --features a,b,c --package shell --test deliberately_long_permission_request";
        let full_reason = "The complete classifier explanation is preserved for the live modal, \
            including its final sentence and punctuation.";
        permission.access_detail = Some(full_request.into());
        permission.classifier_reason = Some(full_reason.into());
        let detail = permission.detail_text();
        assert!(detail.contains("Child session: 019ff931-child"));
        assert!(detail.contains(&format!("Access request:\n{full_request}")));
        assert!(detail.contains("Outcome: unavailable → denied"));
        assert!(detail.contains("Reason: within task scope"));
        assert!(detail.contains(&format!("Classifier reason:\n{full_reason}")));
        assert!(detail.contains("Judgment latency: 3727 ms"));
        assert!(!detail.contains("truncated"));
        assert!(!detail.contains("Access summary"));
    }

    #[test]
    fn replay_detail_identifies_the_safe_summary() {
        let mut permission = event(SubagentPermissionOutcome::Approved);
        permission.access_detail = None;
        permission.classifier_reason = None;
        let detail = permission.detail_text();
        assert!(detail.contains("Access summary (replay-safe): /workspace/docs/03-LM.md"));
    }

    #[test]
    fn group_is_one_line_collapsed_and_one_line_per_member_expanded() {
        let mut block = SubagentPermissionBlock::new(event(SubagentPermissionOutcome::Approved));
        assert_eq!(block.output(&ctx(DisplayMode::Collapsed)).lines.len(), 1);
        block.push(event(SubagentPermissionOutcome::Denied));
        assert_eq!(block.output(&ctx(DisplayMode::Collapsed)).lines.len(), 1);
        assert_eq!(block.output(&ctx(DisplayMode::Expanded)).lines.len(), 3);
    }
}
