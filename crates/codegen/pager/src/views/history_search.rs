//! Prompt history search with background-thread nucleo matching.
//!
//! Architecture mirrors the file search `FuzzyFileMatcherDaemon`:
//! - A background `std::thread` owns the nucleo `Matcher` + `MultiPattern`.
//! - The UI thread sends queries via a channel (`set_query`) — never blocks.
//! - The background thread scores items, computes indices, writes results
//!   to `Arc<Mutex<…>>`.
//! - The worker publishes a snapshot and wakes the pager; the UI reducer then
//!   applies it via `poll()` exactly when completion is signaled.

use std::sync::{
    Arc, Mutex,
    mpsc::{SyncSender, sync_channel},
};
use std::thread::{self, JoinHandle};

use nucleo::{
    Config, Matcher, Utf32String,
    pattern::{CaseMatching, MultiPattern, Normalization},
};

// ---------------------------------------------------------------------------
// Public data types
// ---------------------------------------------------------------------------

/// A single entry in the prompt history.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub text: String,
}

/// A matched result with highlight positions (produced by the daemon).
#[derive(Debug, Clone)]
pub struct HistoryMatchResult {
    pub text: String,
    pub indices: Vec<u32>,
}

// ---------------------------------------------------------------------------
// Shared state (daemon → UI)
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct Snapshot {
    items: Arc<[HistoryMatchResult]>,
    generation: usize,
}

// ---------------------------------------------------------------------------
// Daemon messages (UI → daemon)
// ---------------------------------------------------------------------------

enum Msg {
    SetItems(Vec<String>),
    SetItemsAndQuery(Vec<String>, String),
    SetQuery(String),
    Stop,
}

// ---------------------------------------------------------------------------
// Background daemon
// ---------------------------------------------------------------------------

struct Daemon {
    shared: Arc<Mutex<Snapshot>>,
    tx: SyncSender<()>,
    pending: Arc<Mutex<Option<(usize, Msg)>>>,
    next_generation: std::cell::Cell<usize>,
    handle: Option<JoinHandle<()>>,
}

const MAX_RESULTS: usize = 100;

impl Daemon {
    fn new() -> Self {
        let shared = Arc::new(Mutex::new(Snapshot::default()));
        let (tx, rx) = sync_channel::<()>(1);
        let pending = Arc::new(Mutex::new(None));
        let worker_pending = Arc::clone(&pending);

        let out = shared.clone();
        let worker = move || {
            let mut pattern = MultiPattern::new(1);
            let mut matcher = Matcher::new(Config::DEFAULT);
            let mut items: Vec<(String, Utf32String)> = Vec::new();
            let mut prev_q = String::new();

            while rx.recv().is_ok() {
                let msg = worker_pending.lock().unwrap().take();
                let Some((generation, msg)) = msg else { continue };

                match msg {
                    Msg::SetItems(new) => {
                        items = build_items(new);
                        prev_q.clear();
                        publish_matches(&items, "", &mut pattern, &mut matcher, &out, generation);
                    }
                    Msg::SetItemsAndQuery(new, query) => {
                        items = build_items(new);
                        prev_q.clear();
                        let trimmed = query.trim().to_string();
                        publish_matches(
                            &items,
                            &trimmed,
                            &mut pattern,
                            &mut matcher,
                            &out,
                            generation,
                        );
                        prev_q = trimmed;
                    }
                    Msg::SetQuery(query) => {
                        let trimmed = query.trim().to_string();

                        if trimmed.is_empty() {
                            publish_matches(
                                &items,
                                "",
                                &mut pattern,
                                &mut matcher,
                                &out,
                                generation,
                            );
                            prev_q.clear();
                        } else {
                            let append = !prev_q.is_empty()
                                && trimmed.as_bytes().starts_with(prev_q.as_bytes())
                                && !trimmed.ends_with('\\')
                                && !trimmed
                                    .as_bytes()
                                    .last()
                                    .is_some_and(|b| b.is_ascii_whitespace());
                            publish_query_matches(
                                &items,
                                &trimmed,
                                append,
                                &mut pattern,
                                &mut matcher,
                                &out,
                                generation,
                            );
                            prev_q = trimmed;
                        }
                    }
                    Msg::Stop => break,
                }
                crate::async_view::wake();
            }
        };
        let handle = thread::Builder::new()
            .name("history-search".into())
            .spawn(worker);

        let handle = match handle {
            Ok(h) => Some(h),
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "history search daemon thread spawn failed; history search disabled"
                );
                None
            }
        };

        Self { shared, tx, pending, next_generation: std::cell::Cell::new(0), handle }
    }
}

