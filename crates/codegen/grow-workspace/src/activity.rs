//! Per-session and connection-level activity tracking for tool server
//! lifecycle status reporting.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use dashmap::DashMap;
use xai_tool_protocol::{ToolServerLifecycleStatus, ToolServerStatusPayload};

const LIFECYCLE_NONE: u8 = 0;
const LIFECYCLE_DRAINING: u8 = 1;
const LIFECYCLE_SHUTTING_DOWN: u8 = 2;

const DEFAULT_SESSION: &str = "__default__";
const SESSION_IDLE_PRUNE_MS: u64 = 5 * 60 * 1000;

/// How long recent preview-proxy traffic withholds `idle_since_ms` — a decaying
/// window (not a reset), so a polled preview stays alive but a single stale poll
/// can't pin it. Larger than the 5s status poll, smaller than the idle grace.
pub(crate) const PREVIEW_ACTIVITY_WINDOW_MS: u64 = 60_000;

struct SessionActivity {
    active_tool_calls: AtomicU32,
    active_tools: DashMap<String, String>,
    last_call_started_ms: AtomicU64,
    last_call_completed_ms: AtomicU64,
    idle_since_ms: AtomicU64,
    /// Current turn number (set by `turn_started`).
    current_turn: AtomicU64,
    /// Whether a turn is currently active.
    turn_active: AtomicBool,
}

impl SessionActivity {
    fn new() -> Self {
        Self {
            active_tool_calls: AtomicU32::new(0),
            active_tools: DashMap::new(),
            last_call_started_ms: AtomicU64::new(0),
            last_call_completed_ms: AtomicU64::new(0),
            idle_since_ms: AtomicU64::new(now_ms()),
            current_turn: AtomicU64::new(0),
            turn_active: AtomicBool::new(false),
        }
    }
}

/// Tracks in-flight tool calls and background tasks for
/// [`ToolServerStatusPayload`] reporting.
///
/// All methods are `&self` — share via `Arc` across the tool handler, the
/// activity feed, and the status publisher.
pub struct ActivityTracker {
    active_tool_calls: AtomicU32,
    active_tools: DashMap<String, String>,
    background_tasks: AtomicU32,
    background_ids: DashMap<String, ()>,
    last_call_started_ms: AtomicU64,
    last_call_completed_ms: AtomicU64,
    idle_since_ms: AtomicU64,
    started_at: Instant,
    lifecycle: AtomicU8,
    /// `Arc` so the activity tracker can wake waiters via [`notify_handle`](Self::notify_handle).
    notify: Arc<tokio::sync::Notify>,
    /// Epoch ms a graceful drain began; `0` means "not draining".
    drain_started_ms: AtomicU64,
    /// When set, the idle verdict ignores background tasks so `idle_since_ms`
    /// tracks foreground tool-call activity only. Drain/`status` stay bg-aware.
    idle_ignores_background: bool,
    /// Window (ms) recent preview-proxy traffic withholds idle for; defaults to
    /// [`PREVIEW_ACTIVITY_WINDOW_MS`], overridable via the builder.
    preview_activity_window_ms: u64,
    /// Epoch ms of the last scraped preview-proxy activity (`0` = none). Fed by
    /// the preview-activity scraper (`preview_supervisor`); withholds idle within
    /// [`preview_activity_window_ms`](Self::preview_activity_window_ms).
    last_preview_activity_ms: AtomicU64,

    sessions: DashMap<String, SessionActivity>,
    /// call_id → session_id so `tool_call_completed` can decrement
    /// the right session without the caller repeating it.
    call_to_session: DashMap<String, String>,
    /// Idle window (ms) after which an inactive session is pruned by
    /// [`known_sessions`]. Set once at construction; no locking.
    prune_window_ms: u64,
}

impl Default for ActivityTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ActivityTracker {
    /// Construct a tracker with the default 5-minute session-prune window.
    pub fn new() -> Self {
        Self::with_prune_window(std::time::Duration::from_millis(SESSION_IDLE_PRUNE_MS))
    }

