//! Derived group model for the scrollback's view-time folds.
//!
//! One scan owns every grouping decision: verb-group runs claim their entries
//! first, then group truncation ("N more") runs over the rest. The
//! scan produces [`GroupSpan`]s — the authoritative description of every
//! fold — and [`project_to_layout`] is the single writer that turns spans
//! into the per-entry `EntryLayoutInfo` flags the renderer and navigation
//! consume. Keeping the decision (scan) and the flag writes (projection)
//! in one module means a consumer can never observe a fold shape the model
//! doesn't describe.
//!
//! The spans are stored on the layout cache (see `LayoutCache::groups`) and
//! rebuilt whenever the folds are re-applied. Like the per-entry flags, they
//! go stale between an incremental entry append and the next structural
//! rebuild (`gaps_may_be_dirty` covers both).
//!
//! Per-entry run classification ([`super::verb_group::run_step`]) and the rendered header label
//! stay in [`super::verb_group`]; this module owns run *shapes*.

use std::collections::HashSet;
use std::ops::Range;

use indexmap::IndexMap;

use super::types::EntryLayoutInfo;
use super::verb_group::scan_run_forward;
use crate::scrollback::block::BlockContent;
use crate::scrollback::entry::{EntryId, ScrollbackEntry};
use crate::scrollback::types::DisplayMode;

/// One folded region of the transcript, in entry indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupSpan {
    /// Original contiguous activity range, including leading/trailing Thought.
    /// Dense fallback spans also include their visible tail.
    pub range: Range<usize>,
    /// Which fold produced this span and its count data.
    pub kind: GroupKind,
    /// Whether the user manually expanded this group (keyed by the first
    /// entry's ID in `ScrollbackState::expanded_groups`).
    pub expanded: bool,
}

/// The fold families. All render a synthetic header row; they differ in
/// when they fold and what the header says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKind {
    /// Eagerly folded run of verb-groupable members (tool calls and subagent
    /// rows) — the aggregated "Read 2 files" header. `members` counts
    /// label-bearing members only; claimed thoughts fold in but never count.
    VerbRun { members: usize },
    /// Budget truncation of an over-long dense run — the "N more" header.
    /// `participants` counts the entries eligible to hide (hidden thinking
    /// excluded); `hidden` is how many of them the collapsed state conceals
    /// (`participants - max_visible`, > 0 by the fold gate). The header row
    /// is the first hidden participant, so its plain count shows
    /// `hidden - 1` while its aggregated label describes all `hidden`.
    Truncation { participants: usize, hidden: usize },
}

/// Binary-search sorted, disjoint spans for the one containing `idx`.
pub fn span_containing(spans: &[GroupSpan], idx: usize) -> Option<&GroupSpan> {
    let pos = spans.partition_point(|s| s.range.end <= idx);
    spans.get(pos).filter(|s| s.range.contains(&idx))
}

/// Reset every entry's group flags, scan for spans, and project them onto
/// the layout slice. Returns the spans for the caller to store on the
/// layout cache. Reads the `group_tool_verbs` / `show_thinking_blocks`
/// settings once so both fold families and the projection agree within a
/// rebuild.
pub(super) fn apply(
    entries: &IndexMap<EntryId, ScrollbackEntry>,
    layout_cache: &mut [EntryLayoutInfo],
    max_visible: usize,
    expanded_groups: &HashSet<EntryId>,
) -> Vec<GroupSpan> {
    for info in layout_cache.iter_mut() {
        info.height = info.unfolded_height;
        info.group_header_count = 0;
        info.group_collapse_header = false;
        info.verb_group_header = false;
    }
    let group_tool_verbs = crate::appearance::cache::load_group_tool_verbs();
    let show_thinking = crate::appearance::cache::load_show_thinking_blocks();
    let spans = scan(
        entries,
        max_visible,
        expanded_groups,
        group_tool_verbs,
        show_thinking,
    );
    project_to_layout(&spans, entries, layout_cache, show_thinking);
    spans
}

