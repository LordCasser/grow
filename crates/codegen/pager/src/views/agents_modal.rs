//! Agent-definition catalog and configuration modal.

use crate::app::bundle::BundleState;
use crate::input::line_editor::{LineEditOutcome, LineEditor};
use crate::theme::Theme;
use crate::views::modal_window::{
    self, ModalSizing, ModalWindowConfig, ModalWindowState, Shortcut,
};
use agent::config::{AgentDefinition, AgentScope, BuiltinAgentName};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use shell::agent::config::AgentSelectionConfig;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tools::registry::types::ToolServerConfig;
use tools::types::template_renderer::TemplateRenderer;
use unicode_width::UnicodeWidthStr;

/// The modal has one canonical surface. The enum remains typed because editor
/// refresh actions carry the surface they originated from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentsTab {
    Agents,
}

impl AgentsTab {
    pub const ALL: &[Self] = &[Self::Agents];

    pub fn label(self) -> &'static str {
        match self {
            Self::Agents => "Agents",
        }
    }
}

pub struct AgentListEntry {
    pub name: String,
    pub description: String,
    pub scope: AgentScope,
    pub source_path: Option<PathBuf>,
    pub enabled: bool,
    pub is_builtin: bool,
    pub expanded: bool,
    pub definition: AgentDefinition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentsModalMessageKind {
    Error,
    Info,
}

#[derive(Debug, Clone)]
pub struct AgentsModalMessage {
    pub kind: AgentsModalMessageKind,
    pub text: String,
}

impl AgentsModalMessage {
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            kind: AgentsModalMessageKind::Error,
            text: text.into(),
        }
    }

    pub fn info(text: impl Into<String>) -> Self {
        Self {
            kind: AgentsModalMessageKind::Info,
            text: text.into(),
        }
    }
}

pub enum AgentsModalOutcome {
    Close,
    Changed,
    Unchanged,
    ViewAgent {
        title: String,
        source_path: Option<PathBuf>,
        content: Option<String>,
    },
    EditInEditor {
        path: PathBuf,
        tab: AgentsTab,
    },
}

pub struct AgentsModalState {
    pub window: ModalWindowState,
    pub active_tab: AgentsTab,
    pub agents: Vec<AgentListEntry>,
    pub selected: usize,
    pub scroll: usize,
    search: LineEditor,
    pub search_active: bool,
    pub(crate) row_map: Vec<(u16, usize)>,
    pub(crate) content_rect: Option<Rect>,
    pub message: Option<AgentsModalMessage>,
    pub cwd: PathBuf,
    pub default_agent: String,
    pub active_agent: Option<String>,
}

fn user_visible_builtins() -> &'static [BuiltinAgentName] {
    &[
        BuiltinAgentName::Grow,
        BuiltinAgentName::GeneralPurpose,
        BuiltinAgentName::Explore,
        BuiltinAgentName::BrowserUse,
    ]
}

impl AgentsModalState {
    pub fn new(
        cwd: &Path,
        toggle: &HashMap<String, bool>,
        _bundle: &BundleState,
        active_agent: Option<String>,
    ) -> Self {
        Self {
            window: ModalWindowState::with_tabs(1),
            active_tab: AgentsTab::Agents,
            agents: build_agent_list(cwd, toggle),
            selected: 0,
            scroll: 0,
            search: LineEditor::default(),
            search_active: false,
            row_map: Vec::new(),
            content_rect: None,
            message: None,
            cwd: cwd.to_path_buf(),
            default_agent: resolve_default_agent_name(cwd),
            active_agent,
        }
    }

    fn rebuild_agents(&mut self) {
        self.agents = build_agent_list(&self.cwd, &load_agent_toggle());
        self.selected = self.selected.min(self.agents.len().saturating_sub(1));
    }

    pub fn refresh_after_editor(&mut self, _tab: AgentsTab) {
        self.rebuild_agents();
    }

    pub fn search_query(&self) -> &str {
        self.search.text()
    }

    pub fn search_cursor_byte(&self) -> usize {
        self.search.cursor_byte()
    }