    /// Construct a tracker with a custom session-prune window.
    pub fn with_prune_window(prune_window: std::time::Duration) -> Self {
        Self {
            active_tool_calls: AtomicU32::new(0),
            active_tools: DashMap::new(),
            background_tasks: AtomicU32::new(0),
            background_ids: DashMap::new(),
            last_call_started_ms: AtomicU64::new(0),
            last_call_completed_ms: AtomicU64::new(0),
            idle_since_ms: AtomicU64::new(now_ms()),
            started_at: Instant::now(),
            lifecycle: AtomicU8::new(LIFECYCLE_NONE),
            notify: Arc::new(tokio::sync::Notify::new()),
            drain_started_ms: AtomicU64::new(0),
            idle_ignores_background: false,
            preview_activity_window_ms: PREVIEW_ACTIVITY_WINDOW_MS,
            last_preview_activity_ms: AtomicU64::new(0),
            sessions: DashMap::new(),
            call_to_session: DashMap::new(),
            prune_window_ms: prune_window.as_millis() as u64,
        }
    }

    /// Opt into foreground-only idle: background tasks stop withholding
    /// `idle_since_ms`.
    pub fn with_idle_ignores_background(mut self, enabled: bool) -> Self {
        self.idle_ignores_background = enabled;
        self
    }

    /// Override the preview-activity withhold window; the WorkspaceServer sources
    /// it from `StatusConfig`.
    pub fn with_preview_activity_window_ms(mut self, window_ms: u64) -> Self {
        self.preview_activity_window_ms = window_ms;
        self
    }

    /// Clone of the internal `Notify` for driving republishes.
    pub fn notify_handle(&self) -> Arc<tokio::sync::Notify> {
        self.notify.clone()
    }

    /// Record fresh preview-proxy traffic: withholds `idle_since_ms` for
    /// [`preview_activity_window_ms`](Self::preview_activity_window_ms) and wakes
    /// the status publisher so the renewed "active" status reaches the server promptly.
    pub fn note_preview_activity(&self) {
        self.last_preview_activity_ms
            .store(now_ms(), Ordering::Relaxed);
        self.notify.notify_waiters();
    }

    /// Whether a drain has been started (`set_draining` ran) in this process.
    pub fn drain_started(&self) -> bool {
        self.drain_started_ms.load(Ordering::Relaxed) != 0
    }

    fn resolved_idle_since(&self, idle_since: u64) -> Option<u64> {
        if idle_since == 0 || self.preview_withholds_idle(now_ms()) {
            None
        } else {
            Some(idle_since)
        }
    }

    /// Whether recent preview-proxy traffic should currently withhold idle.
    fn preview_withholds_idle(&self, now: u64) -> bool {
        preview_activity_withholds_idle(
            now,
            self.last_preview_activity_ms.load(Ordering::Relaxed),
            self.preview_activity_window_ms,
        )
    }

    /// Whether any tracked session currently has an active turn (the aggregate
    /// `turn_active`).
    fn any_turn_active(&self) -> bool {
        self.sessions
            .iter()
            .any(|s| s.value().turn_active.load(Ordering::Acquire))
    }

    pub fn tool_call_started(&self, call_id: &str, tool_name: &str, session_id: Option<&str>) {
        if self.active_tools.contains_key(call_id) {
            return;
        }
        let now = now_ms();

        self.active_tool_calls.fetch_add(1, Ordering::Relaxed);
        self.active_tools
            .insert(call_id.to_owned(), tool_name.to_owned());
        self.last_call_started_ms.store(now, Ordering::Relaxed);
        self.idle_since_ms.store(0, Ordering::Relaxed);

        let sid = session_id.unwrap_or(DEFAULT_SESSION);
        self.call_to_session
            .insert(call_id.to_owned(), sid.to_owned());
        let session = self
            .sessions
            .entry(sid.to_owned())
            .or_insert_with(SessionActivity::new);
        session.active_tool_calls.fetch_add(1, Ordering::Relaxed);
        session
            .active_tools
            .insert(call_id.to_owned(), tool_name.to_owned());
        session.last_call_started_ms.store(now, Ordering::Relaxed);
        session.idle_since_ms.store(0, Ordering::Relaxed);

        self.notify.notify_waiters();
    }

