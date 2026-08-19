//! Read-only bundled Agent catalog pane.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crossterm::event::{KeyEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::StatefulWidget;

use crate::app::bundle::BundleState;
use crate::appearance::LayoutConfig;
use crate::scrollback::layout::HorizontalLayout;
use crate::theme::Theme;

use super::list_pane::{
    ListItem, ListPane, ListPaneConfig, ListPaneState, ListPaneStyle, WrapMode,
};
use super::overlay::OverlayState;

struct CatalogEntry {
    id: u64,
    label: String,
    styled: Line<'static>,
    header: bool,
}

impl ListItem for CatalogEntry {
    fn content(&self) -> &Line<'_> {
        &self.styled
    }

    fn stable_id(&self) -> u64 {
        self.id
    }

    fn is_selectable(&self) -> bool {
        !self.header
    }

    fn search_text(&self) -> &str {
        &self.label
    }
}

const MAX_CATALOG_HEIGHT: u16 = 8;
const MAX_CATALOG_FRACTION: f32 = 0.15;

pub struct SubagentCatalogPane {
    entries: Vec<CatalogEntry>,
    pub list_state: ListPaneState,
    list_style: ListPaneStyle,
    pub overlay: OverlayState,
}

impl Default for SubagentCatalogPane {
    fn default() -> Self {
        Self::new()
    }
}

impl SubagentCatalogPane {
    pub fn new() -> Self {
        let config = ListPaneConfig {
            follow_enabled: false,
            wrap_toggle_enabled: false,
            search_enabled: true,
            copy_enabled: false,
            show_selection_when_unfocused: false,
            visual_select_enabled: false,
            filter_enabled: true,
            goto_line_enabled: false,
        };
        Self {
            entries: Vec::new(),
            list_state: ListPaneState::new_with_config(WrapMode::NoWrap, false, config),
            list_style: ListPaneStyle::default(),
            overlay: OverlayState::hidden(),
        }
    }

    pub fn sync_from_bundle(&mut self, state: &BundleState) {
        self.entries.clear();
        if !state.has_cache || state.agents.is_empty() {
            return;
        }
        let theme = Theme::current();
        let mut header_hash = DefaultHasher::new();
        "Agents".hash(&mut header_hash);
        self.entries.push(CatalogEntry {
            id: header_hash.finish(),
            label: "Agents".to_string(),
            styled: Line::from(Span::styled(
                "Agents",
                Style::default()
                    .fg(theme.gray_bright)
                    .add_modifier(Modifier::BOLD),
            )),
            header: true,
        });
        for name in &state.agents {
            let mut hash = DefaultHasher::new();
            name.hash(&mut hash);
            self.entries.push(CatalogEntry {
                id: hash.finish(),
                label: name.clone(),
                styled: Line::from(Span::styled(
                    format!("  {name}"),
                    Style::default().fg(theme.text_primary),
                )),
                header: false,
            });
        }
    }

    pub fn is_visible(&self) -> bool {
        self.overlay.visible
    }

    pub fn on_state_change(&mut self) {
        if !self.overlay.visible {
            self.list_state.close_input_bar();
        }
    }

    pub fn desired_height(&self, view_height: u16) -> u16 {
        if !self.overlay.visible || view_height < 12 {
            return 0;
        }
        if self.entries.is_empty() {
            return 1;
        }
        let fraction = (view_height as f32 * MAX_CATALOG_FRACTION).floor() as u16;
        (self.entries.len() as u16)
            .min(MAX_CATALOG_HEIGHT.min(fraction).max(1))
            .max(1)
    }

    pub fn selected_entry(&self) -> Option<(&str, &str)> {
        let id = self.list_state.selected_id()?;
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.id == id && !entry.header)?;
        Some(("agent", &entry.label))
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        !self.entries.is_empty() && self.list_state.handle_key_event(key, &self.entries)
    }

    pub fn handle_paste(&mut self, text: &str) -> bool {
        self.list_state.handle_paste(text, &self.entries)
    }

    pub fn handle_scroll(&mut self, lines: i32, col: u16, row: u16) {
        let max = match self.list_state.viewport_height() {
            0..=5 => 1,
            6..=10 => 2,
            _ => lines.unsigned_abs() as i32,
        };
        let capped = lines.signum() * lines.abs().min(max);
        self.list_state
            .handle_scroll_event(capped, col, row, &self.entries);
    }

    pub fn handle_mouse(&mut self, kind: MouseEventKind, col: u16, row: u16, area: Rect) -> bool {
        !self.entries.is_empty()
            && self
                .list_state
                .handle_mouse_event(kind, col, row, area, &self.entries)
    }

    fn content_area(area: Rect, layout: &LayoutConfig) -> Rect {
        let left = HorizontalLayout::ACCENT + layout.block_pad_left;
        Rect {
            x: area.x + left,
            y: area.y,
            width: area.width.saturating_sub(left + layout.block_pad_right),
            height: area.height,
        }
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer, focused: bool, layout: &LayoutConfig) {
        let inner = Self::content_area(area, layout);
        if self.entries.is_empty() {
            if inner.height > 0 && inner.width > 0 {
                let span = Span::styled(
                    "No bundled Agents.",
                    Style::default().fg(Theme::current().gray_bright),
                );
                buf.set_span(inner.x, inner.y, &span, inner.width);
            }
            return;
        }
        self.list_state
            .prepare_layout(&self.entries, inner.width, inner.height);
        ListPane::new(&self.entries)
            .focused(focused)
            .style(self.list_style)
            .render(inner, buf, &mut self.list_state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_projects_agents_only() {
        let mut pane = SubagentCatalogPane::new();
        pane.sync_from_bundle(&BundleState {
            has_cache: true,
            agents: vec!["review".into()],
            ..Default::default()
        });
        assert_eq!(pane.entries.len(), 2);
        pane.list_state.select_by_id(pane.entries[1].id);
        assert_eq!(pane.selected_entry(), Some(("agent", "review")));
    }
}