/// Activity runs claim their entries first; dense truncation covers the rest.
/// A grouped activity can never be truncated again. Returns spans sorted by
/// start index; spans never overlap.
pub(super) fn scan(
    entries: &IndexMap<EntryId, ScrollbackEntry>,
    max_visible: usize,
    expanded_groups: &HashSet<EntryId>,
    group_tool_verbs: bool,
    show_thinking: bool,
) -> Vec<GroupSpan> {
    let mut spans = Vec::new();
    let mut claimed = vec![false; entries.len()];
    let (verb_spans, verb_claimed) =
        scan_verb_runs(entries, expanded_groups, group_tool_verbs, show_thinking);
    spans.extend(verb_spans);
    for (claimed, verb_claimed) in claimed.iter_mut().zip(verb_claimed) {
        *claimed |= verb_claimed;
    }
    spans.extend(scan_truncations(
        entries,
        max_visible,
        expanded_groups,
        show_thinking,
        &claimed,
    ));
    // Both scans emit in ascending order over disjoint ranges; interleave.
    spans.sort_unstable_by_key(|s| s.range.start);
    spans
}

/// Find continuous activities independently of member visibility. Claim their
/// full ranges even when a singleton keeps its own action row, so dense
/// truncation cannot introduce a second fold inside the activity.
fn scan_verb_runs(
    entries: &IndexMap<EntryId, ScrollbackEntry>,
    expanded_groups: &HashSet<EntryId>,
    group_tool_verbs: bool,
    show_thinking: bool,
) -> (Vec<GroupSpan>, Vec<bool>) {
    let n = entries.len();
    let mut spans = Vec::new();
    let mut claimed = vec![false; n];
    if n == 0 || !group_tool_verbs {
        return (spans, claimed);
    }

    let entry_at = |i: usize| entries.get_index(i).map(|(_, e)| e);
    let mut i = 0;
    while i < n {
        let Some(scan) = scan_run_forward(entry_at, i, show_thinking) else {
            i += 1;
            continue;
        };
        // Membership is stable even when individual members remain visible.
        claimed[i..scan.end].fill(true);

        if !scan.folds() {
            i = scan.end;
            continue;
        }
        let first_id = *entries.get_index(i).expect("index within entries").0;
        spans.push(GroupSpan {
            range: i..scan.end,
            kind: GroupKind::VerbRun {
                members: scan.members,
            },
            expanded: expanded_groups.contains(&first_id),
        });
        i = scan.end;
    }
    (spans, claimed)
}

/// Collapsed+groupable entries that may join a truncation run.
/// Hidden thinking is excluded so tools elect their own "N more" header.
fn participates_in_truncation(entry: &ScrollbackEntry, show_thinking: bool) -> bool {
    entry.block.is_groupable()
        && entry.display_mode == DisplayMode::Collapsed
        && !entry.is_hidden_thinking(show_thinking)
}

/// Find consecutive runs of collapsed+groupable entries longer than
/// `max_visible + 1`. Hidden thinking is transparent (skipped, not a
/// run-breaker), mirroring the gap rule in `recompute_gap_after`, so an
/// interspersed thought can't split a run and suppress truncation. Entries
/// claimed by the verb scan break runs.
fn scan_truncations(
    entries: &IndexMap<EntryId, ScrollbackEntry>,
    max_visible: usize,
    expanded_groups: &HashSet<EntryId>,
    show_thinking: bool,
    claimed: &[bool],
) -> Vec<GroupSpan> {
    let mut spans = Vec::new();
    if max_visible == 0 || entries.is_empty() {
        return spans;
    }

    let n = entries.len();
    let mut i = 0;
    while i < n {
        let (_, entry) = entries.get_index(i).unwrap();
        if claimed[i] || !participates_in_truncation(entry, show_thinking) {
            i += 1;
            continue;
        }

        let group_start = i;
        let mut group_len = 1;
        let mut j = i + 1;
        while j < n {
            let (_, e) = entries.get_index(j).unwrap();
            if claimed[j] {
                break;
            }
            if participates_in_truncation(e, show_thinking) {
                group_len += 1;
            } else if !e.is_hidden_thinking(show_thinking) {
                break;
            }
            j += 1;
        }
        let group_end = j;

        if group_len <= max_visible + 1 {
            i = group_end;
            continue;
        }

        let first_id = *entries.get_index(group_start).unwrap().0;
        spans.push(GroupSpan {
            range: group_start..group_end,
            kind: GroupKind::Truncation {
                participants: group_len,
                hidden: group_len - max_visible,
            },
            expanded: expanded_groups.contains(&first_id),
        });
        i = group_end;
    }
    spans
}

