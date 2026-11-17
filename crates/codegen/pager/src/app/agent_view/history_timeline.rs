//! Fixed-cut prompt catalog for the timeline rail.
//!
//! The catalog is metadata only: ordinal, source, message id, display segment,
//! preview, and the opaque around target. It is not refreshed when a live
//! prompt arrives after the cut. Those prompts stay on the original tracker
//! and are appended to the rail from the installed history prefix boundary.

use std::collections::BTreeMap;

use shell::session::storage::{HistorySourceKey, HistoryTimelineQuery, HistoryTimelineResponse};

use super::AgentView;
use crate::app::actions::Effect;
use crate::app::root::{ActiveView, AppView};
use crate::scrollback::block::RenderBlock;
use crate::scrollback::entry::EntryId;
use crate::views::timeline::{RailViewport, TimelineHit};

const TIMELINE_PAGE_LIMIT: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
struct CatalogRow {
    source: HistorySourceKey,
    message_id: Option<String>,
    display_segment: Option<u32>,
    preview: String,
    target: String,
}

#[derive(Debug, Default)]
pub(super) struct HistoryTimeline {
    generation: u64,
    cut: Option<String>,
    total: Option<u64>,
    by_ordinal: BTreeMap<u64, CatalogRow>,
    inflight: Option<u64>,
    request_seq: u64,
    jump_seq: u64,
    pending_ordinal: Option<u64>,
    /// The active catalog read failed or returned an unusable response.
    /// Automatic re-reads stop until an explicit user action retires this;
    /// without it, every error completion and every paint-time ensure would
    /// re-send the same query forever. Transient and per-view: `retire`
    /// resets it on a session switch; it is never persisted.
    read_blocked: bool,
}

impl HistoryTimeline {
    fn retire(&mut self) {
        *self = Self {
            generation: self.generation.wrapping_add(1),
            request_seq: self.request_seq,
            jump_seq: self.jump_seq.wrapping_add(1),
            ..Self::default()
        };
    }
}

impl AgentView {
    pub(in crate::app) fn retire_history_timeline(&mut self) {
        self.history_timeline.retire();
    }

    /// A superseding explicit target — a cached or live reveal, returning to
    /// the live tail, or closing the rail — retires the pending timeline jump
    /// so its late reads cannot replace the window the new target revealed.
    /// The retired catalog read stays gated by `jump_seq`/`pending_ordinal`:
    /// its response may still fill the same-cut cache, but it can no longer
    /// issue a body page or move the reveal. The retired body page is dropped
    /// by the `history_generation`/`history_page_request` admission, and an
    /// in-flight initial Latest is re-requested under the bumped generation
    /// instead of staying pending forever. Without a pending jump nothing is
    /// retired, so unrelated clicks and rail closes never cancel a passive
    /// Latest/Before/After read.
    pub(in crate::app) fn suppress_timeline_jump(&mut self, effects: &mut Vec<Effect>) {
        if self.history_timeline.pending_ordinal.is_none() {
            return;
        }
        self.history_timeline.jump_seq = self.history_timeline.jump_seq.wrapping_add(1);
        self.history_timeline.pending_ordinal = None;
        self.history_timeline.inflight = None;
        if self.history_page_request.take().is_some() {
            self.history_generation = self.history_generation.wrapping_add(1);
            if self.history_latest_pending {
                self.request_latest_history_page(effects);
            }
        }
    }

    /// Explicit user actions — re-enabling the rail, clicking an uncached
    /// target — clear the failure block so the read is retried once.
    /// Background completions and paints never call this.
    pub(in crate::app) fn resume_history_timeline_reads(&mut self) {
        self.history_timeline.read_blocked = false;
    }

    /// Confirmed catalog size for this cut. `None` until Newest lands, or
    /// when the cached cut is not the active history binding.
    pub(in crate::app) fn global_timeline_total(&self) -> Option<u64> {
        if self.is_subagent_view || self.history_cut.is_none() {
            return None;
        }
        let total = self.history_timeline.total?;
        (self.history_timeline.cut.as_deref() == self.history_cut.as_deref()).then_some(total)
    }

    /// Rail count: catalog ordinals plus live user prompts strictly after the
    /// installed history prefix. Without an installed prefix the cut boundary
    /// is not proven, so live prompts are not invented onto the catalog.
    pub(in crate::app) fn timeline_rail_count(&self, show_timeline: bool) -> u64 {
        if show_timeline && self.history_cut.is_some() {
            let Some(total) = self.global_timeline_total() else {
                return 0;
            };
            total + self.live_timeline_ids().len() as u64
        } else {
            self.scrollback.turn_count() as u64
        }
    }

    pub(in crate::app) fn global_rail_viewport(&self, catalog_total: u64) -> RailViewport {
        let live = self.live_timeline_ids();
        let total = catalog_total + live.len() as u64;
        let active = self.active_global_ordinal(catalog_total, &live);
        let at_live_end = self.scrollback.is_follow_mode()
            || (self.history_newer_cursor().is_none()
                && !self.history_has_gap()
                && !self.scrollback.has_content_below());
        let up_target = active
            .and_then(|ordinal| ordinal.checked_sub(1))
            .or_else(|| (active.is_none() && at_live_end && total > 0).then_some(total - 1));
        let down_target = active.and_then(|ordinal| {
            let next = ordinal + 1;
            (next < total).then_some(next)
        });
        let down_returns_live =
            !at_live_end && active.map(|ordinal| ordinal + 1 >= total).unwrap_or(true);
        RailViewport {
            active,
            up_target,
            down_target,
            at_bottom: at_live_end,
            down_returns_live,
        }
    }

    pub(crate) fn timeline_hover_text(&self, ordinal: u64) -> Option<String> {
        if let Some(total) = self.global_timeline_total() {
            if ordinal < total {
                return self
                    .history_timeline
                    .by_ordinal
                    .get(&ordinal)
                    .map(|row| row.preview.clone());
            }
            let live = self.live_timeline_ids();
            let index = usize::try_from(ordinal - total).ok()?;
            let entry_id = *live.get(index)?;
            let turn_idx = self.scrollback.turns().iter().position(|turn| {
                self.scrollback
                    .entry(turn.prompt_index)
                    .is_some_and(|entry| entry.id == entry_id)
            })?;
            return self.scrollback.turn_preview(turn_idx);
        }
        let turn_idx = usize::try_from(ordinal).ok()?;
        self.scrollback.turn_preview(turn_idx)
    }

    /// One catalog read for the visible owner. Newest until `total` is known;
    /// a hovered uncached ordinal reads AroundOrdinal. Never a full walk, and
    /// never a second request while one is in flight. A read that failed stays
    /// blocked until an explicit user action retires the block
    /// ([`Self::resume_history_timeline_reads`]).
    pub(in crate::app) fn ensure_history_timeline(&mut self, effects: &mut Vec<Effect>) {
        if !self.scrollback.appearance().show_timeline
            || self.is_subagent_view
            || self.history_cut.is_none()
            || self.history_timeline.read_blocked
            || self.history_timeline.inflight.is_some()
        {
            return;
        }
        if self.global_timeline_total().is_none() {
            self.push_timeline_request(HistoryTimelineQuery::Newest, None, effects);
            return;
        }
        let Some(TimelineHit::Tick(ordinal)) = self.timeline_hover else {
            return;
        };
        let Some(total) = self.global_timeline_total() else {
            return;
        };
        if ordinal >= total || self.history_timeline.by_ordinal.contains_key(&ordinal) {
            return;
        }
        self.push_timeline_request(
            HistoryTimelineQuery::AroundOrdinal { ordinal },
            None,
            effects,
        );
    }

    pub(in crate::app) fn activate_timeline_hit(
        &mut self,
        hit: TimelineHit,
        effects: &mut Vec<Effect>,
    ) {
        let ordinal = match hit {
            TimelineHit::Tick(ordinal) => ordinal,
            TimelineHit::Up | TimelineHit::Down => {
                let Some(total) = self.global_timeline_total() else {
                    return;
                };
                let viewport = self.global_rail_viewport(total);
                match hit {
                    TimelineHit::Up => {
                        let Some(ordinal) = viewport.up_target else {
                            return;
                        };
                        ordinal
                    }
                    TimelineHit::Down => {
                        if viewport.down_returns_live && viewport.down_target.is_none() {
                            self.suppress_timeline_jump(effects);
                            self.scrollback.goto_bottom();
                            return;
                        }
                        let Some(ordinal) = viewport.down_target else {
                            return;
                        };
                        ordinal
                    }
                    TimelineHit::Tick(_) => unreachable!(),
                }
            }
        };
        self.activate_timeline_ordinal(ordinal, effects);
    }

