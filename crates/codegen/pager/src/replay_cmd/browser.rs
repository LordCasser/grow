//! Passive transcript browsing, using the same content viewers as AgentView.

use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
};

use crate::motion::FrameStamp;
use crate::scrollback::block::{BlockContent, RenderBlock};
use crate::scrollback::entry::EntryId;
use crate::scrollback::entry::ScrollbackEntry;
use crate::scrollback::search::ScrollbackSearchState;
use crate::scrollback::state::ScrollbackState;
use crate::scrollback::text_selection::{
    ActiveTextDrag, PendingTextDrag, ResolvedSelectionBoundaries, ResolvedSelectionModel,
    SelectionKind, drag_threshold_exceeded, reconstruct_full_selection_text_with_boundaries,
    reconstruct_selection_text_with_boundaries, render_active_selection_overlay,
};
use crate::scrollback::types::{BlockContext, DisplayMode};
use crate::scrollback::{ScratchBuffer, ScrollbackPane};
use crate::theme::Theme;
use crate::views::block_viewer::{BlockViewerPane, ViewerKind};
use crate::views::modal_window::{
    ModalSizing, ModalWindowConfig, ModalWindowOutcome, ModalWindowState, handle_modal_mouse,
    render_modal_window,
};

pub(super) enum BrowseAction {
    None,
    Back,
    Child(String),
    Workflow(String),
}

struct WorkflowPicker {
    modal: ModalWindowState,
    title: String,
    summary: String,
    children: Vec<(String, String)>,
    selected: usize,
    scroll: usize,
    rows: Vec<Rect>,
    pressed: Option<usize>,
}

#[derive(Default)]
pub(super) struct Browser {
    pub(super) viewer: Option<BlockViewerPane>,
    search: Option<ScrollbackSearchState>,
    area: Rect,
    hovered: Option<usize>,
    pressed: Option<(EntryId, u16, u16)>,
    pending_text: Option<(EntryId, PendingTextDrag)>,
    drag: Option<(EntryId, ActiveTextDrag)>,
    selected_text: Option<(EntryId, ActiveTextDrag)>,
    selection_visible_start: Option<usize>,
    selection_model: ResolvedSelectionModel,
    selection_boundaries: ResolvedSelectionBoundaries,
    scratch: ScratchBuffer,
    help_previous_viewer: Option<BlockViewerPane>,
    help_previous_picker: Option<WorkflowPicker>,
    help_open: bool,
    feedback: Option<String>,
    workflow_picker: Option<WorkflowPicker>,
}

impl Browser {
    pub(super) fn shortcuts_hints(&self) -> Vec<crate::views::shortcuts_bar::HintItem> {
        use crate::views::shortcuts_bar::HintItem;
        if let Some(viewer) = &self.viewer {
            return viewer.shortcuts_hints();
        }
        if self.workflow_picker.is_some() {
            return vec![
                HintItem::paired(crate::key!(Up), crate::key!(Down), "选择"),
                HintItem::new(crate::key!(Enter), "打开过程"),
                HintItem::new(crate::key!(Esc), "返回"),
            ];
        }
        if self.search.is_some() {
            return vec![
                HintItem::new(crate::key!(Enter), "查看结果"),
                HintItem::new(crate::key!(Esc), "关闭搜索"),
            ];
        }
        vec![
            HintItem::new(crate::key!(Enter), "详情"),
            HintItem::paired(crate::key!(Left), crate::key!(Right), "折叠"),
            HintItem::new(crate::key!('/'), "搜索"),
            HintItem::new(crate::key!('y'), "复制"),
            HintItem::new(crate::key!('t'), "任务"),
        ]
    }
    pub(super) fn owns_input(&self) -> bool {
        self.viewer.is_some() || self.search.is_some() || self.workflow_picker.is_some()
    }

    pub(super) fn has_search_or_help(&self) -> bool {
        self.search.is_some() || self.viewer.is_some() || self.workflow_picker.is_some()
    }