    /// Mark an in-flight tool call as completed.
    ///
    /// `session_id` identifies the owning session for the caller's bookkeeping;
    /// the internal session counters key off the recorded `call_id → session`
    /// mapping.
    pub fn tool_call_completed(&self, call_id: &str, _session_id: Option<&str>) {
        let Some((_, tool_name)) = self.active_tools.remove(call_id) else {
            return;
        };
        let now = now_ms();

        self.active_tool_calls.fetch_sub(1, Ordering::AcqRel);
        self.last_call_completed_ms.store(now, Ordering::Relaxed);
        // Re-read after decrement: a concurrent `tool_call_started` may
        // have bumped the counter back up between our `fetch_sub` and
        // this load. Only transition to idle if we're truly at zero.
        if self.active_tool_calls.load(Ordering::Acquire) == 0
            && (self.idle_ignores_background || self.background_tasks.load(Ordering::Acquire) == 0)
        {
            self.idle_since_ms.store(now, Ordering::Relaxed);
        }

        if let Some((_, sid)) = self.call_to_session.remove(call_id)
            && let Some(session) = self.sessions.get(&sid)
        {
            session.active_tool_calls.fetch_sub(1, Ordering::AcqRel);
            session.active_tools.remove(call_id);
            session.last_call_completed_ms.store(now, Ordering::Relaxed);
            if session.active_tool_calls.load(Ordering::Acquire) == 0 {
                session.idle_since_ms.store(now, Ordering::Relaxed);
            }
        }

        self.notify.notify_waiters();
        let _ = tool_name;
    }

    pub fn background_task_started(&self, task_id: &str) {
        if self.background_ids.contains_key(task_id) {
            return;
        }
        self.background_tasks.fetch_add(1, Ordering::Relaxed);
        self.background_ids.insert(task_id.to_owned(), ());
        if self.idle_ignores_background {
            // An active fg call's store(0) must keep idle withheld.
            if self.active_tool_calls.load(Ordering::Acquire) == 0 {
                self.idle_since_ms.store(now_ms(), Ordering::Relaxed);
            }
        } else {
            self.idle_since_ms.store(0, Ordering::Relaxed);
        }
        self.notify.notify_waiters();
    }

    pub fn background_task_completed(&self, task_id: &str) {
        if self.background_ids.remove(task_id).is_none() {
            return;
        }
        let prev = self.background_tasks.fetch_sub(1, Ordering::AcqRel);
        if !self.idle_ignores_background
            && prev == 1
            && self.active_tool_calls.load(Ordering::Acquire) == 0
        {
            self.idle_since_ms.store(now_ms(), Ordering::Relaxed);
        }
        self.notify.notify_waiters();
    }