    fn activate_timeline_ordinal(&mut self, ordinal: u64, effects: &mut Vec<Effect>) {
        let Some(total) = self.global_timeline_total() else {
            return;
        };
        if ordinal >= total {
            let live = self.live_timeline_ids();
            let Some(index) = usize::try_from(ordinal - total).ok() else {
                return;
            };
            let Some(entry_id) = live.get(index).copied() else {
                return;
            };
            self.suppress_timeline_jump(effects);
            self.reveal_entry(entry_id);
            return;
        }
        if self.history_timeline.by_ordinal.contains_key(&ordinal) {
            self.suppress_timeline_jump(effects);
            self.history_timeline.jump_seq = self.history_timeline.jump_seq.wrapping_add(1);
            self.history_timeline.pending_ordinal = Some(ordinal);
            self.jump_catalog_ordinal(ordinal, effects);
            return;
        }
        // An explicit click on an uncached ordinal retries a blocked read.
        self.history_timeline.read_blocked = false;
        self.push_timeline_request(
            HistoryTimelineQuery::AroundOrdinal { ordinal },
            Some(ordinal),
            effects,
        );
    }

    /// Admit one catalog response. Identity is verified before the in-flight
    /// nonce is consumed, so a wrong-binding packet with the same nonce can
    /// neither clear the active read nor block anything. Cache fills stay
    /// independent of the jump gate. A failed or unusable result of the
    /// *active* request enters the transient read block; a retired read's
    /// late failure only returns and never blocks a newer in-flight request.
    pub(in crate::app) fn admit_history_timeline(
        &mut self,
        show_timeline: bool,
        binding_epoch: u32,
        request_id: u64,
        generation: u64,
        session_id: &acp_transport::protocol::SessionId,
        jump_ordinal: Option<u64>,
        jump_seq: u64,
        result: Result<HistoryTimelineResponse, String>,
        effects: &mut Vec<Effect>,
    ) {
        if self.session.session_id.as_ref() != Some(session_id)
            || self.session_binding_epoch != binding_epoch
            || generation != self.history_timeline.generation
        {
            return;
        }
        let active_request = self.history_timeline.inflight == Some(request_id);
        if active_request {
            self.history_timeline.inflight = None;
        }
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                if active_request {
                    self.block_timeline_reads(show_timeline, &error);
                }
                return;
            }
        };
        if response.session_id != *session_id
            || self.history_cut.as_deref() != Some(response.cut.as_str())
        {
            if active_request {
                self.block_timeline_reads(show_timeline, "response does not match this cut");
            }
            return;
        }
        self.history_timeline.cut = Some(response.cut);
        self.history_timeline.total = Some(response.total_entries);
        for item in response.entries {
            let entry = item.entry;
            self.history_timeline.by_ordinal.insert(
                entry.ordinal,
                CatalogRow {
                    source: entry.source,
                    message_id: entry.message_id,
                    display_segment: entry.display_segment,
                    preview: entry.preview,
                    target: item.target,
                },
            );
        }
        if !show_timeline
            || jump_ordinal.is_none()
            || jump_seq != self.history_timeline.jump_seq
            || self.history_timeline.pending_ordinal != jump_ordinal
        {
            return;
        }
        if let Some(ordinal) = jump_ordinal {
            self.jump_catalog_ordinal(ordinal, effects);
        }
    }

    /// Enter the transient failure block because the active catalog read
    /// produced no usable result, and say so once while the rail is visible.
    /// Private helper of `admit_history_timeline`.
    fn block_timeline_reads(&mut self, show_timeline: bool, reason: &str) {
        self.history_timeline.read_blocked = true;
        if show_timeline {
            self.show_toast(&format!(
                "Could not load timeline: {}",
                crate::app::root::effects::sanitize_user_error(reason)
            ));
        }
    }

    pub(in crate::app) fn reveal_adopted_timeline_target(
        &mut self,
        target_source: Option<HistorySourceKey>,
    ) {
        let Some(ordinal) = self.history_timeline.pending_ordinal else {
            return;
        };
        let Some(row) = self.history_timeline.by_ordinal.get(&ordinal).cloned() else {
            return;
        };
        if target_source != Some(row.source) {
            return;
        }
        if self.reveal_catalog_row(&row) {
            self.history_timeline.pending_ordinal = None;
        }
    }

    fn jump_catalog_ordinal(&mut self, ordinal: u64, effects: &mut Vec<Effect>) {
        let Some(row) = self.history_timeline.by_ordinal.get(&ordinal).cloned() else {
            return;
        };
        if self.reveal_catalog_row(&row) {
            self.history_timeline.pending_ordinal = None;
            return;
        }
        self.request_history_around(&row.target, effects);
    }

    fn reveal_catalog_row(&mut self, row: &CatalogRow) -> bool {
        let Some(entry_id) = self.history_entry_id_for_timeline(
            row.source,
            row.message_id.as_deref(),
            row.display_segment,
        ) else {
            return false;
        };
        self.reveal_entry(entry_id)
    }

    fn reveal_entry(&mut self, entry_id: EntryId) -> bool {
        let Some(index) = self.scrollback.index_of_id(entry_id) else {
            return false;
        };
        if !self.scrollback.jump_to_entry(entry_id) {
            self.scrollback.scroll_to_entry_top(index);
        }
        true
    }

    fn push_timeline_request(
        &mut self,
        query: HistoryTimelineQuery,
        jump_ordinal: Option<u64>,
        effects: &mut Vec<Effect>,
    ) -> bool {
        let Some(cut) = self.history_cut.clone() else {
            return false;
        };
        let Some(session_id) = self.session.session_id.clone() else {
            return false;
        };
        if jump_ordinal.is_some() {
            self.history_timeline.jump_seq = self.history_timeline.jump_seq.wrapping_add(1);
            self.history_timeline.pending_ordinal = jump_ordinal;
        }
        self.history_timeline.request_seq = self.history_timeline.request_seq.wrapping_add(1);
        let request_id = self.history_timeline.request_seq;
        self.history_timeline.inflight = Some(request_id);
        effects.push(Effect::ReadHistoryTimeline {
            agent_id: self.session.id,
            session_id,
            binding_epoch: self.session_binding_epoch,
            request_id,
            generation: self.history_timeline.generation,
            cut,
            limit: TIMELINE_PAGE_LIMIT,
            query,
            jump_ordinal,
            jump_seq: self.history_timeline.jump_seq,
        });
        true
    }

    fn live_timeline_ids(&self) -> Vec<EntryId> {
        let prefix = self.history_prefix_len();
        if prefix == 0 && self.history_interval_entry_ids().is_none() {
            return Vec::new();
        }
        self.scrollback
            .turns()
            .iter()
            .filter(|turn| turn.prompt_index >= prefix)
            .filter_map(|turn| {
                self.scrollback
                    .entry(turn.prompt_index)
                    .map(|entry| entry.id)
            })
            .collect()
    }

    fn active_global_ordinal(&self, catalog_total: u64, live: &[EntryId]) -> Option<u64> {
        let turn_idx = self.scrollback.active_turn_for_viewport()?;
        let prompt_index = self.scrollback.turns().get(turn_idx)?.prompt_index;
        let entry_id = self.scrollback.entry(prompt_index)?.id;
        if prompt_index >= self.history_prefix_len() && self.history_prefix_len() > 0 {
            let position = live.iter().position(|id| *id == entry_id)?;
            return Some(catalog_total + position as u64);
        }
        let message_id = self.scrollback.entry(prompt_index).and_then(|entry| {
            let RenderBlock::UserPrompt(block) = &entry.block else {
                return None;
            };
            block.message_id.clone()
        });
        self.history_timeline
            .by_ordinal
            .iter()
            .find_map(|(ordinal, row)| {
                let id = self.history_entry_id_for_timeline(
                    row.source,
                    row.message_id.as_deref(),
                    row.display_segment,
                )?;
                (id == entry_id
                    && message_id
                        .as_ref()
                        .zip(row.message_id.as_ref())
                        .is_none_or(|(left, right)| left == right))
                .then_some(*ordinal)
            })
    }
}