    pub(super) fn hint(&self) -> String {
        if self.workflow_picker.is_some() {
            "↑/↓ 选择子 Agent  Enter 打开过程  Esc/q 返回  F8 暂停".to_owned()
        } else if let Some(viewer) = &self.viewer {
            let mut hint = "Esc/q 关闭详情  / 搜索  v 选择  y 复制  w 换行".to_owned();
            if matches!(
                viewer.kind,
                ViewerKind::Markdown | ViewerKind::Communication
            ) {
                hint.push_str("  r 原文");
            }
            if viewer.kind == ViewerKind::Communication {
                hint.push_str("  D 数据");
            }
            hint
        } else if self
            .search
            .as_ref()
            .is_some_and(ScrollbackSearchState::is_composing)
        {
            "输入本地查询  Enter 查看结果  Esc 结束编辑  F8 暂停".to_owned()
        } else if self.search.is_some() {
            "n/N 匹配  Enter 刷新已显示内容  Esc/q 关闭结果  F8 暂停".to_owned()
        } else {
            "Space 暂停  +/- 倍速  Enter 详情  ←/→ 折叠  / 搜索  y 复制  Esc/q 返回".to_owned()
        }
    }

    pub(super) fn feedback(&self) -> Option<&str> {
        self.feedback.as_deref()
    }

    pub(super) fn clear_pointer(&mut self) {
        self.invalidate_geometry();
        self.selected_text = None;
    }

    pub(super) fn invalidate_geometry(&mut self) {
        self.hovered = None;
        self.cancel_pending_pointer();
        self.selection_visible_start = None;
        if let Some(picker) = &mut self.workflow_picker {
            picker.rows.clear();
            picker.modal.popup_area = None;
            picker.modal.close_button_rect = None;
        }
    }

    pub(super) fn cancel_pending_pointer(&mut self) {
        self.pressed = None;
        self.pending_text = None;
        self.drag = None;
        if let Some(picker) = &mut self.workflow_picker {
            picker.pressed = None;
        }
    }

    pub(super) fn open_workflow_picker(
        &mut self,
        state: &ScrollbackState,
        children: Vec<(String, String)>,
    ) {
        let Some(entry) = state.selected().and_then(|index| state.entry(index)) else {
            return;
        };
        let RenderBlock::Workflow(workflow) = &entry.block else {
            return;
        };
        self.workflow_picker = Some(WorkflowPicker {
            modal: ModalWindowState::new(),
            title: format!("Workflow · {}", workflow.name),
            summary: format!("{} · {:?}", workflow.objective, workflow.status),
            children,
            selected: 0,
            scroll: 0,
            rows: Vec::new(),
            pressed: None,
        });
    }

    pub(super) fn open_help(&mut self, status: &str) {
        if self.help_open {
            return;
        }
        self.help_previous_viewer = self.viewer.take();
        self.help_previous_picker = self.workflow_picker.take();
        self.help_open = true;
        let help = format!(
            "{status}\n\nRead-only replay. F8 pauses from any view. Space pauses in the transcript; +/- changes speed. ] advances to the next saved record. Tab selects an entry; Enter opens its details or subagent. / searches the already visible transcript. Esc/q closes one layer or returns to the parent; Ctrl-C exits. End follows the latest content. Recorded links and tools are never executed."
        );
        self.viewer = Some(BlockViewerPane::for_plain_text("Replay help", &help));
    }