/// The single writer of group heights, gaps, and header flags. Every layout
/// consequence of a fold happens here, driven by stable spans and member
/// visibility. Native heights remain independent of the projected layout.
pub(super) fn project_to_layout(
    spans: &[GroupSpan],
    entries: &IndexMap<EntryId, ScrollbackEntry>,
    layout_cache: &mut [EntryLayoutInfo],
    show_thinking: bool,
) {
    for span in spans {
        match span.kind {
            GroupKind::VerbRun { members } => {
                project_verb_run(span, members, entries, layout_cache, show_thinking);
            }
            GroupKind::Truncation {
                participants,
                hidden,
            } => {
                project_truncation(
                    span,
                    participants,
                    hidden,
                    entries,
                    layout_cache,
                    show_thinking,
                );
            }
        }
    }
}

/// Project visibility without changing the group's membership or anchor.
/// The first slot contains a synthetic summary and, when visible, the first
/// member's complete native content. All other rows retain source order.
fn project_verb_run(
    span: &GroupSpan,
    members: usize,
    entries: &IndexMap<EntryId, ScrollbackEntry>,
    layout_cache: &mut [EntryLayoutInfo],
    show_thinking: bool,
) {
    for idx in span.range.clone() {
        let (_, entry) = entries.get_index(idx).unwrap();
        let cached = &mut layout_cache[idx];
        let visible = super::verb_group::member_visible(entry, span.expanded, show_thinking);
        cached.height = if visible { cached.unfolded_height } else { 0 };
        if idx == span.range.start {
            cached.verb_group_header = true;
            cached.group_collapse_header = span.expanded;
            cached.group_header_count = members.min(u16::MAX as usize) as u16;
            cached.height = cached.height.saturating_add(1);
        }
        if idx + 1 != span.range.end {
            cached.gap_after = 0;
        }
    }
}