    fn reset_selection_after_search_change(&mut self) {
        if let Some(&first) = self.filtered_indices().first() {
            self.selected = first;
        }
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        if self.search_query().is_empty() {
            return (0..self.agents.len()).collect();
        }
        let query = self.search_query().to_lowercase();
        self.agents
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                entry.name.to_lowercase().contains(&query)
                    || entry.description.to_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect()
    }

    pub fn select_next(&mut self) {
        let indices = self.filtered_indices();
        if indices.is_empty() {
            return;
        }
        let current = indices.iter().position(|index| *index == self.selected);
        let next = current.map_or(0, |index| (index + 1).min(indices.len() - 1));
        self.selected = indices[next];
    }

    pub fn select_prev(&mut self) {
        let indices = self.filtered_indices();
        if indices.is_empty() {
            return;
        }
        let current = indices.iter().position(|index| *index == self.selected);
        let previous = current.map_or(indices.len() - 1, |index| index.saturating_sub(1));
        self.selected = indices[previous];
    }

    pub fn expand(&mut self) {
        if let Some(entry) = self.agents.get_mut(self.selected) {
            entry.expanded = true;
        }
    }

    pub fn collapse(&mut self) {
        if let Some(entry) = self.agents.get_mut(self.selected) {
            entry.expanded = false;
        }
    }
}

pub fn build_switch_agent_catalog(cwd: &Path) -> Vec<crate::slash::command::AgentArg> {
    let mut seen = HashSet::new();
    let mut catalog = Vec::new();
    for entry in build_agent_list(cwd, &HashMap::new()) {
        if !entry.definition.is_primary_agent_eligible() || !seen.insert(entry.name.clone()) {
            continue;
        }
        catalog.push(crate::slash::command::AgentArg {
            name: entry.name,
            description: entry.description,
            scope: entry.scope.label().to_string(),
        });
    }
    append_plugin_switch_agents(cwd, &mut catalog, &mut seen);
    catalog
}

fn append_plugin_switch_agents(
    cwd: &Path,
    catalog: &mut Vec<crate::slash::command::AgentArg>,
    seen: &mut HashSet<String>,
) {
    use agent::plugins::{PluginRegistry, TrustStore, discover_plugins};

    let project_trusted = shell::agent::folder_trust::project_scope_allowed(cwd);
    let trust_store = TrustStore::load();
    let mut plugin_config = load_plugins_discovery_config();
    let discovered = discover_plugins(Some(cwd), &plugin_config, &trust_store, project_trusted);
    if discovered.is_empty() {
        return;
    }
    plugin_config.populate_plugin_lists(&discovered);
    let registry = PluginRegistry::from_discovered(
        discovered,
        &plugin_config.disabled,
        &plugin_config.enabled,
    );
    for plugin in registry.enabled_plugins() {
        for agent_dir in &plugin.agent_dirs {
            let Ok(entries) = std::fs::read_dir(agent_dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
                    continue;
                }
                let definition = if plugin.trusted {
                    AgentDefinition::from_file(&path).ok()
                } else {
                    AgentDefinition::from_file_frontmatter_only(&path).ok()
                };
                let Some(definition) = definition else {
                    continue;
                };
                if !definition.is_primary_agent_eligible() {
                    continue;
                }
                let qualified = format!("{}:{}", plugin.name, definition.name);
                if !seen.insert(qualified.clone()) {
                    continue;
                }
                let scope = match plugin.scope {
                    agent::plugins::PluginScope::Project => "project",
                    _ => "user",
                };
                catalog.push(crate::slash::command::AgentArg {
                    name: qualified,
                    description: definition.description,
                    scope: scope.to_string(),
                });
            }
        }
    }
}

