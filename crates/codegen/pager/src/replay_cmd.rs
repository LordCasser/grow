//! An offline, read-only presentation of a captured session.

use std::collections::VecDeque;
use std::io::stdout;
#[cfg(test)]
use std::time::Duration;
use std::time::Instant;

use acp_transport::protocol as acp;
use anyhow::{Result, bail};
use crossterm::ExecutableCommand;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
#[cfg(test)]
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use shell::extensions::notification::SessionUpdate as GrowUpdate;
use shell::session::storage::SessionUpdate;
use shell::session::storage::transcript::TranscriptEvent;
use unicode_segmentation::UnicodeSegmentation;

use crate::transcript_projection::TranscriptProjection;

mod browser;
mod idle;
mod panel;
mod time;
mod tree;

// Any representable monotonic interval must remain finite after scaling to ms.
const MAX_SPEED: f64 = f64::MAX / (u64::MAX as f64 * 1000.0);
const MAX_EVENTS_PER_TICK: usize = 256;

#[derive(Debug, clap::Args, Clone)]
pub struct ReplayArgs {
    /// Session ID to replay (one session at a time)
    pub session_id: String,
    /// Playback speed relative to recorded intervals
    #[arg(long, default_value_t = 1.0, value_parser = parse_speed)]
    pub speed: f64,
}

fn parse_speed(value: &str) -> Result<f64, String> {
    let speed = value
        .parse::<f64>()
        .map_err(|_| "speed must be a positive finite number")?;
    if !speed.is_finite() || speed <= 0.0 || speed > MAX_SPEED {
        return Err("speed must be a positive finite number within the clock range".into());
    }
    Ok(speed)
}

struct ReplayItem {
    due_ms: f64,
    exact_time: bool,
    next_dependency_due_ms: Option<f64>,
    event: TranscriptEvent,
}

fn source_time(event: &TranscriptEvent) -> Option<u64> {
    let agent_time = match &event.update {
        SessionUpdate::Acp(n) => n
            .meta
            .as_ref()
            .and_then(|m| m.get("agentTimestampMs"))
            .and_then(|v| v.as_u64()),
        SessionUpdate::Grow(n) => n
            .meta
            .as_ref()
            .and_then(|m| m.get("agentTimestampMs"))
            .and_then(|v| v.as_u64()),
        SessionUpdate::ResponseReplayProjection(_) => None,
    };
    // Use the notification's original time, never a synthetic envelope's write time.
    agent_time.or(event.timestamp_ms)
}