/// Collapsed: the first participating entry becomes the "N more" header
/// (count excludes the header itself), older participants hide, and the
/// last `max_visible` stay untouched. Expanded: entry 0 becomes a
/// standalone collapse header (`height=1`, content replaced) counting the
/// `participants - 1` entries below it, which all keep their own rows.
/// Hidden thinking is skipped in both states.
/// `verb_group::truncation_header_label` mirrors this walk's participant
/// rule for the header's aggregated label; a new transparency category here
/// must update that walk too.
fn project_truncation(
    span: &GroupSpan,
    participants: usize,
    hidden: usize,
    entries: &IndexMap<EntryId, ScrollbackEntry>,
    layout_cache: &mut [EntryLayoutInfo],
    show_thinking: bool,
) {
    if span.expanded {
        let cached = &mut layout_cache[span.range.start];
        cached.group_collapse_header = true;
        cached.group_header_count = (participants - 1).min(u16::MAX as usize) as u16;
        cached.height = 1;
        cached.gap_after = 0;
        return;
    }

    let mut seen = 0;
    for idx in span.range.clone() {
        let (_, e) = entries.get_index(idx).unwrap();
        if e.is_hidden_thinking(show_thinking) {
            continue;
        }
        let cached = &mut layout_cache[idx];
        if seen == 0 {
            cached.height = 1;
            cached.gap_after = 0;
            cached.group_header_count = (hidden - 1).min(u16::MAX as usize) as u16;
        } else if seen < hidden {
            cached.height = 0;
            cached.gap_after = 0;
            cached.group_header_count = 0;
        }
        seen += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scrollback::block::RenderBlock;
    use crate::scrollback::blocks::tool::{
        ReadToolCallBlock, SkillDisplayStatus, ToolCallBlock, hook::ToolCallHookData,
    };

    fn read() -> ScrollbackEntry {
        ScrollbackEntry::new(RenderBlock::ToolCall(ToolCallBlock::Read(
            ReadToolCallBlock::new("/x/src/main.rs"),
        )))
        .with_display_mode(DisplayMode::Collapsed)
    }

    fn execute() -> ScrollbackEntry {
        ScrollbackEntry::new(RenderBlock::execute("ls")).with_display_mode(DisplayMode::Collapsed)
    }

    fn thought() -> ScrollbackEntry {
        ScrollbackEntry::new(RenderBlock::thinking("hmm")).with_display_mode(DisplayMode::Collapsed)
    }

    fn skill() -> ScrollbackEntry {
        ScrollbackEntry::new(RenderBlock::ToolCall(ToolCallBlock::skill_call(
            Some("deploy"),
            None,
            Some("/x/skills/deploy/SKILL.md"),
            None,
            SkillDisplayStatus::Loaded,
            None,
        )))
    }

    fn map(entries: Vec<ScrollbackEntry>) -> IndexMap<EntryId, ScrollbackEntry> {
        entries
            .into_iter()
            .enumerate()
            .map(|(i, e)| (EntryId::new(i as u64), e))
            .collect()
    }

    /// Seed layout distinct from anything the projection writes, so a
    /// changed entry is distinguishable from an untouched one.
    fn seeded_layout(n: usize) -> Vec<EntryLayoutInfo> {
        vec![
            EntryLayoutInfo {
                height: 5,
                unfolded_height: 5,
                gap_after: 1,
                ..Default::default()
            };
            n
        ]
    }

    fn scan_and_project(
        entries: &IndexMap<EntryId, ScrollbackEntry>,
        layout: &mut [EntryLayoutInfo],
        max_visible: usize,
        expanded: &HashSet<EntryId>,
    ) -> Vec<GroupSpan> {
        let spans = scan(entries, max_visible, expanded, true, true);
        project_to_layout(&spans, entries, layout, true);
        spans
    }

    fn reset_and_project(
        spans: &[GroupSpan],
        entries: &IndexMap<EntryId, ScrollbackEntry>,
        layout: &mut [EntryLayoutInfo],
    ) {
        for info in layout.iter_mut() {
            info.height = info.unfolded_height;
            info.group_header_count = 0;
            info.group_collapse_header = false;
            info.verb_group_header = false;
        }
        project_to_layout(spans, entries, layout, true);
    }

    #[test]
    fn mixed_tool_activity_keeps_leading_interleaved_and_trailing_thoughts() {
        let mut failed_read = read().with_display_mode(DisplayMode::Expanded);
        let RenderBlock::ToolCall(ToolCallBlock::Read(block)) = &mut failed_read.block else {
            unreachable!();
        };
        block.error = Some("permission denied".into());
        let entries = map(vec![
            thought(),
            execute().with_display_mode(DisplayMode::Expanded),
            thought(),
            failed_read,
            thought(),
        ]);
        let mut layout = seeded_layout(entries.len());
        let spans = scan_and_project(&entries, &mut layout, 10, &HashSet::new());

        assert_eq!(
            spans,
            vec![GroupSpan {
                range: 0..5,
                kind: GroupKind::VerbRun { members: 2 },
                expanded: false,
            }]
        );

        // The leading Thought anchors the summary. Its completed body is
        // folded, while the expanded native tool keeps its own full height.
        assert!(layout[0].verb_group_header);
        assert_eq!(layout[0].group_header_count, 2);
        assert_eq!(layout[0].height, 1);
        assert_eq!(layout[1].height, 5);
        assert_eq!(layout[2].height, 0);
        assert_eq!(
            layout[3].height, 5,
            "expanded failed tool remains inspectable"
        );
        assert_eq!(layout[4].height, 0);
    }

    #[test]
    fn skill_rows_and_conversation_entries_separate_activity_groups() {
        let entries = map(vec![
            execute(),
            thought(),
            read(),
            skill(),
            execute(),
            thought(),
            execute(),
            ScrollbackEntry::new(RenderBlock::user_prompt("next turn")),
            execute(),
            execute(),
        ]);
        let spans = scan(&entries, 10, &HashSet::new(), true, true);
        assert_eq!(
            spans,
            vec![
                GroupSpan {
                    range: 0..3,
                    kind: GroupKind::VerbRun { members: 2 },
                    expanded: false,
                },
                GroupSpan {
                    range: 4..7,
                    kind: GroupKind::VerbRun { members: 2 },
                    expanded: false,
                },
                GroupSpan {
                    range: 8..10,
                    kind: GroupKind::VerbRun { members: 2 },
                    expanded: false,
                },
            ]
        );
    }

    #[test]
    fn single_tool_with_thought_forms_a_group_but_pure_thought_does_not() {
        let entries = map(vec![thought(), execute()]);
        let spans = scan(&entries, 10, &HashSet::new(), true, true);
        assert_eq!(spans.len(), 1);
        assert_eq!(
            spans[0].kind,
            GroupKind::VerbRun { members: 1 },
            "Thought contributes to the fold threshold but not the tool count"
        );

        let thoughts = map(vec![thought(), thought()]);
        assert!(scan(&thoughts, 10, &HashSet::new(), true, true).is_empty());

        let single_tool = map(vec![execute()]);
        assert!(scan(&single_tool, 10, &HashSet::new(), true, true).is_empty());
    }

    #[test]
    fn disabling_verb_groups_routes_activity_through_dense_truncation() {
        let mut list = vec![execute(); 6];
        list.push(thought());
        list.extend((0..6).map(|_| execute()));
        let entries = map(list);
        let mut layout = seeded_layout(entries.len());
        let spans = scan(&entries, 10, &HashSet::new(), false, false);
        project_to_layout(&spans, &entries, &mut layout, false);
        assert_eq!(
            spans,
            vec![GroupSpan {
                range: 0..13,
                kind: GroupKind::Truncation {
                    participants: 12,
                    hidden: 2,
                },
                expanded: false,
            }]
        );
        assert_eq!(layout[0].group_header_count, 1);
        assert_eq!(layout[0].height, 1);
        assert_eq!(layout[1].height, 0);
        assert_eq!(
            layout[6].height, 5,
            "hidden Thought is transparent to dense projection"
        );
        assert_eq!(layout[2].height, 5);
    }

    #[test]
    fn grouped_activity_is_not_truncated_again_by_dense_budget() {
        let mut list = Vec::new();
        for _ in 0..6 {
            list.push(execute());
            list.push(thought());
        }
        let entries = map(list);
        let mut layout = seeded_layout(entries.len());
        let spans = scan_and_project(&entries, &mut layout, 2, &HashSet::new());
        assert_eq!(
            spans,
            vec![GroupSpan {
                range: 0..12,
                kind: GroupKind::VerbRun { members: 6 },
                expanded: false,
            }]
        );
        assert!(layout.iter().all(|info| !info.group_collapse_header));
        assert_eq!(
            layout.iter().filter(|info| info.verb_group_header).count(),
            1
        );
        assert_eq!(layout[0].height, 1);
        assert!(layout[1..].iter().all(|info| info.height == 0));
    }

    #[test]
    fn expanded_activity_restores_native_member_heights_and_keeps_anchor() {
        let entries = map(vec![thought(), execute(), thought(), read()]);
        let expanded: HashSet<EntryId> = [EntryId::new(0)].into();
        let spans = scan(&entries, 10, &expanded, true, true);
        assert_eq!(spans[0].range, 0..4);
        assert!(spans[0].expanded);

        let mut layout = seeded_layout(entries.len());
        reset_and_project(&spans, &entries, &mut layout);
        assert!(layout[0].group_collapse_header);
        assert_eq!(
            layout[0].height, 6,
            "header is stacked above the first native row"
        );
        assert!(layout[1..].iter().all(|info| info.height == 5));
    }

    #[test]
    fn folded_activity_keeps_running_explicit_pending_and_hook_members_visible() {
        let mut running = execute();
        running.is_running = true;
        let expanded = execute().with_display_mode(DisplayMode::Expanded);
        let mut pending = execute();
        pending.is_pending_user_input = true;
        let mut hooked = execute();
        hooked.hook_data = Some(ToolCallHookData::default());
        let entries = map(vec![running, expanded, pending, hooked, execute()]);
        let mut layout = seeded_layout(entries.len());
        let spans = scan_and_project(&entries, &mut layout, 10, &HashSet::new());
        assert_eq!(spans[0].kind, GroupKind::VerbRun { members: 5 });
        assert_eq!(
            layout[0].height, 6,
            "running first member keeps its preview below the summary"
        );
        assert_eq!(layout[1].height, 5);
        assert_eq!(layout[2].height, 5);
        assert_eq!(layout[3].height, 5);
        assert_eq!(layout[4].height, 0, "ordinary collapsed member folds away");
    }

    #[test]
    fn repeated_reset_and_projection_is_idempotent_for_the_same_spans() {
        let entries = map(vec![thought(), execute(), thought(), read()]);
        let spans = scan(&entries, 10, &HashSet::new(), true, true);
        let mut layout = seeded_layout(entries.len());
        reset_and_project(&spans, &entries, &mut layout);
        let projected = layout.clone();
        reset_and_project(&spans, &entries, &mut layout);
        assert_eq!(layout, projected);
    }

    #[test]
    fn span_containing_hits_inside_and_misses_gaps_and_ends() {
        let span = |start: usize, end: usize| GroupSpan {
            range: start..end,
            kind: GroupKind::VerbRun { members: 1 },
            expanded: false,
        };
        let spans = [span(2, 5), span(9, 12)];
        assert!(span_containing(&spans, 1).is_none());
        assert_eq!(span_containing(&spans, 2), Some(&spans[0]));
        assert_eq!(span_containing(&spans, 4), Some(&spans[0]));
        assert!(span_containing(&spans, 5).is_none(), "end is exclusive");
        assert!(span_containing(&spans, 7).is_none(), "gap between spans");
        assert_eq!(span_containing(&spans, 11), Some(&spans[1]));
        assert!(span_containing(&spans, 12).is_none());
        assert!(span_containing(&[], 0).is_none());
    }

    #[test]
    fn spans_are_sorted_and_disjoint() {
        let mut list = vec![execute(), execute(), skill(), execute()];
        list.extend((0..12).map(|_| execute()));
        list.extend([execute(), execute()]);
        let entries = map(list);
        let spans = scan(&entries, 10, &HashSet::new(), true, true);
        assert!(spans.len() >= 2);
        for pair in spans.windows(2) {
            assert!(pair[0].range.end <= pair[1].range.start);
        }
    }
    #[test]
    fn standalone_receipt_separates_activity_without_hiding_the_interaction() {
        let receipt = ScrollbackEntry::new(RenderBlock::ToolCall(ToolCallBlock::Other(
            crate::scrollback::blocks::tool::OtherToolCallBlock::new("Received", "message")
                .with_immutable_event_id("parent-message:receipt"),
        )))
        .with_display_mode(DisplayMode::Collapsed);
        let entries = map(vec![execute(), read(), receipt, execute(), read()]);
        let mut layout = seeded_layout(entries.len());
        let spans = scan_and_project(&entries, &mut layout, 10, &HashSet::new());
        assert_eq!(
            spans
                .iter()
                .map(|span| span.range.clone())
                .collect::<Vec<_>>(),
            vec![0..2, 3..5]
        );
        assert_eq!(
            layout[2].height, 5,
            "independent interaction remains visible"
        );
        assert!(!layout[2].is_group_header());
    }
}
