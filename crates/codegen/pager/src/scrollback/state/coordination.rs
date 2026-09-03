//! Passive inquiry events share the normal tool renderer, not the primary
//! turn's tool tracker. A sideband must not claim or complete a primary turn.

use super::*;
use crate::scrollback::blocks::tool::OtherToolCallBlock;

impl ScrollbackState {
    fn coordination_entry_id(&self, source_peer_id: &str, inquiry_id: &str) -> Option<EntryId> {
        self.entries
            .iter()
            .find_map(|(id, entry)| match &entry.block {
                RenderBlock::ToolCall(ToolCallBlock::Other(block))
                    if block.coordination.as_ref().is_some_and(|row| {
                        row.source_peer_id == source_peer_id && row.inquiry_id == inquiry_id
                    }) =>
                {
                    Some(*id)
                }
                _ => None,
            })
    }

    pub(crate) fn upsert_coordination_row(
        &mut self,
        mut block: OtherToolCallBlock,
        is_replay: bool,
    ) -> bool {
        let row = block
            .coordination
            .as_ref()
            .expect("coordination row identity");
        let terminal = row.terminal;
        let mut running = !is_replay && !terminal;
        let existing = self.coordination_entry_id(&row.source_peer_id, &row.inquiry_id);
        let id = if let Some(id) = existing {
            let entry = self.entries.get(&id).expect("existing inquiry row");
            let RenderBlock::ToolCall(ToolCallBlock::Other(previous)) = &entry.block else {
                unreachable!()
            };
            // Replayed receipts/approvals cannot resurrect a completed row.
            if previous
                .coordination
                .as_ref()
                .is_some_and(|row| row.terminal)
            {
                return false;
            }
            // An older replay receipt must not stop a sideband observed live.
            running |= entry.is_running && !terminal;
            if previous.name == block.name
                && previous.output == block.output
                && previous.error == block.error
                && previous.coordination == block.coordination
                && entry.is_running == running
            {
                return false;
            }
            block.started_at = previous.started_at;
            block.elapsed_ms = previous.elapsed_ms;
            if terminal && is_replay {
                block.finish();
            }
            let started_at = block.started_at;
            self.replace_tool_block(
                id,
                RenderBlock::ToolCall(ToolCallBlock::Other(block)),
                started_at,
            );
            id
        } else {
            self.push_block(RenderBlock::ToolCall(ToolCallBlock::Other(block)))
        };
        if terminal && !is_replay {
            self.finish_running(id);
        } else {
            // Historical starts alone are not proof that a sideband is still live.
            self.set_entry_running(id, running);
        }
        self.mark_structurally_dirty(id);
        self.bump_content_generation();
        true
    }

