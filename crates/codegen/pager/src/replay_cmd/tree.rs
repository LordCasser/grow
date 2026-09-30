use std::collections::{HashMap, HashSet};
use std::io::stdout;
use std::ops::Range;
use std::time::{Duration, Instant, UNIX_EPOCH};

use anyhow::Result;
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::{Terminal, backend::CrosstermBackend, buffer::Buffer, layout::Rect};
use shell::session::storage::transcript::{TranscriptNode, TranscriptSession, capture_tree_at};

use super::browser::{BrowseAction, Browser};
use super::idle::{GapKind, TimeMap};
use super::panel::{Panel, PanelAction, PanelInfo};
use super::{
    GrowUpdate, MAX_EVENTS_PER_TICK, MAX_SPEED, PlayState, PlaybackClock, Player, ReplayArgs,
    SessionUpdate, TerminalGuard,
};
use crate::motion::FrameStamp;

struct Node {
    id: String,
    title: String,
    parent: Option<usize>,
    spawn_at_ms: Option<u64>,
    visible_children: HashSet<String>,
    workflow_children: HashMap<String, Vec<String>>,
    playable: bool,
    player: Player,
    browser: Browser,
}

struct Replay {
    nodes: Vec<Node>,
    by_id: HashMap<String, usize>,
    path: Vec<usize>,
    clock: PlaybackClock,
    time_map: TimeMap,
    cursor: usize,
    panel: Panel,
    panel_area: Rect,
    panel_press: Option<PanelAction>,
    last_notice: Option<String>,
    notice_frontier_ms: f64,
    total_events: usize,
}

fn activity_marker(event: &shell::session::storage::transcript::TranscriptEvent) -> bool {
    match &event.update {
        SessionUpdate::Acp(notification) => !matches!(
            &notification.update,
            acp_transport::protocol::SessionUpdate::CurrentModeUpdate(_)
        ),
        SessionUpdate::Grow(notification) => !matches!(
            &notification.update,
            GrowUpdate::GoalUpdated { .. }
                | GrowUpdate::WorkflowUpdated { .. }
                | GrowUpdate::SubagentProgress { .. }
                | GrowUpdate::ControlStateUpdate(_)
                | GrowUpdate::UiNotice(_)
        ),
        SessionUpdate::ResponseReplayProjection(_) => true,
    }
}