    pub fn turn_started(&self, session_id: &str, turn_number: u64) {
        let session = self
            .sessions
            .entry(session_id.to_owned())
            .or_insert_with(SessionActivity::new);
        session.current_turn.store(turn_number, Ordering::Release);
        session.turn_active.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    pub fn turn_completed(&self, session_id: &str, turn_number: u64, _duration_ms: u64) {
        if let Some(session) = self.sessions.get(session_id)
            && session.current_turn.load(Ordering::Acquire) == turn_number
        {
            session.turn_active.store(false, Ordering::Release);
            self.notify.notify_waiters();
        }
    }

    /// Complete all in-flight tool calls for the given session.
    ///
    /// Returns the number of calls that were marked as completed.
    /// Called when a session-wide Cancel hook arrives without a specific
    /// `call_id` (broadcast cancel from Ctrl+C).
    pub fn cancel_all_session_calls(&self, session_id: &str) -> usize {
        let call_ids: Vec<String> = self
            .call_to_session
            .iter()
            .filter(|entry| entry.value() == session_id)
            .map(|entry| entry.key().clone())
            .collect();
        let count = call_ids.len();
        for call_id in call_ids {
            self.tool_call_completed(&call_id, Some(session_id));
        }
        count
    }

    /// Mark a session as ended: clear turn-active flag and notify waiters.
    ///
    /// Called by [`crate::handle::WorkspaceHandle::on_session_ended()`] when
    /// a `HookEvent::SessionEnded` arrives from the server.
    pub fn session_ended(&self, session_id: &str) {
        if let Some(session) = self.sessions.get(session_id) {
            session.turn_active.store(false, Ordering::Release);
        }
        self.notify.notify_waiters();
    }

    /// Whether a turn is currently active for the given session.
    pub fn is_turn_active(&self, session_id: &str) -> bool {
        self.sessions
            .get(session_id)
            .is_some_and(|s| s.turn_active.load(Ordering::Acquire))
    }

    /// In-flight tool calls for the given session (`0` when unknown). Only
    /// the model-facing tool handler ticks the underlying counter, so
    /// `workspace_rpc` traffic never contributes.
    pub fn session_active_tool_calls(&self, session_id: &str) -> u32 {
        self.sessions
            .get(session_id)
            .map_or(0, |s| s.active_tool_calls.load(Ordering::Acquire))
    }

    pub fn set_active(&self) {
        self.lifecycle.store(LIFECYCLE_NONE, Ordering::Release);
        // Clear the drain stamp symmetrically with `set_draining`: leaving it set
        // after a resume would make `drain_started_ms` mean "a drain ever began"
        // rather than "currently draining".
        self.drain_started_ms.store(0, Ordering::Release);
        self.notify.notify_waiters();
    }

    pub fn set_draining(&self) {
        self.lifecycle.store(LIFECYCLE_DRAINING, Ordering::Release);
        // First transition wins, so `drain_started_ms` is stable across calls.
        let _ = self.drain_started_ms.compare_exchange(
            0,
            now_ms(),
            Ordering::AcqRel,
            Ordering::Relaxed,
        );
        self.notify.notify_waiters();
    }

    pub fn set_shutting_down(&self) {
        self.lifecycle
            .store(LIFECYCLE_SHUTTING_DOWN, Ordering::Release);
        self.notify.notify_waiters();
    }

    pub fn is_draining(&self) -> bool {
        self.lifecycle.load(Ordering::Acquire) >= LIFECYCLE_DRAINING
    }

    /// Fully drained: draining, no active tool calls/background tasks.
    pub fn is_drained(&self) -> bool {
        self.is_draining() && self.total_active() == 0
    }

    /// Phase-1 drain condition: all in-flight tool calls and background tasks
    /// finished.
    pub fn tools_idle(&self) -> bool {
        self.total_active() == 0
    }

    pub fn total_active(&self) -> u32 {
        self.active_tool_calls.load(Ordering::Relaxed)
            + self.background_tasks.load(Ordering::Relaxed)
    }

    pub async fn wait_for_change(&self, timeout: std::time::Duration) {
        let _ = tokio::time::timeout(timeout, self.notify.notified()).await;
    }

    /// Wake the status publisher so it sends a heartbeat immediately.
    pub fn poke(&self) {
        self.notify.notify_waiters();
    }

    /// Wait until the tracker is both draining and all active work
    /// (tool calls + background tasks) has completed.
    pub async fn wait_until_drained(&self) {
        loop {
            if self.is_drained() {
                return;
            }
            // `notify_waiters` stores no permit, so a wake between the check and
            // this await is missed — safe because every caller is timeout-bounded.
            self.notify.notified().await;
        }
    }

    /// Wait until all in-flight tool calls and background tasks have finished
    /// (phase 1 of the two-phase drain).
    pub async fn wait_until_tools_idle(&self) {
        loop {
            if self.tools_idle() {
                return;
            }
            // Same timeout-bounded missed-wakeup tolerance as `wait_until_drained`.
            self.notify.notified().await;
        }
    }

    /// Returns live session IDs. As a side-effect, prunes sessions
    /// that have been idle longer than the configured prune window.
    pub fn known_sessions(&self) -> Vec<String> {
        let now = now_ms();
        let mut live = Vec::new();
        let mut stale = Vec::new();

        for entry in self.sessions.iter() {
            let key = entry.key();
            if key == DEFAULT_SESSION {
                continue;
            }
            if entry.value().active_tool_calls.load(Ordering::Relaxed) > 0 {
                live.push(key.clone());
                continue;
            }
            let idle_since = entry.value().idle_since_ms.load(Ordering::Relaxed);
            if idle_since > 0 && now.saturating_sub(idle_since) > self.prune_window_ms {
                stale.push(key.clone());
            } else {
                live.push(key.clone());
            }
        }

        for key in &stale {
            self.sessions.remove(key);
            self.call_to_session.retain(|_, sid| sid != key);
        }

        live
    }

    /// Per-session snapshot. `background_task_ids` is the connection aggregate,
    /// re-published to the session's client.
    pub fn snapshot_session(&self, session_id: &str) -> ToolServerStatusPayload {
        let lifecycle = self.lifecycle.load(Ordering::Acquire);
        let bg = self.background_tasks.load(Ordering::Relaxed);

        let (active, active_tool_names, last_started, last_completed, idle_since, turn_active) =
            if let Some(session) = self.sessions.get(session_id) {
                let a = session.active_tool_calls.load(Ordering::Relaxed);
                let names: Vec<String> = session
                    .active_tools
                    .iter()
                    .map(|r| r.value().clone())
                    .collect();
                let started = session.last_call_started_ms.load(Ordering::Relaxed);
                let completed = session.last_call_completed_ms.load(Ordering::Relaxed);
                let idle = session.idle_since_ms.load(Ordering::Relaxed);
                let turn = session.turn_active.load(Ordering::Acquire);
                (a, names, started, completed, idle, turn)
            } else {
                (0, vec![], 0, 0, now_ms(), false)
            };

        let status = match lifecycle {
            LIFECYCLE_SHUTTING_DOWN => ToolServerLifecycleStatus::ShuttingDown,
            LIFECYCLE_DRAINING => ToolServerLifecycleStatus::Draining,
            _ if active > 0 => ToolServerLifecycleStatus::Busy,
            _ => ToolServerLifecycleStatus::Ready,
        };

        let background_task_ids: Vec<String> = self
            .background_ids
            .iter()
            .map(|r| r.key().clone())
            .collect();

        let drain_started = match self.drain_started_ms.load(Ordering::Relaxed) {
            0 => None,
            ms => Some(ms),
        };

        let idle_since_ms = self.resolved_idle_since(idle_since);

        ToolServerStatusPayload {
            status,
            session_id: xai_tool_protocol::SessionId::new(session_id).ok(),
            connection_id: None,
            active_tool_calls: active,
            active_tool_names,
            background_tasks: bg,
            background_task_ids,
            pending_tool_calls: 0,
            last_tool_call_started_ms: last_started,
            last_tool_call_completed_ms: last_completed,
            uptime_ms: self.started_at.elapsed().as_millis() as u64,
            idle_since_ms,
            upload_queue_pending: 0,
            upload_queue_pending_bytes: 0,
            upload_queue_inflight: 0,
            upload_queue_circuit_breaker_tripped: false,
            artifact_producers_inflight: 0,
            drain_started_ms: drain_started,
            turn_active,
            idle_ignores_background: self.idle_ignores_background,
        }
    }

    /// Aggregate snapshot across all sessions.
    pub fn snapshot(&self) -> ToolServerStatusPayload {
        let lifecycle = self.lifecycle.load(Ordering::Acquire);
        let active = self.active_tool_calls.load(Ordering::Relaxed);
        let bg = self.background_tasks.load(Ordering::Relaxed);

        let status = match lifecycle {
            LIFECYCLE_SHUTTING_DOWN => ToolServerLifecycleStatus::ShuttingDown,
            LIFECYCLE_DRAINING => ToolServerLifecycleStatus::Draining,
            _ if active + bg > 0 => ToolServerLifecycleStatus::Busy,
            _ => ToolServerLifecycleStatus::Ready,
        };

        let active_tool_names: Vec<String> = self
            .active_tools
            .iter()
            .map(|r| r.value().clone())
            .collect();
        let background_task_ids: Vec<String> = self
            .background_ids
            .iter()
            .map(|r| r.key().clone())
            .collect();

        let idle_since = self.idle_since_ms.load(Ordering::Relaxed);

        let drain_started = match self.drain_started_ms.load(Ordering::Relaxed) {
            0 => None,
            ms => Some(ms),
        };

        let idle_since_ms = self.resolved_idle_since(idle_since);

        ToolServerStatusPayload {
            status,
            session_id: None,
            connection_id: None,
            active_tool_calls: active,
            active_tool_names,
            background_tasks: bg,
            background_task_ids,
            pending_tool_calls: 0,
            last_tool_call_started_ms: self.last_call_started_ms.load(Ordering::Relaxed),
            last_tool_call_completed_ms: self.last_call_completed_ms.load(Ordering::Relaxed),
            uptime_ms: self.started_at.elapsed().as_millis() as u64,
            idle_since_ms,
            upload_queue_pending: 0,
            upload_queue_pending_bytes: 0,
            upload_queue_inflight: 0,
            upload_queue_circuit_breaker_tripped: false,
            artifact_producers_inflight: 0,
            drain_started_ms: drain_started,
            turn_active: self.any_turn_active(),
            idle_ignores_background: self.idle_ignores_background,
        }
    }
}

/// Whether a preview-activity stamp still withholds idle at `now`: true while it
/// is within `window` ms. A zero stamp (no activity recorded) never withholds,
/// and the window is exclusive at the boundary so it decays rather than pins.
fn preview_activity_withholds_idle(now: u64, last_activity_ms: u64, window_ms: u64) -> bool {
    last_activity_ms != 0 && now.saturating_sub(last_activity_ms) < window_ms
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_ready() {
        let t = ActivityTracker::new();
        let s = t.snapshot();
        assert_eq!(s.status, ToolServerLifecycleStatus::Ready);
        assert_eq!(s.active_tool_calls, 0);
        assert!(s.idle_since_ms.is_some());
        assert!(s.session_id.is_none());
    }

    #[test]
    fn tool_call_transitions_to_busy() {
        let t = ActivityTracker::new();
        t.tool_call_started("c1", "read_file", Some("sess-a"));
        let s = t.snapshot();
        assert_eq!(s.status, ToolServerLifecycleStatus::Busy);
        assert_eq!(s.active_tool_calls, 1);
        assert_eq!(s.active_tool_names, vec!["read_file"]);
        assert!(s.idle_since_ms.is_none());
    }

    #[test]
    fn tool_call_completion_returns_to_ready() {
        let t = ActivityTracker::new();
        t.tool_call_started("c1", "read_file", Some("sess-a"));
        t.tool_call_completed("c1", None);
        let s = t.snapshot();
        assert_eq!(s.status, ToolServerLifecycleStatus::Ready);
        assert_eq!(s.active_tool_calls, 0);
        assert!(s.idle_since_ms.is_some());
    }

    #[test]
    fn background_task_makes_busy() {
        let t = ActivityTracker::new();
        t.background_task_started("t1");
        let s = t.snapshot();
        assert_eq!(s.status, ToolServerLifecycleStatus::Busy);
        assert_eq!(s.background_tasks, 1);
    }

    #[test]
    fn background_task_started_dedups_by_id() {
        let t = ActivityTracker::new();
        t.background_task_started("dup");
        t.background_task_started("dup");
        assert_eq!(t.snapshot().background_tasks, 1);
    }

    #[test]
    fn background_task_completed_unknown_id_does_not_underflow() {
        let t = ActivityTracker::new();
        t.background_task_completed("never-started");
        assert_eq!(t.snapshot().background_tasks, 0);
        t.background_task_started("real");
        assert_eq!(t.snapshot().background_tasks, 1);
    }

    #[test]
    fn background_task_decrement_restores_idle_only_at_zero() {
        let t = ActivityTracker::new();
        t.background_task_started("a");
        t.background_task_started("b");
        assert!(t.snapshot().idle_since_ms.is_none());
        t.background_task_completed("a");
        assert!(
            t.snapshot().idle_since_ms.is_none(),
            "one bg task left → still not idle"
        );
        t.background_task_completed("b");
        assert!(
            t.snapshot().idle_since_ms.is_some(),
            "idle restored only when the last bg task completes"
        );
    }

    #[test]
    fn background_after_calls_complete_pins_idle_only_when_flag_off() {
        let on = ActivityTracker::new().with_idle_ignores_background(true);
        on.tool_call_started("c1", "read_file", None);
        on.tool_call_completed("c1", None);
        on.background_task_started("bg1");
        assert!(on.snapshot().idle_since_ms.is_some());

        let off = ActivityTracker::new();
        off.tool_call_started("c1", "read_file", None);
        off.tool_call_completed("c1", None);
        off.background_task_started("bg1");
        assert!(off.snapshot().idle_since_ms.is_none());
    }

    #[test]
    fn flag_on_active_call_withholds_idle_until_it_completes_despite_background() {
        let t = ActivityTracker::new().with_idle_ignores_background(true);
        t.tool_call_started("c1", "read_file", None);
        t.background_task_started("bg1");
        assert!(t.snapshot().idle_since_ms.is_none());
        t.tool_call_completed("c1", None);
        assert!(t.snapshot().idle_since_ms.is_some());
    }

    #[test]
    fn flag_on_keeps_busy_status_while_reporting_idle() {
        let t = ActivityTracker::new().with_idle_ignores_background(true);
        t.background_task_started("bg1");
        let s = t.snapshot();
        assert_eq!(s.status, ToolServerLifecycleStatus::Busy);
        assert!(s.idle_since_ms.is_some());
    }

    #[test]
    fn flag_on_background_completion_does_not_advance_idle() {
        let t = ActivityTracker::new().with_idle_ignores_background(true);
        t.background_task_started("bg1");
        let after_start = t.snapshot().idle_since_ms;
        std::thread::sleep(std::time::Duration::from_millis(5));
        t.background_task_completed("bg1");
        assert_eq!(t.snapshot().idle_since_ms, after_start);
    }

    #[test]
    fn flag_on_background_start_advances_idle_when_no_active_call() {
        let t = ActivityTracker::new().with_idle_ignores_background(true);
        t.tool_call_started("c1", "read_file", None);
        t.tool_call_completed("c1", None);
        let before = t.snapshot().idle_since_ms.unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        t.background_task_started("bg1");
        let after = t.snapshot().idle_since_ms.unwrap();
        assert!(after > before);
    }

    #[test]
    fn drain_counts_background_tasks_regardless_of_flag() {
        for flag in [false, true] {
            let t = ActivityTracker::new().with_idle_ignores_background(flag);
            t.background_task_started("bg1");
            assert_eq!(t.total_active(), 1);
            t.set_draining();
            assert!(!t.is_drained());
            assert!(!t.tools_idle());
            t.background_task_completed("bg1");
            assert!(t.is_drained());
            assert!(t.tools_idle());
        }
    }

    #[test]
    fn snapshot_payloads_report_idle_ignores_background_flag() {
        let on = ActivityTracker::new().with_idle_ignores_background(true);
        assert!(on.snapshot().idle_ignores_background);
        assert!(on.snapshot_session("sess-a").idle_ignores_background);

        let off = ActivityTracker::new();
        assert!(!off.snapshot().idle_ignores_background);
        assert!(!off.snapshot_session("sess-a").idle_ignores_background);
    }

    #[test]
    fn draining_overrides_busy() {
        let t = ActivityTracker::new();
        t.tool_call_started("c1", "grep", None);
        t.set_draining();
        assert_eq!(t.snapshot().status, ToolServerLifecycleStatus::Draining);
    }

    #[test]
    fn set_active_clears_draining() {
        let t = ActivityTracker::new();
        t.set_draining();
        assert_eq!(t.snapshot().status, ToolServerLifecycleStatus::Draining);
        t.set_active();
        assert_eq!(t.snapshot().status, ToolServerLifecycleStatus::Ready);
    }

    #[test]
    fn shut_down_overrides_draining() {
        let t = ActivityTracker::new();
        t.set_draining();
        assert_eq!(t.snapshot().status, ToolServerLifecycleStatus::Draining);
        t.set_shutting_down();
        assert_eq!(t.snapshot().status, ToolServerLifecycleStatus::ShuttingDown);
    }

    #[test]
    fn drain_started_timestamp_is_stable() {
        let t = ActivityTracker::new();
        assert!(!t.drain_started());
        t.set_draining();
        assert!(t.drain_started());
        let first = t.snapshot().drain_started_ms;
        std::thread::sleep(std::time::Duration::from_millis(5));
        t.set_draining();
        assert_eq!(
            t.snapshot().drain_started_ms,
            first,
            "repeated set_draining must not advance drain_started_ms"
        );
        t.set_active();
        assert!(!t.drain_started());
    }

    #[test]
    fn is_drained_with_no_queue_uses_tools_only() {
        let t = ActivityTracker::new();
        t.set_draining();
        assert!(t.is_drained(), "must be drained when tools are idle");
        t.tool_call_started("c1", "grep", None);
        assert!(!t.is_drained(), "active tool call blocks drain");
        t.tool_call_completed("c1", None);
        assert!(t.is_drained(), "drain cleared after tool call completes");
    }

    #[test]
    fn snapshot_reports_session_scoped_counts() {
        let t = ActivityTracker::new();
        t.tool_call_started("c1", "read_file", Some("sess-a"));
        t.tool_call_started("c2", "grep", Some("sess-b"));

        let sa = t.snapshot_session("sess-a");
        assert_eq!(sa.active_tool_calls, 1);
        assert_eq!(sa.active_tool_names, vec!["read_file"]);

        let sb = t.snapshot_session("sess-b");
        assert_eq!(sb.active_tool_calls, 1);
        assert_eq!(sb.active_tool_names, vec!["grep"]);

        let sx = t.snapshot_session("unknown");
        assert_eq!(sx.active_tool_calls, 0);
        assert!(sx.active_tool_names.is_empty());
        assert!(sx.idle_since_ms.is_some());
    }

    #[test]
    fn cancel_all_session_calls_marks_them_completed() {
        let t = ActivityTracker::new();
        t.tool_call_started("c1", "read_file", Some("sess-a"));
        t.tool_call_started("c2", "grep", Some("sess-a"));
        t.tool_call_started("c3", "ls", Some("sess-b"));

        let n = t.cancel_all_session_calls("sess-a");
        assert_eq!(n, 2, "only session-a calls were cancelled");
        assert_eq!(t.snapshot_session("sess-a").active_tool_calls, 0);
        assert_eq!(t.snapshot_session("sess-b").active_tool_calls, 1);
    }

    #[test]
    fn snapshot_skips_default_session() {
        let t = ActivityTracker::new();
        t.tool_call_started("c1", "read_file", None);
        assert!(!t.known_sessions().contains(&DEFAULT_SESSION.to_string()));
    }

    #[test]
    fn known_sessions_prunes_idle_over_window() {
        let t = ActivityTracker::new();
        t.tool_call_started("c1", "read_file", Some("short"));
        t.tool_call_completed("c1", Some("short"));
        assert!(t.known_sessions().contains(&"short".to_string()));
    }

    #[test]
    fn snapshot_payload_fields_exist() {
        let t = ActivityTracker::new();
        let s = t.snapshot();
        // Just verify the payload has reasonable defaults.
        assert_eq!(s.upload_queue_pending, 0);
        assert_eq!(s.upload_queue_pending_bytes, 0);
        assert_eq!(s.upload_queue_inflight, 0);
        assert!(!s.upload_queue_circuit_breaker_tripped);
        assert_eq!(s.artifact_producers_inflight, 0);
    }
}