    /// A cursor-found reload may stage the completion in a fresh tail while
    /// the original receipt lives in the stashed transcript. Keep its row ID.
    pub(super) fn merge_coordination_rows_from_tail(&mut self, tail: &mut Self) {
        let updates: Vec<_> = tail
            .entries
            .iter()
            .filter_map(|(id, entry)| {
                let RenderBlock::ToolCall(ToolCallBlock::Other(block)) = &entry.block else {
                    return None;
                };
                let row = block.coordination.as_ref()?;
                let existing = self.coordination_entry_id(&row.source_peer_id, &row.inquiry_id)?;
                Some((
                    *id,
                    existing,
                    block.clone(),
                    !entry.is_running && !row.terminal,
                ))
            })
            .collect();
        for (tail_id, original_id, block, is_replay) in updates {
            self.upsert_coordination_row(block, is_replay);
            if tail.is_committed(tail_id) {
                self.minimal_commit.mark_committed(original_id);
            }
            tail.remove_entry(tail_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scrollback::blocks::tool::CoordinationRow;

    fn row(id: &str, terminal: bool) -> OtherToolCallBlock {
        let title = if terminal {
            "Answered session peer"
        } else {
            "Answering session peer"
        };
        let mut block = OtherToolCallBlock::new(title, "").with_output(format!(
            "Inquiry ID: {id}\nQuestion: status?\nAnswer: working"
        ));
        block.coordination = Some(CoordinationRow {
            source_peer_id: "peer".into(),
            inquiry_id: id.into(),
            terminal,
        });
        block
    }

    #[test]
    fn coordination_same_inquiry_id_from_different_peers_never_merges() {
        let mut state = ScrollbackState::new();
        state.upsert_coordination_row(row("same", true), false);
        let mut other = row("same", false);
        other.coordination.as_mut().unwrap().source_peer_id = "other-peer".into();
        state.upsert_coordination_row(other.clone(), false);
        assert_eq!(state.len(), 2);
        assert!(state.entry(1).unwrap().is_running);

        // Cursor reload must use the same composite identity too.
        let mut tail = state.fresh_continuation();
        other.coordination.as_mut().unwrap().terminal = true;
        other.name = "Answered session other".into();
        tail.upsert_coordination_row(other, true);
        state.append_entries_from(tail);
        assert_eq!(state.len(), 2);
        assert!(!state.has_running_entries());
        let RenderBlock::ToolCall(ToolCallBlock::Other(first)) = &state.entry(0).unwrap().block
        else {
            panic!()
        };
        let RenderBlock::ToolCall(ToolCallBlock::Other(second)) = &state.entry(1).unwrap().block
        else {
            panic!()
        };
        assert_eq!(first.name, "Answered session peer");
        assert_eq!(second.name, "Answered session other");
    }

    #[test]
    fn coordination_foreground_cleanup_preserves_passive_timing() {
        let mut state = ScrollbackState::new();
        state.upsert_coordination_row(row("one", false), false);
        let ordinary = state.push_block(RenderBlock::execute("cargo test"));
        state.set_entry_running(ordinary, true);
        state.finish_all_running();
        assert!(state.entry(0).unwrap().is_running);
        assert!(!state.get_by_id(ordinary).unwrap().is_running);
        let RenderBlock::ToolCall(ToolCallBlock::Other(block)) = &state.entry(0).unwrap().block
        else {
            panic!()
        };
        assert!(block.started_at.is_some());
        assert!(block.elapsed_ms.is_none());
    }

    #[test]
    fn coordination_old_replay_receipt_does_not_stop_a_live_row() {
        let mut state = ScrollbackState::new();
        assert!(state.upsert_coordination_row(row("one", false), false));
        assert!(!state.upsert_coordination_row(row("one", false), true));
        assert!(state.entry(0).unwrap().is_running);
        assert!(state.has_running_entries());
        assert_eq!(state.len(), 1);
    }

    #[test]
    fn coordination_collapsed_row_has_run_like_running_and_finished_chrome() {
        let mut state = ScrollbackState::new();
        state.upsert_coordination_row(row("one", false), false);
        let entry = state.entry(0).unwrap();
        let ctx = entry.context(100, &AppearanceConfig::default(), None);
        assert_eq!(ctx.mode, DisplayMode::Collapsed);
        assert!(entry.block.accent(&ctx).unwrap().animated);
        assert!(entry.block.bullet(&ctx).unwrap().animated);

        state.upsert_coordination_row(row("one", true), false);
        let entry = state.entry(0).unwrap();
        let ctx = entry.context(100, &AppearanceConfig::default(), None);
        assert_eq!(ctx.mode, DisplayMode::Collapsed);
        assert!(!entry.block.accent(&ctx).unwrap().animated);
        assert!(!entry.block.bullet(&ctx).unwrap().animated);
    }

    #[test]
    fn coordination_independent_inquiries_from_one_source_update_their_own_rows() {
        let mut state = ScrollbackState::new();
        state.upsert_coordination_row(row("one", false), false);
        state.upsert_coordination_row(row("two", false), false);
        let first = state.entry(0).unwrap().id;
        let second = state.entry(1).unwrap().id;
        state.upsert_coordination_row(row("two", true), false);
        assert!(state.get_by_id(first).unwrap().is_running);
        assert!(!state.get_by_id(second).unwrap().is_running);
        state.upsert_coordination_row(row("one", true), false);
        assert_eq!(state.len(), 2);
        assert!(!state.has_running_entries());
    }

    #[test]
    fn coordination_tail_completion_merges_into_original_id_and_preserves_manual_fold() {
        let mut state = ScrollbackState::new();
        state.appearance.scrollback.scroll.respect_manual_folds = true;
        state.upsert_coordination_row(row("one", false), false);
        let id = state.entry(0).unwrap().id;
        state.set_selected(Some(0));
        state.expand_selected();
        assert!(state.get_by_id(id).unwrap().display_mode_pinned);
        let mut tail = state.fresh_continuation();
        tail.upsert_coordination_row(row("one", true), false);
        tail.mark_committed(0);
        let unrelated = tail.push_block(RenderBlock::notice("later activity"));
        state.append_entries_from(tail);

        assert_eq!(state.len(), 2);
        assert_eq!(state.entry(0).unwrap().id, id);
        assert_eq!(state.entry(1).unwrap().id, unrelated);
        let entry = state.get_by_id(id).unwrap();
        assert_eq!(entry.display_mode, DisplayMode::Expanded);
        assert!(!entry.is_running);
        assert!(
            state.is_committed(id),
            "an emitted tail completion must not be emitted twice"
        );
        assert!(!state.has_running_entries());
        let RenderBlock::ToolCall(ToolCallBlock::Other(block)) = &entry.block else {
            panic!("not a tool row")
        };
        assert_eq!(block.name, "Answered session peer");
        assert!(
            block.elapsed_ms.is_some(),
            "preserve timing from the original running row"
        );
    }

    #[test]
    fn coordination_replayed_completion_freezes_live_timing_without_a_replay_flash() {
        let mut state = ScrollbackState::new();
        state.upsert_coordination_row(row("one", false), false);
        state.upsert_coordination_row(row("one", true), true);
        let entry = state.entry(0).unwrap();
        assert!(!entry.is_running);
        assert!(entry.finished_at.is_none());
        let RenderBlock::ToolCall(ToolCallBlock::Other(block)) = &entry.block else {
            panic!("not a tool row")
        };
        assert!(block.elapsed_ms.is_some());
    }

    #[test]
    fn coordination_late_tail_receipt_does_not_downgrade_finished_state() {
        let mut state = ScrollbackState::new();
        state.upsert_coordination_row(row("one", true), false);
        let id = state.entry(0).unwrap().id;
        let mut tail = state.fresh_continuation();
        tail.upsert_coordination_row(row("one", false), false);
        state.append_entries_from(tail);
        assert_eq!(state.len(), 1);
        assert_eq!(state.entry(0).unwrap().id, id);
        assert!(!state.has_running_entries());
        let RenderBlock::ToolCall(ToolCallBlock::Other(block)) = &state.entry(0).unwrap().block
        else {
            panic!("not a tool row")
        };
        assert_eq!(block.name, "Answered session peer");
    }

    #[test]
    fn coordination_row_cannot_capture_main_tool_hooks_or_join_tool_groups() {
        let mut state = ScrollbackState::new();
        let real_tool = state.push_block(RenderBlock::ToolCall(ToolCallBlock::Other(
            OtherToolCallBlock::new("list_active_sessions", "").with_output("{\"sessions\":[]}"),
        )));
        state.upsert_coordination_row(row("one", true), false);
        assert_eq!(state.last_tool_call_entry_id(), Some(real_tool));
        assert!(state.entry(0).unwrap().block.is_groupable());
        assert!(!state.entry(1).unwrap().block.is_groupable());
    }
}