pub(in crate::app) fn visible_history_session(
    app: &AppView,
) -> Option<acp_transport::protocol::SessionId> {
    let ActiveView::Agent(id) = app.active_view else {
        return None;
    };
    let agent = app.agents.get(&id)?;
    if let Some(child_sid) = agent.active_subagent.as_deref()
        && let Some(child) = agent.subagent_views.get(child_sid)
    {
        return child.session.session_id.clone();
    }
    agent.session.session_id.clone()
}

/// At most one catalog read for the agent whose transcript is on screen.
pub(in crate::app) fn ensure_visible_history_timeline(app: &mut AppView) -> Vec<Effect> {
    if !app.appearance.show_timeline {
        return Vec::new();
    }
    let ActiveView::Agent(id) = app.active_view else {
        return Vec::new();
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return Vec::new();
    };
    let mut effects = Vec::new();
    if let Some(child_sid) = agent.active_subagent.clone()
        && agent.subagent_views.contains_key(&child_sid)
    {
        if let Some(child) = agent.subagent_views.get_mut(&child_sid) {
            child.ensure_history_timeline(&mut effects);
        }
        return effects;
    }
    agent.ensure_history_timeline(&mut effects);
    effects
}

/// Clear a blocked catalog read for the visible owner. Re-enabling the rail
/// is an explicit retry; closing it never re-requests toward an error.
pub(in crate::app) fn resume_visible_history_timeline(app: &mut AppView) {
    let ActiveView::Agent(id) = app.active_view else {
        return;
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return;
    };
    if let Some(child_sid) = agent.active_subagent.clone()
        && agent.subagent_views.contains_key(&child_sid)
    {
        if let Some(child) = agent.subagent_views.get_mut(&child_sid) {
            child.resume_history_timeline_reads();
        }
        return;
    }
    agent.resume_history_timeline_reads();
}