fn schedule(events: Vec<TranscriptEvent>) -> (VecDeque<ReplayItem>, Option<u64>, bool) {
    let first_time = events.iter().find_map(source_time);
    let mut previous = 0.0_f64;
    let mut estimated = first_time.is_none();
    let mut queue: VecDeque<ReplayItem> = events
        .into_iter()
        .map(|event| {
            let event_time = source_time(&event);
            let exact_time = event_time.is_some() && first_time.is_some();
            let due_ms = match (first_time, event_time) {
                (Some(start), Some(time)) => previous.max(time.saturating_sub(start) as f64),
                _ => {
                    estimated = true;
                    previous + 250.0
                }
            };
            previous = due_ms;
            ReplayItem {
                due_ms,
                exact_time,
                next_dependency_due_ms: None,
                event,
            }
        })
        .collect();
    let mut next_dependency: Option<(f64, bool)> = None;
    for item in queue.iter_mut().rev() {
        let is_text = matches!(&item.event.update, SessionUpdate::Acp(notification)
            if matches!(&notification.update, acp::SessionUpdate::AgentMessageChunk(_) | acp::SessionUpdate::AgentThoughtChunk(_)));
        if is_text && item.exact_time {
            item.next_dependency_due_ms =
                next_dependency.and_then(|(due, exact)| exact.then_some(due));
        }
        if !independent_of_text(&item.event) {
            next_dependency = Some((item.due_ms, item.exact_time));
        }
    }
    (queue, first_time, estimated)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlayState {
    Playing,
    Paused,
    Finished,
}

struct PlaybackClock {
    state: PlayState,
    speed: f64,
    virtual_anchor_ms: f64,
    wall_anchor: Instant,
}

impl PlaybackClock {
    fn new(speed: f64, now: Instant) -> Self {
        Self {
            state: PlayState::Playing,
            speed,
            virtual_anchor_ms: 0.0,
            wall_anchor: now,
        }
    }
    fn now_ms(&self, now: Instant) -> f64 {
        if self.state == PlayState::Playing {
            self.virtual_anchor_ms
                + now
                    .saturating_duration_since(self.wall_anchor)
                    .as_secs_f64()
                    * 1000.0
                    * self.speed
        } else {
            self.virtual_anchor_ms
        }
    }
    fn pause_or_resume(&mut self, now: Instant) {
        match self.state {
            PlayState::Playing => {
                self.virtual_anchor_ms = self.now_ms(now);
                self.state = PlayState::Paused;
            }
            PlayState::Paused => {
                self.wall_anchor = now;
                self.state = PlayState::Playing;
            }
            PlayState::Finished => {}
        }
    }
    fn set_speed(&mut self, speed: f64, now: Instant) {
        if self.state == PlayState::Finished {
            return;
        }
        self.virtual_anchor_ms = self.now_ms(now);
        self.wall_anchor = now;
        self.speed = speed;
    }
}

struct Reveal {
    notification: Box<acp::SessionNotification>,
    text: String,
    sent_bytes: usize,
    sent_graphemes: usize,
    total_graphemes: usize,
    start_ms: f64,
    end_ms: f64,
}

impl Reveal {
    fn from_event(
        event: &TranscriptEvent,
        due_ms: f64,
        next_dependency_due_ms: Option<f64>,
    ) -> Option<Self> {
        let SessionUpdate::Acp(notification) = &event.update else {
            return None;
        };
        let mut projection_notification = notification.clone();
        let text = match &mut projection_notification.update {
            acp::SessionUpdate::AgentMessageChunk(chunk)
            | acp::SessionUpdate::AgentThoughtChunk(chunk) => {
                let acp::ContentBlock::Text(text) = &mut chunk.content else {
                    return None;
                };
                std::mem::take(&mut text.text)
            }
            _ => return None,
        };
        let total_graphemes = text.graphemes(true).count();
        if total_graphemes <= 1 {
            return None;
        }
        let duration = (total_graphemes as f64 / 60.0 * 1000.0).clamp(200.0, 4000.0);
        let end_ms = next_dependency_due_ms.map_or(due_ms + duration, |next_due_ms| {
            (due_ms + duration).min(next_due_ms)
        });
        Some(Self {
            notification: projection_notification,
            text,
            sent_bytes: 0,
            sent_graphemes: 0,
            total_graphemes,
            start_ms: due_ms,
            end_ms,
        })
    }

    fn advance(&mut self, now_ms: f64, projection: &mut TranscriptProjection) {
        let progress = if self.end_ms <= self.start_ms {
            if now_ms >= self.end_ms { 1.0 } else { 0.0 }
        } else {
            ((now_ms - self.start_ms) / (self.end_ms - self.start_ms)).clamp(0.0, 1.0)
        };
        let target =
            ((self.total_graphemes as f64 * progress).ceil() as usize).min(self.total_graphemes);
        if target <= self.sent_graphemes {
            return;
        }
        let remaining = &self.text[self.sent_bytes..];
        let added_bytes = remaining
            .graphemes(true)
            .take(target - self.sent_graphemes)
            .map(str::len)
            .sum::<usize>();
        let delta = &self.text[self.sent_bytes..self.sent_bytes + added_bytes];
        let mut notification = (*self.notification).clone();
        match &mut notification.update {
            acp::SessionUpdate::AgentMessageChunk(chunk)
            | acp::SessionUpdate::AgentThoughtChunk(chunk) => {
                if let acp::ContentBlock::Text(text) = &mut chunk.content {
                    text.text = delta.to_owned();
                }
            }
            _ => unreachable!(),
        }
        projection.apply(TranscriptEvent {
            update: SessionUpdate::Acp(Box::new(notification)),
            timestamp_ms: None,
            simulated_source: true,
        });
        self.sent_bytes += added_bytes;
        self.sent_graphemes = target;
    }
    fn complete(&self) -> bool {
        self.sent_bytes == self.text.len()
    }
}

struct Player {
    queue: VecDeque<ReplayItem>,
    reveal: Option<Reveal>,
    projection: TranscriptProjection,
    first_source_ms: Option<u64>,
    estimated: bool,
    total: usize,
}

impl Player {
    fn new(events: Vec<TranscriptEvent>) -> Self {
        let total = events.len();
        let (queue, first_source_ms, estimated) = schedule(events);
        Self {
            queue,
            reveal: None,
            projection: TranscriptProjection::default(),
            first_source_ms,
            estimated,
            total,
        }
    }
    fn advance_to(&mut self, virtual_now: f64, budget: usize) -> usize {
        if let Some(reveal) = self.reveal.as_mut() {
            reveal.advance(virtual_now, &mut self.projection);
        }
        if self.reveal.as_ref().is_some_and(Reveal::complete) {
            self.reveal = None;
        }
        let mut delivered = 0;
        while delivered < budget
            && self
                .queue
                .front()
                .is_some_and(|item| item.due_ms <= virtual_now)
        {
            let item = self.queue.pop_front().expect("front existed");
            delivered += 1;
            if !independent_of_text(&item.event)
                && let Some(old) = self.reveal.as_mut()
            {
                // Text boundaries close before any later fact that could depend on them.
                old.advance(old.end_ms, &mut self.projection);
                self.reveal = None;
            }
            if let Some(mut reveal) =
                Reveal::from_event(&item.event, item.due_ms, item.next_dependency_due_ms)
            {
                reveal.advance(virtual_now, &mut self.projection);
                if !reveal.complete() {
                    self.reveal = Some(reveal);
                }
            } else {
                self.projection.apply(item.event);
            }
        }
        delivered
    }

    fn is_finished(&self) -> bool {
        self.queue.is_empty() && self.reveal.is_none()
    }
}

fn independent_of_text(event: &TranscriptEvent) -> bool {
    matches!(&event.update, SessionUpdate::Acp(notification) if matches!(&notification.update, acp::SessionUpdate::CurrentModeUpdate(_)))
        || matches!(&event.update, SessionUpdate::Grow(notification) if matches!(
            &notification.update,
            GrowUpdate::UiNotice(_) | GrowUpdate::GoalUpdated { .. }
                | GrowUpdate::WorkflowUpdated { .. } | GrowUpdate::SubagentProgress { .. }
                | GrowUpdate::ControlStateUpdate(_)
        ))
}

struct TerminalGuard;
impl TerminalGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        if let Err(error) = stdout().execute(EnterAlternateScreen) {
            let _ = terminal::disable_raw_mode();
            return Err(error.into());
        }
        if let Err(error) = stdout().execute(EnableMouseCapture) {
            let _ = stdout().execute(LeaveAlternateScreen);
            let _ = terminal::disable_raw_mode();
            return Err(error.into());
        }
        if let Err(error) = stdout().execute(EnableBracketedPaste) {
            let _ = stdout().execute(DisableMouseCapture);
            let _ = stdout().execute(LeaveAlternateScreen);
            let _ = terminal::disable_raw_mode();
            return Err(error.into());
        }
        Ok(Self)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = stdout().execute(DisableBracketedPaste);
        let _ = stdout().execute(DisableMouseCapture);
        let _ = stdout().execute(LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

pub fn run(args: ReplayArgs) -> Result<()> {
    if !args.speed.is_finite() || args.speed <= 0.0 || args.speed > MAX_SPEED {
        bail!("speed must be a positive finite number within the clock range");
    }
    tree::run(args)
}

#[cfg(test)]
fn handle_terminal_event(event: Event, player: &mut tests::Player, now: Instant) -> bool {
    match event {
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollUp => player.projection.scrollback.scroll_up(3),
            MouseEventKind::ScrollDown => player.projection.scrollback.scroll_down(3),
            _ => {}
        },
        Event::Paste(_) => {}
        Event::Key(key) => {
            if key.kind != KeyEventKind::Press {
                return false;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return true,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return true;
                }
                KeyCode::Char(' ') => player.clock.pause_or_resume(now),
                KeyCode::Char('+') | KeyCode::Char('=') => {
                    let speed = player.clock.speed * 2.0;
                    if speed.is_finite() && speed <= MAX_SPEED {
                        player.clock.set_speed(speed, now);
                    }
                }
                KeyCode::Char('-') => {
                    let speed = player.clock.speed / 2.0;
                    if speed > 0.0 {
                        player.clock.set_speed(speed, now);
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => player.projection.scrollback.scroll_up(2),
                KeyCode::Down | KeyCode::Char('j') => player.projection.scrollback.scroll_down(2),
                KeyCode::PageUp => player.projection.scrollback.scroll_up(15),
                KeyCode::PageDown => player.projection.scrollback.scroll_down(15),
                KeyCode::Home => player.projection.scrollback.goto_top(),
                KeyCode::End => player.projection.scrollback.goto_bottom(),
                KeyCode::BackTab => player.projection.scrollback.select_prev(),
                KeyCode::Tab => player.projection.scrollback.select_next(),
                KeyCode::Enter => player.projection.scrollback.toggle_fold_selected(),
                _ => {}
            }
        }
        _ => {}
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // Single-node clock harness; production owns one clock for the whole tree.
    pub(super) struct Player {
        pub(super) clock: PlaybackClock,
        playback: super::Player,
    }
    impl std::ops::Deref for Player {
        type Target = super::Player;
        fn deref(&self) -> &Self::Target {
            &self.playback
        }
    }
    impl std::ops::DerefMut for Player {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.playback
        }
    }
    impl Player {
        fn new(events: Vec<TranscriptEvent>, speed: f64, now: Instant) -> Self {
            Self {
                clock: PlaybackClock::new(speed, now),
                playback: super::Player::new(events),
            }
        }
        fn tick(&mut self, now: Instant) {
            if self.clock.state != PlayState::Playing {
                return;
            }
            let at = self.clock.now_ms(now);
            self.playback.advance_to(at, MAX_EVENTS_PER_TICK);
            if self.playback.is_finished() {
                self.clock.virtual_anchor_ms = at;
                self.clock.state = PlayState::Finished;
            }
        }
    }

    fn text_event(text: &str, at_ms: Option<u64>) -> TranscriptEvent {
        let notification = acp::SessionNotification::new(
            acp::SessionId::new("replay-test"),
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new(text),
            ))),
        );
        TranscriptEvent {
            update: SessionUpdate::Acp(Box::new(notification)),
            timestamp_ms: at_ms,
            simulated_source: false,
        }
    }

    fn acp_event(update: acp::SessionUpdate, at_ms: Option<u64>) -> TranscriptEvent {
        TranscriptEvent {
            update: SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
                acp::SessionId::new("replay-test"),
                update,
            ))),
            timestamp_ms: at_ms,
            simulated_source: false,
        }
    }

    fn grow_event(update: GrowUpdate, at_ms: Option<u64>) -> TranscriptEvent {
        TranscriptEvent {
            update: SessionUpdate::Grow(Box::new(
                shell::extensions::notification::SessionNotification {
                    session_id: acp::SessionId::new("replay-test"),
                    update,
                    meta: None,
                },
            )),
            timestamp_ms: at_ms,
            simulated_source: false,
        }
    }

    fn visible_text(player: &Player) -> String {
        player
            .projection
            .scrollback
            .iter_entries()
            .filter_map(|(_, entry)| match &entry.block {
                crate::scrollback::RenderBlock::AgentMessage(message) => Some(message.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    fn projection_markdown(projection: &TranscriptProjection) -> String {
        crate::scrollback::export::render_entries_to_full_markdown(
            projection.scrollback.iter_entries().map(|(_, entry)| entry),
            &[],
        )
    }
    #[test]
    fn speed_requires_positive_finite_value() {
        for invalid in ["0", "-1", "NaN", "inf", "-inf", "1e308", "nope"] {
            assert!(parse_speed(invalid).is_err());
        }
        assert_eq!(parse_speed("4.5").unwrap(), 4.5);
    }
    #[test]
    fn virtual_clock_pauses_and_changes_speed_without_drift() {
        let start = Instant::now();
        let mut clock = PlaybackClock::new(2.0, start);
        clock.pause_or_resume(start + Duration::from_millis(100));
        assert_eq!(clock.now_ms(start + Duration::from_secs(30)), 200.0);
        clock.set_speed(4.0, start + Duration::from_secs(30));
        clock.pause_or_resume(start + Duration::from_secs(30));
        assert_eq!(clock.now_ms(start + Duration::from_secs(31)), 4200.0);
    }

    #[test]
    fn grapheme_reveal_finishes_with_the_canonical_body_at_multiple_speeds() {
        for speed in [0.5, 1.0, 8.0] {
            let start = Instant::now();
            let mut player = Player::new(
                vec![text_event("e\u{301}🙂中文 markdown **ok**", Some(1000))],
                speed,
                start,
            );
            player.tick(start);
            assert_ne!(visible_text(&player), "e\u{301}🙂中文 markdown **ok**");
            player
                .clock
                .pause_or_resume(start + Duration::from_millis(20));
            let frozen = visible_text(&player);
            player.tick(start + Duration::from_secs(2));
            assert_eq!(visible_text(&player), frozen);
            player.clock.pause_or_resume(start + Duration::from_secs(2));
            player.tick(start + Duration::from_secs(20));
            assert_eq!(visible_text(&player), "e\u{301}🙂中文 markdown **ok**");
            assert_eq!(player.clock.state, PlayState::Finished);
        }
    }

    #[test]
    fn source_order_wins_over_backwards_timestamps() {
        let (queue, _, _) = schedule(vec![
            text_event("first", Some(2000)),
            text_event("second", Some(1000)),
        ]);
        assert!(queue[0].due_ms <= queue[1].due_ms);
    }

    #[test]
    fn due_text_boundary_finishes_previous_reveal_before_delivery() {
        let start = Instant::now();
        let mut player = Player::new(
            vec![
                text_event("first message", Some(1000)),
                text_event(" second", Some(1001)),
            ],
            1.0,
            start,
        );
        player.tick(start + Duration::from_millis(50));
        assert_eq!(player.queue.len(), 0);
        assert!(visible_text(&player).starts_with("first message"));
        assert!(player.reveal.is_some());
        player.tick(start + Duration::from_secs(10));
        assert_eq!(visible_text(&player), "first message second");
    }

    #[test]
    fn cancellation_due_during_reveal_finishes_text_and_delivers_terminal() {
        let start = Instant::now();
        let mut player = Player::new(
            vec![
                text_event("completed before cancellation", Some(1000)),
                grow_event(
                    GrowUpdate::TurnCompleted {
                        prompt_id: "prompt-1".into(),
                        identity: None,
                        stop_reason: "cancelled".into(),
                        agent_result: None,
                        usage: None,
                    },
                    Some(1100),
                ),
            ],
            1.0,
            start,
        );
        player.tick(start);
        assert_ne!(visible_text(&player), "completed before cancellation");
        assert_eq!(player.reveal.as_ref().unwrap().end_ms, 100.0);
        player.tick(start + Duration::from_millis(100));
        assert_eq!(visible_text(&player), "completed before cancellation");
        assert!(player.queue.is_empty());
        assert!(player.reveal.is_none());
    }

    #[test]
    fn new_user_message_due_during_reveal_does_not_inherit_previous_text() {
        let start = Instant::now();
        let mut player = Player::new(
            vec![
                text_event("prior response", Some(1000)),
                acp_event(
                    acp::SessionUpdate::UserMessageChunk(acp::ContentChunk::new(
                        acp::ContentBlock::Text(acp::TextContent::new("next prompt")),
                    )),
                    Some(1100),
                ),
            ],
            1.0,
            start,
        );
        player.tick(start);
        player.tick(start + Duration::from_millis(100));
        assert_eq!(visible_text(&player), "prior response");
        assert!(player.queue.is_empty());
        assert!(player.reveal.is_none());
        assert!(projection_markdown(&player.projection).contains("next prompt"));
    }

    #[test]
    fn tool_boundary_due_during_reveal_finishes_text_before_tool() {
        let start = Instant::now();
        let mut player = Player::new(
            vec![
                text_event("before tool boundary", Some(1000)),
                acp_event(
                    acp::SessionUpdate::ToolCall(acp::ToolCall::new("tool-1", "Run command")),
                    Some(1100),
                ),
            ],
            1.0,
            start,
        );
        player.tick(start);
        player.tick(start + Duration::from_millis(100));
        assert_eq!(visible_text(&player), "before tool boundary");
        assert!(player.queue.is_empty());
        assert!(player.reveal.is_none());
        assert!(projection_markdown(&player.projection).contains("Run command"));
    }

    #[test]
    fn pause_freezes_reveal_and_due_boundary_until_resume() {
        let start = Instant::now();
        let mut player = Player::new(
            vec![
                text_event("paused partial response", Some(1000)),
                grow_event(
                    GrowUpdate::TurnCompleted {
                        prompt_id: "prompt-1".into(),
                        identity: None,
                        stop_reason: "cancelled".into(),
                        agent_result: None,
                        usage: None,
                    },
                    Some(1100),
                ),
            ],
            1.0,
            start,
        );
        player.tick(start + Duration::from_millis(25));
        player
            .clock
            .pause_or_resume(start + Duration::from_millis(25));
        let frozen = visible_text(&player);
        assert!(!frozen.is_empty());
        player.tick(start + Duration::from_secs(5));
        assert_eq!(visible_text(&player), frozen);
        assert_eq!(player.queue.len(), 1);
        player.clock.pause_or_resume(start + Duration::from_secs(5));
        player.tick(start + Duration::from_secs(5) + Duration::from_millis(75));
        assert_eq!(visible_text(&player), "paused partial response");
        assert!(player.queue.is_empty());
    }

    #[test]
    fn speed_change_preserves_partial_reveal_then_delivers_due_boundary() {
        let start = Instant::now();
        let mut player = Player::new(
            vec![
                text_event("speed adjusted response", Some(1000)),
                grow_event(
                    GrowUpdate::TurnCompleted {
                        prompt_id: "prompt-1".into(),
                        identity: None,
                        stop_reason: "cancelled".into(),
                        agent_result: None,
                        usage: None,
                    },
                    Some(1100),
                ),
            ],
            1.0,
            start,
        );
        player.tick(start + Duration::from_millis(25));
        let partial = visible_text(&player);
        assert!(!partial.is_empty());
        assert_ne!(partial, "speed adjusted response");
        player
            .clock
            .set_speed(2.0, start + Duration::from_millis(25));
        player.tick(start + Duration::from_millis(64));
        assert_eq!(visible_text(&player), "speed adjusted response");
        assert!(player.queue.is_empty());
    }

    #[test]
    fn paste_payload_is_ignored_as_one_event() {
        let start = Instant::now();
        let mut player = Player::new(vec![text_event("response", Some(1000))], 1.0, start);
        let state = player.clock.state;
        let speed = player.clock.speed;
        let queue_len = player.queue.len();
        assert!(!handle_terminal_event(
            Event::Paste("q +=- /quit".into()),
            &mut player,
            start + Duration::from_secs(1),
        ));
        assert_eq!(player.clock.state, state);
        assert_eq!(player.clock.speed, speed);
        assert_eq!(player.queue.len(), queue_len);
        assert_eq!(visible_text(&player), "");
    }

    #[test]
    fn independent_notice_arrives_while_text_is_still_revealing() {
        let start = Instant::now();
        let notice = shell::extensions::notification::SessionNotification {
            session_id: acp::SessionId::new("replay-test"),
            update: GrowUpdate::UiNotice(shell::extensions::notification::UiNotice {
                correlation_id: "notice-1".into(),
                category: shell::extensions::notification::UiNoticeCategory::Lifecycle,
                subject: None,
                description: None,
                message: "independent state".into(),
                tone: shell::extensions::notification::UiNoticeTone::Info,
                details: None,
            }),
            meta: None,
        };
        let mut player = Player::new(
            vec![
                text_event(&"streaming ".repeat(30), Some(1000)),
                TranscriptEvent {
                    update: SessionUpdate::Grow(Box::new(notice)),
                    timestamp_ms: Some(1500),
                    simulated_source: false,
                },
            ],
            1.0,
            start,
        );
        player.tick(start + Duration::from_millis(600));
        assert_eq!(player.queue.len(), 0);
        assert!(player.reveal.is_some());
        assert!(projection_markdown(&player.projection).contains("independent state"));
        player.tick(start + Duration::from_secs(10));
        assert_eq!(visible_text(&player), "streaming ".repeat(30));
    }

    #[test]
    fn large_due_batch_yields_between_frames_without_losing_updates() {
        let start = Instant::now();
        let mut player = Player::new(
            (0..600).map(|_| text_event("x", Some(1000))).collect(),
            1000.0,
            start,
        );
        player.tick(start);
        assert_eq!(player.queue.len(), 600 - MAX_EVENTS_PER_TICK);
        player.tick(start);
        player.tick(start);
        assert_eq!(player.clock.state, PlayState::Finished);
        assert_eq!(visible_text(&player), "x".repeat(600));
    }

    #[test]
    fn completed_replay_matches_full_export_projection_across_speeds_and_pause() {
        fn events() -> Vec<TranscriptEvent> {
            let mut thought = text_event("Reasoning 🙂", Some(1200));
            if let SessionUpdate::Acp(notification) = &mut thought.update {
                notification.update =
                    acp::SessionUpdate::AgentThoughtChunk(acp::ContentChunk::new(
                        acp::ContentBlock::Text(acp::TextContent::new("Reasoning 🙂")),
                    ));
            }
            vec![
                thought,
                text_event("Final **answer**", Some(1500)),
                text_event(" and more", Some(1700)),
            ]
        }
        let mut full = TranscriptProjection::default();
        for event in events() {
            full.apply(event);
        }
        let expected = projection_markdown(&full);
        assert!(expected.contains("Reasoning 🙂"));
        assert!(expected.contains("Final **answer** and more"));
        for speed in [1.0, 4.0] {
            let start = Instant::now();
            let mut replay = Player::new(events(), speed, start);
            replay.tick(start + Duration::from_millis(100));
            replay
                .clock
                .pause_or_resume(start + Duration::from_millis(100));
            replay.tick(start + Duration::from_secs(3));
            replay.clock.pause_or_resume(start + Duration::from_secs(3));
            replay.tick(start + Duration::from_secs(30));
            assert_eq!(replay.clock.state, PlayState::Finished);
            assert_eq!(projection_markdown(&replay.projection), expected);
        }
    }

    fn mode_event(mode: &str, at_ms: u64) -> TranscriptEvent {
        acp_event(
            acp::SessionUpdate::CurrentModeUpdate(acp::CurrentModeUpdate::new(
                acp::SessionModeId::new(mode),
            )),
            Some(at_ms),
        )
    }

    fn goal_event(status: &str, at_ms: u64) -> TranscriptEvent {
        grow_event(serde_json::from_value(serde_json::json!({
            "sessionUpdate": "goal_updated", "goal_id": if status == "cleared" { "" } else { "g1" },
            "objective": "inspect the workspace", "status": status, "elapsed_ms": at_ms,
            "tokens_used": 123, "token_budget": null, "created_at": "2026-09-29T00:00:00Z",
            "updated_at": "2026-09-29T00:00:01Z"
        })).unwrap(), Some(at_ms))
    }

    #[test]
    fn behavior_and_goal_changes_do_not_force_text_reveal_or_run_a_continuation() {
        let start = Instant::now();
        let body = "text still streaming ".repeat(15);
        let mut player = Player::new(
            vec![
                mode_event("normal", 0),
                text_event(&body, Some(10)),
                mode_event("goal", 100),
                goal_event("active", 110),
                mode_event("normal", 140),
                goal_event("paused", 150),
                goal_event("cleared", 160),
                grow_event(
                    GrowUpdate::TurnCompleted {
                        prompt_id: "p1".into(),
                        identity: None,
                        stop_reason: "cancelled".into(),
                        agent_result: None,
                        usage: None,
                    },
                    Some(1000),
                ),
            ],
            1.0,
            start,
        );
        player.tick(start + Duration::from_millis(200));
        assert_eq!(
            player.projection.behavior_label().as_deref(),
            Some("normal")
        );
        assert!(!projection_markdown(&player.projection).contains("Goal ·"));
        let partial = visible_text(&player);
        assert!(!partial.is_empty() && partial.len() < body.len());
        player
            .clock
            .pause_or_resume(start + Duration::from_millis(200));
        player.tick(start + Duration::from_secs(10));
        assert_eq!(visible_text(&player), partial);
        player
            .clock
            .pause_or_resume(start + Duration::from_secs(10));
        player.tick(start + Duration::from_secs(12));
        assert_eq!(visible_text(&player), body);
        assert_eq!(player.clock.state, PlayState::Finished);
        assert!(projection_markdown(&player.projection).contains("cancelled"));
    }

    #[test]
    fn recorded_goal_continuation_and_exit_have_export_parity_at_multiple_speeds() {
        fn events() -> Vec<TranscriptEvent> {
            let internal = acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(
                "private continuation directive",
            )))
            .meta(
                serde_json::json!({"messageId": "p2", "hideFromScrollback": true})
                    .as_object()
                    .cloned(),
            );
            vec![
                mode_event("goal", 0),
                goal_event("active", 10),
                text_event("First turn", Some(100)),
                grow_event(
                    GrowUpdate::TurnCompleted {
                        prompt_id: "p1".into(),
                        identity: None,
                        stop_reason: "end_turn".into(),
                        agent_result: None,
                        usage: None,
                    },
                    Some(200),
                ),
                acp_event(acp::SessionUpdate::UserMessageChunk(internal), Some(300)),
                text_event("Recorded continuation", Some(400)),
                grow_event(
                    GrowUpdate::TurnCompleted {
                        prompt_id: "p2".into(),
                        identity: None,
                        stop_reason: "cancelled".into(),
                        agent_result: None,
                        usage: None,
                    },
                    Some(600),
                ),
                goal_event("paused", 610),
                mode_event("normal", 620),
                goal_event("cleared", 630),
                acp_event(
                    acp::SessionUpdate::UserMessageChunk(acp::ContentChunk::new(
                        acp::ContentBlock::Text(acp::TextContent::new("follow-up request")),
                    )),
                    Some(700),
                ),
                text_event("Ordinary reply", Some(800)),
            ]
        }
        let mut full = TranscriptProjection::default();
        for event in events() {
            full.apply(event);
        }
        let expected = projection_markdown(&full);
        assert!(!expected.contains("private continuation directive"));
        assert!(!expected.contains("Goal ·"));
        for speed in [0.5, 1.0, 8.0] {
            let start = Instant::now();
            let mut player = Player::new(events(), speed, start);
            player.tick(start + Duration::from_secs_f64(0.605 / speed));
            assert_eq!(player.projection.behavior_label().as_deref(), Some("goal"));
            let interrupted = projection_markdown(&player.projection);
            assert!(interrupted.contains("cancelled"));
            assert!(interrupted.contains("Goal · inspect the workspace · active"));
            player.tick(start + Duration::from_secs(30));
            assert_eq!(player.clock.state, PlayState::Finished);
            assert_eq!(
                player.projection.behavior_label().as_deref(),
                Some("normal")
            );
            assert_eq!(projection_markdown(&player.projection), expected);
        }
    }
}