fn build_items(items: Vec<String>) -> Vec<(String, Utf32String)> {
    items
        .into_iter()
        .filter(|s| !s.is_empty())
        .map(|s| {
            let u = Utf32String::from(s.as_str());
            (s, u)
        })
        .collect()
}

fn publish_matches(
    items: &[(String, Utf32String)],
    query: &str,
    pattern: &mut MultiPattern,
    matcher: &mut Matcher,
    out: &Arc<Mutex<Snapshot>>,
    generation: usize,
) {
    if query.is_empty() {
        // Items arrive most-recent-first; reverse so the most recent prompt is
        // last (rendered at the bottom of the overlay, nearest the prompt).
        let mut all: Vec<HistoryMatchResult> = items
            .iter()
            .take(MAX_RESULTS)
            .map(|(s, _)| HistoryMatchResult {
                text: s.clone(),
                indices: Vec::new(),
            })
            .collect();
        all.reverse();
        *out.lock().unwrap() = Snapshot {
            items: all.into(),
            generation,
        };
    } else {
        publish_query_matches(items, query, false, pattern, matcher, out, generation);
    }
}

fn publish_query_matches(
    items: &[(String, Utf32String)],
    query: &str,
    append: bool,
    pattern: &mut MultiPattern,
    matcher: &mut Matcher,
    out: &Arc<Mutex<Snapshot>>,
    generation: usize,
) {
    pattern.reparse(0, query, CaseMatching::Smart, Normalization::Smart, append);

    let mut hits: Vec<(usize, u32)> = Vec::new();
    for (i, (_, u)) in items.iter().enumerate() {
        if let Some(sc) = pattern.score(std::slice::from_ref(u), matcher) {
            hits.push((i, sc));
        }
    }
    hits.sort_unstable_by(|a, b| b.1.cmp(&a.1));
    if hits.len() > MAX_RESULTS {
        hits.truncate(MAX_RESULTS);
    }

    let col = pattern.column_pattern(0);
    let mut matched: Vec<HistoryMatchResult> = hits
        .into_iter()
        .map(|(i, _)| {
            let (text, u) = &items[i];
            let mut idx = Vec::new();
            col.indices(u.slice(..), matcher, &mut idx);
            HistoryMatchResult {
                text: text.clone(),
                indices: idx,
            }
        })
        .collect();
    // `hits` is sorted best-first; reverse so the best match is last (rendered
    // at the bottom of the overlay, selected by default).
    matched.reverse();
    *out.lock().unwrap() = Snapshot {
        items: matched.into(),
        generation,
    };
}

/// Preserve the latest item refresh together with its following query.
fn merge_pending(current: Msg, next: Msg) -> Msg {
    match (current, next) {
        (Msg::Stop, _) | (_, Msg::Stop) => Msg::Stop,
        (Msg::SetItems(items), Msg::SetQuery(query))
        | (Msg::SetItemsAndQuery(items, _), Msg::SetQuery(query)) => {
            Msg::SetItemsAndQuery(items, query)
        }
        (_, next) => next,
    }
}

impl Daemon {
    fn submit(&self, msg: Msg) -> usize {
        let generation = self.next_generation.get().checked_add(1).expect("history request generation exhausted");
        self.next_generation.set(generation);
        let mut pending = self.pending.lock().unwrap();
        *pending = Some((generation, match pending.take() {
            Some((_, current)) => merge_pending(current, msg),
            None => msg,
        }));
        // A full channel already carries the required wake. Matching and
        // publishing happen outside this short pending-state critical section.
        let _ = self.tx.try_send(());
        generation
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.submit(Msg::Stop);
    }
}

// ---------------------------------------------------------------------------
// HistorySearchState (UI-thread side)
// ---------------------------------------------------------------------------