fn load_plugins_discovery_config() -> agent::plugins::discovery::DiscoveryConfig {
    use agent::plugins::discovery::DiscoveryConfig;

    let Ok(text) = std::fs::read_to_string(config::grow_home().join("config.toml")) else {
        return DiscoveryConfig::default();
    };
    let Ok(value) = text.parse::<toml::Value>() else {
        return DiscoveryConfig::default();
    };
    let Some(plugins) = value.get("plugins") else {
        return DiscoveryConfig::default();
    };
    let strings = |key: &str| {
        plugins
            .get(key)
            .and_then(toml::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    DiscoveryConfig {
        cli_plugin_dirs: Vec::new(),
        config_paths: strings("paths").into_iter().map(PathBuf::from).collect(),
        disabled: strings("disabled"),
        enabled: strings("enabled"),
    }
}

pub fn build_agent_list(cwd: &Path, toggle: &HashMap<String, bool>) -> Vec<AgentListEntry> {
    let mut entries = Vec::new();
    for &builtin in user_visible_builtins() {
        let definition = builtin.definition();
        let name = definition.name.clone();
        entries.push(AgentListEntry {
            enabled: toggle.get(&name).copied().unwrap_or(true),
            name,
            description: definition.description.clone(),
            scope: AgentScope::BuiltIn,
            source_path: None,
            is_builtin: true,
            expanded: false,
            definition,
        });
    }

    let subagent_names: Vec<String> = BuiltinAgentName::subagent_variants()
        .iter()
        .map(|builtin| builtin.definition().name)
        .collect();
    let priority = |scope| match scope {
        AgentScope::Project => 3,
        AgentScope::User => 2,
        AgentScope::Bundled => 1,
        AgentScope::BuiltIn => 0,
    };
    for definition in agent::discovery::discover(cwd) {
        if definition.scope == AgentScope::BuiltIn {
            continue;
        }
        if subagent_names.contains(&definition.name) && definition.scope != AgentScope::Project {
            continue;
        }
        let replacement = AgentListEntry {
            enabled: toggle.get(&definition.name).copied().unwrap_or(true),
            name: definition.name.clone(),
            description: definition.description.clone(),
            scope: definition.scope,
            source_path: definition.source_path.clone(),
            is_builtin: false,
            expanded: false,
            definition,
        };
        if let Some(index) = entries
            .iter()
            .position(|entry| entry.name == replacement.name)
        {
            if priority(replacement.scope) > priority(entries[index].scope) {
                entries[index] = replacement;
            }
        } else {
            entries.push(replacement);
        }
    }
    entries
}

pub fn load_agent_toggle() -> HashMap<String, bool> {
    let Ok(root) = shell::config::load_effective_config() else {
        return HashMap::new();
    };
    root.get("subagents")
        .and_then(|value| value.get("toggle"))
        .and_then(toml::Value::as_table)
        .map(|table| {
            table
                .iter()
                .filter_map(|(name, value)| value.as_bool().map(|enabled| (name.clone(), enabled)))
                .collect()
        })
        .unwrap_or_default()
}

fn load_agent_selection_config() -> AgentSelectionConfig {
    shell::config::load_effective_config()
        .ok()
        .and_then(|root| shell::agent::config::Config::new_from_toml_cfg(&root).ok())
        .map(|config| config.agent)
        .unwrap_or_default()
}

fn load_config_agent_name() -> Option<String> {
    load_agent_selection_config()
        .name
        .filter(|name| !name.is_empty())
}

pub fn resolve_default_agent_name(cwd: &Path) -> String {
    shell::agent::mvp_agent::MvpAgent::resolve_agent_definition(
        cwd,
        None,
        &load_agent_selection_config(),
        None,
        None,
    )
    .name
}

fn refresh_default_agent(state: &mut AgentsModalState) {
    state.default_agent = resolve_default_agent_name(&state.cwd);
}

pub fn set_default_agent(name: Option<&str>) -> Result<(), String> {
    let path = config::grow_home().join("config.toml");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let Some(mut document) = crate::config_toml_edit::read_config_document_for_edit(&path) else {
        return Err("Could not read or parse config.toml".to_string());
    };
    if let Some(name) = name {
        if !document.contains_key("agent") {
            document["agent"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        document["agent"]
            .as_table_mut()
            .ok_or("[agent] is not a table")?["name"] = toml_edit::value(name);
    } else if let Some(table) = document
        .get_mut("agent")
        .and_then(toml_edit::Item::as_table_mut)
    {
        table.remove("name");
    }
    std::fs::write(path, document.to_string())
        .map_err(|error| format!("Failed to write config.toml: {error}"))
}

pub fn toggle_agent(name: &str, enabled: bool) -> Result<(), String> {
    let path = config::grow_home().join("config.toml");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let Some(mut document) = crate::config_toml_edit::read_config_document_for_edit(&path) else {
        return Err("Could not read or parse config.toml".to_string());
    };
    if !document.contains_key("subagents") {
        document["subagents"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let subagents = document["subagents"]
        .as_table_mut()
        .ok_or("subagents is not a table")?;
    if !subagents.contains_key("toggle") {
        subagents["toggle"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    subagents["toggle"]
        .as_table_mut()
        .ok_or("subagents.toggle is not a table")?[name] = toml_edit::value(enabled);
    std::fs::write(path, document.to_string())
        .map_err(|error| format!("Failed to write config.toml: {error}"))
}

pub fn format_agent_detail(entry: &AgentListEntry) -> Vec<String> {
    let definition = &entry.definition;
    let mode = match definition.prompt_composition {
        agent::config::PromptComposition::Extend => "extend",
        agent::config::PromptComposition::Full => "full",
    };
    let mut lines = vec![format!("  Prompt mode: {mode}")];
    if definition.tool_config.tools.is_empty() {
        lines.push("  Tools: (none)".to_string());
    } else {
        lines.push(format!("  Tools ({})", definition.tool_config.tools.len()));
        for tool in &definition.tool_config.tools {
            let name = tool.name_override.as_deref().unwrap_or_else(|| {
                tool.id
                    .rsplit_once(':')
                    .map_or(tool.id.as_str(), |(_, name)| name)
            });
            lines.push(format!("    • {name}"));
        }
    }
    if !definition.skills.is_empty() {
        lines.push(format!("  Skills: {}", definition.skills.join(", ")));
    }
    if let Some(path) = &entry.source_path {
        lines.push(format!("  Source: {}", path.display()));
    }
    lines.push(format!("  Scope: {}", entry.scope.label()));
    lines
}

fn synthesize_agent_markdown(entry: &AgentListEntry) -> String {
    entry.definition.prompt_body.as_ref().map_or_else(
        || format!("*{} uses the base system prompt.*\n", entry.name),
        |body| render_prompt_body(body, &entry.definition.tool_config),
    )
}

fn render_prompt_body(body: &str, tool_config: &ToolServerConfig) -> String {
    let mut tools = HashMap::new();
    for tool in &tool_config.tools {
        if let Some(kind) = tool.kind {
            let name = tool.name_override.clone().unwrap_or_else(|| {
                tool.id
                    .rsplit_once(':')
                    .map_or_else(|| tool.id.clone(), |(_, name)| name.to_string())
            });
            tools.entry(kind).or_insert(name);
        }
    }
    TemplateRenderer::new(tools, HashMap::new())
        .render(body)
        .unwrap_or_else(|_| body.to_string())
}

fn modal_sizing(compact: bool) -> ModalSizing {
    ModalSizing {
        width_pct: 0.70,
        max_width: 100,
        min_width: 44,
        v_margin: 4,
        h_pad: 2,
        v_pad: 1,
        footer_lines: 2,
    }
    .with_compact(compact)
}

fn shortcuts(search_active: bool) -> Vec<Shortcut<'static>> {
    let mut shortcuts = vec![
        Shortcut {
            label: "j/k nav",
            clickable: false,
            id: 0,
        },
        Shortcut {
            label: "e/E fold",
            clickable: false,
            id: 0,
        },
        Shortcut {
            label: "Enter view",
            clickable: false,
            id: 0,
        },
        Shortcut {
            label: "/ search",
            clickable: false,
            id: 0,
        },
        Shortcut {
            label: "t spawn",
            clickable: false,
            id: 0,
        },
        Shortcut {
            label: "s default",
            clickable: false,
            id: 0,
        },
        Shortcut {
            label: "Esc close",
            clickable: false,
            id: 0,
        },
    ];
    modal_window::push_vim_nav_search_hint(&mut shortcuts, search_active);
    shortcuts
}

#[derive(Clone, Copy)]
enum RowKind {
    Agent(usize),
    Description(usize),
    Detail,
}

struct RenderRow {
    kind: RowKind,
    text: String,
}

pub fn render_agents_modal(
    buf: &mut Buffer,
    area: Rect,
    state: &mut AgentsModalState,
    compact: bool,
    theme: &Theme,
) {
    let shortcuts = shortcuts(state.search_active);
    let config = ModalWindowConfig {
        title: "Agents",
        tabs: None,
        shortcuts: &shortcuts,
        sizing: modal_sizing(compact),
        fold_info: None,
    };
    let Some(layout) =
        modal_window::render_modal_window(buf, area, &mut state.window, &config, theme)
    else {
        return;
    };
    let content = layout.content;
    state.content_rect = Some(content);
    state.row_map.clear();
    let mut y = content.y;

    if let Some(message) = &state.message {
        let color = match message.kind {
            AgentsModalMessageKind::Error => theme.accent_error,
            AgentsModalMessageKind::Info => theme.text_secondary,
        };
        buf.set_string(content.x, y, &message.text, Style::default().fg(color));
        y += 2;
    }
    let blurb = "t controls child spawn; /agent remains available for every definition.";
    buf.set_string(content.x, y, blurb, Style::default().fg(theme.gray_dim));
    y += 2;

    if state.search_active || !state.search_query().is_empty() {
        let prefix = format!("/ {}", state.search_query());
        buf.set_string(content.x, y, prefix, Style::default().fg(theme.accent_user));
        if state.search_active && content.width > 0 {
            let cursor = (2 + state.search_query()[..state.search_cursor_byte()].width()) as u16;
            if let Some(cell) = buf.cell_mut((content.x + cursor.min(content.width - 1), y)) {
                cell.set_style(Style::default().fg(theme.bg_base).bg(theme.text_primary));
            }
        }
        y += 2;
    }

    let mut rows = Vec::new();
    for index in state.filtered_indices() {
        let entry = &state.agents[index];
        let marker = if entry.expanded { "▼" } else { "▶" };
        let enabled = if entry.enabled {
            crate::glyphs::filled_dot()
        } else {
            "○"
        };
        let mut suffix = String::new();
        if state.active_agent.as_deref() == Some(entry.name.as_str()) {
            suffix.push_str(" active");
        }
        if state.default_agent == entry.name {
            suffix.push_str(" default");
        }
        if !entry.enabled {
            suffix.push_str(" [spawn off]");
        }
        rows.push(RenderRow {
            kind: RowKind::Agent(index),
            text: format!(
                "{marker} {enabled} {}{suffix} [{}]",
                entry.name,
                entry.scope.label()
            ),
        });
        if !entry.description.is_empty() {
            rows.push(RenderRow {
                kind: RowKind::Description(index),
                text: format!("    {}", entry.description),
            });
        }
        if entry.expanded {
            rows.extend(
                format_agent_detail(entry)
                    .into_iter()
                    .map(|text| RenderRow {
                        kind: RowKind::Detail,
                        text,
                    }),
            );
        }
    }

    let height = content.height.saturating_sub(y - content.y) as usize;
    if rows.is_empty() {
        buf.set_string(
            content.x,
            y,
            "No matching agents",
            Style::default().fg(theme.gray_dim),
        );
        return;
    }
    let selected_row = rows
        .iter()
        .position(|row| matches!(row.kind, RowKind::Agent(index) if index == state.selected))
        .unwrap_or(0);
    if selected_row < state.scroll {
        state.scroll = selected_row;
    } else if selected_row >= state.scroll.saturating_add(height) {
        state.scroll = selected_row.saturating_sub(height.saturating_sub(1));
    }
    state.scroll = state.scroll.min(rows.len().saturating_sub(height));
    for (offset, row) in rows.iter().skip(state.scroll).take(height).enumerate() {
        let row_y = y + offset as u16;
        let owner = match row.kind {
            RowKind::Agent(index) | RowKind::Description(index) => Some(index),
            RowKind::Detail => None,
        };
        if let Some(index) = owner {
            state.row_map.push((row_y, index));
        }
        let selected = owner == Some(state.selected);
        if selected {
            let fill = Style::default().bg(theme.bg_highlight);
            for x in content.x..content.x + content.width {
                if let Some(cell) = buf.cell_mut((x, row_y)) {
                    cell.set_style(fill);
                }
            }
        }
        let style = match row.kind {
            RowKind::Agent(_) => Style::default()
                .fg(theme.text_primary)
                .add_modifier(Modifier::BOLD),
            RowKind::Description(_) | RowKind::Detail => Style::default().fg(theme.gray),
        };
        let display = crate::render::line_utils::truncate_str(&row.text, content.width as usize);
        buf.set_string(
            content.x,
            row_y,
            display,
            if selected {
                style.bg(theme.bg_highlight)
            } else {
                style
            },
        );
    }
}

pub fn handle_agents_key(state: &mut AgentsModalState, key: &KeyEvent) -> AgentsModalOutcome {
    state.message = None;
    if state.search_active {
        match key.code {
            KeyCode::Esc => {
                state.search.reset();
                state.search_active = false;
                return AgentsModalOutcome::Changed;
            }
            KeyCode::Enter => {
                state.search_active = false;
                return AgentsModalOutcome::Changed;
            }
            _ => {}
        }
        let outcome = state.search.handle_key(key);
        if outcome == LineEditOutcome::TextChanged {
            state.reset_selection_after_search_change();
        }
        return line_edit_outcome(outcome);
    }

    let config = ModalWindowConfig {
        title: "Agents",
        tabs: None,
        shortcuts: &[],
        sizing: modal_sizing(false),
        fold_info: None,
    };
    if matches!(
        modal_window::handle_modal_key(&mut state.window, key, &config),
        modal_window::ModalWindowOutcome::CloseRequested
    ) {
        return AgentsModalOutcome::Close;
    }
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            state.select_next();
            AgentsModalOutcome::Changed
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.select_prev();
            AgentsModalOutcome::Changed
        }
        KeyCode::Char('e') | KeyCode::Right => {
            state.expand();
            AgentsModalOutcome::Changed
        }
        KeyCode::Char('E') | KeyCode::Left => {
            state.collapse();
            AgentsModalOutcome::Changed
        }
        KeyCode::PageDown | KeyCode::Char('d')
            if key.code == KeyCode::PageDown || key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            for _ in 0..10 {
                state.select_next();
            }
            AgentsModalOutcome::Changed
        }
        KeyCode::PageUp | KeyCode::Char('u')
            if key.code == KeyCode::PageUp || key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            for _ in 0..10 {
                state.select_prev();
            }
            AgentsModalOutcome::Changed
        }
        KeyCode::Enter | KeyCode::Char('o') => {
            state
                .agents
                .get(state.selected)
                .map_or(AgentsModalOutcome::Unchanged, |entry| {
                    AgentsModalOutcome::ViewAgent {
                        title: format!("{} — prompt extension", entry.name),
                        source_path: entry.source_path.clone(),
                        content: entry
                            .source_path
                            .is_none()
                            .then(|| synthesize_agent_markdown(entry)),
                    }
                })
        }
        KeyCode::Char('/') | KeyCode::Char('i') if key.modifiers.is_empty() => {
            state.search_active = true;
            AgentsModalOutcome::Changed
        }
        KeyCode::Char('q') => AgentsModalOutcome::Close,
        KeyCode::Char('s') => {
            if let Some(entry) = state.agents.get(state.selected) {
                let name = entry.name.clone();
                let clear = load_config_agent_name().as_deref() == Some(name.as_str());
                match set_default_agent((!clear).then_some(name.as_str())) {
                    Ok(()) => {
                        refresh_default_agent(state);
                        state.message = Some(AgentsModalMessage::info(if clear {
                            format!(
                                "Cleared default; new sessions use '{}'",
                                state.default_agent
                            )
                        } else {
                            format!("New sessions use '{}'", state.default_agent)
                        }));
                    }
                    Err(error) => state.message = Some(AgentsModalMessage::error(error)),
                }
            }
            AgentsModalOutcome::Changed
        }
        KeyCode::Char('t') => {
            if let Some(entry) = state.agents.get(state.selected) {
                let name = entry.name.clone();
                match toggle_agent(&name, !entry.enabled) {
                    Ok(()) => state.rebuild_agents(),
                    Err(error) => state.message = Some(AgentsModalMessage::error(error)),
                }
            }
            AgentsModalOutcome::Changed
        }
        _ => AgentsModalOutcome::Unchanged,
    }
}

pub fn handle_agents_paste(state: &mut AgentsModalState, text: &str) -> AgentsModalOutcome {
    if !state.search_active {
        return AgentsModalOutcome::Unchanged;
    }
    let outcome = state.search.insert_paste(text);
    if outcome == LineEditOutcome::TextChanged {
        state.reset_selection_after_search_change();
    }
    line_edit_outcome(outcome)
}

fn line_edit_outcome(outcome: LineEditOutcome) -> AgentsModalOutcome {
    match outcome {
        LineEditOutcome::TextChanged
        | LineEditOutcome::HandledNoChange
        | LineEditOutcome::CursorChanged => AgentsModalOutcome::Changed,
        LineEditOutcome::Unhandled => AgentsModalOutcome::Unchanged,
    }
}

pub fn handle_agents_mouse(state: &mut AgentsModalState, mouse: &MouseEvent) -> AgentsModalOutcome {
    match modal_window::handle_modal_mouse(&mut state.window, mouse.kind, mouse.column, mouse.row) {
        modal_window::ModalWindowOutcome::CloseRequested => return AgentsModalOutcome::Close,
        modal_window::ModalWindowOutcome::Handled => return AgentsModalOutcome::Changed,
        _ => {}
    }
    let in_content = state
        .content_rect
        .is_some_and(|area| area.contains(ratatui::layout::Position::new(mouse.column, mouse.row)));
    if !in_content {
        return AgentsModalOutcome::Unchanged;
    }
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            state.select_prev();
            AgentsModalOutcome::Changed
        }
        MouseEventKind::ScrollDown => {
            state.select_next();
            AgentsModalOutcome::Changed
        }
        MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
            let Some((_, index)) = state.row_map.iter().find(|(row, _)| *row == mouse.row) else {
                return AgentsModalOutcome::Unchanged;
            };
            if *index == state.selected {
                if state.agents[*index].expanded {
                    state.collapse();
                } else {
                    state.expand();
                }
            } else {
                state.selected = *index;
            }
            AgentsModalOutcome::Changed
        }
        _ => AgentsModalOutcome::Unchanged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_one_surface() {
        assert_eq!(AgentsTab::ALL, &[AgentsTab::Agents]);
        assert_eq!(AgentsTab::Agents.label(), "Agents");
    }

    #[test]
    fn builtin_catalog_is_nonempty() {
        let cwd = tempfile::tempdir().unwrap();
        let list = build_agent_list(cwd.path(), &HashMap::new());
        assert!(list.iter().any(|entry| entry.name == "grow"));
    }

    #[test]
    fn navigation_clamps_at_edges() {
        let cwd = tempfile::tempdir().unwrap();
        let mut state =
            AgentsModalState::new(cwd.path(), &HashMap::new(), &BundleState::default(), None);
        state.select_prev();
        assert_eq!(state.selected, 0);
        for _ in 0..state.agents.len() + 2 {
            state.select_next();
        }
        assert_eq!(state.selected, state.agents.len() - 1);
    }
}
