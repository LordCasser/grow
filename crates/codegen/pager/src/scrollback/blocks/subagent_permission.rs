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

    /// Completes the `len`/`is_empty` pair.
    ///
    /// The block is always constructed with at least one member (`new` and
    /// `new_in_epoch` take the first event) and members only grow via `push`
    /// and `extend`, so `is_empty()` returns `false` for every value today.
    /// The method exists for the standard pair invariant
    /// `is_empty() == (len() == 0)` and as the clippy-required counterpart to
    /// the public `len()`.
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// Aggregated summary line for multi-member blocks, using the same
    /// verb-first syntax as tool verb-group headers ("Read 3 files"): each
    /// outcome renders as `{verb} {count} {noun}`, buckets appear in member
    /// first-appearance order, and a distinct-subagent suffix closes the line.
    fn header_line(&self, theme: &Theme) -> BlockLine {
        let mut buckets: Vec<(SubagentPermissionOutcome, usize)> = Vec::new();
        let mut children = HashSet::new();
        for event in &self.members {
            children.insert(event.child_session_id.as_str());
            match buckets
                .iter_mut()
                .find(|(outcome, _)| *outcome == event.outcome)
            {
                Some((_, count)) => *count += 1,
                None => buckets.push((event.outcome, 1)),
            }
        }

        let mut spans: Vec<Span<'static>> = Vec::new();
        for (i, (outcome, count)) in buckets.iter().enumerate() {
            let verb = match outcome {
                SubagentPermissionOutcome::Approved => "Approved",
                SubagentPermissionOutcome::Denied => "Denied",
                SubagentPermissionOutcome::TimedOut => "Timed out",
                SubagentPermissionOutcome::Unavailable => "Unavailable",
                SubagentPermissionOutcome::Cancelled => "Cancelled",
            };
            let outcome_style = if *outcome == SubagentPermissionOutcome::Approved {
                theme.fg(theme.accent_success)
            } else {
                theme.fg(theme.accent_error)
            };
            if i > 0 {
                spans.push(Span::styled(", ", theme.muted()));
            }
            spans.push(Span::styled(verb, outcome_style));
            let noun = if *count == 1 { "request" } else { "requests" };
            spans.push(Span::styled(
                format!(" {count} {noun}"),
                theme.fg(theme.gray_bright).add_modifier(Modifier::BOLD),
            ));
        }
        let subagent_noun = if children.len() == 1 {
            "subagent"
        } else {
            "subagents"
        };
        spans.push(Span::styled(
            format!(" · {} {subagent_noun}", children.len()),
            theme.muted(),
        ));
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
        false
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
        event_with_child(outcome, "019ff931-child")
    }

    fn event_with_child(
        outcome: SubagentPermissionOutcome,
        child_session_id: &str,
    ) -> SubagentPermissionEvent {
        SubagentPermissionEvent {
            child_session_id: child_session_id.into(),
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

    /// Permission blocks render compact like `SearchToolCallBlock`: no vpad
    /// rows, so a singleton occupies exactly one rendered row and the mouse
    /// hit-test row math is identity. Re-enabling vpad must fail here AND
    /// desync `permission_member_at_screen_row`.
    #[test]
    fn block_has_no_vertical_padding() {
        let appearance = AppearanceConfig::default();
        let single = SubagentPermissionBlock::new(event(SubagentPermissionOutcome::Approved));
        assert!(!single.has_vpad_for(&appearance));
        let mut multi = SubagentPermissionBlock::new(event(SubagentPermissionOutcome::Approved));
        multi.push(event(SubagentPermissionOutcome::Denied));
        assert!(!multi.has_vpad_for(&appearance));
    }

    /// `is_empty()` delegates to the same `members` field as `len()`, so the
    /// pair invariant `is_empty() == (len() == 0)` holds by construction for
    /// both single-member and multi-member blocks. The comparison is written
    /// as `len().eq(&0)`: the literal `len() == 0` would trip
    /// `clippy::len_zero`, whose suggested "fix" (`is_empty()`) would reduce
    /// the assertion to a tautology and stop exercising `len()`.
    #[test]
    fn is_empty_matches_len_for_single_and_multi_member_blocks() {
        let single = SubagentPermissionBlock::new(event(SubagentPermissionOutcome::Approved));
        assert_eq!(single.is_empty(), single.len().eq(&0));
        assert!(!single.is_empty());

        let mut multi = SubagentPermissionBlock::new(event(SubagentPermissionOutcome::Approved));
        multi.push(event(SubagentPermissionOutcome::Denied));
        assert_eq!(multi.is_empty(), multi.len().eq(&0));
        assert!(!multi.is_empty());
    }

    /// Plain text of the collapsed header row (multi-member blocks only).
    fn header_text(block: &SubagentPermissionBlock) -> String {
        block.output(&ctx(DisplayMode::Collapsed)).lines[0]
            .content
            .to_string()
    }

    #[test]
    fn header_aggregates_outcomes_in_first_appearance_order() {
        use SubagentPermissionOutcome::{Approved, Denied};
        let mut block = SubagentPermissionBlock::new(event_with_child(Denied, "child-a"));
        block.push(event_with_child(Approved, "child-b"));
        block.push(event_with_child(Denied, "child-a"));
        block.push(event_with_child(Denied, "child-b"));
        assert_eq!(
            header_text(&block),
            "Denied 3 requests, Approved 1 request · 2 subagents"
        );

        // Reverse member order reverses the buckets: order is member
        // first-appearance, not outcome kind.
        let mut block = SubagentPermissionBlock::new(event_with_child(Approved, "child-a"));
        block.push(event_with_child(Denied, "child-b"));
        block.push(event_with_child(Denied, "child-a"));
        assert_eq!(
            header_text(&block),
            "Approved 1 request, Denied 2 requests · 2 subagents"
        );
    }

    #[test]
    fn header_colors_verbs_by_outcome_and_counts_bold_gray() {
        use SubagentPermissionOutcome::{Approved, Denied};
        let theme = Theme::current();
        let mut block = SubagentPermissionBlock::new(event_with_child(Denied, "child-a"));
        block.push(event_with_child(Approved, "child-a"));
        let line = &block.output(&ctx(DisplayMode::Collapsed)).lines[0].content;
        // Spans: [Denied, " 1 request", ", ", Approved, " 1 request", " · 1 subagent"].
        assert_eq!(line.spans[0].content, "Denied");
        assert_eq!(line.spans[0].style.fg, Some(theme.accent_error));
        assert_eq!(line.spans[1].content, " 1 request");
        assert_eq!(line.spans[1].style.fg, Some(theme.gray_bright));
        assert!(line.spans[1].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(line.spans[2].content, ", ");
        assert_eq!(line.spans[2].style, theme.muted());
        assert_eq!(line.spans[3].content, "Approved");
        assert_eq!(line.spans[3].style.fg, Some(theme.accent_success));
        assert_eq!(line.spans[4].content, " 1 request");
        assert_eq!(line.spans[4].style.fg, Some(theme.gray_bright));
        assert!(line.spans[4].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(line.spans[5].content, " · 1 subagent");
        assert_eq!(line.spans[5].style, theme.muted());
    }

    #[test]
    fn header_uses_singular_nouns_for_single_counts() {
        use SubagentPermissionOutcome::{Approved, Denied};
        let mut block = SubagentPermissionBlock::new(event_with_child(Denied, "child-a"));
        block.push(event_with_child(Approved, "child-a"));
        assert_eq!(
            header_text(&block),
            "Denied 1 request, Approved 1 request · 1 subagent"
        );
    }

    #[test]
    fn header_all_denied_has_no_stray_comma_or_title() {
        use SubagentPermissionOutcome::Denied;
        let mut block = SubagentPermissionBlock::new(event_with_child(Denied, "child-a"));
        block.push(event_with_child(Denied, "child-a"));
        block.push(event_with_child(Denied, "child-b"));
        let text = header_text(&block);
        assert_eq!(text, "Denied 3 requests · 2 subagents");
        assert!(
            !text.contains(','),
            "single bucket must not emit separators: {text}"
        );
        assert!(
            !text.contains("◇"),
            "the old title must not be rendered: {text}"
        );
    }
}