#[cfg(test)]
impl crate::app::root::AppView {
    /// The shared fixture lives in `root::tests`, not on `AppView`.
    /// Callers outside that module resolve it here.
    pub(crate) fn test_app_with_agent() -> Self {
        crate::app::root::tests::test_app_with_agent()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::actions::{Action, Effect, TaskResult};
    use crate::app::root::dispatch;
    use crate::app::session::AgentId;
    use crate::views::timeline::{RailViewport, TimelineHit, compute_rail};
    use acp_transport::protocol as acp;
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;
    use shell::session::storage::{
        HistoryEntity, HistoryPageQuery, HistoryPageResponse, HistoryRow, HistorySourceKey,
        HistoryTimelineEntry, HistoryTimelineQuery, HistoryTimelineResponse, HistoryTimelineTarget,
        SessionUpdate,
    };

    fn session() -> acp::SessionId {
        acp::SessionId::new("timeline-session")
    }

    fn source(record: u64) -> HistorySourceKey {
        HistorySourceKey { record, part: 0 }
    }

    fn user_row(
        session_id: &acp::SessionId,
        record: u64,
        message_id: &str,
        text: &str,
    ) -> HistoryRow {
        let mut meta = serde_json::Map::new();
        meta.insert(
            "messageId".into(),
            serde_json::Value::String(message_id.to_string()),
        );
        HistoryRow {
            source: source(record),
            entity: Some(HistoryEntity::UserMessage {
                message_id: message_id.to_string(),
            }),
            fragment: None,
            update: SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
                session_id.clone(),
                acp::SessionUpdate::UserMessageChunk(
                    acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(
                        text.to_string(),
                    )))
                    .meta(Some(meta)),
                ),
            ))),
        }
    }

    fn page(
        session_id: &acp::SessionId,
        rows: Vec<HistoryRow>,
        older: Option<&str>,
        newer: Option<&str>,
        target: Option<HistorySourceKey>,
    ) -> HistoryPageResponse {
        HistoryPageResponse {
            session_id: session_id.clone(),
            cut: "cut".into(),
            rows,
            range: None,
            older_cursor: older.map(str::to_string),
            newer_cursor: newer.map(str::to_string),
            target_source: target,
            descendant_ownership: None,
        }
    }

    fn catalog_row(
        ordinal: u64,
        record: u64,
        message_id: &str,
        preview: &str,
        target: &str,
    ) -> HistoryTimelineTarget {
        HistoryTimelineTarget {
            entry: HistoryTimelineEntry {
                ordinal,
                source: source(record),
                message_id: Some(message_id.to_string()),
                display_segment: None,
                preview: preview.to_string(),
            },
            target: target.to_string(),
        }
    }

    fn catalog(
        session_id: &acp::SessionId,
        total: u64,
        entries: Vec<HistoryTimelineTarget>,
    ) -> HistoryTimelineResponse {
        HistoryTimelineResponse {
            session_id: session_id.clone(),
            cut: "cut".into(),
            total_entries: total,
            entries,
            older_cursor: None,
        }
    }

    fn armed() -> (crate::app::root::AppView, AgentId, acp::SessionId) {
        let mut app = crate::app::root::tests::test_app_with_agent();
        let id = AgentId(0);
        let session_id = session();
        let agent = app.agents.get_mut(&id).unwrap();
        agent.session.session_id = Some(session_id.clone());
        agent.session_binding_epoch = 4;
        agent.history_cut = Some("cut".into());
        agent.scrollback.prepare_layout(80, 20);
        (app, id, session_id)
    }

    fn show_rail(app: &mut crate::app::root::AppView, enabled: bool) {
        let mut config = app.appearance.clone();
        config.show_timeline = enabled;
        app.set_appearance(config);
    }

    fn timeline_effects(effects: &[Effect]) -> Vec<&Effect> {
        effects
            .iter()
            .filter(|effect| matches!(effect, Effect::ReadHistoryTimeline { .. }))
            .collect()
    }

    #[test]
    fn disabled_timeline_sends_no_catalog_request() {
        let (mut app, id, _) = armed();
        show_rail(&mut app, false);
        let mut effects = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .ensure_history_timeline(&mut effects);
        assert!(
            effects.is_empty(),
            "the agent helper is not the enable gate"
        );
        let visible = ensure_visible_history_timeline(&mut app);
        assert!(visible.is_empty());
    }

    #[test]
    fn enabling_requests_one_newest_page() {
        let (mut app, _, _) = armed();
        show_rail(&mut app, true);
        let first = ensure_visible_history_timeline(&mut app);
        assert!(matches!(
            timeline_effects(&first).as_slice(),
            [Effect::ReadHistoryTimeline {
                query: HistoryTimelineQuery::Newest,
                limit: 128,
                jump_ordinal: None,
                ..
            }]
        ));
        let second = ensure_visible_history_timeline(&mut app);
        assert!(second.is_empty(), "the same cut does not request twice");
    }

    #[test]
    fn uncached_click_uses_around_ordinal_then_one_around() {
        let (mut app, id, session_id) = armed();
        show_rail(&mut app, true);
        let newest = ensure_visible_history_timeline(&mut app);
        let Effect::ReadHistoryTimeline {
            request_id,
            generation,
            binding_epoch,
            ..
        } = &newest[0]
        else {
            panic!("newest catalog request");
        };
        dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch: *binding_epoch,
                request_id: *request_id,
                generation: *generation,
                jump_ordinal: None,
                jump_seq: 0,
                result: Ok(catalog(&session_id, 400, vec![])),
            }),
            &mut app,
        );
        assert_eq!(app.agents[&id].global_timeline_total(), Some(400));
        let mut effects = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .activate_timeline_hit(TimelineHit::Tick(200), &mut effects);
        let Effect::ReadHistoryTimeline {
            query: HistoryTimelineQuery::AroundOrdinal { ordinal: 200 },
            jump_ordinal: Some(200),
            jump_seq,
            request_id,
            generation,
            binding_epoch,
            ..
        } = effects[0]
        else {
            panic!("far click must read around the ordinal, not walk Before: {effects:?}");
        };
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::ReadHistoryPage { .. }))
        );
        let jumped = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                jump_ordinal: Some(200),
                jump_seq,
                result: Ok(catalog(
                    &session_id,
                    400,
                    vec![
                        catalog_row(200, 90, "m-200", "same", "signed-200"),
                        catalog_row(201, 91, "m-201", "same", "signed-201"),
                    ],
                )),
            }),
            &mut app,
        );
        assert!(matches!(
            jumped.as_slice(),
            [Effect::ReadHistoryPage {
                query: HistoryPageQuery::Around { target },
                ..
            }] if target == "signed-200"
        ));
    }

    #[test]
    fn rapid_clicks_keep_the_latest_target() {
        let (mut app, id, session_id) = armed();
        show_rail(&mut app, true);
        let newest = ensure_visible_history_timeline(&mut app);
        let Effect::ReadHistoryTimeline {
            request_id,
            generation,
            binding_epoch,
            ..
        } = newest[0]
        else {
            panic!("newest");
        };
        dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                jump_ordinal: None,
                jump_seq: 0,
                result: Ok(catalog(&session_id, 400, vec![])),
            }),
            &mut app,
        );
        let mut first = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .activate_timeline_hit(TimelineHit::Tick(10), &mut first);
        let mut second = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .activate_timeline_hit(TimelineHit::Tick(11), &mut second);
        let Effect::ReadHistoryTimeline {
            request_id: first_id,
            jump_seq: first_seq,
            generation,
            binding_epoch,
            ..
        } = first[0]
        else {
            panic!("first click");
        };
        let Effect::ReadHistoryTimeline {
            request_id: second_id,
            jump_seq: second_seq,
            ..
        } = second[0]
        else {
            panic!("second click");
        };
        let late = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id: first_id,
                generation,
                jump_ordinal: Some(10),
                jump_seq: first_seq,
                result: Ok(catalog(
                    &session_id,
                    400,
                    vec![catalog_row(10, 10, "m-10", "same", "signed-10")],
                )),
            }),
            &mut app,
        );
        assert!(
            !late.iter().any(|effect| matches!(
                effect,
                Effect::ReadHistoryPage { query: HistoryPageQuery::Around { target }, .. }
                    if target == "signed-10"
            )),
            "a late catalog page may cache but must not jump the old ordinal"
        );
        let current = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id: second_id,
                generation,
                jump_ordinal: Some(11),
                jump_seq: second_seq,
                result: Ok(catalog(
                    &session_id,
                    400,
                    vec![catalog_row(11, 11, "m-11", "same", "signed-11")],
                )),
            }),
            &mut app,
        );
        assert!(matches!(
            current.as_slice(),
            [Effect::ReadHistoryPage {
                query: HistoryPageQuery::Around { target },
                ..
            }] if target == "signed-11"
        ));
    }

    #[test]
    fn wrong_cut_or_closed_timeline_does_not_jump() {
        let (mut app, id, session_id) = armed();
        show_rail(&mut app, true);
        let newest = ensure_visible_history_timeline(&mut app);
        let Effect::ReadHistoryTimeline {
            request_id,
            generation,
            binding_epoch,
            ..
        } = newest[0]
        else {
            panic!("newest");
        };
        let mut wrong = catalog(&session_id, 8, vec![catalog_row(1, 1, "m", "p", "signed")]);
        wrong.cut = "other".into();
        let effects = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                jump_ordinal: Some(1),
                jump_seq: 1,
                result: Ok(wrong),
            }),
            &mut app,
        );
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::ReadHistoryPage { .. })),
            "a mismatched cut must not jump"
        );
        assert_eq!(app.agents[&id].global_timeline_total(), None);

        show_rail(&mut app, false);
        app.agents
            .get_mut(&id)
            .unwrap()
            .suppress_timeline_jump(&mut Vec::new());
        let closed = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id,
                binding_epoch,
                request_id,
                generation,
                jump_ordinal: Some(1),
                jump_seq: 1,
                result: Ok(catalog(
                    &acp::SessionId::new("timeline-session"),
                    8,
                    vec![catalog_row(1, 1, "m", "p", "signed")],
                )),
            }),
            &mut app,
        );
        assert!(
            !closed
                .iter()
                .any(|effect| matches!(effect, Effect::ReadHistoryPage { .. })),
            "closing the rail must not turn a late catalog response into a jump"
        );
        assert!(!app.appearance.show_timeline);
    }

    #[test]
    fn around_keeps_gap_cursors_live_tail_and_failure_keeps_the_window() {
        let (mut app, id, session_id) = armed();
        show_rail(&mut app, true);
        {
            let agent = app.agents.get_mut(&id).unwrap();
            agent
                .scrollback
                .push_block(crate::scrollback::block::RenderBlock::notice("live tail"));
            agent.session.plan_mode_active = true;
        }
        let usage = app.agents[&id].session.session_usage.clone();
        let mut request = Vec::new();
        assert!(
            app.agents
                .get_mut(&id)
                .unwrap()
                .request_latest_history_page(&mut request)
        );
        let Effect::ReadHistoryPage {
            request_id,
            generation,
            binding_epoch,
            ..
        } = request[0]
        else {
            panic!("latest page");
        };
        let loaded = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryPageLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                query: HistoryPageQuery::Latest,
                result: Ok(page(
                    &session_id,
                    vec![user_row(&session_id, 30, "near", "near prompt")],
                    Some("older-near"),
                    None,
                    None,
                )),
            }),
            &mut app,
        );
        let before = transcript(&app.agents[&id]);
        assert!(before.contains("near prompt"));
        assert!(before.contains("live tail"));

        let newest = timeline_effects(&loaded);
        let Effect::ReadHistoryTimeline {
            request_id,
            generation,
            binding_epoch,
            ..
        } = newest[0]
        else {
            panic!("catalog");
        };
        dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch: *binding_epoch,
                request_id: *request_id,
                generation: *generation,
                jump_ordinal: None,
                jump_seq: 0,
                result: Ok(catalog(
                    &session_id,
                    400,
                    vec![catalog_row(3, 90, "far", "far prompt", "signed-far")],
                )),
            }),
            &mut app,
        );
        let mut click = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .activate_timeline_hit(TimelineHit::Tick(3), &mut click);
        let Effect::ReadHistoryPage {
            request_id,
            generation,
            binding_epoch,
            query: HistoryPageQuery::Around { target },
            ..
        } = &click[0]
        else {
            panic!("cached far ordinal jumps with one around: {click:?}");
        };
        assert_eq!(target, "signed-far");
        dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryPageLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch: *binding_epoch,
                request_id: *request_id,
                generation: *generation,
                query: HistoryPageQuery::Around {
                    target: target.clone(),
                },
                result: Err("offline".into()),
            }),
            &mut app,
        );
        let agent = &app.agents[&id];
        let failed = transcript(agent);
        assert!(
            failed.contains("near prompt"),
            "a failed around keeps the prefix"
        );
        assert!(failed.contains("live tail"));
        assert_eq!(agent.history_older_cursor(), Some("older-near"));
        assert!(!agent.history_has_gap());

        let mut retry = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .activate_timeline_hit(TimelineHit::Tick(3), &mut retry);
        let Effect::ReadHistoryPage {
            request_id,
            generation,
            binding_epoch,
            ..
        } = retry[0]
        else {
            panic!("retry around");
        };
        dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryPageLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                query: HistoryPageQuery::Around {
                    target: "signed-far".into(),
                },
                result: Ok(page(
                    &session_id,
                    vec![user_row(&session_id, 90, "far", "far prompt")],
                    Some("older-far"),
                    Some("newer-far"),
                    Some(source(90)),
                )),
            }),
            &mut app,
        );
        let agent = &app.agents[&id];
        let jumped = transcript(agent);
        assert!(jumped.contains("far prompt"));
        assert!(jumped.contains("live tail"));
        assert!(agent.history_has_gap());
        assert_eq!(agent.history_older_cursor(), Some("older-far"));
        assert_eq!(agent.history_newer_cursor(), Some("newer-far"));
        assert_eq!(agent.session.session_usage, usage);
        assert!(agent.session.plan_mode_active);
        assert!(!agent.scrollback.is_follow_mode());
        dispatch::dispatch(Action::GotoBottom, &mut app);
        assert!(app.agents[&id].scrollback.is_follow_mode());
    }

    /// Installs a one-row Latest prefix (catalog ordinal 7 maps to it), then
    /// adopts the Newest catalog with `extra` far entries. The live prompt is
    /// pushed first so it survives as the tail below the prefix.
    fn install_window_and_catalog(
        app: &mut crate::app::root::AppView,
        id: AgentId,
        session_id: &acp::SessionId,
        extra: Vec<HistoryTimelineTarget>,
        with_live_prompt: bool,
    ) {
        if with_live_prompt {
            app.agents
                .get_mut(&id)
                .unwrap()
                .scrollback
                .push_block(RenderBlock::UserPrompt(
                    crate::scrollback::blocks::UserPromptBlock::new("live prompt"),
                ));
        }
        let mut request = Vec::new();
        assert!(
            app.agents
                .get_mut(&id)
                .unwrap()
                .request_latest_history_page(&mut request)
        );
        let Effect::ReadHistoryPage {
            request_id,
            generation,
            binding_epoch,
            ..
        } = request[0]
        else {
            panic!("latest page");
        };
        let loaded = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryPageLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                query: HistoryPageQuery::Latest,
                result: Ok(page(
                    session_id,
                    vec![user_row(session_id, 30, "m-local", "local prompt")],
                    Some("older-near"),
                    None,
                    None,
                )),
            }),
            app,
        );
        let newest = timeline_effects(&loaded);
        let Effect::ReadHistoryTimeline {
            request_id,
            generation,
            binding_epoch,
            ..
        } = newest[0]
        else {
            panic!("newest catalog request");
        };
        let mut entries = vec![catalog_row(7, 30, "m-local", "local", "signed-local")];
        entries.extend(extra);
        dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch: *binding_epoch,
                request_id: *request_id,
                generation: *generation,
                jump_ordinal: None,
                jump_seq: 0,
                result: Ok(catalog(session_id, 400, entries)),
            }),
            app,
        );
    }

    fn far_around_in_flight(
        app: &mut crate::app::root::AppView,
        id: AgentId,
        ordinal: u64,
        target: &str,
    ) -> (u64, u64, u32) {
        let mut far = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .activate_timeline_hit(TimelineHit::Tick(ordinal), &mut far);
        let Effect::ReadHistoryPage {
            request_id,
            generation,
            binding_epoch,
            query: HistoryPageQuery::Around { target: around },
            ..
        } = &far[0]
        else {
            panic!("the cached far ordinal jumps with one around: {far:?}");
        };
        assert_eq!(around, target);
        (*request_id, *generation, *binding_epoch)
    }

    fn late_around_page(session_id: &acp::SessionId) -> HistoryPageResponse {
        page(
            session_id,
            vec![user_row(session_id, 90, "m-far", "far prompt")],
            Some("older-far"),
            Some("newer-far"),
            Some(source(90)),
        )
    }

    #[test]
    fn cached_local_target_discards_a_late_around_page() {
        let (mut app, id, session_id) = armed();
        show_rail(&mut app, true);
        install_window_and_catalog(
            &mut app,
            id,
            &session_id,
            vec![catalog_row(3, 90, "m-far", "far prompt", "signed-far")],
            false,
        );
        let local_id = app.agents[&id]
            .history_entry_id_for_timeline(source(30), Some("m-local"), None)
            .expect("local prompt installed");
        let (request_id, generation, binding_epoch) =
            far_around_in_flight(&mut app, id, 3, "signed-far");

        let mut click = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .activate_timeline_hit(TimelineHit::Tick(7), &mut click);
        assert!(
            !click
                .iter()
                .any(|effect| matches!(effect, Effect::ReadHistoryPage { .. })),
            "the local target reveals from the installed window without a body page"
        );
        assert_eq!(app.agents[&id].history_page_request, None);

        let dropped = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryPageLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                query: HistoryPageQuery::Around {
                    target: "signed-far".into(),
                },
                result: Ok(late_around_page(&session_id)),
            }),
            &mut app,
        );
        assert!(
            dropped.is_empty(),
            "the superseded around page must not adopt"
        );
        let foreign_session = acp::SessionId::new("foreign-session");
        let foreign = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryPageLoaded {
                agent_id: AgentId(99),
                session_id: foreign_session.clone(),
                binding_epoch,
                request_id,
                generation,
                query: HistoryPageQuery::Around {
                    target: "signed-far".into(),
                },
                result: Ok(late_around_page(&foreign_session)),
            }),
            &mut app,
        );
        assert!(
            foreign.is_empty(),
            "a foreign owner must not reach the view"
        );

        let agent = &app.agents[&id];
        let shown = transcript(agent);
        assert!(shown.contains("local prompt"));
        assert!(
            !shown.contains("far prompt"),
            "the superseded page must not replace the window"
        );
        assert_eq!(agent.history_older_cursor(), Some("older-near"));
        assert_eq!(agent.history_newer_cursor(), None);
        assert!(!agent.history_has_gap());
        assert_eq!(
            agent.history_entry_id_for_timeline(source(30), Some("m-local"), None),
            Some(local_id),
            "the installed window keeps its entry ids"
        );
        assert_eq!(agent.scrollback.index_of_id(local_id), Some(0));
        assert_eq!(agent.scrollback.current_turn(), Some(0));
    }

    #[test]
    fn live_target_discards_a_late_around_page() {
        let (mut app, id, session_id) = armed();
        show_rail(&mut app, true);
        install_window_and_catalog(
            &mut app,
            id,
            &session_id,
            vec![catalog_row(3, 90, "m-far", "far prompt", "signed-far")],
            true,
        );
        let live_id = {
            let agent = &app.agents[&id];
            let prompt_index = agent.scrollback.turn(1).unwrap().prompt_index;
            agent.scrollback.entry(prompt_index).unwrap().id
        };
        let (request_id, generation, binding_epoch) =
            far_around_in_flight(&mut app, id, 3, "signed-far");

        // Catalog total 400 plus one live prompt: the live ordinal is 400.
        let mut live = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .activate_timeline_hit(TimelineHit::Tick(400), &mut live);
        assert!(
            !live.iter().any(|effect| matches!(
                effect,
                Effect::ReadHistoryPage { .. } | Effect::ReadHistoryTimeline { .. }
            )),
            "the live target reveals locally and retires the pending jump"
        );
        let agent = &app.agents[&id];
        assert_eq!(agent.history_page_request, None);
        assert_eq!(
            agent.scrollback.current_turn(),
            Some(1),
            "the live prompt is revealed"
        );

        let dropped = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryPageLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                query: HistoryPageQuery::Around {
                    target: "signed-far".into(),
                },
                result: Ok(late_around_page(&session_id)),
            }),
            &mut app,
        );
        assert!(dropped.is_empty());
        let agent = &app.agents[&id];
        let shown = transcript(agent);
        assert!(shown.contains("local prompt"));
        assert!(shown.contains("live prompt"));
        assert!(!shown.contains("far prompt"));
        assert_eq!(agent.history_older_cursor(), Some("older-near"));
        assert!(!agent.history_has_gap());
        assert!(
            agent.scrollback.index_of_id(live_id).is_some(),
            "the superseded page must not remove the revealed live entry"
        );
        assert_eq!(agent.scrollback.current_turn(), Some(1));
    }

    /// Far window installed and revealed, then a jump to an uncached ordinal
    /// resolved through its catalog page so one Around body page (signed-mid)
    /// is in flight while the window still shows the far interval.
    fn mid_around_in_flight_over_far_window(
        app: &mut crate::app::root::AppView,
        id: AgentId,
        session_id: &acp::SessionId,
    ) -> (u64, u64, u32) {
        let (far_request, far_generation, far_epoch) =
            far_around_in_flight(app, id, 399, "signed-far");
        let adopted = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryPageLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch: far_epoch,
                request_id: far_request,
                generation: far_generation,
                query: HistoryPageQuery::Around {
                    target: "signed-far".into(),
                },
                result: Ok(late_around_page(session_id)),
            }),
            app,
        );
        assert!(
            adopted.is_empty(),
            "the far window adopts without further pages"
        );
        {
            let agent = &app.agents[&id];
            assert!(transcript(agent).contains("far prompt"));
            assert!(agent.history_has_gap());
            assert!(!agent.scrollback.is_follow_mode());
        }
        let mut uncached = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .activate_timeline_hit(TimelineHit::Tick(10), &mut uncached);
        let Effect::ReadHistoryTimeline {
            request_id,
            generation,
            binding_epoch,
            jump_ordinal,
            jump_seq,
            ..
        } = uncached[0]
        else {
            panic!("the uncached ordinal reads the catalog: {uncached:?}");
        };
        let resolved = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                jump_ordinal,
                jump_seq,
                result: Ok(catalog(
                    session_id,
                    400,
                    vec![catalog_row(10, 50, "m-mid", "mid prompt", "signed-mid")],
                )),
            }),
            app,
        );
        let Effect::ReadHistoryPage {
            request_id,
            generation,
            binding_epoch,
            query: HistoryPageQuery::Around { target },
            ..
        } = &resolved[0]
        else {
            panic!("the resolved ordinal jumps with one around: {resolved:?}");
        };
        assert_eq!(target, "signed-mid");
        (*request_id, *generation, *binding_epoch)
    }

    #[test]
    fn down_to_live_abandons_the_pending_jump() {
        let (mut app, id, session_id) = armed();
        show_rail(&mut app, true);
        install_window_and_catalog(
            &mut app,
            id,
            &session_id,
            vec![catalog_row(399, 90, "m-far", "far prompt", "signed-far")],
            false,
        );
        let (request_id, generation, binding_epoch) =
            mid_around_in_flight_over_far_window(&mut app, id, &session_id);

        let mut down = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .activate_timeline_hit(TimelineHit::Down, &mut down);
        assert!(
            !down.iter().any(|effect| matches!(
                effect,
                Effect::ReadHistoryPage { .. } | Effect::ReadHistoryTimeline { .. }
            )),
            "returning to the live tail must not arm new reads"
        );
        assert!(app.agents[&id].scrollback.is_follow_mode());
        assert_eq!(app.agents[&id].history_page_request, None);

        let dropped = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryPageLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                query: HistoryPageQuery::Around {
                    target: "signed-mid".into(),
                },
                result: Ok(page(
                    &session_id,
                    vec![user_row(&session_id, 50, "m-mid", "mid prompt")],
                    Some("older-mid"),
                    Some("newer-mid"),
                    Some(source(50)),
                )),
            }),
            &mut app,
        );
        assert!(dropped.is_empty());
        let agent = &app.agents[&id];
        let shown = transcript(agent);
        assert!(shown.contains("far prompt"));
        assert!(
            !shown.contains("mid prompt"),
            "the abandoned jump must not replace the live-bottom window"
        );
        assert_eq!(agent.history_older_cursor(), Some("older-far"));
        assert_eq!(agent.history_newer_cursor(), Some("newer-far"));
        assert!(agent.history_has_gap());
        assert!(agent.scrollback.is_follow_mode());
    }

    #[test]
    fn goto_bottom_abandons_the_pending_jump() {
        let (mut app, id, session_id) = armed();
        show_rail(&mut app, true);
        install_window_and_catalog(
            &mut app,
            id,
            &session_id,
            vec![catalog_row(399, 90, "m-far", "far prompt", "signed-far")],
            false,
        );
        let (request_id, generation, binding_epoch) =
            mid_around_in_flight_over_far_window(&mut app, id, &session_id);

        let effects = dispatch::dispatch(Action::GotoBottom, &mut app);
        assert!(
            !effects.iter().any(|effect| matches!(
                effect,
                Effect::ReadHistoryPage {
                    query: HistoryPageQuery::Around { .. },
                    ..
                } | Effect::ReadHistoryTimeline { .. }
            )),
            "goto bottom retires the pending jump and must not re-arm an around: {effects:?}"
        );
        assert!(app.agents[&id].scrollback.is_follow_mode());
        let passive_request = effects.iter().find_map(|effect| match effect {
            Effect::ReadHistoryPage {
                request_id,
                query: HistoryPageQuery::Before { .. } | HistoryPageQuery::After { .. },
                ..
            } => Some(*request_id),
            _ => None,
        });
        assert_eq!(app.agents[&id].history_page_request, passive_request);
        assert_ne!(
            passive_request,
            Some(request_id),
            "the retired Around is never retained"
        );

        let dropped = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryPageLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                query: HistoryPageQuery::Around {
                    target: "signed-mid".into(),
                },
                result: Ok(page(
                    &session_id,
                    vec![user_row(&session_id, 50, "m-mid", "mid prompt")],
                    Some("older-mid"),
                    Some("newer-mid"),
                    Some(source(50)),
                )),
            }),
            &mut app,
        );
        assert!(dropped.is_empty());
        let agent = &app.agents[&id];
        let shown = transcript(agent);
        assert!(shown.contains("far prompt"));
        assert!(!shown.contains("mid prompt"));
        assert_eq!(agent.history_older_cursor(), Some("older-far"));
        assert_eq!(agent.history_newer_cursor(), Some("newer-far"));
        assert!(agent.scrollback.is_follow_mode());
    }

    #[test]
    fn closing_the_rail_without_a_pending_jump_keeps_the_latest_load_alive() {
        let (mut app, id, session_id) = armed();
        app.agents
            .get_mut(&id)
            .unwrap()
            .arm_history_load(Some("cut".into()));
        show_rail(&mut app, true);
        let newest = ensure_visible_history_timeline(&mut app);
        let Effect::ReadHistoryTimeline {
            request_id: newest_id,
            generation: catalog_generation,
            binding_epoch: catalog_epoch,
            ..
        } = newest[0]
        else {
            panic!("newest catalog request");
        };
        let mut request = Vec::new();
        assert!(
            app.agents
                .get_mut(&id)
                .unwrap()
                .request_latest_history_page(&mut request)
        );
        let Effect::ReadHistoryPage {
            request_id: latest_id,
            generation: latest_generation,
            binding_epoch: latest_epoch,
            ..
        } = request[0]
        else {
            panic!("latest page");
        };

        show_rail(&mut app, false);
        let mut effects = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .suppress_timeline_jump(&mut effects);
        assert!(
            effects.is_empty(),
            "without a pending jump the close retires nothing"
        );
        {
            let agent = &app.agents[&id];
            assert_eq!(agent.history_page_request, Some(latest_id));
            assert!(agent.history_latest_pending);
            assert_eq!(agent.history_timeline.inflight, Some(newest_id));
        }

        dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch: catalog_epoch,
                request_id: newest_id,
                generation: catalog_generation,
                jump_ordinal: None,
                jump_seq: 0,
                result: Ok(catalog(&session_id, 400, vec![])),
            }),
            &mut app,
        );
        let adopted = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryPageLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch: latest_epoch,
                request_id: latest_id,
                generation: latest_generation,
                query: HistoryPageQuery::Latest,
                result: Ok(page(
                    &session_id,
                    vec![user_row(&session_id, 30, "m-local", "local prompt")],
                    Some("older-near"),
                    None,
                    None,
                )),
            }),
            &mut app,
        );
        let agent = &app.agents[&id];
        assert!(
            transcript(agent).contains("local prompt"),
            "the initial Latest is not left pending forever"
        );
        assert!(!agent.history_latest_pending);
        assert_eq!(agent.history_page_request, None);
        assert!(timeline_effects(&adopted).is_empty(), "the rail is closed");

        show_rail(&mut app, true);
        let reopen = ensure_visible_history_timeline(&mut app);
        assert!(
            reopen.is_empty(),
            "the closed interval kept the same-cut catalog cache"
        );
        assert_eq!(app.agents[&id].global_timeline_total(), Some(400));
    }

    #[test]
    fn late_catalog_response_after_abandon_only_fills_the_cache() {
        let (mut app, id, session_id) = armed();
        show_rail(&mut app, true);
        install_window_and_catalog(&mut app, id, &session_id, vec![], false);
        let mut uncached = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .activate_timeline_hit(TimelineHit::Tick(3), &mut uncached);
        let Effect::ReadHistoryTimeline {
            request_id,
            generation,
            binding_epoch,
            jump_ordinal,
            jump_seq,
            ..
        } = uncached[0]
        else {
            panic!("the uncached ordinal reads the catalog: {uncached:?}");
        };
        let mut abandon = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .activate_timeline_hit(TimelineHit::Tick(7), &mut abandon);
        assert!(abandon.is_empty(), "the local target reveals without reads");

        let late = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                jump_ordinal,
                jump_seq,
                result: Ok(catalog(
                    &session_id,
                    400,
                    vec![catalog_row(3, 90, "m-far", "far preview", "signed-far")],
                )),
            }),
            &mut app,
        );
        assert!(
            !late
                .iter()
                .any(|effect| matches!(effect, Effect::ReadHistoryPage { .. })),
            "the retired jump's catalog response must not issue a body page"
        );
        let agent = &app.agents[&id];
        assert_eq!(agent.global_timeline_total(), Some(400));
        assert_eq!(
            agent.timeline_hover_text(3),
            Some("far preview".to_string())
        );
        let shown = transcript(agent);
        assert!(shown.contains("local prompt"));
        assert!(!shown.contains("far prompt"));
        assert_eq!(agent.history_older_cursor(), Some("older-near"));
        assert!(!agent.history_has_gap());

        let mut cached = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .activate_timeline_hit(TimelineHit::Tick(3), &mut cached);
        assert!(
            matches!(
                cached.as_slice(),
                [Effect::ReadHistoryPage {
                    query: HistoryPageQuery::Around { target },
                    ..
                }] if target == "signed-far"
            ),
            "the cached ordinal still jumps with one around, without a second catalog read"
        );
        assert!(
            !cached
                .iter()
                .any(|effect| matches!(effect, Effect::ReadHistoryTimeline { .. }))
        );
    }

    #[test]
    fn failed_newest_does_not_auto_retry() {
        let (mut app, id, session_id) = armed();
        show_rail(&mut app, true);
        let newest = ensure_visible_history_timeline(&mut app);
        let Effect::ReadHistoryTimeline {
            request_id,
            generation,
            binding_epoch,
            ..
        } = newest[0]
        else {
            panic!("newest catalog request");
        };
        let failed = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                jump_ordinal: None,
                jump_seq: 0,
                result: Err("storage offline".into()),
            }),
            &mut app,
        );
        assert!(
            !failed
                .iter()
                .any(|effect| matches!(effect, Effect::ReadHistoryTimeline { .. })),
            "a failed catalog read must not immediately re-request (regression: it did)"
        );
        {
            let agent = &app.agents[&id];
            assert_eq!(agent.history_timeline.inflight, None);
            assert!(agent.history_timeline.read_blocked);
            assert_eq!(agent.global_timeline_total(), None);
        }
        let idle = dispatch::dispatch(Action::GotoBottom, &mut app);
        assert!(
            !idle
                .iter()
                .any(|effect| matches!(effect, Effect::ReadHistoryTimeline { .. })),
            "background completions must not retry a blocked read"
        );
        let mut retried = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .ensure_history_timeline(&mut retried);
        assert!(retried.is_empty(), "ensure stays quiet while blocked");
        assert!(
            ensure_visible_history_timeline(&mut app).is_empty(),
            "the paint-time ensure stays quiet while blocked"
        );
    }

    #[test]
    fn reenabling_the_rail_retries_a_failed_newest_once() {
        let (mut app, id, session_id) = armed();
        show_rail(&mut app, true);
        let newest = ensure_visible_history_timeline(&mut app);
        let Effect::ReadHistoryTimeline {
            request_id,
            generation,
            binding_epoch,
            ..
        } = newest[0]
        else {
            panic!("newest catalog request");
        };
        dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                jump_ordinal: None,
                jump_seq: 0,
                result: Err("storage offline".into()),
            }),
            &mut app,
        );
        let closed = dispatch::dispatch(Action::SetTimeline(false), &mut app);
        assert!(
            !closed
                .iter()
                .any(|effect| matches!(effect, Effect::ReadHistoryTimeline { .. })),
            "closing the rail must not re-request toward the recorded error"
        );
        let reopened = dispatch::dispatch(Action::SetTimeline(true), &mut app);
        assert!(
            matches!(
                timeline_effects(&reopened).as_slice(),
                [Effect::ReadHistoryTimeline {
                    query: HistoryTimelineQuery::Newest,
                    ..
                }]
            ),
            "re-enabling retries the failed read exactly once"
        );
        let retry = timeline_effects(&reopened);
        let Effect::ReadHistoryTimeline {
            request_id,
            generation,
            binding_epoch,
            ..
        } = retry[0]
        else {
            unreachable!("the matches! above proved a Newest effect");
        };
        dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch: *binding_epoch,
                request_id: *request_id,
                generation: *generation,
                jump_ordinal: None,
                jump_seq: 0,
                result: Ok(catalog(&session_id, 400, vec![])),
            }),
            &mut app,
        );
        {
            let agent = &app.agents[&id];
            assert_eq!(
                agent.global_timeline_total(),
                Some(400),
                "the retry recovers"
            );
            assert!(!agent.history_timeline.read_blocked);
        }
        assert!(
            ensure_visible_history_timeline(&mut app).is_empty(),
            "a recovered rail is quiet again"
        );
    }

    #[test]
    fn failed_hover_read_keeps_cache_and_retries_only_on_click() {
        let (mut app, id, session_id) = armed();
        show_rail(&mut app, true);
        install_window_and_catalog(&mut app, id, &session_id, vec![], false);
        app.agents.get_mut(&id).unwrap().timeline_hover = Some(TimelineHit::Tick(3));
        let hover = ensure_visible_history_timeline(&mut app);
        let Effect::ReadHistoryTimeline {
            request_id,
            generation,
            binding_epoch,
            ..
        } = hover[0]
        else {
            panic!("a hover miss reads the catalog once: {hover:?}");
        };
        let failed = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                jump_ordinal: None,
                jump_seq: 0,
                result: Err("storage offline".into()),
            }),
            &mut app,
        );
        assert!(
            !failed
                .iter()
                .any(|effect| matches!(effect, Effect::ReadHistoryTimeline { .. }))
        );
        {
            let agent = &app.agents[&id];
            assert_eq!(
                agent.global_timeline_total(),
                Some(400),
                "the cache survives"
            );
            assert_eq!(agent.history_older_cursor(), Some("older-near"));
            assert!(!agent.history_has_gap());
            assert!(transcript(agent).contains("local prompt"));
            assert_eq!(agent.history_timeline.inflight, None);
            assert!(agent.history_timeline.read_blocked);
        }
        assert!(
            ensure_visible_history_timeline(&mut app).is_empty(),
            "no automatic retry after the hover read failed"
        );

        let mut click = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .activate_timeline_hit(TimelineHit::Tick(3), &mut click);
        assert!(
            matches!(
                timeline_effects(&click).as_slice(),
                [Effect::ReadHistoryTimeline {
                    query: HistoryTimelineQuery::AroundOrdinal { ordinal: 3 },
                    ..
                }]
            ),
            "an explicit click retries the blocked read with one query"
        );
        let agent = &app.agents[&id];
        assert!(!agent.history_timeline.read_blocked);
        assert!(agent.history_timeline.inflight.is_some());
    }

    #[test]
    fn a_retired_reads_late_failure_does_not_block_the_active_request() {
        let (mut app, id, session_id) = armed();
        show_rail(&mut app, true);
        install_window_and_catalog(&mut app, id, &session_id, vec![], false);
        let mut first = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .activate_timeline_hit(TimelineHit::Tick(3), &mut first);
        let Effect::ReadHistoryTimeline {
            request_id: first_id,
            jump_seq: first_seq,
            ..
        } = first[0]
        else {
            panic!("the first uncached click reads the catalog");
        };
        let mut second = Vec::new();
        app.agents
            .get_mut(&id)
            .unwrap()
            .activate_timeline_hit(TimelineHit::Tick(5), &mut second);
        let Effect::ReadHistoryTimeline {
            request_id: second_id,
            jump_ordinal: second_ordinal,
            jump_seq: second_seq,
            generation,
            binding_epoch,
            ..
        } = second[0]
        else {
            panic!("the second uncached click supersedes the first");
        };

        let stale = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id: first_id,
                generation,
                jump_ordinal: Some(3),
                jump_seq: first_seq,
                result: Err("late failure of the retired read".into()),
            }),
            &mut app,
        );
        assert!(
            stale.is_empty(),
            "the retired read's late failure does nothing"
        );
        {
            let agent = &app.agents[&id];
            assert_eq!(
                agent.history_timeline.inflight,
                Some(second_id),
                "the newer nonce survives an older read's failure"
            );
            assert!(
                !agent.history_timeline.read_blocked,
                "an old failure must not block the newer read"
            );
        }
        let current = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id: second_id,
                generation,
                jump_ordinal: second_ordinal,
                jump_seq: second_seq,
                result: Ok(catalog(
                    &session_id,
                    400,
                    vec![catalog_row(5, 11, "m-11", "mid prompt", "signed-11")],
                )),
            }),
            &mut app,
        );
        assert!(
            matches!(
                current.as_slice(),
                [Effect::ReadHistoryPage {
                    query: HistoryPageQuery::Around { target },
                    ..
                }] if target == "signed-11"
            ),
            "the newer read still resolves and jumps"
        );
        assert!(!app.agents[&id].history_timeline.read_blocked);
    }

    #[test]
    fn invalid_response_for_the_active_read_blocks_automatic_retries() {
        let (mut app, id, session_id) = armed();
        show_rail(&mut app, true);
        let newest = ensure_visible_history_timeline(&mut app);
        let Effect::ReadHistoryTimeline {
            request_id,
            generation,
            binding_epoch,
            ..
        } = newest[0]
        else {
            panic!("newest catalog request");
        };
        let mut wrong = catalog(&session_id, 8, vec![catalog_row(1, 1, "m", "p", "signed")]);
        wrong.cut = "other".into();
        let rejected = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                jump_ordinal: None,
                jump_seq: 0,
                result: Ok(wrong),
            }),
            &mut app,
        );
        assert!(
            !rejected
                .iter()
                .any(|effect| matches!(effect, Effect::ReadHistoryTimeline { .. })),
            "an unusable response of the active read must not immediately re-request"
        );
        let agent = &app.agents[&id];
        assert!(agent.history_timeline.read_blocked);
        assert_eq!(
            agent.global_timeline_total(),
            None,
            "a mismatched cut fills no cache"
        );
        assert!(
            ensure_visible_history_timeline(&mut app).is_empty(),
            "ensure stays quiet while blocked"
        );
    }

    #[test]
    fn wrong_binding_response_leaves_the_active_nonce_alone() {
        let (mut app, id, session_id) = armed();
        show_rail(&mut app, true);
        install_window_and_catalog(&mut app, id, &session_id, vec![], false);
        app.agents.get_mut(&id).unwrap().timeline_hover = Some(TimelineHit::Tick(3));
        let hover = ensure_visible_history_timeline(&mut app);
        let Effect::ReadHistoryTimeline {
            request_id,
            generation,
            binding_epoch,
            ..
        } = hover[0]
        else {
            panic!("hover miss read");
        };
        let foreign = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch: binding_epoch.wrapping_add(1),
                request_id,
                generation,
                jump_ordinal: None,
                jump_seq: 0,
                result: Err("late packet of a retired binding".into()),
            }),
            &mut app,
        );
        assert!(
            !foreign
                .iter()
                .any(|effect| matches!(effect, Effect::ReadHistoryTimeline { .. }))
        );
        {
            let agent = &app.agents[&id];
            assert_eq!(
                agent.history_timeline.inflight,
                Some(request_id),
                "a wrong-binding packet with the same nonce must not clear the active read"
            );
            assert!(!agent.history_timeline.read_blocked);
        }
        assert!(ensure_visible_history_timeline(&mut app).is_empty());

        let admitted = dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                jump_ordinal: None,
                jump_seq: 0,
                result: Ok(catalog(
                    &session_id,
                    400,
                    vec![catalog_row(3, 90, "m-far", "far preview", "signed-far")],
                )),
            }),
            &mut app,
        );
        assert!(
            !admitted
                .iter()
                .any(|effect| matches!(effect, Effect::ReadHistoryPage { .. })),
            "the hover read carries no jump"
        );
        let agent = &app.agents[&id];
        assert_eq!(agent.history_timeline.inflight, None);
        assert_eq!(
            agent.timeline_hover_text(3),
            Some("far preview".to_string()),
            "the correctly bound response still fills the cache"
        );
    }

    #[test]
    fn mouse_click_on_the_rail_requests_the_global_ordinal() {
        let (mut app, id, session_id) = armed();
        show_rail(&mut app, true);
        let newest = ensure_visible_history_timeline(&mut app);
        let Effect::ReadHistoryTimeline {
            request_id,
            generation,
            binding_epoch,
            ..
        } = newest[0]
        else {
            panic!("newest");
        };
        dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryTimelineLoaded {
                agent_id: id,
                session_id: session_id.clone(),
                binding_epoch,
                request_id,
                generation,
                jump_ordinal: None,
                jump_seq: 0,
                result: Ok(catalog(&session_id, 400, vec![])),
            }),
            &mut app,
        );
        let rail = compute_rail(
            Rect::new(0, 0, 80, 20),
            76,
            400,
            RailViewport {
                active: Some(0),
                up_target: None,
                down_target: Some(1),
                at_bottom: false,
                down_returns_live: true,
            },
        )
        .unwrap();
        let row = rail.ticks_y;
        app.agents.get_mut(&id).unwrap().timeline_rail = Some(rail);
        let mut effects = Vec::new();
        app.agents.get_mut(&id).unwrap().handle_mouse(
            &MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 76,
                row,
                modifiers: KeyModifiers::NONE,
            },
            &mut effects,
        );
        assert!(matches!(
            effects.first(),
            Some(Effect::ReadHistoryTimeline {
                query: HistoryTimelineQuery::AroundOrdinal { ordinal: 0 },
                ..
            })
        ));
    }

    #[test]
    fn fullscreen_child_page_is_adopted_on_the_child() {
        let (mut app, id, _) = armed();
        let child_session = acp::SessionId::new("timeline-child");
        let mut child = crate::app::agent_view::test_fixtures::make_agent();
        child.session.session_id = Some(child_session.clone());
        child.session_binding_epoch = 9;
        child.history_cut = Some("cut".into());
        child
            .scrollback
            .push_block(crate::scrollback::block::RenderBlock::notice("child tail"));
        let mut effects = Vec::new();
        child.arm_history_load(Some("cut".into()));
        assert!(child.request_latest_history_page(&mut effects));
        let Effect::ReadHistoryPage {
            request_id,
            generation,
            binding_epoch,
            ..
        } = effects[0]
        else {
            panic!("child page");
        };
        app.agents
            .get_mut(&id)
            .unwrap()
            .subagent_views
            .insert("child".into(), Box::new(child));
        app.agents
            .get_mut(&id)
            .unwrap()
            .open_subagent_fullscreen("child".into());
        dispatch::dispatch(
            Action::TaskComplete(TaskResult::HistoryPageLoaded {
                agent_id: AgentId(99),
                session_id: child_session.clone(),
                binding_epoch,
                request_id,
                generation,
                query: HistoryPageQuery::Latest,
                result: Ok(page(
                    &child_session,
                    vec![user_row(&child_session, 4, "child-msg", "child prompt")],
                    Some("child-older"),
                    None,
                    None,
                )),
            }),
            &mut app,
        );
        let child = &app.agents[&id].subagent_views["child"];
        assert!(transcript(child).contains("child prompt"));
        assert!(transcript(child).contains("child tail"));
        assert_eq!(child.history_older_cursor(), Some("child-older"));
        assert!(
            app.agents[&id].history_page_request.is_none(),
            "the parent window is not the child page owner"
        );
    }

    fn transcript(agent: &AgentView) -> String {
        (0..agent.scrollback.len())
            .filter_map(|index| agent.scrollback.get(index))
            .map(|entry| format!("{:?}", entry.block))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