/// UI-side state for the history search overlay.
///
/// The UI thread never runs nucleo. All matching happens on the daemon
/// thread. The UI sends queries via `update_query()` and applies signaled
/// snapshots via `poll()`, exactly like `FuzzyFileMatcherDaemon`.
pub struct HistorySearchState {
    active: bool,
    saved_text: String,
    snapshot: Snapshot,
    requested_generation: usize,
    pub selected: usize,
    /// While `true`, selection tracks the bottom-most (most-recent / best-match)
    /// entry as results stream in. Set on `activate`, cleared once the user
    /// navigates (Up/Down/PageUp/PageDown/click). This makes the overlay open
    /// with the most recent prompt selected at the bottom of the list.
    stick_to_bottom: bool,
    /// The last query sent to the daemon. Used to distinguish a genuine query
    /// change (user typing → re-anchor selection to the best match) from a
    /// re-application of the same query (e.g. a late background
    /// `PromptHistoryLoaded` refresh → must not clobber the user's selection).
    last_query: String,
    /// Mouse-hovered result index (visual highlight only).
    hovered: Option<usize>,
    /// Browse mode (Up-arrow entry point): the selection lives in the
    /// composer (live-populated on every move), typing detaches to edit,
    /// and Down at the newest closes. Search mode (`/history`)
    /// keeps the composer as the filter query instead.
    browse: bool,
    daemon: Daemon,
}

impl Default for HistorySearchState {
    fn default() -> Self {
        Self::new()
    }
}