fn duration_label(ms: u64) -> String {
    let secs = ms / 1_000;
    let hours = secs / 3_600;
    let minutes = (secs % 3_600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        format!("{hours}h {minutes:02}m {seconds:02}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

impl Replay {
    fn new(sessions: Vec<(TranscriptNode, TranscriptSession)>, speed: f64, now: Instant) -> Self {
        let by_id: HashMap<_, _> = sessions
            .iter()
            .enumerate()
            .map(|(index, (node, _))| (node.session_id.clone(), index))
            .collect();
        let mut evidence = Vec::with_capacity(sessions.len());
        let mut nodes: Vec<_> = sessions
            .into_iter()
            .map(|(node, session)| {
                evidence.push((
                    session.activity,
                    session.recovery_gaps,
                    session.open_activity,
                    session.unknown_activity,
                ));
                let mut visible_children = HashSet::new();
                let mut workflow_children = HashMap::<String, Vec<String>>::new();
                for event in &session.events {
                    if let SessionUpdate::Grow(notification) = &event.update
                        && let GrowUpdate::SubagentSpawned {
                            child_session_id,
                            workflow_run_id,
                            ..
                        } = &notification.update
                    {
                        let first = visible_children.insert(child_session_id.clone());
                        if first && let Some(run_id) = workflow_run_id {
                            workflow_children
                                .entry(run_id.clone())
                                .or_default()
                                .push(child_session_id.clone());
                        }
                    }
                }
                Node {
                    title: node.title.unwrap_or_else(|| node.session_id.clone()),
                    parent: node.parent_id.and_then(|id| by_id.get(&id).copied()),
                    spawn_at_ms: node.spawn_at_ms,
                    visible_children,
                    workflow_children,
                    playable: false,
                    id: node.session_id,
                    player: Player::new(session.events),
                    browser: Browser::default(),
                }
            })
            .collect();
        if !nodes.is_empty() {
            nodes[0].playable = true;
        }
        // Only a card retained in its parent's final display sequence makes a
        // descendant reachable. Topology alone would leak rewound branches.
        for index in 1..nodes.len() {
            if let Some(parent) = nodes[index].parent {
                nodes[index].playable = nodes[parent].playable
                    && nodes[parent].visible_children.contains(&nodes[index].id);
            }
        }
        // In a child, recorded timestamps may predate the validated spawn or
        // the parent's displayed card. Resolve that causal floor once, before
        // building the S→P map, so the map and the actual queue agree.
        let mut anchors = vec![None; nodes.len()];
        for index in 0..nodes.len() {
            let card_at = nodes[index].parent.and_then(|parent| {
                nodes[parent]
                    .player
                    .queue
                    .iter()
                    .find_map(|item| match &item.event.update {
                        SessionUpdate::Grow(notification)
                            if matches!(
                                &notification.update,
                                GrowUpdate::SubagentSpawned { child_session_id, .. }
                                    if *child_session_id == nodes[index].id
                            ) =>
                        {
                            anchors[parent].map(|anchor: u64| {
                                anchor.saturating_add(item.due_ms.max(0.0) as u64)
                            })
                        }
                        _ => None,
                    })
            });
            anchors[index] = nodes[index]
                .player
                .first_source_ms
                .into_iter()
                .chain(nodes[index].spawn_at_ms)
                .chain(card_at)
                .max();
            if nodes[index].parent.is_some() && card_at.is_none() {
                nodes[index].player.estimated = true;
            }
        }
        let mut markers = Vec::new();
        let mut protected = Vec::<Range<u64>>::new();
        let mut recovery = Vec::<Range<u64>>::new();
        let mut open_starts = Vec::new();
        let mut last_source = None;
        let mut unknown_global = false;
        for (index, (active, recovered, open, unknown)) in evidence.into_iter().enumerate() {
            unknown_global |= unknown;
            let first = nodes[index].player.first_source_ms;
            let anchor = anchors[index];
            let shift = anchor
                .zip(first)
                .map_or(0, |(anchor, first)| anchor.saturating_sub(first));
            let move_range =
                |span: Range<u64>| span.start.saturating_add(shift)..span.end.saturating_add(shift);
            // Even an unseen branch can have occupied execution time, but its
            // hidden display updates do not extend the replay's time bounds.
            protected.extend(active.into_iter().map(move_range));
            recovery.extend(recovered.into_iter().map(move_range));
            open_starts.extend(open.into_iter().map(|start| start.saturating_add(shift)));
            if nodes[index].playable {
                if let Some(anchor) = anchor {
                    let mut first_effective = None;
                    let mut last_effective = None;
                    for item in &nodes[index].player.queue {
                        let at = anchor.saturating_add(item.due_ms.max(0.0) as u64);
                        first_effective.get_or_insert(at);
                        last_effective = Some(at);
                        if activity_marker(&item.event) {
                            markers.push(at);
                        }
                    }
                    markers.extend(first_effective);
                    markers.extend(last_effective);
                    last_source = last_source.into_iter().chain(last_effective).max();
                    if nodes[index].player.estimated
                        && let (Some(start), Some(end)) = (first_effective, last_effective)
                        && end > start
                    {
                        protected.push(start..end);
                    }
                } else if nodes[index].player.total > 0 {
                    unknown_global = true;
                }
            }
        }
        if let Some(last) = last_source {
            protected.extend(
                open_starts
                    .into_iter()
                    .filter(|start| *start < last)
                    .map(|start| start..last),
            );
        }
        if unknown_global
            && let (Some(start), Some(end)) = (markers.iter().min(), markers.iter().max())
        {
            protected.push(*start..*end);
        }
        let mut time_map = TimeMap::new(markers, protected, recovery);
        for index in 0..nodes.len() {
            let anchor = anchors[index].or(time_map.origin);
            if let Some(anchor) = anchor {
                for item in &mut nodes[index].player.queue {
                    let source = anchor.saturating_add(item.due_ms.max(0.0) as u64);
                    item.due_ms = time_map.to_playback(source);
                }
            }
            if nodes[index].playable
                && let Some(last) = nodes[index].player.queue.back()
            {
                time_map.playback_end = time_map.playback_end.max(last.due_ms);
            }
        }
        let total_events = nodes
            .iter()
            .filter(|node| node.playable)
            .map(|node| node.player.total)
            .sum();
        Self {
            nodes,
            by_id,
            path: vec![0],
            clock: PlaybackClock::new(speed, now),
            time_map,
            cursor: 0,
            panel: Panel::default(),
            panel_area: Rect::default(),
            panel_press: None,
            last_notice: None,
            notice_frontier_ms: 0.0,
            total_events,
        }
    }

    fn tick(&mut self, now: Instant) {
        if self.clock.state != PlayState::Playing {
            return;
        }
        let previous = self.notice_frontier_ms;
        let virtual_now = self.clock.now_ms(now);
        self.advance_to(virtual_now);
        let delivered_frontier = self.displayed_position(now);
        if let Some(gap) = self.time_map.crossed_gap(previous, delivered_frontier) {
            let kind = match gap.kind {
                GapKind::Idle => "IDLE 时间",
                GapKind::RecoveryEstimate => "中断空档（估算）",
            };
            self.last_notice = Some(format!(
                "跳过 {} {kind} · 按 1s 重放",
                duration_label(gap.source.end - gap.source.start)
            ));
        }
        self.notice_frontier_ms = delivered_frontier;
        if self
            .nodes
            .iter()
            .filter(|node| node.playable)
            .all(|node| node.player.is_finished())
        {
            self.clock.virtual_anchor_ms = virtual_now;
            self.clock.state = PlayState::Finished;
        }
    }

    fn advance_to(&mut self, virtual_now: f64) {
        if self.nodes.is_empty() {
            return;
        }
        let mut budget = MAX_EVENTS_PER_TICK;
        let started = Instant::now();
        let count = self.nodes.len();
        for step in 0..count {
            let index = (self.cursor + step) % count;
            if !self.nodes[index].playable {
                continue;
            }
            budget -= self.nodes[index]
                .player
                .advance_to(virtual_now, budget.min(32));
            if budget == 0 || started.elapsed() >= Duration::from_millis(8) {
                break;
            }
        }
        self.cursor = (self.cursor + 1) % count;
    }

    fn displayed_position(&self, now: Instant) -> f64 {
        let target = self.clock.now_ms(now);
        self.nodes
            .iter()
            .filter(|node| node.playable)
            .filter_map(|node| node.player.queue.front())
            .filter(|item| item.due_ms <= target)
            .fold(target, |at, item| at.min(item.due_ms))
    }

    fn next_record(&mut self, now: Instant) {
        if self.clock.state == PlayState::Finished {
            return;
        }
        let Some((next_index, next)) = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.playable)
            .filter_map(|(index, node)| node.player.queue.front().map(|item| (index, item.due_ms)))
            .min_by(|left, right| left.1.total_cmp(&right.1))
        else {
            return;
        };
        let current = self.clock.now_ms(now);
        let target = current.max(next);
        if target > current {
            self.last_notice = Some(
                match (
                    self.time_map.to_source(current),
                    self.time_map.to_source(target),
                ) {
                    (Some(before), Some(after)) => format!(
                        "手动跳过 {} 等待时间",
                        duration_label(after.saturating_sub(before))
                    ),
                    _ => "手动跳到下一记录 · 时间估算".to_owned(),
                },
            );
        }
        self.clock.virtual_anchor_ms = target;
        self.clock.wall_anchor = now;
        self.advance_to(target);
        if self.nodes[next_index]
            .player
            .queue
            .front()
            .is_some_and(|item| item.due_ms <= target)
        {
            // A large all-due tree can exhaust the shared frame budget before
            // the selected node. An explicit step must deliver its boundary
            // even while the global clock remains paused.
            self.nodes[next_index].player.advance_to(target, 1);
        }
        self.notice_frontier_ms = self.displayed_position(now);
        if self
            .nodes
            .iter()
            .filter(|node| node.playable)
            .all(|node| node.player.is_finished())
        {
            self.clock.state = PlayState::Finished;
        }
    }

    fn active(&self) -> usize {
        *self.path.last().expect("root exists")
    }

    fn panel_action(&mut self, action: PanelAction, now: Instant) -> bool {
        match action {
            PanelAction::Back => {
                return self.handle(
                    Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
                    now,
                );
            }
            PanelAction::Toggle => self.clock.pause_or_resume(now),
            PanelAction::SpeedUp => {
                let speed = self.clock.speed * 2.0;
                if speed.is_finite() && speed <= MAX_SPEED {
                    self.clock.set_speed(speed, now);
                }
            }
            PanelAction::SpeedDown => {
                let speed = self.clock.speed / 2.0;
                if speed > 0.0 {
                    self.clock.set_speed(speed, now);
                }
            }
            PanelAction::Next => self.next_record(now),
            PanelAction::Follow => {
                let active = self.active();
                self.nodes[active]
                    .player
                    .projection
                    .scrollback
                    .toggle_follow();
            }
            PanelAction::Help => {
                let index = self.active();
                let state = format!(
                    "Session: {}\nPath: {}\nPlayback: {:?} at {:.2}×\nHistorical time: {}\nLast notice: {}",
                    self.nodes[index].id,
                    self.path
                        .iter()
                        .map(|index| self.nodes[*index].title.as_str())
                        .collect::<Vec<_>>()
                        .join(" › "),
                    self.clock.state,
                    self.clock.speed,
                    self.time_map
                        .to_source(self.displayed_position(now))
                        .and_then(super::time::format_recorded_time)
                        .unwrap_or_else(|| "unknown".to_owned()),
                    self.last_notice.as_deref().unwrap_or("none")
                );
                self.nodes[index].browser.open_help(&state);
            }
        }
        false
    }

    fn handle(&mut self, event: Event, now: Instant) -> bool {
        if matches!(&event, Event::Key(key) if key.kind == KeyEventKind::Release) {
            return false;
        }
        if matches!(&event, Event::Key(key) if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            return true;
        }
        if matches!(&event, Event::Key(key) if key.code == KeyCode::F(8)) {
            self.clock.pause_or_resume(now);
            return false;
        }
        if let Event::Resize(_, _) = &event {
            self.panel_press = None;
            self.panel = Panel::default();
            self.panel_area = Rect::default();
            let active = self.active();
            self.nodes[active].browser.invalidate_geometry();
        }
        if let Event::Mouse(mouse) = &event {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if self.panel_area.contains((mouse.column, mouse.row).into()) =>
                {
                    self.panel_press = self.panel.hit(mouse.column, mouse.row);
                    return false;
                }
                MouseEventKind::Up(MouseButton::Left) if self.panel_press.is_some() => {
                    let pressed = self.panel_press.take();
                    if pressed == self.panel.hit(mouse.column, mouse.row)
                        && let Some(action) = pressed
                    {
                        return self.panel_action(action, now);
                    }
                    return false;
                }
                MouseEventKind::Drag(MouseButton::Left) => self.panel_press = None,
                _ => {}
            }
        }
        let index = self.active();
        if !self.nodes[index].browser.owns_input() {
            if let Event::Key(key) = &event {
                match key.code {
                    KeyCode::Char(' ') => {
                        self.clock.pause_or_resume(now);
                        return false;
                    }
                    KeyCode::Char('+' | '=') => {
                        let speed = self.clock.speed * 2.0;
                        if speed.is_finite() && speed <= MAX_SPEED {
                            self.clock.set_speed(speed, now);
                        }
                        return false;
                    }
                    KeyCode::Char('-') => {
                        let speed = self.clock.speed / 2.0;
                        if speed > 0.0 {
                            self.clock.set_speed(speed, now);
                        }
                        return false;
                    }
                    KeyCode::Char(']') if !self.nodes[index].browser.has_search_or_help() => {
                        self.next_record(now);
                        return false;
                    }
                    _ => {}
                }
            }
        }
        let node = &mut self.nodes[index];
        match node
            .browser
            .handle(event, &mut node.player.projection.scrollback)
        {
            BrowseAction::Back if self.path.len() == 1 => return true,
            BrowseAction::Back => {
                self.nodes[index].browser.cancel_pending_pointer();
                self.path.pop();
            }
            BrowseAction::Child(id) => {
                // Only a displayed card in the current parent can request this.
                if let Some(child) = self.by_id.get(&id).copied()
                    && self.nodes[child].parent == Some(index)
                    && self.nodes[child].playable
                {
                    self.nodes[index].browser.cancel_pending_pointer();
                    self.nodes[child].browser.cancel_pending_pointer();
                    self.path.push(child);
                }
            }
            BrowseAction::Workflow(run_id) => {
                let children = self.nodes[index]
                    .workflow_children
                    .get(&run_id)
                    .into_iter()
                    .flatten()
                    .filter_map(|id| self.by_id.get(id).copied())
                    .filter(|child| {
                        self.nodes[*child].playable
                            && self.nodes[index]
                                .player
                                .projection
                                .has_subagent(&self.nodes[*child].id)
                    })
                    .map(|child| {
                        (
                            self.nodes[child].id.clone(),
                            self.nodes[child].title.clone(),
                        )
                    })
                    .collect();
                let node = &mut self.nodes[index];
                node.browser
                    .open_workflow_picker(&node.player.projection.scrollback, children);
            }
            BrowseAction::None => {}
        }
        false
    }

    fn draw(&mut self, area: Rect, buf: &mut Buffer, now: Instant, origin: Instant) {
        let index = self.active();
        let virtual_ms = self.displayed_position(now).clamp(0.0, u64::MAX as f64) as u64;
        let source_ms = self.time_map.to_source(virtual_ms as f64);
        let historical_wall = source_ms
            .and_then(|ms| UNIX_EPOCH.checked_add(Duration::from_millis(ms)))
            .unwrap_or(UNIX_EPOCH);
        let stamp =
            FrameStamp::at_virtual(origin, Duration::from_millis(virtual_ms), historical_wall);
        let panel_height = Panel::height(area);
        let content = Rect::new(
            area.x,
            area.y,
            area.width,
            area.height.saturating_sub(panel_height),
        );
        self.panel_area = Rect::new(area.x, content.bottom(), area.width, panel_height);
        let node = &mut self.nodes[index];
        node.browser
            .draw(content, buf, &mut node.player.projection.scrollback, stamp);
        let node_estimated = node.player.estimated;
        let hint = node.browser.hint();
        let behavior = node
            .player
            .projection
            .behavior_label()
            .map_or("Behavior: 未知".to_owned(), |mode| {
                format!("Behavior: {mode}")
            });
        let goal = node.player.projection.goal_status().unwrap_or("none");
        let unresolved = node.player.projection.unresolved_tools();
        let business = format!("历史 {behavior} · Goal: {goal} · 未完成工具: {unresolved}");
        let feedback = node.browser.feedback().map(str::to_owned);
        let follow = node.player.projection.scrollback.is_follow_mode();
        let state = match self.clock.state {
            PlayState::Playing => "Playing",
            PlayState::Paused => "Paused",
            PlayState::Finished => "Finished",
        };
        let shown: usize = self
            .nodes
            .iter()
            .filter(|node| node.playable)
            .map(|node| node.player.total - node.player.queue.len())
            .sum();
        let catching_up = self
            .nodes
            .iter()
            .filter(|node| node.playable)
            .filter_map(|node| node.player.queue.front())
            .any(|item| item.due_ms <= self.clock.now_ms(now));
        let progress = if self.clock.state == PlayState::Finished {
            100
        } else if self.time_map.playback_end > 0.0 {
            ((virtual_ms as f64 / self.time_map.playback_end * 100.0) as u8).min(99)
        } else {
            0
        };
        let time = source_ms
            .and_then(super::time::format_recorded_time)
            .unwrap_or_else(|| "时间未知".to_owned());
        let path = self
            .path
            .iter()
            .map(|index| self.nodes[*index].title.as_str())
            .collect::<Vec<_>>()
            .join(" › ");
        let original_duration = (!node_estimated
            && self
                .nodes
                .iter()
                .filter(|node| node.playable)
                .all(|node| !node.player.estimated))
        .then(|| {
            self.time_map
                .origin
                .map(|first| self.time_map.end.saturating_sub(first) / 1_000)
        })
        .flatten();
        let historical = if !follow {
            format!("正在浏览历史 · End 跟随 · {business}")
        } else if node_estimated || self.time_map.origin.is_none() {
            format!("时间估算 · {business}")
        } else if self.clock.state == PlayState::Finished {
            format!("播放结束，可继续浏览 · {business}")
        } else if catching_up {
            format!("追赶记录 · {business}")
        } else {
            format!("{business} · 已交付 {shown}/{} 记录", self.total_events)
        };
        let notice = feedback.as_deref().or(self.last_notice.as_deref());
        let info = PanelInfo {
            state,
            speed: self.clock.speed,
            progress,
            path: &path,
            time: &time,
            play_current: (virtual_ms as f64 / 1_000.0).min(self.time_map.playback_end / 1_000.0),
            play_total: self.time_map.playback_end / 1_000.0,
            original_duration,
            historical: &historical,
            notice,
            hint: &hint,
            has_parent: self.path.len() > 1,
            follow,
            next: self.clock.state != PlayState::Finished
                && self
                    .nodes
                    .iter()
                    .any(|node| node.playable && !node.player.queue.is_empty()),
        };
        self.panel.render(self.panel_area, buf, &info);
    }
}

pub(super) fn run(args: ReplayArgs) -> Result<()> {
    eprintln!("Replay · 验证会话树来源…");
    let mut snapshot = capture_tree_at(&args.session_id, &shell::util::grow_home::grow_home())?;
    eprintln!("Replay · 准备 {} 个节点…", snapshot.nodes.len());
    let mut sessions = Vec::with_capacity(snapshot.nodes.len());
    for node in snapshot.nodes.clone() {
        let session = snapshot.read_session(&node.session_id)?;
        sessions.push((node, session));
    }
    drop(snapshot);
    crate::appearance::cache::set_show_thinking_blocks(true);
    let origin = Instant::now();
    let mut replay = Replay::new(sessions, args.speed, origin);
    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    loop {
        let now = Instant::now();
        replay.tick(now);
        terminal.draw(|frame| {
            let area = frame.area();
            replay.draw(area, frame.buffer_mut(), now, origin);
        })?;
        if event::poll(Duration::from_millis(
            if replay.clock.state == PlayState::Playing {
                16
            } else {
                100
            },
        ))? && replay.handle(event::read()?, Instant::now())
        {
            break;
        }
    }
    let _ = terminal.clear();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use acp_transport::protocol as acp;

    fn text(id: &str, at: u64) -> shell::session::storage::transcript::TranscriptEvent {
        shell::session::storage::transcript::TranscriptEvent {
            update: SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
                acp::SessionId::new(id),
                acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                    acp::ContentBlock::Text(acp::TextContent::new(id)),
                )),
            ))),
            timestamp_ms: Some(at),
            simulated_source: false,
        }
    }

    fn spawn(at: u64) -> shell::session::storage::transcript::TranscriptEvent {
        spawn_named("root", "child", at)
    }

    fn spawn_named(
        parent: &str,
        child: &str,
        at: u64,
    ) -> shell::session::storage::transcript::TranscriptEvent {
        let update: GrowUpdate = serde_json::from_value(serde_json::json!({
            "sessionUpdate": "subagent_spawned",
            "subagent_id": child, "parent_session_id": parent,
            "child_session_id": child, "subagent_type": "explore",
            "description": "inspect"
        }))
        .unwrap();
        shell::session::storage::transcript::TranscriptEvent {
            update: SessionUpdate::Grow(Box::new(
                shell::extensions::notification::SessionNotification {
                    session_id: acp::SessionId::new(parent),
                    update,
                    meta: None,
                },
            )),
            timestamp_ms: Some(at),
            simulated_source: false,
        }
    }

    fn goal(at: u64, status: &str) -> shell::session::storage::transcript::TranscriptEvent {
        let update: GrowUpdate = serde_json::from_value(serde_json::json!({
            "sessionUpdate": "goal_updated",
            "goal_id": "g1",
            "objective": "inspect",
            "status": status,
            "token_budget": null,
            "tokens_used": 10,
            "usage_incomplete": true,
            "elapsed_ms": 100,
            "created_at": "2026-09-29T00:00:00Z",
            "updated_at": "2026-09-29T00:01:00Z"
        }))
        .unwrap();
        shell::session::storage::transcript::TranscriptEvent {
            update: SessionUpdate::Grow(Box::new(
                shell::extensions::notification::SessionNotification {
                    session_id: acp::SessionId::new("root"),
                    update,
                    meta: None,
                },
            )),
            timestamp_ms: Some(at),
            simulated_source: false,
        }
    }

    fn session(
        id: &str,
        events: Vec<shell::session::storage::transcript::TranscriptEvent>,
        activity: Vec<Range<u64>>,
    ) -> TranscriptSession {
        TranscriptSession {
            session_id: id.into(),
            title: None,
            last_timeline_seq: None,
            source_bytes: 0,
            events,
            activity,
            recovery_gaps: vec![],
            open_activity: vec![],
            unknown_activity: false,
        }
    }

    fn node(id: &str, parent: Option<&str>, spawn_at_ms: Option<u64>) -> TranscriptNode {
        TranscriptNode {
            session_id: id.into(),
            title: Some(id.into()),
            parent_id: parent.map(str::to_owned),
            depth: usize::from(parent.is_some()),
            children: vec![],
            last_timeline_seq: None,
            spawn_at_ms,
        }
    }

    #[test]
    fn production_tree_gates_child_and_excludes_unreachable_branch_from_duration() {
        let start = Instant::now();
        let mut replay = Replay::new(
            vec![
                (
                    node("root", None, None),
                    session(
                        "root",
                        vec![text("root", 1_000), spawn(2_000), text("root", 100_000)],
                        vec![],
                    ),
                ),
                (
                    node("child", Some("root"), Some(2_000)),
                    session(
                        "child",
                        vec![text("child", 500), text("child", 3_000)],
                        vec![],
                    ),
                ),
                (
                    node("hidden", Some("root"), Some(2_000)),
                    session("hidden", vec![text("hidden", 10_000_000)], vec![]),
                ),
            ],
            1.0,
            start,
        );
        assert!(replay.nodes[1].playable);
        assert!(!replay.nodes[2].playable);
        assert_eq!(replay.time_map.end, 100_000);
        replay.tick(start);
        assert_eq!(replay.nodes[1].player.queue.len(), 2);
        assert!(replay.nodes[1].player.queue[0].due_ms >= replay.nodes[0].player.queue[0].due_ms);
        replay.tick(start + Duration::from_secs(2));
        assert!(replay.nodes[1].player.queue.len() < 2);
        assert_eq!(replay.nodes[2].player.queue.len(), 1);
    }

    #[test]
    fn sibling_execution_prevents_a_false_idle_skip() {
        let replay = Replay::new(
            vec![
                (
                    node("root", None, None),
                    session(
                        "root",
                        vec![text("root", 1_000), spawn(2_000), text("root", 100_000)],
                        vec![],
                    ),
                ),
                (
                    node("child", Some("root"), Some(2_000)),
                    session(
                        "child",
                        vec![text("child", 2_000), text("child", 99_000)],
                        vec![2_000..99_000],
                    ),
                ),
            ],
            16.0,
            Instant::now(),
        );
        assert!(replay.time_map.gaps.is_empty());
    }

    #[test]
    fn nested_subagent_navigation_preserves_each_page_and_the_shared_clock() {
        let start = Instant::now();
        let mut replay = Replay::new(
            vec![
                (
                    node("root", None, None),
                    session("root", vec![spawn_named("root", "child", 1_000)], vec![]),
                ),
                (
                    node("child", Some("root"), Some(1_000)),
                    session(
                        "child",
                        vec![spawn_named("child", "grandchild", 1_000)],
                        vec![],
                    ),
                ),
                (
                    node("grandchild", Some("child"), Some(1_000)),
                    session("grandchild", vec![text("grandchild", 1_000)], vec![]),
                ),
            ],
            1.0,
            start,
        );
        assert_eq!(replay.path.len(), 1);
        replay.tick(start);
        let clock_before = replay.clock.now_ms(start);
        let enter = Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        replay.nodes[0]
            .player
            .projection
            .scrollback
            .set_selected(Some(0));
        replay.handle(enter.clone(), start);
        assert_eq!(replay.path, vec![0, 1]);
        replay.nodes[1]
            .player
            .projection
            .scrollback
            .set_selected(Some(0));
        replay.handle(enter, start);
        assert_eq!(replay.path, vec![0, 1, 2]);
        let back = Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        replay.handle(back.clone(), start);
        replay.handle(back, start);
        assert_eq!(replay.path, vec![0]);
        assert_eq!(
            replay.nodes[0].player.projection.scrollback.selected(),
            Some(0)
        );
        assert_eq!(replay.clock.now_ms(start), clock_before);
    }

    #[test]
    fn goal_metadata_inside_idle_is_delivered_without_splitting_the_gap() {
        let start = Instant::now();
        let mut replay = Replay::new(
            vec![(
                node("root", None, None),
                session(
                    "root",
                    vec![
                        text("first", 1_000),
                        goal(15_000, "active"),
                        goal(30_000, "paused"),
                        goal(45_000, "active"),
                        goal(60_000, "paused"),
                        text("last", 100_000),
                    ],
                    vec![],
                ),
            )],
            1.0,
            start,
        );
        assert_eq!(replay.time_map.gaps.len(), 1);
        replay.tick(start + Duration::from_millis(800));
        assert_eq!(
            replay.nodes[0].player.projection.goal_status(),
            Some("paused")
        );
        assert_eq!(replay.nodes[0].player.queue.len(), 1);
    }

    #[test]
    fn due_work_uses_a_shared_budget_and_each_node_makes_progress() {
        let start = Instant::now();
        let mut parent_events = vec![spawn(1_000)];
        parent_events.extend((0..320).map(|_| text("root", 1_000)));
        let child_events = (0..320).map(|_| text("child", 1_000)).collect();
        let mut replay = Replay::new(
            vec![
                (
                    node("root", None, None),
                    session("root", parent_events, vec![]),
                ),
                (
                    node("child", Some("root"), Some(1_000)),
                    session("child", child_events, vec![]),
                ),
            ],
            16.0,
            start,
        );
        replay.tick(start);
        assert!(replay.nodes[0].player.queue.len() < 321);
        assert!(replay.nodes[1].player.queue.len() < 320);
        assert!(
            replay.nodes[0].player.queue.len() + replay.nodes[1].player.queue.len()
                >= 640 - MAX_EVENTS_PER_TICK
        );
        for _ in 0..25 {
            replay.tick(start);
        }
        replay.tick(start + Duration::from_secs(1));
        assert_eq!(replay.clock.state, PlayState::Finished);
    }

    #[test]
    fn detail_keeps_local_keys_while_global_and_panel_controls_remain_available() {
        let start = Instant::now();
        let mut replay = Replay::new(
            vec![(
                node("root", None, None),
                session(
                    "root",
                    vec![text("first", 1_000), text("second", 100_000)],
                    vec![],
                ),
            )],
            1.0,
            start,
        );
        replay.tick(start + Duration::from_millis(300));
        replay.nodes[0]
            .player
            .projection
            .scrollback
            .set_selected(Some(0));
        assert!(!replay.handle(
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            start + Duration::from_millis(300),
        ));
        assert!(replay.nodes[0].browser.viewer.is_some());
        let remaining = replay.nodes[0].player.queue.len();
        replay.handle(
            Event::Key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE)),
            start + Duration::from_millis(300),
        );
        assert_eq!(replay.nodes[0].player.queue.len(), remaining);
        replay.handle(
            Event::Key(KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE)),
            start + Duration::from_millis(300),
        );
        assert_eq!(replay.clock.state, PlayState::Paused);

        let area = Rect::new(0, 0, 100, 30);
        let mut buf = Buffer::empty(area);
        replay.draw(area, &mut buf, start + Duration::from_millis(300), start);
        let control = (0..area.width)
            .flat_map(|x| (0..area.height).map(move |y| (x, y)))
            .find(|(x, y)| replay.panel.hit(*x, *y) == Some(PanelAction::Next))
            .expect("next control should be clickable");
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            replay.handle(
                Event::Mouse(crossterm::event::MouseEvent {
                    kind,
                    column: control.0,
                    row: control.1,
                    modifiers: KeyModifiers::NONE,
                }),
                start + Duration::from_millis(300),
            );
        }
        assert_eq!(replay.nodes[0].player.queue.len(), 0);
        assert_eq!(replay.clock.state, PlayState::Paused);
        assert!(replay.nodes[0].browser.viewer.is_some());
    }

    #[test]
    fn explicit_step_delivers_a_boundary_even_when_the_shared_budget_is_exhausted() {
        let start = Instant::now();
        let sessions = (0..300)
            .map(|index| {
                let id = format!("node-{index}");
                (
                    node(&id, None, None),
                    session(&id, vec![text("x", 1_000)], vec![]),
                )
            })
            .collect();
        let mut replay = Replay::new(sessions, 1.0, start);
        // The production scheduler is exercised with a synthetic all-due
        // tree, including its maximum-node budget edge.
        for node in &mut replay.nodes {
            node.playable = true;
        }
        replay.clock.pause_or_resume(start);
        replay.cursor = 1;
        replay.next_record(start);
        assert!(replay.nodes[0].player.queue.is_empty());
        assert_eq!(replay.clock.state, PlayState::Paused);
    }
}
