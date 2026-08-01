//! Inline media: image viewer keys, media click
//! handling, and mermaid diagram affordances.

use super::AgentView;
use crate::app::app_view::InputOutcome;
use crate::render::SafeBuf;
use crate::terminal::overlay::{self, PostFlush};
use crate::theme::Theme;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

impl AgentView {
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
                self.image_load_rx = None;
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
            // Load bytes from disk (or use cached bytes if available).
            if !self.inline_media_cache.contains_key(path) {
                let raw = std::fs::read(path).ok()?;
                let bytes = crate::terminal::image::prepare_overlay_image_bytes(&raw)?;
                // Bound the cache: a long image-heavy session must not pin
                // every encoded image for its lifetime. Evicting drops only
                // CPU-side bytes — Kitty placements already transmitted stay
                // valid on the GPU (`inline_media_ids` is kept); an evicted
                // path re-reads from disk if it needs a re-transmit.
                const INLINE_MEDIA_CACHE_MAX_BYTES: usize = 64 * 1024 * 1024;
                let incoming = bytes.len();
                if incoming < INLINE_MEDIA_CACHE_MAX_BYTES {
                    let mut total: usize = self
                        .inline_media_cache
                        .values()
                        .map(Vec::len)
                        .sum::<usize>()
                        + incoming;
                    while total > INLINE_MEDIA_CACHE_MAX_BYTES {
                        // HashMap iteration order is arbitrary — treat as random eviction.
                        let Some(victim) = self.inline_media_cache.keys().next().cloned() else {
                            break;
                        };
                        if let Some(evicted) = self.inline_media_cache.remove(&victim) {
                            total -= evicted.len();
                        }
                    }
                }
                self.inline_media_cache.insert(path.clone(), bytes);
            }
            let image_id = self.get_or_alloc_media_id(path);
            let bytes = self.inline_media_cache.get(path)?;
            transmit_esc = crate::terminal::image::transmit_inline_image(bytes, image_id)?;
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
        let place_esc = crate::terminal::image::place_inline_image(
            image_data,
            w,
            h,
            placement.screen_rect,
            placement.full_rows,
            placement.top_crop_rows,
            image_id,
            emit_iterm,
        )?;
        if emit_iterm
            && crate::terminal::image::detect_graphics_protocol()
                == crate::terminal::image::GraphicsProtocol::ITerm2
        {
            self.inline_media_iterm_emitted
                .insert(path.clone(), placement.screen_rect);
        }

        Some(format!("{transmit_esc}{place_esc}"))
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
    pub(crate) fn take_inline_media_clear_escapes(&mut self) -> Option<String> {
        let mut clear_esc = self
            .take_own_inline_media_clear_escapes()
            .unwrap_or_default();
        if let Some(esc) = self.take_subagent_inline_media_clear_escapes() {
            clear_esc.push_str(&esc);
        }
        (!clear_esc.is_empty()).then_some(clear_esc)
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
        self.inline_media_active = false;
        let mut clear_esc = String::new();
        for &id in self.inline_media_ids.values() {
            clear_esc.push_str(&crate::terminal::image::clear_kitty_image(id));
        }
        self.inline_media_ids.clear();
        self.inline_media_iterm_emitted.clear();
        self.last_placed_ids.clear();
        (!clear_esc.is_empty()).then_some(clear_esc)
    }

    /// Subagent fullscreen views render inline media with their own ids —
    /// drain those (recursively), leaving this view's placements alone.
    pub(super) fn take_subagent_inline_media_clear_escapes(&mut self) -> Option<String> {
        let mut clear_esc = String::new();
        for child in self.subagent_views.values_mut() {
            if let Some(esc) = child.take_inline_media_clear_escapes() {
                clear_esc.push_str(&esc);
            }
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
                    crate::app::mermaid_worker::MermaidClickAction::Open
                } else {
                    crate::app::mermaid_worker::MermaidClickAction::CopyPath
                };
                self.request_mermaid_render(source, action);
            }
        }
    }

    // -- /gboom easter egg input ------------------------------------------------

    /// Handle a key event in the `/gboom` game modal.
    pub(super) fn handle_gboom_key(&mut self, key: &KeyEvent) -> InputOutcome {
        let Some(ref mut gboom) = self.gboom else {
            return InputOutcome::Unchanged;
        };
        match gboom.handle_key(key) {
            crate::gboom::GboomKeyOutcome::Close => {
                // Clear the kitty image before closing so no stale frame
                // lingers in the cell grid.
                shell::util::with_locked_stderr(|stderr| {
                    let clear = PostFlush::from(overlay::clear_kitty());
                    let _ = clear.write_to(stderr);
                });
                self.gboom = None;
            }
            crate::gboom::GboomKeyOutcome::Changed => {}
        }
        InputOutcome::Changed
    }

    /// Handle a key-release in the `/gboom` modal (un-latch movement).
    pub(super) fn handle_gboom_release(&mut self, key: &KeyEvent) -> InputOutcome {
        if let Some(ref mut gboom) = self.gboom {
            gboom.handle_release(key);
        }
        InputOutcome::Changed
    }

    pub(super) fn handle_gboom_mouse(&mut self, mouse: &MouseEvent) -> InputOutcome {
        if let Some(ref mut gboom) = self.gboom {
            gboom.handle_mouse(mouse);
        }
        InputOutcome::Changed
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
}