    pub(super) fn draw(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        state: &mut ScrollbackState,
        stamp: FrameStamp,
    ) {
        if self.area.width != area.width
            || self.area.height != area.height
            || self
                .selection_visible_start
                .is_some_and(|start| start != state.visible_entry_range().start)
        {
            self.invalidate_geometry();
        }
        if self
            .selected_text
            .is_some_and(|(id, _)| state.index_of_id(id).is_none())
        {
            self.selected_text = None;
        }
        if let Some(search) = self.search.as_mut()
            && search.poll()
        {
            Self::reveal_match(search, state);
        }
        let composing = self.search.as_ref().is_some_and(|s| s.is_composing());
        let content = Rect::new(
            area.x,
            area.y,
            area.width,
            area.height.saturating_sub(u16::from(composing)),
        );
        self.area = content;
        state.prepare_layout(content.width, content.height);
        let rendered = ScrollbackPane::new()
            .active(true)
            .with_frame(stamp)
            .with_hovered_entry(self.hovered)
            .with_search_highlight(self.search.as_ref().and_then(|s| s.highlight_regex()))
            .render_with_scratch_and_selection_boundaries(content, buf, state, &mut self.scratch);
        let output = rendered.output;
        if let Some(selection) = output.selection_box {
            selection.render(buf);
        }
        self.selection_model = output.selection_model;
        self.selection_boundaries = rendered.selection_boundaries;
        if let Some((id, drag)) = self.drag.or(self.selected_text)
            && state.index_of_id(id).is_some()
        {
            render_active_selection_overlay(&self.selection_model, &drag, None, buf);
        }
        if composing
            && area.height > 0
            && let Some(search) = &self.search
        {
            let label = format!(
                "/{}  · {} matches · Enter search, Esc close",
                search.query(),
                search.match_count()
            );
            buf.set_stringn(
                area.x,
                area.bottom() - 1,
                label,
                area.width as usize,
                ratatui::style::Style::default(),
            );
        }
        if let Some(viewer) = &mut self.viewer {
            // PlainText viewers own their immutable text instead of a live entry.
            let placeholder = ScrollbackEntry::new(RenderBlock::notice(""));
            let entry = if viewer.kind == ViewerKind::PlainText {
                Some(&placeholder)
            } else {
                state.get_by_id(viewer.entry_id)
            };
            let Some(entry) = entry else {
                self.viewer = None;
                return;
            };
            viewer.tick(entry);
            let theme = Theme::current();
            let config = ModalWindowConfig {
                title: "Details",
                tabs: None,
                shortcuts: &[],
                sizing: ModalSizing {
                    width_pct: 0.95,
                    max_width: u16::MAX,
                    min_width: 20,
                    v_margin: 1,
                    h_pad: 2,
                    v_pad: 0,
                    footer_lines: 0,
                },
                fold_info: None,
            };
            if let Some(modal) = render_modal_window(buf, area, &mut viewer.modal, &config, &theme)
            {
                let context = BlockContext {
                    mode: DisplayMode::Expanded,
                    is_running: entry.is_running,
                    width: modal.content.width,
                    raw: entry.raw,
                    max_lines: None,
                    appearance: state.appearance().clone(),
                    is_selected: false,
                    cwd: None,
                };
                let preamble: Vec<Line<'static>> = entry
                    .block
                    .preamble(&context)
                    .map(|text| text.lines)
                    .unwrap_or_default();
                viewer.render_content(modal.content, buf, entry, true, &preamble);
                viewer.render_text_drag_overlay(buf);
            }
        }
        if let Some(picker) = &mut self.workflow_picker {
            let theme = Theme::current();
            let config = ModalWindowConfig {
                title: &picker.title,
                tabs: None,
                shortcuts: &[],
                sizing: ModalSizing {
                    width_pct: 0.8,
                    max_width: 100,
                    min_width: 30,
                    v_margin: 2,
                    h_pad: 2,
                    v_pad: 0,
                    footer_lines: 0,
                },
                fold_info: None,
            };
            picker.rows.clear();
            if let Some(modal) = render_modal_window(buf, area, &mut picker.modal, &config, &theme)
            {
                let content = modal.content;
                if content.height > 0 {
                    buf.set_stringn(
                        content.x,
                        content.y,
                        &picker.summary,
                        content.width as usize,
                        Style::default().fg(theme.text_secondary).bg(theme.bg_base),
                    );
                }
                if picker.children.is_empty() && content.height > 2 {
                    buf.set_stringn(
                        content.x,
                        content.y + 2,
                        "当前播放位置没有已出现的子 Agent；关闭后可重新查看。",
                        content.width as usize,
                        Style::default().fg(theme.gray).bg(theme.bg_base),
                    );
                }
                let available = content.height.saturating_sub(2) as usize;
                if available > 0 {
                    if picker.selected < picker.scroll {
                        picker.scroll = picker.selected;
                    }
                    if picker.selected >= picker.scroll.saturating_add(available) {
                        picker.scroll = picker.selected + 1 - available;
                    }
                }
                for (index, (id, title)) in picker
                    .children
                    .iter()
                    .enumerate()
                    .skip(picker.scroll)
                    .take(available)
                {
                    let Ok(row) = u16::try_from(index - picker.scroll) else {
                        break;
                    };
                    let y = content.y.saturating_add(2).saturating_add(row);
                    if y >= content.bottom() {
                        break;
                    }
                    let style = if index == picker.selected {
                        Style::default()
                            .fg(theme.accent_assistant)
                            .bg(theme.bg_base)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.text_primary).bg(theme.bg_base)
                    };
                    buf.set_stringn(
                        content.x,
                        y,
                        format!(
                            "{} {} · {}",
                            if index == picker.selected { "›" } else { " " },
                            title,
                            id
                        ),
                        content.width as usize,
                        style,
                    );
                    picker.rows.push(Rect::new(content.x, y, content.width, 1));
                }
            }
        }
    }

    fn reveal_match(search: &ScrollbackSearchState, state: &mut ScrollbackState) {
        if let Some(found) = search.current()
            && let Some(index) = state.index_of_id(found.entry_id)
        {
            state.set_selected(Some(index));
            state.expand_selected();
            state.reveal_entry_line(index, found.line_in_entry);
        }
    }

    pub(super) fn open_selected(&mut self, state: &mut ScrollbackState) -> BrowseAction {
        if state.is_selected_group_header() {
            state.toggle_group_expansion();
            return BrowseAction::None;
        }
        let Some(entry) = state.selected().and_then(|index| state.entry(index)) else {
            return BrowseAction::None;
        };
        if let RenderBlock::Subagent(child) = &entry.block {
            return BrowseAction::Child(child.child_session_id.clone());
        }
        if let RenderBlock::BgTask(task) = &entry.block {
            self.viewer = Some(BlockViewerPane::for_plain_text(
                "Recorded background task",
                &format!(
                    "Command: {}\nTask ID: {}\nRecorded status: {:?}\n\nHistorical stdout was not captured in the session transcript.",
                    task.command, task.task_id, task.kind
                ),
            ));
            return BrowseAction::None;
        }
        if let RenderBlock::Workflow(workflow) = &entry.block {
            return BrowseAction::Workflow(workflow.run_id.clone());
        }
        self.viewer = BlockViewerPane::for_entry(entry).or_else(|| {
            // Passive fallback exposes saved text, never opens paths or external viewers.
            entry
                .block
                .copy_text(true)
                .map(|text| BlockViewerPane::for_plain_text("Recorded content", &text))
        });
        BrowseAction::None
    }

    pub(super) fn handle(&mut self, event: Event, state: &mut ScrollbackState) -> BrowseAction {
        if matches!(&event, Event::Key(_))
            || matches!(&event, Event::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Down(_)))
        {
            self.feedback = None;
        }
        if let Some(picker) = &mut self.workflow_picker {
            let mut close = false;
            let mut child = None;
            match event {
                Event::Key(key) => match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => close = true,
                    KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                        picker.selected = picker.selected.saturating_sub(1)
                    }
                    KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                        picker.selected =
                            (picker.selected + 1).min(picker.children.len().saturating_sub(1))
                    }
                    KeyCode::Enter => {
                        child = picker
                            .children
                            .get(picker.selected)
                            .map(|(id, _)| id.clone())
                    }
                    _ => {}
                },
                Event::Mouse(mouse) => {
                    if handle_modal_mouse(&mut picker.modal, mouse.kind, mouse.column, mouse.row)
                        == ModalWindowOutcome::CloseRequested
                    {
                        close = true;
                    }
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            picker.pressed = picker
                                .rows
                                .iter()
                                .position(|rect| rect.contains((mouse.column, mouse.row).into()))
                                .map(|visible| picker.scroll + visible);
                            if let Some(index) = picker.pressed {
                                picker.selected = index;
                            }
                        }
                        MouseEventKind::Up(MouseButton::Left) => {
                            let released = picker
                                .rows
                                .iter()
                                .position(|rect| rect.contains((mouse.column, mouse.row).into()))
                                .map(|visible| picker.scroll + visible);
                            if picker.pressed.take() == released {
                                child = released.and_then(|index| {
                                    picker.children.get(index).map(|(id, _)| id.clone())
                                });
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            picker.selected = picker.selected.saturating_sub(1)
                        }
                        MouseEventKind::ScrollDown => {
                            picker.selected =
                                (picker.selected + 1).min(picker.children.len().saturating_sub(1))
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            if close || child.is_some() {
                self.workflow_picker = None;
            }
            return child.map_or(BrowseAction::None, BrowseAction::Child);
        }
        if let Some(viewer) = &mut self.viewer {
            let close = match &event {
                Event::Key(key) if viewer.is_close_key(key) => true,
                Event::Key(key) => {
                    viewer.handle_key(key);
                    false
                }
                Event::Paste(text) => {
                    viewer.handle_paste(text);
                    false
                }
                Event::Mouse(mouse) => {
                    match handle_modal_mouse(&mut viewer.modal, mouse.kind, mouse.column, mouse.row)
                    {
                        ModalWindowOutcome::CloseRequested => true,
                        ModalWindowOutcome::Handled => false,
                        _ => {
                            match mouse.kind {
                                MouseEventKind::ScrollDown => viewer.handle_scroll(3),
                                MouseEventKind::ScrollUp => viewer.handle_scroll(-3),
                                _ => {
                                    viewer.handle_mouse(mouse.kind, mouse.column, mouse.row);
                                }
                            }
                            false
                        }
                    }
                }
                _ => false,
            };
            if close {
                self.viewer = if self.help_open {
                    self.help_previous_viewer.take()
                } else {
                    None
                };
                if self.help_open {
                    self.workflow_picker = self.help_previous_picker.take();
                }
                self.help_open = false;
            } else {
                let text = viewer.drag_copy_text.take().or_else(|| {
                    state
                        .get_by_id_mut(viewer.entry_id)
                        .and_then(|entry| viewer.apply_pending_entry_actions(entry))
                });
                if let Some(text) = text {
                    self.feedback = Some(Self::copy_feedback(&text));
                }
            }
            return BrowseAction::None;
        }
        if let Some(search) = &mut self.search
            && search.is_composing()
        {
            match event {
                Event::Key(key) => match key.code {
                    KeyCode::Esc => search.accept(),
                    KeyCode::Enter => search.accept(),
                    _ => {
                        search.handle_query_key(&key, state);
                    }
                },
                Event::Paste(text) => {
                    search.apply_query_paste(&text, state);
                }
                _ => {}
            }
            return BrowseAction::None;
        }
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.clear_pointer();
                    state.scroll_up(3);
                }
                MouseEventKind::ScrollDown => {
                    self.clear_pointer();
                    state.scroll_down(3);
                }
                MouseEventKind::Moved => {
                    self.hovered = self
                        .area
                        .contains((mouse.column, mouse.row).into())
                        .then(|| state.entry_index_at_screen_row(mouse.row, self.area))
                        .flatten();
                }
                MouseEventKind::Down(MouseButton::Left)
                    if self.area.contains((mouse.column, mouse.row).into()) =>
                {
                    self.selected_text = None;
                    self.pressed = None;
                    self.pending_text = None;
                    if state.is_follow_mode() {
                        state.toggle_follow();
                    }
                    self.selection_visible_start = Some(state.visible_entry_range().start);
                    if let Some(index) = state.entry_index_at_screen_row(mouse.row, self.area) {
                        state.set_selected(Some(index));
                        if let Some(id) = Self::id_at(state, index) {
                            self.pressed = Some((id, mouse.column, mouse.row));
                            self.pending_text = self
                                .selection_model
                                .hit_test_selectable_range(mouse.column, mouse.row)
                                .map(|anchor| {
                                    (
                                        id,
                                        PendingTextDrag {
                                            anchor,
                                            start_col: mouse.column,
                                            start_row: mouse.row,
                                            anchor_content_width: self
                                                .selection_model
                                                .visible_block_content_width(anchor.entry_idx),
                                        },
                                    )
                                });
                        }
                    }
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    if let Some((id, pending)) = self.pending_text
                        && drag_threshold_exceeded(&pending, mouse.column, mouse.row)
                        && state.index_of_id(id).is_some()
                    {
                        let head = self
                            .selection_model
                            .hit_test_nearest_in_range(pending.anchor, mouse.column, mouse.row)
                            .unwrap_or(pending.anchor);
                        self.drag = Some((
                            id,
                            ActiveTextDrag {
                                anchor: pending.anchor,
                                head,
                                kind: SelectionKind::Linear,
                                anchor_content_width: pending.anchor_content_width,
                            },
                        ));
                        self.pressed = None;
                    } else if let Some((id, drag)) = &mut self.drag {
                        if state.index_of_id(*id).is_some() {
                            drag.head = self
                                .selection_model
                                .hit_test_nearest_in_range(drag.anchor, mouse.column, mouse.row)
                                .unwrap_or(drag.head);
                        }
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    self.pending_text = None;
                    if let Some((id, drag)) = self.drag.take() {
                        let text = self.copy_drag_text(state, id, &drag);
                        if let Some(text) = text {
                            self.feedback = Some(Self::copy_feedback(&text));
                        }
                        self.selected_text = Some((id, drag));
                        return BrowseAction::None;
                    }
                    if let Some((id, col, row)) = self.pressed.take()
                        && col == mouse.column
                        && row == mouse.row
                        && self.area.contains((col, row).into())
                        && state
                            .entry_index_at_screen_row(row, self.area)
                            .and_then(|index| Self::id_at(state, index))
                            == Some(id)
                        && let Some(index) = state.index_of_id(id)
                    {
                        state.set_selected(Some(index));
                        // Expanded verb groups share an entry with member 0; only
                        // their actual header row collapses the group.
                        if state
                            .get_cached_entry_layouts()
                            .and_then(|items| items.get(index))
                            .is_some_and(|info| {
                                info.verb_group_header && info.group_collapse_header
                            })
                            && state
                                .entry_screen_area(index, self.area)
                                .is_some_and(|(rect, _, _)| rect.y == row)
                        {
                            state.collapse_group_if_expanded();
                        } else {
                            return self.open_selected(state);
                        }
                    }
                }
                _ => {}
            },
            Event::Key(key) => match key.code {
                KeyCode::Esc | KeyCode::Char('q') if self.search.is_some() => self.search = None,
                KeyCode::Esc | KeyCode::Char('q') if self.selected_text.is_some() => {
                    self.selected_text = None
                }
                KeyCode::Esc | KeyCode::Char('q') => return BrowseAction::Back,
                KeyCode::Up | KeyCode::Char('k') => {
                    self.clear_pointer();
                    state.scroll_up(2);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.clear_pointer();
                    state.scroll_down(2);
                }
                KeyCode::BackTab => state.select_prev(),
                KeyCode::Tab => state.select_next(),
                KeyCode::PageUp => {
                    self.clear_pointer();
                    state.scroll_up(self.area.height.max(1));
                }
                KeyCode::PageDown => {
                    self.clear_pointer();
                    state.scroll_down(self.area.height.max(1));
                }
                KeyCode::Home => {
                    self.clear_pointer();
                    state.goto_top();
                }
                KeyCode::End => {
                    self.clear_pointer();
                    state.goto_bottom();
                }
                KeyCode::Left | KeyCode::Char('h') => state.collapse_selected(),
                KeyCode::Right | KeyCode::Char('l') => state.expand_selected(),
                KeyCode::Enter if self.search.is_some() => {
                    if let Some(search) = &mut self.search {
                        let query = search.query().to_owned();
                        search.update_query(&query, state);
                    }
                }
                KeyCode::Enter => return self.open_selected(state),
                KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return self.open_selected(state);
                }
                KeyCode::Char('r') => state.toggle_raw_selected(),
                KeyCode::Char('/') => self.search = Some(ScrollbackSearchState::open()),
                KeyCode::Char('?') => self.open_help("Replay status is shown in the panel."),
                KeyCode::Char('n' | 'N') => {
                    if let Some(search) = self.search.as_mut() {
                        if key.code == KeyCode::Char('n') {
                            search.next();
                        } else {
                            search.prev();
                        }
                        Self::reveal_match(search, state);
                    }
                }
                KeyCode::Char('y') => {
                    if let Some(text) = state
                        .selected()
                        .and_then(|index| state.entry(index))
                        .and_then(|entry| entry.block.copy_text(entry.raw))
                    {
                        self.feedback = Some(Self::copy_feedback(&text));
                    }
                }
                _ => {}
            },
            _ => {}
        }
        BrowseAction::None
    }

    fn copy_feedback(text: &str) -> String {
        Self::delivery_feedback(crate::clipboard::SystemClipboard::try_set(text))
    }

    fn delivery_feedback(delivery: crate::clipboard::ClipboardDelivery) -> String {
        if delivery.reported_success() {
            "已复制到剪贴板".to_owned()
        } else {
            "复制失败：剪贴板不可用".to_owned()
        }
    }

    fn copy_drag_text(
        &self,
        state: &ScrollbackState,
        id: EntryId,
        drag: &ActiveTextDrag,
    ) -> Option<String> {
        let content_width = drag.anchor_content_width.or_else(|| {
            self.selection_model
                .visible_block_content_width(drag.anchor.entry_idx)
        });
        if drag.anchor.entry_idx == drag.head.entry_idx
            && let (Some(entry), Some(width)) = (state.get_by_id(id), content_width)
        {
            entry.ensure_cached(width, state.appearance(), false, state.cwd());
            let rendered = entry.cached_rendered_output_ref();
            if let Some(text) = reconstruct_full_selection_text_with_boundaries(
                &rendered.output.lines,
                &rendered.boundaries,
                drag,
            ) {
                return Some(text);
            }
        }
        reconstruct_selection_text_with_boundaries(
            &self.selection_model,
            &self.selection_boundaries,
            drag,
        )
    }

    fn id_at(state: &ScrollbackState, index: usize) -> Option<EntryId> {
        state.iter_entries().nth(index).map(|(id, _)| id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scrollback::text_selection::RangeHit;
    use crossterm::event::KeyEvent;

    #[test]
    fn workflow_picker_opens_only_a_supplied_recorded_child() {
        let mut state = ScrollbackState::new();
        state.push_block(RenderBlock::Workflow(
            crate::scrollback::blocks::WorkflowBlock::started("run", "workflow", "inspect"),
        ));
        state.set_selected(Some(0));
        let mut browser = Browser::default();
        assert!(
            matches!(browser.open_selected(&mut state), BrowseAction::Workflow(id) if id == "run")
        );
        browser.open_workflow_picker(&state, vec![("child".into(), "Child".into())]);
        assert!(browser.owns_input());
        let action = browser.handle(
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            &mut state,
        );
        assert!(matches!(action, BrowseAction::Child(id) if id == "child"));
        assert!(!browser.owns_input());
    }

    #[test]
    fn resize_invalidates_pointer_hits_without_losing_a_recorded_selection() {
        let mut state = ScrollbackState::new();
        state.push_block(RenderBlock::notice("captured text"));
        let id = state.iter_entries().next().expect("entry").0;
        let hit = RangeHit {
            entry_idx: 0,
            range_id: 0,
            block_line_idx: 0,
            col_within_range: 1,
        };
        let mut browser = Browser::default();
        browser.selected_text = Some((
            id,
            ActiveTextDrag {
                anchor: hit,
                head: hit,
                kind: SelectionKind::Linear,
                anchor_content_width: Some(40),
            },
        ));
        browser.pressed = Some((id, 2, 3));
        browser.invalidate_geometry();
        assert!(browser.pressed.is_none());
        assert_eq!(browser.selected_text.map(|selection| selection.0), Some(id));
        browser.clear_pointer();
        assert!(browser.selected_text.is_none());
    }

    #[test]
    fn text_drag_cancels_a_pending_detail_click() {
        let mut state = ScrollbackState::new();
        state.push_block(RenderBlock::agent_message("captured text"));
        let id = state.iter_entries().next().expect("entry").0;
        let anchor = RangeHit {
            entry_idx: 0,
            range_id: 0,
            block_line_idx: 0,
            col_within_range: 0,
        };
        let mut browser = Browser::default();
        browser.pressed = Some((id, 2, 3));
        browser.pending_text = Some((
            id,
            PendingTextDrag {
                anchor,
                start_col: 2,
                start_row: 3,
                anchor_content_width: Some(40),
            },
        ));
        browser.handle(
            Event::Mouse(crossterm::event::MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: 10,
                row: 3,
                modifiers: KeyModifiers::NONE,
            }),
            &mut state,
        );
        assert!(browser.drag.is_some());
        assert!(browser.pressed.is_none());
        assert!(browser.viewer.is_none());
    }

    #[test]
    fn clipboard_failure_is_visible_in_the_replay_panel() {
        assert!(
            Browser::delivery_feedback(crate::clipboard::ClipboardDelivery::Failed)
                .contains("复制失败")
        );
    }

    #[test]
    fn expanded_group_header_and_first_member_have_distinct_mouse_targets() {
        crate::appearance::cache::set_group_tool_verbs(true);
        crate::appearance::cache::set_show_thinking_blocks(false);
        let mut state = ScrollbackState::new();
        for index in 0..3 {
            state.push_block(RenderBlock::ToolCall(
                crate::scrollback::ToolCallBlock::Read(
                    crate::scrollback::blocks::ReadToolCallBlock::new(format!("f{index}.rs"))
                        .with_content("captured line".into(), 1),
                ),
            ));
        }
        state.prepare_layout(80, 20);
        state.set_selected(Some(0));
        assert!(state.toggle_group_expansion());

        let area = Rect::new(0, 0, 80, 20);
        let mut browser = Browser::default();
        let mut buffer = Buffer::empty(area);
        browser.draw(area, &mut buffer, &mut state, FrameStamp::default());
        let first = state.entry_screen_area(0, area).expect("first entry").0;
        assert!(first.height >= 2);
        let click = |browser: &mut Browser, state: &mut ScrollbackState, y| {
            for kind in [
                MouseEventKind::Down(MouseButton::Left),
                MouseEventKind::Up(MouseButton::Left),
            ] {
                browser.handle(
                    Event::Mouse(crossterm::event::MouseEvent {
                        kind,
                        column: first.x.saturating_add(2),
                        row: y,
                        modifiers: KeyModifiers::NONE,
                    }),
                    state,
                );
            }
        };
        click(&mut browser, &mut state, first.y + 1);
        assert!(browser.viewer.is_some(), "member row opens details");
        browser.handle(
            Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            &mut state,
        );
        assert!(browser.viewer.is_none());
        click(&mut browser, &mut state, first.y);
        assert!(browser.viewer.is_none(), "header row only folds group");
        assert!(state.is_selected_group_header());
    }
}
