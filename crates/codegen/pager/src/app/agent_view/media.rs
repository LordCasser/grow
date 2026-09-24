//! Inline media: image viewer keys, media click
//! handling, and mermaid diagram affordances.

use super::AgentView;
use crate::app::root::InputOutcome;
use crate::render::SafeBuf;
use crate::terminal::overlay::{self, PostFlush};
use crate::theme::Theme;
use crossterm::event::KeyEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

// Keep filesystem-backed inline-media work from scaling with the number of
// distinct paths in one frame. Saturated requests remain eligible on the next
// render pass, so backpressure does not become a sticky load failure.
const INLINE_MEDIA_MAX_WORKERS: usize = 2;
const INLINE_MEDIA_MAX_PENDING_PER_VIEW: usize = 2;
const INLINE_MEDIA_MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
static INLINE_MEDIA_WORKERS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

struct InlineMediaWorkerPermit;

impl InlineMediaWorkerPermit {
    fn try_acquire() -> Option<Self> {
        use std::sync::atomic::Ordering;
        INLINE_MEDIA_WORKERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < INLINE_MEDIA_MAX_WORKERS).then_some(active + 1)
            })
            .ok()
            .map(|_| Self)
    }
}

impl Drop for InlineMediaWorkerPermit {
    fn drop(&mut self) {
        INLINE_MEDIA_WORKERS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

fn read_inline_media_bounded(path: &std::path::Path) -> Option<Vec<u8>> {
    use std::io::Read;

    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > INLINE_MEDIA_MAX_IMAGE_BYTES as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(INLINE_MEDIA_MAX_IMAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= INLINE_MEDIA_MAX_IMAGE_BYTES).then_some(bytes)
}

fn append_escape_with_limit(output: &mut String, escape: &str, limit: usize) -> Option<()> {
    let new_len = output.len().checked_add(escape.len())?;
    if new_len > limit {
        return None;
    }
    output.try_reserve(escape.len()).ok()?;
    output.push_str(escape);
    Some(())
}

impl AgentView {
    pub(crate) fn has_visible_inline_media_load(&self) -> bool {
        self.inline_media_loading_visible
    }

    /// Invalidate inline-media state at a session/replay boundary. In-flight
    /// workers keep their old mailbox and may finish safely, but neither their
    /// bytes nor path-derived caches can enter the new session projection.
    ///
    /// GPU image ids deliberately survive until the next draw: the placement
    /// reconciler needs them to emit the corresponding Kitty delete escapes.
    /// With the CPU cache cleared, a replayed path cannot re-place an old GPU
    /// image; it must load fresh bytes and transmit again after that cleanup.
    pub(crate) fn reset_inline_media_loader(&mut self) {
        self.inline_media_cache.clear();
        self.inline_media_pending.clear();
        self.inline_media_failed.clear();
        self.inline_media_completions = Default::default();
        self.inline_media_loading_visible = false;
        self.media_link_paths.clear();
        self.media_link_paths_gen = None;
    }

    pub(crate) fn apply_inline_media_completions(&mut self) -> bool {
        let completed = {
            let mut mailbox = self.inline_media_completions.lock().unwrap();
            std::mem::take(&mut *mailbox)
        };
        if completed.is_empty() {
            return false;
        }
        for (path, result) in completed {
            self.inline_media_pending.remove(&path);
            if let Some(bytes) = result {
                if self.insert_inline_media_cache(path.clone(), bytes) {
                    self.inline_media_failed.remove(&path);
                } else {
                    self.inline_media_failed.insert(path);
                }
            } else {
                self.inline_media_failed.insert(path);
            }
        }
        true
    }

    fn insert_inline_media_cache(&mut self, path: std::path::PathBuf, bytes: Vec<u8>) -> bool {
        const MAX_BYTES: usize = 64 * 1024 * 1024;
        let incoming = bytes.len();
        if incoming > MAX_BYTES {
            return false;
        }
        let mut total = self
            .inline_media_cache
            .values()
            .map(Vec::len)
            .sum::<usize>()
            + incoming;
        while total > MAX_BYTES {
            let Some(victim) = self.inline_media_cache.keys().next().cloned() else {
                break;
            };
            if let Some(evicted) = self.inline_media_cache.remove(&victim) {
                total -= evicted.len();
            }
        }
        self.inline_media_cache.insert(path, bytes);
        true
    }

    pub(super) fn request_inline_media_load(&mut self, path: &std::path::Path) {
        if self.inline_media_cache.contains_key(path)
            || self.inline_media_pending.contains(path)
            || self.inline_media_failed.contains(path)
        {
            return;
        }
        if self.inline_media_pending.len() >= INLINE_MEDIA_MAX_PENDING_PER_VIEW {
            return;
        }
        let Some(permit) = InlineMediaWorkerPermit::try_acquire() else {
            return;
        };
        let path = path.to_path_buf();
        self.inline_media_pending.insert(path.clone());
        let mailbox = self.inline_media_completions.clone();
        let worker_path = path.clone();
        let protocol = crate::terminal::image::detect_graphics_protocol();
        let spawn = std::thread::Builder::new()
            .name("inline-media-load".into())
            .spawn(move || {
                let _permit = permit;
                let mut result = None;
                // Tool output can announce a path just before its final rename.
                // Retry off-thread for a bounded window instead of using frame
                // ticks as a file-existence poller.
                for _ in 0..40 {
                    result = read_inline_media_bounded(&worker_path)
                        .and_then(|raw| {
                            crate::terminal::image::prepare_overlay_image_bytes_for_protocol(
                                &raw, protocol,
                            )
                        })
                        .filter(|prepared| prepared.len() <= INLINE_MEDIA_MAX_IMAGE_BYTES);
                    if result.is_some() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                mailbox.lock().unwrap().push((worker_path, result));
                crate::async_view::wake();
            });
        if spawn.is_err() {
            self.inline_media_pending.remove(&path);
            self.inline_media_failed.insert(path);
        }
    }

    // -- Image viewer input --------------------------------------------------

    /// Handle a key event in the image viewer modal.
    pub(super) fn handle_image_viewer_key(&mut self, key: &KeyEvent) -> InputOutcome {
        use crossterm::event::KeyCode;

        if self.image_viewer.is_none() {
            return InputOutcome::Unchanged;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                // Clear the Kitty image before closing.
                // Old code bypassed STDERR_OUTPUT_LOCK which could interleave
                // mid-frame. Safe to revert: content is valid escapes, not raw text.
                shell::util::with_locked_stderr(|stderr| {
                    let clear = PostFlush::from(overlay::clear_kitty());
                    let _ = clear.write_to(stderr);
                });
                self.image_viewer = None;
                // The viewer's decoded/re-encoded overlay image (tens of MB
                // for screenshots/renders) just dropped; input path, so a
                // synchronous purge lands between interactions.
                crate::memory_release::release_retained_memory_with("image-viewer-close");
            }
            _ => {}
        }
        InputOutcome::Changed
    }

    // -- Inline media rendering -----------------------------------------------

    /// Build Kitty/iTerm2 escape sequences for an inline media placement.
    pub(super) fn build_inline_media_escapes(
        &mut self,
        placement: &crate::scrollback::render::InlineMediaPlacement,
    ) -> Option<String> {
        use crate::prompt_images::decode_image_dimensions;

        let path = &placement.info.path;

        // Static image.
        // Allocate the Kitty id only *after* bytes are in hand: a not-yet-written
        // path (or a failed read) must return `None` without recording an id, or
        // the next time the path is seen `needs_transmit` would be false and only
        // `place` (no `transmit`) would emit — leaving a blank image.
        let needs_transmit = !self.inline_media_ids.contains_key(path);
        let mut transmit_esc = String::new();

        if needs_transmit {
            // Cache misses are fulfilled by the event-driven loader. Never do
            // filesystem I/O or decoding in the render path.
            if !self.inline_media_cache.contains_key(path) {
                return None;
            }
            let image_id = self.get_or_alloc_media_id(path);
            let bytes = self.inline_media_cache.get(path)?;
            let Some(escape) = crate::terminal::image::transmit_inline_image(bytes, image_id)
            else {
                self.discard_unplaced_inline_media(path);
                return None;
            };
            transmit_esc = escape;
        }

        let image_id = self.get_or_alloc_media_id(path);
        let image_data = self.inline_media_cache.get(path)?;
        let (w, h) = decode_image_dimensions(image_data)
            .unwrap_or((placement.info.width, placement.info.height));

        // iTerm2 has no place-only escape — re-emit when placement moves.
        let emit_iterm = self
            .inline_media_iterm_emitted
            .get(path)
            .is_none_or(|last| *last != placement.screen_rect);
        let Some(place_esc) = crate::terminal::image::place_inline_image(
            image_data,
            w,
            h,
            placement.screen_rect,
            placement.full_rows,
            placement.top_crop_rows,
            image_id,
            emit_iterm,
        ) else {
            self.discard_unplaced_inline_media(path);
            return None;
        };

        let mut escapes = transmit_esc;
        let Some(combined_len) = escapes.len().checked_add(place_esc.len()) else {
            self.discard_unplaced_inline_media(path);
            return None;
        };
        if combined_len > crate::terminal::image::MAX_TERMINAL_IMAGE_ESCAPE_BYTES
            || escapes.try_reserve(place_esc.len()).is_err()
        {
            self.discard_unplaced_inline_media(path);
            return None;
        }
        escapes.push_str(&place_esc);
        if emit_iterm
            && crate::terminal::image::detect_graphics_protocol()
                == crate::terminal::image::GraphicsProtocol::ITerm2
        {
            self.inline_media_iterm_emitted
                .insert(path.clone(), placement.screen_rect);
        }
        Some(escapes)
    }

    /// Discard an ID allocated for a failed first upload, but keep a previously
    /// placed ID until the bounded stale-clear path has emitted its delete.
    pub(super) fn discard_unplaced_inline_media(&mut self, path: &std::path::Path) {
        let was_placed = self
            .inline_media_ids
            .get(path)
            .is_some_and(|id| self.last_placed_ids.contains(id));
        if !was_placed {
            self.inline_media_ids.remove(path);
            self.inline_media_iterm_emitted.remove(path);
        }
    }

    /// The escape was built but the draw accumulator rejected it, so iTerm2
    /// must not treat the attempted rectangle as one it has already received.
    pub(super) fn reject_inline_media_escape(&mut self, path: &std::path::Path) {
        self.discard_unplaced_inline_media(path);
        self.inline_media_iterm_emitted.remove(path);
    }

    /// Paint each visible Mermaid affordance row (`◇ mermaid [Open Image]
    /// [Copy Image Path] [Copy Source]`) and register its click hit-rects.
    ///
    /// The leading `◇ mermaid` label is a dim, non-clickable marker. Every button
    /// is always clickable (`[Open]`/`[Copy path]` render lazily on click); a
    /// button whose hit-rect is under the mouse is highlighted, the rest are dim.
    /// A trailing dim `rendering…` hint follows the buttons while an on-click
    /// render for that diagram is in flight. The whole layout (label + button +
    /// hint columns) comes from
    /// [`affordance_row`](crate::scrollback::blocks::mermaid_content::affordance_row)
    /// so the painted labels and the hit-rects can't drift, and each segment is
    /// clipped to `screen_rect.width` (which excludes the timestamp reserve).
    pub(super) fn paint_diagram_affordances(
        &mut self,
        buf: &mut Buffer,
        placements: Vec<crate::scrollback::render::DiagramAffordancePlacement>,
        theme: &Theme,
    ) {
        use crate::scrollback::blocks::mermaid_content::affordance_row;
        use ratatui::style::Modifier;
        use unicode_width::UnicodeWidthStr;

        let (hover_col, hover_row) = self.last_mouse_pos;
        for aff in placements {
            let crate::scrollback::render::DiagramAffordancePlacement {
                screen_rect: rect,
                source,
            } = aff;
            // The transient `rendering…` hint shows only while an on-click render
            // for this diagram is in flight.
            let rendering = self.diagram_is_rendering(&source);
            let row = affordance_row(rendering);
            // A segment is drawn only if it fits wholly within the row width
            // (which already excludes the timestamp reserve), so labels never
            // spill past the content area and hit-rects stay inside the row.
            let fits =
                |col: u16, label: &str| col + UnicodeWidthStr::width(label) as u16 <= rect.width;

            // Leading dim, non-clickable `◇ mermaid` label.
            let (label_col, label_text) = row.label;
            if fits(label_col, label_text) {
                buf.set_string_safe(
                    rect.x.saturating_add(label_col),
                    rect.y,
                    label_text,
                    Style::default().fg(theme.gray_dim),
                );
            }

            // Register the diagram's source once — moved, not cloned (the
            // placement is owned and used only here) — when at least one button
            // fits; every fitting button below indexes into it for click routing.
            let source_idx = if row.buttons.iter().any(|b| fits(b.col, b.label)) {
                let idx = self.inline_media_hits.mermaid_sources.len();
                self.inline_media_hits.mermaid_sources.push(source);
                Some(idx)
            } else {
                None
            };
            for btn in row.buttons {
                if !fits(btn.col, btn.label) {
                    continue;
                }
                let bx = rect.x.saturating_add(btn.col);
                let width = UnicodeWidthStr::width(btn.label) as u16;
                let hit = Rect {
                    x: bx,
                    y: rect.y,
                    width,
                    height: 1,
                };
                // Hovered button is highlighted; idle buttons stay at the normal
                // `gray` (brighter than the dim `◇ mermaid` label) so they remain
                // discoverable at rest.
                let style = if hit.contains((hover_col, hover_row).into()) {
                    Style::default()
                        .fg(theme.text_primary)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                } else {
                    Style::default().fg(theme.gray)
                };
                buf.set_string_safe(bx, rect.y, btn.label, style);
                if let Some(idx) = source_idx {
                    self.inline_media_hits
                        .mermaid_buttons
                        .push((hit, btn.kind, idx));
                }
            }

            // Trailing dim `rendering…` hint after the buttons (not clickable).
            if let Some((col, status)) = row.status
                && fits(col, status)
            {
                buf.set_string_safe(
                    rect.x.saturating_add(col),
                    rect.y,
                    status,
                    Style::default().fg(theme.gray_dim),
                );
            }
        }
    }

    /// Whether the diagram with `source` has an on-click render in flight (drives
    /// the affordance row's transient `rendering…` hint).
    fn diagram_is_rendering(&self, source: &str) -> bool {
        self.mermaid_is_rendering(source)
    }

    /// Get or allocate a Kitty image ID for the given media path.
    fn get_or_alloc_media_id(&mut self, path: &std::path::Path) -> u32 {
        if let Some(&id) = self.inline_media_ids.get(path) {
            return id;
        }
        let id = self.next_inline_media_id;
        self.next_inline_media_id += 1;
        self.inline_media_ids.insert(path.to_path_buf(), id);
        id
    }

    /// Drain this agent's inline-media placement tracking and return the
    /// Kitty delete escapes for every image it has placed on the GPU.
    ///
    /// Kitty graphics are independent of the cell grid: they survive
    /// redraws until explicitly deleted, and every regular clear path
    /// lives inside [`AgentView::draw`]. When another view takes over the
    /// frame (e.g. the agent dashboard), those per-frame clears stop
    /// running, so the caller uses this to delete whatever this agent
    /// left on screen. Resetting `inline_media_ids` forces a fresh
    /// transmit when this agent next draws; any active inline playback
    /// is stopped, mirroring the scrolled-off-screen clear path.
    ///
    /// Returns `None` when this agent (and its subagent views) has no
    /// placements.
    #[cfg(test)]
    pub(crate) fn take_inline_media_clear_escapes(&mut self) -> Option<String> {
        let mut clear_esc = String::new();
        self.append_inline_media_clear_tree_with_limit(
            &mut clear_esc,
            crate::terminal::image::MAX_TERMINAL_IMAGE_ESCAPE_BYTES,
        );
        (!clear_esc.is_empty()).then_some(clear_esc)
    }

    pub(crate) fn append_inline_media_clear_tree_with_limit(
        &mut self,
        output: &mut String,
        limit: usize,
    ) {
        self.append_inline_media_clears_with_limit(output, limit);
        for child in self.subagent_views.values_mut() {
            child.append_inline_media_clear_tree_with_limit(output, limit);
        }
    }

    /// This view's own placements only, leaving `subagent_views` untouched.
    /// Used by the fullscreen-subagent takeover in [`AgentView::draw`]: the
    /// parent's images must be deleted, but the child is about to draw and
    /// manages its own placements — draining it too would just force a
    /// re-transmit.
    pub(super) fn take_own_inline_media_clear_escapes(&mut self) -> Option<String> {
        if !self.inline_media_active && self.inline_media_ids.is_empty() {
            return None;
        }
        let mut clear_esc = String::new();
        self.append_inline_media_clears_with_limit(
            &mut clear_esc,
            crate::terminal::image::MAX_TERMINAL_IMAGE_ESCAPE_BYTES,
        );
        (!clear_esc.is_empty()).then_some(clear_esc)
    }

    #[cfg(test)]
    pub(super) fn append_inline_media_clears(&mut self, output: &mut String) {
        self.append_inline_media_clears_with_limit(
            output,
            crate::terminal::image::MAX_TERMINAL_IMAGE_ESCAPE_BYTES,
        );
    }

    pub(super) fn append_inline_media_clears_with_budget(
        &mut self,
        output: &mut String,
        limit: usize,
    ) {
        self.append_inline_media_clears_with_limit(output, limit);
    }

    #[cfg(test)]
    pub(super) fn append_obsolete_inline_media_clears(
        &mut self,
        output: &mut String,
        current_ids: &mut std::collections::HashSet<u32>,
    ) {
        self.append_obsolete_inline_media_clears_with_limit(
            output,
            current_ids,
            crate::terminal::image::MAX_TERMINAL_IMAGE_ESCAPE_BYTES,
        );
    }

    pub(super) fn append_obsolete_inline_media_clears_with_budget(
        &mut self,
        output: &mut String,
        current_ids: &mut std::collections::HashSet<u32>,
        limit: usize,
    ) {
        self.append_obsolete_inline_media_clears_with_limit(output, current_ids, limit);
    }

    fn append_inline_media_clears_with_limit(&mut self, output: &mut String, limit: usize) {
        let last_placed_ids = &mut self.last_placed_ids;
        let inline_media_ids = &mut self.inline_media_ids;
        inline_media_ids.retain(|_, id| {
            let clear = crate::terminal::image::clear_kitty_image(*id);
            if append_escape_with_limit(output, &clear, limit).is_some() {
                last_placed_ids.remove(id);
                false
            } else {
                true
            }
        });
        self.inline_media_iterm_emitted
            .retain(|path, _| self.inline_media_ids.contains_key(path));
        self.inline_media_active = !self.inline_media_ids.is_empty();
    }

    fn append_obsolete_inline_media_clears_with_limit(
        &mut self,
        output: &mut String,
        current_ids: &mut std::collections::HashSet<u32>,
        limit: usize,
    ) {
        let inline_media_ids = &mut self.inline_media_ids;
        inline_media_ids.retain(|_, id| {
            if current_ids.contains(id) {
                return true;
            }
            let clear = crate::terminal::image::clear_kitty_image(*id);
            if append_escape_with_limit(output, &clear, limit).is_some() {
                false
            } else {
                current_ids.insert(*id);
                true
            }
        });
        self.inline_media_iterm_emitted
            .retain(|path, _| self.inline_media_ids.contains_key(path));
        self.inline_media_active = !self.inline_media_ids.is_empty();
    }

    /// Subagent fullscreen views render inline media with their own ids —
    /// drain those (recursively), leaving this view's placements alone.
    pub(super) fn take_subagent_inline_media_clear_escapes(&mut self) -> Option<String> {
        let mut clear_esc = String::new();
        for child in self.subagent_views.values_mut() {
            child.append_inline_media_clear_tree_with_limit(
                &mut clear_esc,
                crate::terminal::image::MAX_TERMINAL_IMAGE_ESCAPE_BYTES,
            );
        }
        (!clear_esc.is_empty()).then_some(clear_esc)
    }

    /// Refresh [`Self::media_link_paths`] — the absolute paths of media
    /// generated in this transcript — from scrollback, but only when its
    /// generation has changed. The model prints short session-relative paths
    /// (`images/1.jpg`); resolving them against the actual generated files ties
    /// each link to the file its message produced (correct across forks) and
    /// never opens an out-of-session or arbitrary file.
    pub(crate) fn ensure_media_link_paths(&mut self) {
        let generation = self.scrollback.generation();
        if self.media_link_paths_gen == Some(generation) {
            return;
        }
        self.media_link_paths_gen = Some(generation);
        self.media_link_paths.clear();
        self.media_link_paths.extend(
            self.scrollback
                .iter_entries()
                .filter_map(|(_, entry)| entry.block.media_ref_path()),
        );
    }

    /// Open an image file in the OS-native default application. Shared by the `[Open]` button, the
    /// inline-image click target, and the Enter-key handler.
    pub(crate) fn open_media_natively(&mut self, path: &std::path::Path) -> bool {
        if crate::app::link_opener::open_path(path) {
            self.show_toast("Opening in default app\u{2026}");
            true
        } else {
            self.show_toast("Could not open file");
            false
        }
    }

    // -- Inline media click handling -----------------------------------------

    /// Handle a click on inline media buttons. Returns `Some(InputOutcome)` if
    /// the click was consumed, `None` to fall through to normal handling.
    pub(in crate::app) fn handle_inline_media_click(
        &mut self,
        col: u16,
        row: u16,
    ) -> Option<InputOutcome> {
        let pos = ratatui::layout::Position::new(col, row);

        // [Open] button or inline image → open natively.
        let open_target = self
            .inline_media_hits
            .open_buttons
            .iter()
            .chain(self.inline_media_hits.media_areas.iter())
            .find(|(rect, _)| rect.contains(pos))
            .map(|(_, path)| path.clone());
        if let Some(path) = open_target {
            self.open_media_natively(&path);
            return Some(InputOutcome::Changed);
        }

        // [Copy] button → copy image to clipboard (async).
        if let Some((_, path)) = self
            .inline_media_hits
            .copy_image_buttons
            .iter()
            .find(|(rect, _)| rect.contains(pos))
        {
            let path = path.clone();
            std::thread::spawn(move || {
                if let Err(e) = shell::util::clipboard::set_image_file(&path) {
                    tracing::debug!("copy image failed: {e}");
                }
            });
            self.show_toast("Copied image");
            return Some(InputOutcome::Changed);
        }

        // Click on filepath line → copy path to clipboard.
        if let Some((_, path)) = self
            .inline_media_hits
            .filepath_areas
            .iter()
            .find(|(rect, _)| rect.contains(pos))
        {
            let path_str = path.display().to_string();
            self.copy_to_clipboard(&path_str);
            return Some(InputOutcome::Changed);
        }

        // Mermaid affordance row → render-on-click (Open/Copy path) or copy
        // source. Resolve the kind + source index first so the `mermaid_buttons`
        // borrow ends before the `&mut self` dispatch below.
        let mermaid_hit = self
            .inline_media_hits
            .mermaid_buttons
            .iter()
            .find(|(rect, _, _)| rect.contains(pos))
            .map(|&(_, kind, idx)| (kind, idx));
        if let Some((kind, idx)) = mermaid_hit {
            let source = self
                .inline_media_hits
                .mermaid_sources
                .get(idx)
                .cloned()
                .unwrap_or_default();
            self.on_mermaid_affordance_click(kind, source);
            return Some(InputOutcome::Changed);
        }

        None
    }

    /// Route a Mermaid affordance-row click. `[Copy source]` copies the diagram
    /// source (no render); `[Open]`/`[Copy path]` render it lazily at the live
    /// theme/width and then open the PNG / copy its path. `source` is moved into
    /// the renderer, never cloned. `copy_to_clipboard` owns the copy toast.
    fn on_mermaid_affordance_click(
        &mut self,
        kind: crate::scrollback::blocks::mermaid_content::AffordanceKind,
        source: String,
    ) {
        use crate::scrollback::blocks::mermaid_content::AffordanceKind;
        match kind {
            AffordanceKind::CopySource => {
                if !self.copy_to_clipboard(&source).success() {
                    crate::unified_log::error(
                        "mermaid.copy_source.failed",
                        self.session.session_id.as_ref().map(|s| s.0.as_ref()),
                        Some(serde_json::json!({ "source_len": source.len() })),
                    );
                }
            }
            AffordanceKind::Open | AffordanceKind::CopyPath => {
                let action = if matches!(kind, AffordanceKind::Open) {
                    crate::app::agent_view::mermaid_worker::MermaidClickAction::Open
                } else {
                    crate::app::agent_view::mermaid_worker::MermaidClickAction::CopyPath
                };
                self.request_mermaid_render(source, action);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::memory_release::test_support;

    fn make_agent() -> crate::app::agent_view::AgentView {
        crate::test_util::make_agent_view(None, "/tmp")
    }

    /// Closing the image viewer drops the decoded overlay image — purge
    /// synchronously (input path), exactly once.
    #[test]
    fn image_viewer_close_releases_retained_memory() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        test_support::install_counting_hook();

        let mut agent = make_agent();
        agent.image_viewer = Some(
            crate::prompt_images::ImageViewerState::open_from_path_deferred(std::path::Path::new(
                "x.png",
            )),
        );
        let before = test_support::calls();
        agent.handle_image_viewer_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(agent.image_viewer.is_none());
        assert_eq!(
            test_support::calls(),
            before + 1,
            "closing the image viewer must purge after the image drops"
        );
    }

    #[test]
    fn inline_media_waits_off_thread_and_completes_by_snapshot() {
        let _protocol = crate::terminal::image::set_protocol_for_test(
            crate::terminal::image::GraphicsProtocol::ITerm2,
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("late.png");
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            2,
            2,
            image::Rgba([1, 2, 3, 255]),
        ))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();

        let mut agent = make_agent();
        agent.request_inline_media_load(&path);
        assert!(agent.inline_media_pending.contains(&path));
        // The path did not exist when the request was queued; publishing it
        // now exercises the worker's bounded rename race without depending on
        // another test thread being scheduled promptly under the full suite.
        std::fs::write(&path, encoded.into_inner()).unwrap();

        let mut applied = false;
        for _ in 0..100 {
            if agent.apply_inline_media_completions() {
                applied = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(applied, "loader must publish a completion snapshot");
        assert!(agent.inline_media_cache.contains_key(&path));
        assert!(!agent.inline_media_pending.contains(&path));
    }

    #[test]
    fn inline_media_read_rejects_oversized_input_before_allocating_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oversized.png");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len((super::INLINE_MEDIA_MAX_IMAGE_BYTES + 1) as u64)
            .unwrap();
        assert!(super::read_inline_media_bounded(&path).is_none());
    }

    #[test]
    fn inline_media_requests_have_a_per_view_pending_limit() {
        let _protocol = crate::terminal::image::set_protocol_for_test(
            crate::terminal::image::GraphicsProtocol::ITerm2,
        );
        let dir = tempfile::tempdir().unwrap();
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([1, 2, 3, 255]),
        ))
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
        let mut agent = make_agent();
        for name in ["one.png", "two.png", "three.png"] {
            let path = dir.path().join(name);
            std::fs::write(&path, encoded.get_ref()).unwrap();
            agent.request_inline_media_load(&path);
        }
        assert_eq!(
            agent.inline_media_pending.len(),
            super::INLINE_MEDIA_MAX_PENDING_PER_VIEW
        );
    }

    #[test]
    fn session_boundary_detaches_late_inline_media_completion() {
        let mut agent = make_agent();
        let old_mailbox = agent.inline_media_completions.clone();
        let path = std::path::PathBuf::from("/tmp/old-session.png");
        agent
            .inline_media_cache
            .insert(path.clone(), vec![0x89, b'P', b'N', b'G']);
        agent.inline_media_ids.insert(path.clone(), 7);
        agent.last_placed_ids.insert(7);
        agent.inline_media_pending.insert(path.clone());
        agent.media_link_paths.push(path.clone());
        agent.media_link_paths_gen = Some(agent.scrollback.generation());
        agent.reset_inline_media_loader();
        old_mailbox
            .lock()
            .unwrap()
            .push((path.clone(), Some(vec![1])));

        assert!(!agent.apply_inline_media_completions());
        assert!(!agent.inline_media_cache.contains_key(&path));
        assert!(!agent.inline_media_pending.contains(&path));
        assert!(agent.media_link_paths.is_empty());
        assert!(agent.media_link_paths_gen.is_none());
        assert_eq!(
            agent.inline_media_ids.get(&path),
            Some(&7),
            "the next draw still needs the id to clear the old GPU placement"
        );
        assert!(agent.last_placed_ids.contains(&7));
    }

    #[test]
    fn inline_media_clear_state_is_removed_only_after_escape_append() {
        let mut agent = make_agent();
        let path = std::path::PathBuf::from("/tmp/inline-media-clear.png");
        agent.inline_media_ids.insert(path.clone(), 7);
        agent
            .inline_media_iterm_emitted
            .insert(path.clone(), ratatui::layout::Rect::new(0, 0, 10, 5));
        agent.last_placed_ids.insert(7);
        agent.inline_media_active = true;

        let mut clear = String::new();
        agent.append_inline_media_clears_with_limit(&mut clear, 0);
        assert!(clear.is_empty());
        assert_eq!(agent.inline_media_ids.get(&path), Some(&7));
        assert!(agent.inline_media_iterm_emitted.contains_key(&path));
        assert!(agent.last_placed_ids.contains(&7));
        assert!(agent.inline_media_active);

        agent.append_inline_media_clears(&mut clear);
        assert_eq!(clear, crate::terminal::image::clear_kitty_image(7));
        assert!(!agent.inline_media_ids.contains_key(&path));
        assert!(!agent.inline_media_iterm_emitted.contains_key(&path));
        assert!(!agent.last_placed_ids.contains(&7));
        assert!(!agent.inline_media_active);
    }

    #[test]
    fn failed_inline_media_escape_keeps_previously_placed_id_for_cleanup() {
        let mut agent = make_agent();
        let placed = std::path::PathBuf::from("/tmp/placed-inline-media.png");
        agent.inline_media_ids.insert(placed.clone(), 17);
        agent.last_placed_ids.insert(17);
        agent
            .inline_media_iterm_emitted
            .insert(placed.clone(), ratatui::layout::Rect::new(1, 2, 10, 5));

        agent.discard_unplaced_inline_media(&placed);

        assert_eq!(agent.inline_media_ids.get(&placed), Some(&17));
        assert!(agent.last_placed_ids.contains(&17));
        assert!(agent.inline_media_iterm_emitted.contains_key(&placed));
        let mut clear = String::new();
        agent.append_inline_media_clears_with_limit(&mut clear, usize::MAX);
        assert_eq!(clear, crate::terminal::image::clear_kitty_image(17));
        assert!(!agent.inline_media_ids.contains_key(&placed));

        let unplaced = std::path::PathBuf::from("/tmp/unplaced-inline-media.png");
        agent.inline_media_ids.insert(unplaced.clone(), 18);
        agent.discard_unplaced_inline_media(&unplaced);
        assert!(!agent.inline_media_ids.contains_key(&unplaced));
    }

    #[test]
    fn aggregate_rejection_forces_iterm2_to_retry_placement() {
        let mut agent = make_agent();
        let path = std::path::PathBuf::from("/tmp/rejected-inline-media.png");
        agent.inline_media_ids.insert(path.clone(), 19);
        agent.last_placed_ids.insert(19);
        agent
            .inline_media_iterm_emitted
            .insert(path.clone(), ratatui::layout::Rect::new(1, 2, 10, 5));

        agent.reject_inline_media_escape(&path);

        assert_eq!(agent.inline_media_ids.get(&path), Some(&19));
        assert!(agent.last_placed_ids.contains(&19));
        assert!(
            !agent.inline_media_iterm_emitted.contains_key(&path),
            "the next escape build must include a fresh iTerm2 placement"
        );
    }

    #[test]
    fn obsolete_inline_media_clear_remains_pending_when_frame_budget_is_full() {
        let mut agent = make_agent();
        let path = std::path::PathBuf::from("/tmp/obsolete-inline-media.png");
        agent.inline_media_ids.insert(path.clone(), 11);
        agent.last_placed_ids.insert(11);
        agent.inline_media_active = true;

        let mut output = String::new();
        let mut current_ids = std::collections::HashSet::new();
        agent.append_obsolete_inline_media_clears_with_limit(&mut output, &mut current_ids, 0);
        agent.last_placed_ids = current_ids;
        assert!(output.is_empty());
        assert_eq!(agent.inline_media_ids.get(&path), Some(&11));
        assert!(agent.last_placed_ids.contains(&11));
        assert!(agent.inline_media_active);

        let mut next_frame_ids = std::collections::HashSet::new();
        agent.append_obsolete_inline_media_clears(&mut output, &mut next_frame_ids);
        agent.last_placed_ids = next_frame_ids;
        assert_eq!(output, crate::terminal::image::clear_kitty_image(11));
        assert!(!agent.inline_media_ids.contains_key(&path));
        assert!(!agent.last_placed_ids.contains(&11));
        assert!(!agent.inline_media_active);
    }
}