impl HistorySearchState {
    pub fn new() -> Self {
        Self {
            active: false,
            saved_text: String::new(),
            snapshot: Snapshot::default(),
            requested_generation: 0,
            selected: 0,
            stick_to_bottom: true,
            last_query: String::new(),
            hovered: None,
            browse: false,
            daemon: Daemon::new(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn saved_text(&self) -> &str {
        &self.saved_text
    }

    pub fn result_count(&self) -> usize {
        self.snapshot.items.len()
    }

    pub fn refresh_items(&mut self, history: &[HistoryEntry]) {
        let items: Vec<String> = history.iter().map(|e| e.text.clone()).collect();
        self.submit_request(Msg::SetItems(items));
    }

    /// Activate in SEARCH mode (`/history`): send items to the
    /// daemon, show overlay. The composer is the filter query; navigation
    /// highlights only, Enter/Tab accepts.
    pub fn activate(&mut self, history: &[HistoryEntry], current_text: &str) {
        self.activate_inner(history, current_text, false);
    }

    /// Activate in BROWSE mode (Up on an empty prompt): same panel, but the
    /// caller fills the newest entry straight into the composer and every
    /// selection move live-populates it; typing detaches to edit, and Down
    /// at the newest entry closes the panel.
    pub fn activate_browse(&mut self, history: &[HistoryEntry], current_text: &str) {
        self.activate_inner(history, current_text, true);
    }

    fn activate_inner(&mut self, history: &[HistoryEntry], current_text: &str, browse: bool) {
        if !self.is_available() {
            return;
        }
        self.active = true;
        self.browse = browse;
        self.saved_text = current_text.to_string();
        // Open with the most-recent prompt (rendered at the bottom) selected;
        // `poll` keeps it pinned to the bottom until the user navigates.
        self.stick_to_bottom = true;
        self.last_query.clear();
        self.refresh_items(history);
        self.selected = 0;
    }

    /// False when the matcher thread never started, so the overlay cannot open
    /// and callers must leave the composer alone.
    pub fn is_available(&self) -> bool {
        self.daemon.handle.is_some()
    }

    /// True while the overlay is in browse mode (see [`Self::activate_browse`]).
    pub fn is_browse(&self) -> bool {
        self.active && self.browse
    }

    /// Deactivate: clear overlay (daemon thread stays alive for reuse).
    pub fn deactivate(&mut self) {
        self.active = false;
        self.browse = false;
        self.snapshot = Snapshot::default();
        self.selected = 0;
    }

    /// Send a query update to the daemon (non-blocking, never stalls UI).
    pub fn update_query(&mut self, query: &str) {
        // A genuinely new query (the user typed) re-anchors selection to the
        // best match at the bottom. Re-applying the *same* query (e.g. a late
        // background `PromptHistoryLoaded` refresh that re-sends the current
        // query) must not move a selection the user has already navigated to.
        if query != self.last_query {
            self.last_query = query.to_string();
            self.stick_to_bottom = true;
        }
        self.submit_request(Msg::SetQuery(query.to_string()));
    }

    fn submit_request(&mut self, msg: Msg) {
        self.requested_generation = self.daemon.submit(msg);
        self.snapshot = Snapshot::default();
        self.hovered = None;
    }

    /// Apply a signaled result snapshot. Returns `true` if changed.
    pub fn poll(&mut self) -> bool {
        if !self.active {
            return false;
        }
        let snap = self.daemon.shared.lock().unwrap().clone();
        if snap.generation != self.requested_generation
            || snap.generation == self.snapshot.generation {
            return false;
        }
        self.snapshot = snap;
        let len = self.snapshot.items.len();
        if len == 0 {
            self.selected = 0;
        } else if self.stick_to_bottom {
            // Keep the most-recent / best-match (bottom) entry selected as
            // results stream in or the query narrows.
            self.selected = len - 1;
        } else {
            self.selected = self.selected.min(len - 1);
        }
        true
    }

    /// Currently hovered index (mouse-driven), if any.
    pub fn hovered(&self) -> Option<usize> {
        self.hovered
    }

    /// Set hovered index. Returns `true` if changed.
    pub fn set_hovered(&mut self, index: Option<usize>) -> bool {
        let clamped = index.and_then(|i| {
            if i < self.snapshot.items.len() {
                Some(i)
            } else {
                None
            }
        });
        let changed = clamped != self.hovered;
        self.hovered = clamped;
        changed
    }

    /// Select the hovered item (for click-to-accept). Returns `true` if valid.
    pub fn select_hovered(&mut self) -> bool {
        if let Some(idx) = self.hovered
            && idx < self.snapshot.items.len()
        {
            self.stick_to_bottom = false;
            self.selected = idx;
            true
        } else {
            false
        }
    }

    /// Move the selection one row up (older). No wrap: at the top (oldest)
    /// the selection stays put. Returns `true` when it moved.
    pub fn move_up(&mut self) -> bool {
        let len = self.snapshot.items.len();
        if len == 0 || self.selected == 0 {
            return false;
        }
        self.stick_to_bottom = false;
        self.selected -= 1;
        true
    }

    /// Move the selection one row down (newer). No wrap: returns `false` at
    /// the bottom (newest) — the caller closes the overlay there, so a Down
    /// right after opening (newest is selected) backs out of history.
    pub fn move_down(&mut self) -> bool {
        let len = self.snapshot.items.len();
        if len == 0 || self.selected >= len - 1 {
            return false;
        }
        self.stick_to_bottom = false;
        self.selected += 1;
        true
    }

    /// Move selection by a page (half of visible height).
    pub fn page_move(&mut self, delta: isize, visible_rows: usize) {
        let half = (visible_rows / 2).max(1) as isize;
        let len = self.snapshot.items.len();
        if len == 0 {
            return;
        }
        self.stick_to_bottom = false;
        let max_idx = len - 1;
        let current = self.selected.min(max_idx);
        self.selected = (current as isize + delta * half).clamp(0, max_idx as isize) as usize;
    }

    /// Text of the currently selected entry.
    pub fn selected_text(&self) -> Option<&str> {
        self.snapshot
            .items
            .get(self.selected)
            .map(|r| r.text.as_str())
    }

    /// Get a result at a given index (for rendering).
    pub fn result_at(&self, idx: usize) -> Option<&HistoryMatchResult> {
        self.snapshot.items.get(idx)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_snapshots_cannot_be_accepted_after_reopen_query_or_refresh() {
        let mut state = HistorySearchState::new();
        // Prevent the real worker from racing the injected completion snapshots.
        state.daemon.submit(Msg::Stop);
        state.daemon.handle.take().unwrap().join().unwrap();
        state.daemon.handle = Some(std::thread::spawn(|| {}));
        let make_snapshot = |generation, text: &str| Snapshot {
            generation,
            items: vec![HistoryMatchResult { text: text.into(), indices: vec![] }].into(),
        };
        state.activate(&entries(&["old"]), "");
        let old = state.requested_generation;
        *state.daemon.shared.lock().unwrap() = make_snapshot(old, "old");
        assert!(state.poll());
        assert_eq!(state.selected_text(), Some("old"));
        state.deactivate();
        state.activate(&entries(&["new"]), "");
        assert_eq!(state.selected_text(), None);
        assert!(!state.poll());
        let reopened = state.requested_generation;
        state.update_query("new");
        *state.daemon.shared.lock().unwrap() = make_snapshot(reopened, "outdated");
        assert!(!state.poll());
        assert_eq!(state.result_count(), 0);
        let current = state.requested_generation;
        *state.daemon.shared.lock().unwrap() = make_snapshot(current, "new");
        assert!(state.poll());
        assert!(!state.poll());
        assert_eq!(state.selected_text(), Some("new"));
        state.set_hovered(Some(0));
        state.refresh_items(&entries(&["newer"]));
        assert_eq!(state.selected_text(), None);
        assert_eq!(state.hovered(), None);
        assert!(!state.select_hovered());
        assert!(!state.poll());
    }

    #[test]
    fn pending_updates_and_drop_do_not_wait_for_a_worker() {
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let (tx, rx) = sync_channel(1);
            let pending = Arc::new(Mutex::new(None));
            let daemon = Daemon {
                shared: Arc::new(Mutex::new(Snapshot::default())),
                tx, pending: Arc::clone(&pending), next_generation: std::cell::Cell::new(0), handle: None,
            };
            daemon.submit(Msg::SetItems(vec!["latest item".into()]));
            for index in 0..1000 {
                daemon.submit(Msg::SetQuery(format!("query-{index}")));
            }
            assert!(matches!(&*pending.lock().unwrap(),
                Some((_, Msg::SetItemsAndQuery(items, query)))
                if items == &["latest item"] && query == "query-999"));
            // No receiver has consumed a notification: Drop must still return.
            drop(daemon);
            assert!(matches!(*pending.lock().unwrap(), Some((_, Msg::Stop))));
            assert!(rx.try_recv().is_ok());
            assert!(rx.try_recv().is_err());
            done_tx.send(()).unwrap();
        });
        done_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn pending_refresh_resets_query_and_stop_wins() {
        let old = Msg::SetItemsAndQuery(vec!["old".into()], "old-query".into());
        let refreshed = merge_pending(old, Msg::SetItems(vec!["new".into()]));
        assert!(matches!(&refreshed, Msg::SetItems(items) if items == &["new"]));
        let queried = merge_pending(refreshed, Msg::SetQuery("new-query".into()));
        assert!(matches!(queried, Msg::SetItemsAndQuery(items, query)
            if items == ["new"] && query == "new-query"));
        assert!(matches!(merge_pending(Msg::Stop, Msg::SetQuery("late".into())), Msg::Stop));
        assert!(matches!(merge_pending(Msg::SetQuery("old".into()), Msg::Stop), Msg::Stop));
    }

    fn entries(texts: &[&str]) -> Vec<HistoryEntry> {
        texts
            .iter()
            .map(|t| HistoryEntry {
                text: t.to_string(),
            })
            .collect()
    }

    /// Helper: activate + poll until results arrive.
    fn activate_and_poll(state: &mut HistorySearchState, history: &[HistoryEntry], saved: &str) {
        state.activate(history, saved);
        // The daemon runs on another thread; spin-poll briefly.
        for _ in 0..100 {
            if state.poll() && state.result_count() > 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// Helper: send query + poll until results update.
    fn query_and_poll(state: &mut HistorySearchState, query: &str) {
        state.update_query(query);
        for _ in 0..100 {
            if state.poll() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    fn refresh_items_and_query_are_applied_together() {
        let mut state = HistorySearchState::new();
        state.activate(&[], "");
        state.refresh_items(&entries(&["alpha", "beta"]));
        state.update_query("beta");

        let mut delivered = false;
        for _ in 0..100 {
            if state.poll() && state.result_count() == 1 && state.selected_text() == Some("beta") {
                delivered = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(delivered);
    }

    #[test]
    fn empty_query_returns_all_reversed_most_recent_last() {
        let mut state = HistorySearchState::new();
        // Input is most-recent-first (as `combined_prompt_history` produces).
        let history = entries(&["alpha", "beta", "gamma"]);
        activate_and_poll(&mut state, &history, "");

        assert_eq!(state.result_count(), 3);
        // Reversed: oldest at top, most recent ("alpha") at the bottom.
        assert_eq!(state.result_at(0).unwrap().text, "gamma");
        assert_eq!(state.result_at(1).unwrap().text, "beta");
        assert_eq!(state.result_at(2).unwrap().text, "alpha");
        // Opens with the most-recent prompt (bottom) selected.
        assert_eq!(state.selected, 2);
        assert_eq!(state.selected_text(), Some("alpha"));
    }

    #[test]
    fn non_empty_query_filters() {
        let mut state = HistorySearchState::new();
        let history = entries(&["fix bug", "add feature", "fix typo", "refactor code"]);
        activate_and_poll(&mut state, &history, "");
        query_and_poll(&mut state, "fix");

        assert!(state.result_count() >= 2);
        let texts: Vec<&str> = (0..state.result_count())
            .filter_map(|i| state.result_at(i).map(|r| r.text.as_str()))
            .collect();
        assert!(texts.contains(&"fix bug"));
        assert!(texts.contains(&"fix typo"));
        assert!(!state.result_at(0).unwrap().indices.is_empty());
        // Results are reversed (best match last) and, with no navigation,
        // selection sticks to the bottom-most (best) match.
        assert_eq!(state.selected, state.result_count() - 1);
    }

    #[test]
    fn typing_after_navigation_reanchors_to_best_match() {
        let mut state = HistorySearchState::new();
        activate_and_poll(
            &mut state,
            &entries(&["match1", "match2", "match3", "zzz", "www"]),
            "",
        );
        assert_eq!(state.result_count(), 5);
        assert_eq!(state.selected, 4); // bottom (most recent) selected on open

        // Navigate up off the bottom — selection is no longer sticky.
        state.move_up();
        state.move_up();
        state.move_up();
        assert_eq!(state.selected, 1);

        // Typing a new query re-anchors selection to the best match (bottom).
        query_and_poll(&mut state, "match");
        assert_eq!(state.result_count(), 3);
        assert_eq!(state.selected, state.result_count() - 1);
    }

    #[test]
    fn activate_stores_saved_text() {
        let mut state = HistorySearchState::new();
        state.activate(&entries(&["hello"]), "my draft");
        assert!(state.is_active());
        assert_eq!(state.saved_text(), "my draft");
    }

    #[test]
    fn deactivate_clears_state() {
        let mut state = HistorySearchState::new();
        activate_and_poll(&mut state, &entries(&["a", "b"]), "text");
        assert!(state.is_active());
        state.deactivate();
        assert!(!state.is_active());
        assert_eq!(state.result_count(), 0);
    }

    #[test]
    fn opens_with_most_recent_selected_at_bottom() {
        let mut state = HistorySearchState::new();
        // Input most-recent-first: "a" is most recent, "b" is older.
        activate_and_poll(&mut state, &entries(&["a", "b"]), "");
        // Most recent ("a") is reversed to the bottom (last index) and selected.
        assert_eq!(state.selected, 1);
        assert_eq!(state.selected_text(), Some("a"));
    }

    #[test]
    fn move_up_selects_earlier_then_stops_at_top() {
        let mut state = HistorySearchState::new();
        activate_and_poll(&mut state, &entries(&["a", "b"]), "");
        assert_eq!(state.selected, 1);
        // Up moves toward earlier prompts (up the list).
        assert!(state.move_up());
        assert_eq!(state.selected, 0);
        assert_eq!(state.selected_text(), Some("b"));
        // No wrap: at the oldest entry Up stays put.
        assert!(!state.move_up());
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn move_down_at_bottom_reports_end_instead_of_wrapping() {
        let mut state = HistorySearchState::new();
        activate_and_poll(&mut state, &entries(&["a", "b"]), "");
        assert_eq!(state.selected, 1);
        // At the newest (bottom) entry Down reports the end — the caller
        // closes the panel there ("Down right after opening backs out").
        assert!(!state.move_down());
        assert_eq!(state.selected, 1);
        // From an older entry Down moves normally.
        assert!(state.move_up());
        assert!(state.move_down());
        assert_eq!(state.selected, 1);
    }

    #[test]
    fn browse_mode_flag_tracks_activation_kind() {
        let mut state = HistorySearchState::new();
        state.activate_browse(&entries(&["a"]), "");
        assert!(state.is_active());
        assert!(state.is_browse());
        state.deactivate();
        assert!(!state.is_browse());
        state.activate(&entries(&["a"]), "");
        assert!(state.is_active());
        assert!(!state.is_browse(), "/history search mode is not browse");
    }

    #[test]
    fn no_panic_on_empty_history() {
        let mut state = HistorySearchState::new();
        state.activate(&[], "");
        assert_eq!(state.result_count(), 0);
        assert!(state.selected_text().is_none());
        assert!(!state.move_up());
        assert!(!state.move_down());
    }

    #[test]
    fn default_is_inactive() {
        let state = HistorySearchState::default();
        assert!(!state.is_active());
        assert_eq!(state.result_count(), 0);
    }
}
