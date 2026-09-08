//! Transcript export, block copying, viewer/modal, and input-log dump dispatchers.

use super::ctx::with_active_agent;
use crate::app::transcript_file_writes::{FileWriteKind, TranscriptFileWrite};
use super::session::lifecycle::skip_picker_and_create_session;
use crate::app::actions::Effect;
use crate::app::root::{ActiveView, AppView};
use crate::app::session::AgentId;
use crate::scrollback::block::{BlockContent, RenderBlock};
use crate::scrollback::blocks::ToolCallBlock;
use acp_transport::protocol as acp;
use diagnostics::session_ctx::log_event;

/// Copy the selected block's content to the system clipboard.
///
/// Respects the block's raw/pretty mode for markdown content.
/// Shows a toast notification on theExtensionsTab
pub(super) fn dispatch_copy_block_content(app: &mut AppView) {
    with_active_agent(app, |agent| {
        let Some(idx) = agent.scrollback.selected() else {
            return;
        };
        if agent.scrollback.entry_content_hidden_by_group(idx) {
            return;
        }
        let Some(entry) = agent.scrollback.entry(idx) else {
            return;
        };

        // BgTask blocks: copy stdout from central store
        let text = if let RenderBlock::BgTask(block) = &entry.block {
            let stdout = agent
                .session
                .bg_tasks
                .get(&block.task_id)
                .map(|t| t.stdout.clone())
                .unwrap_or_default();
            if stdout.is_empty() {
                None
            } else {
                Some(stdout)
            }
        } else {
            entry.block.copy_text(entry.raw)
        };

        if let Some(text) = text
            && !text.is_empty()
        {
            agent.copy_to_clipboard(&text);
        }
    });
}

/// Copy the Nth most recent assistant message to the clipboard, or to `file_path`.
pub(super) fn dispatch_copy_assistant_message(
    app: &mut AppView,
    n: usize,
    file_path: Option<std::path::PathBuf>,
) -> Vec<Effect> {
    let mut request = None;
    with_active_agent(app, |agent| {
        if n == 0 {
            agent.scrollback.push_block(RenderBlock::notice(
                "Usage: /copy [N] [file] where N is 1 (latest), 2, 3, ...",
            ));
            return;
        }
        let mut available = 0;
        let mut selected = None;
        for i in (0..agent.scrollback.len()).rev() {
            if let Some(entry) = agent.scrollback.entry(i)
                && let RenderBlock::AgentMessage(msg) = &entry.block
            {
                available += 1;
                if available == n {
                    selected = Some(msg.copy_text(false));
                    break;
                }
            }
        }
        let Some(text) = selected else {
            let message = if available == 0 {
                "No assistant messages to copy".to_string()
            } else {
                format!(
                    "Only {} assistant {} available to copy",
                    available,
                    if available == 1 { "message" } else { "messages" },
                )
            };
            agent.scrollback.push_block(RenderBlock::notice(message));
            return;
        };
        if text.is_empty() {
            agent
                .scrollback
                .push_block(RenderBlock::notice("Assistant message is empty"));
            return;
        }

        if let Some(p) = file_path {
            let path = agent.session.cwd.join(shellexpand::tilde(&p.to_string_lossy()).as_ref());
            request = Some(TranscriptFileWrite {
                agent_id: agent.session.id, session_id: agent.session.session_id.clone(),
                path, content: text, kind: FileWriteKind::Copy,
            });
            return;
        }

        let stats = crate::clipboard::clipboard_stats_suffix(&text);
        let delivery = crate::clipboard::copy_text_or_file(&text);
        agent.scrollback.push_block(RenderBlock::notice(format!(
            "{}{stats}", delivery.summary_message()
        )));
        agent.show_toast_for(
            delivery.toast_message().as_ref(),
            std::time::Duration::from_millis(u64::from(delivery.toast_ticks()) * 33),
        );
    });
    enqueue_file_write(app, request)
}

/// Dispatch for the `/export` command.
/// Collects the active (sub)agent's scrollback, renders a clean Markdown transcript,
/// and either writes it to the (expanded) file or copies it to the clipboard using the
/// full route (native + tmux + OSC 52) with appropriate feedback.
pub(super) fn dispatch_export_conversation(
    app: &mut AppView,
    file_path: Option<std::path::PathBuf>,
) -> Vec<Effect> {
    let mut request = None;
    with_active_agent(app, |agent| {
        if agent.session.loading_replay {
            agent.scrollback.push_block(RenderBlock::notice(
                "Session history is still loading. Try /export again when loading finishes.",
            ));
            return;
        }

        let blocks: Vec<_> = (0..agent.scrollback.len())
            .filter_map(|i| agent.scrollback.entry(i).map(|e| &e.block))
            .collect();

        let md = crate::scrollback::export::render_blocks_to_markdown(blocks);

        if md.is_empty() {
            agent
                .scrollback
                .push_block(RenderBlock::notice("No conversation content to export"));
            return;
        }

        if let Some(p) = file_path {
            // Session-relative resolution stays here; CLI and TUI share file commit logic.
            let expanded = agent
                .session
                .cwd
                .join(shellexpand::tilde(&p.to_string_lossy()).as_ref());
            request = Some(TranscriptFileWrite {
                agent_id: agent.session.id, session_id: agent.session.session_id.clone(),
                path: expanded, content: md, kind: FileWriteKind::Export,
            });
        } else {
            // Clipboard path: stats block (like assistant copy) + route-aware toast
            // (like block content copy / selection). Good UX for a potentially large transcript.
            // The scrollback line reflects where the copy actually landed —
            // same pattern as /copy N — instead of claiming clipboard success
            // when the delivery fell back to the backup file.
            let stats = crate::clipboard::clipboard_stats_suffix(&md);
            let delivery = agent.copy_to_clipboard(&md);
            let block_msg = format!("Conversation: {}{stats}", delivery.summary_message());
            agent.scrollback.push_block(RenderBlock::notice(block_msg));
        }
    });
    enqueue_file_write(app, request)
}

fn enqueue_file_write(app: &mut AppView, request: Option<TranscriptFileWrite>) -> Vec<Effect> {
    let Some(request) = request else { return vec![]; };
    let agent_id = request.agent_id;
    match app.transcript_file_writes.enqueue(request) {
        Ok(effect) => {
            if let Some(agent) = app.agents.get_mut(&agent_id) {
                agent.scrollback.push_block(RenderBlock::notice("Saving file…"));
            }
            effect.into_iter().collect()
        }
        Err(()) => {
            if let Some(agent) = app.agents.get_mut(&agent_id) {
                agent.scrollback.push_block(RenderBlock::notice("Too many file writes pending. Try again after a write finishes."));
            }
            vec![]
        }
    }
}

/// Open the full transcript in `$PAGER`.
///
/// **Minimal mode** renders a full-fidelity ANSI transcript — every block
/// fully expanded (reasoning in full, tool output uncapped, diff colors kept)
/// — a full layout + syntax-highlight + ANSI-serialization pass over the whole
/// session. Rendering that inline froze the event loop for seconds on long
/// sessions ("laggy /transcript"), and the block model is `!Send` (syntect's
/// resumable highlighter state lives inside markdown blocks), so it can't be
/// shipped to a worker either. Instead this only ARMS the request; the minimal
/// render loop builds the transcript **incrementally, a time-budgeted slice
/// per frame** (`full_view::pump_transcript`, the same time-sliced amortization
/// pattern other TUIs use for heavy transcript work), then arms `pending_pager`
/// for the event loop's suspend-into-`$PAGER`.
///
/// **Other modes** keep the compact markdown export (string concatenation, no
/// layout or highlighting — cheap enough to stay synchronous).
pub(crate) fn dispatch_open_transcript_pager(app: &mut AppView) {
    if app.screen_mode.is_minimal() {
        crate::minimal_api::request_minimal_transcript(app);
        return;
    }

    let ActiveView::Agent(root) = app.active_view else { return; };
    let mut md = None;
    let mut loading = false;
    with_active_agent(app, |agent| {
        if agent.session.loading_replay {
            loading = true;
            agent.scrollback.push_block(RenderBlock::notice(
                "Session history is still loading. Try /transcript again when loading finishes.",
            ));
            return;
        }
        let blocks: Vec<_> = (0..agent.scrollback.len())
            .filter_map(|i| agent.scrollback.entry(i).map(|e| &e.block))
            .collect();
        let rendered = crate::scrollback::export::render_blocks_to_markdown(blocks);
        if !rendered.is_empty() {
            md = Some(crate::export_cmd::write_pager_transcript(&rendered, false)
                .map(|path| crate::app::external_pager::PendingPager::new(path, false, root, agent)));
        }
    });

    if loading {
        return;
    }

    let Some(content) = md else {
        with_active_agent(app, |agent| {
            agent.scrollback.push_block(RenderBlock::notice(
                "No conversation transcript to view yet",
            ));
        });
        return;
    };

    match content {
        Ok(request) => {
            app.pending_pager = Some(request);
        }
        Err(e) => {
            with_active_agent(app, |agent| {
                agent.scrollback.push_block(RenderBlock::notice(format!(
                    "Failed to write transcript: {e}"
                )));
            });
        }
    }
}

/// Open the fullscreen block viewer for the selected entry.
/// Falls back to the image viewer only for entries without a normal block viewer.
pub(super) fn dispatch_open_block_viewer(app: &mut AppView) {
    use crate::views::block_viewer::BlockViewerPane;

    with_active_agent(app, |agent| {
        let Some(idx) = agent.scrollback.selected() else {
            return;
        };
        let Some(entry) = agent.scrollback.entry(idx) else {
            return;
        };

        // Block has images/media but terminal can't render pixels — toast and bail.
        let has_media =
            !entry.block.image_references().is_empty() || entry.block.inline_media().is_some();
        if has_media && !crate::terminal::image::detect_graphics_protocol().supports_images() {
            agent.guard_image_support();
            return;
        }

        if !entry.block.has_normal_fullscreen_viewer() {
            // Image: Enter opens the file in the OS-native viewer.
            if let Some(first_ref) = entry.block.image_references().first() {
                let path = first_ref.path.clone();
                agent.open_media_natively(&path);
            }
            return;
        }

        // Try to create a normal viewer for the selected block type.
        let viewer = match &entry.block {
            RenderBlock::Thinking(_) | RenderBlock::AgentMessage(_) => {
                BlockViewerPane::for_markdown(entry.id, entry)
            }
            RenderBlock::ToolCall(ToolCallBlock::Execute(_)) => {
                BlockViewerPane::for_execute(entry.id, entry)
            }
            RenderBlock::ToolCall(ToolCallBlock::Edit(_)) => {
                BlockViewerPane::for_edit(entry.id, entry)
            }
            RenderBlock::ToolCall(ToolCallBlock::Read(_)) => {
                BlockViewerPane::for_read(entry.id, entry)
            }
            RenderBlock::ToolCall(ToolCallBlock::Search(_)) => {
                BlockViewerPane::for_grep(entry.id, entry)
            }
            RenderBlock::ToolCall(ToolCallBlock::ListDir(_)) => {
                BlockViewerPane::for_list_dir(entry.id, entry)
            }
            RenderBlock::ToolCall(ToolCallBlock::WebFetch(_)) => {
                BlockViewerPane::for_web_fetch(entry.id, entry)
            }
            RenderBlock::ToolCall(ToolCallBlock::IntegrationSearch(_)) => {
                BlockViewerPane::for_integration_search(entry.id, entry)
            }
            RenderBlock::ToolCall(ToolCallBlock::UseTool(_)) => {
                BlockViewerPane::for_use_tool(entry.id, entry)
            }
            RenderBlock::BgTask(block) => {
                let stdout = agent
                    .session
                    .bg_tasks
                    .get(&block.task_id)
                    .map(|t| t.stdout.as_str())
                    .unwrap_or("");
                let is_running = agent
                    .session
                    .bg_tasks
                    .get(&block.task_id)
                    .is_some_and(|t| t.status == crate::app::session::BgTaskStatus::Running);
                Some(BlockViewerPane::for_bg_task(
                    entry.id,
                    &block.task_id,
                    stdout,
                    is_running,
                ))
            }
            RenderBlock::SubagentPermission(block) => block.member(0).map(|member| {
                BlockViewerPane::for_plain_text(&member.detail_title(), &member.detail_text())
            }),
            RenderBlock::Notice(notice) if notice.has_details() => {
                Some(BlockViewerPane::for_plain_text(
                    if notice.category == crate::scrollback::blocks::NoticeCategory::Command {
                        "Command result"
                    } else {
                        "Coordination inquiry"
                    },
                    &notice.detail_text(),
                ))
            }
            RenderBlock::ToolCall(ToolCallBlock::Other(block)) => {
                Some(BlockViewerPane::for_plain_text(
                    &block.name,
                    &format!(
                        "{}\n{}\n{}",
                        block.summary,
                        block.error.as_deref().unwrap_or_default(),
                        block.output.as_deref().unwrap_or_default()
                    ),
                ))
            }
            _ => None,
        };

        if viewer.is_some() {
            agent.block_viewer = viewer;
            return;
        }

        // Image: Enter opens the file in the OS-native viewer.
        if let Some(first_ref) = entry.block.image_references().first() {
            let path = first_ref.path.clone();
            agent.open_media_natively(&path);
        }
    });
}

/// Fetch-set that populates every Extensions-modal tab. Shared by the manual
/// open path, the post-CTA-install auth handoff, and the deferred-fetch
/// session-ready handlers so they can't drift and leave a tab stuck on its
/// initial `Loading` state.
pub(super) fn extensions_modal_tab_fetches(
    modal: &mut crate::views::extensions_modal::ExtensionsModalState,
    agent_id: AgentId,
    session_id: acp::SessionId,
) -> Vec<Effect> {
    let mut effects = vec![
        Effect::FetchHooksList {
            agent_id,
            session_id: session_id.clone(),
        },
        Effect::FetchPluginsList {
            agent_id,
            session_id: session_id.clone(),
        },
        Effect::FetchMcpsList {
            agent_id,
            session_id: session_id.clone(),
        },
        Effect::FetchSkillsList {
            agent_id,
            session_id: session_id.clone(),
        },
        Effect::FetchWorkflowsList {
            agent_id,
            session_id: session_id.clone(),
        },
    ];
    push_marketplace_fetch(modal, &mut effects, agent_id, session_id);
    effects
}

/// Push a marketplace list fetch, coalescing overlapping requests: while one
/// is in flight, further requests fold into a single queued refetch that
/// fires when the current response lands (see the field docs on
/// `ExtensionsModalState`). The other tab fetches are cheap local reads and
/// don't need this.
pub(super) fn push_marketplace_fetch(
    modal: &mut crate::views::extensions_modal::ExtensionsModalState,
    effects: &mut Vec<Effect>,
    agent_id: AgentId,
    session_id: acp::SessionId,
) {
    if modal.marketplace_fetch_inflight {
        modal.marketplace_refetch_queued = true;
        return;
    }
    modal.marketplace_fetch_inflight = true;
    effects.push(Effect::FetchMarketplaceList {
        agent_id,
        session_id,
    });
}

/// Open the hooks/plugins modal on the active agent view and fetch list data.
pub(super) fn dispatch_open_extensions_modal(
    app: &mut AppView,
    tab: crate::views::extensions_modal::ExtensionsTab,
    trigger: diagnostics::events::ExtensionsModalTrigger,
) -> Vec<Effect> {
    use crate::views::extensions_modal::ExtensionsModalState;

    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };

    // Mutual exclusivity: close agents modal when opening extensions.
    agent.agents_modal = None;
    let modal = ExtensionsModalState::new(tab);
    agent.extensions_modal = Some(modal);
    log_event(diagnostics::events::ExtensionsModalOpened {
        trigger,
        tab: tab.diagnostics_tab(),
    });

    let Some(session_id) = agent.session.session_id.clone() else {
        // Tabs default to Loading; the fetch fires on SessionCreated. With a
        // picker-deferred session nothing else would create one, so do it now.
        agent.session.set_pending_extensions_fetch();
        return skip_picker_and_create_session(app, id);
    };
    agent.session.clear_pending_extensions_fetch();
    let Some(modal) = agent.extensions_modal.as_mut() else {
        return vec![];
    };
    extensions_modal_tab_fetches(modal, id, session_id)
}

/// Open the agents modal, showing all agent definitions.
pub(super) fn dispatch_open_config_agents_modal(
    app: &mut AppView,
    initial_tab: Option<crate::views::agents_modal::AgentsTab>,
) -> Vec<Effect> {
    use crate::views::agents_modal::{AgentsModalState, load_agent_toggle};

    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let bundle = app.bundle_state.clone();
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };

    // Mutual exclusivity with extensions_modal
    agent.extensions_modal = None;

    let cwd = agent.session.cwd.clone();
    let toggle = load_agent_toggle();
    let session_id = agent.session.session_id.clone();
    let active_agent = agent.session.agent_name().map(str::to_owned);
    let mut modal = AgentsModalState::new(&cwd, &toggle, &bundle, active_agent);
    if let Some(tab) = initial_tab {
        modal.active_tab = tab;
    }
    agent.agents_modal = Some(modal);
    if let Some(session_id) = session_id {
        let revision = agent.session.begin_agent_metadata_read();
        return vec![Effect::FetchSessionAgentName {
            agent_id: id,
            session_id,
            revision,
        }];
    }
    vec![]
}

/// Copy the selected block's metadata (e.g., command) to clipboard.
pub(super) fn dispatch_copy_block_meta(app: &mut AppView) {
    with_active_agent(app, |agent| {
        let Some(idx) = agent.scrollback.selected() else {
            return;
        };
        if agent.scrollback.entry_content_hidden_by_group(idx) {
            return;
        }
        let Some(entry) = agent.scrollback.entry(idx) else {
            return;
        };
        if let Some(text) = entry.block.copy_meta()
            && !text.is_empty()
        {
            agent.copy_to_clipboard(&text);
        }
    });
}

/// Dump the input flight recorder to a JSON file for debugging.
/// See `input_log.rs` module docs for lifecycle/removal instructions.
fn input_dump_target(app: &mut AppView) -> Option<&mut crate::app::agent_view::AgentView> {
    let id = match app.active_view {
        ActiveView::Agent(id) => id,
        ActiveView::AgentDashboard => app.dashboard.as_ref()?.attached_agent?,
        _ => return None,
    };
    fn descend(agent: &mut crate::app::agent_view::AgentView) -> &mut crate::app::agent_view::AgentView {
        if agent.permission_queue.is_empty()
            && let Some(sid) = agent.active_subagent.clone()
            && agent.subagent_views.contains_key(&sid)
        {
            return descend(agent.subagent_views.get_mut(&sid).unwrap());
        }
        agent
    }
    app.agents.get_mut(&id).map(descend)
}

fn build_input_dump(
    agent: &crate::app::agent_view::AgentView,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<crate::input_log::InputDump> {
    if agent.input_log.entry_count() == 0 { return None; }
    let time_span_ms = agent.input_log.time_span_ms();
    let entries = agent.input_log.snapshot_entries();
    let entry_count = entries.len();
    let terminal = crate::terminal::terminal_context().diagnostics_snapshot();
    let session_id = agent.session.session_id.as_ref().map(|s| s.0.to_string());
    let pager_version = crate::client_identity::PAGER_CLIENT_VERSION;

    Some(crate::input_log::InputDump {
        dumped_at: now.to_rfc3339(),
        session_id,
        pager_version,
        terminal,
        active_pane: format!("{:?}", agent.active_pane),
        textarea_cursor: agent.prompt.cursor(),
        textarea_text_len: agent.prompt.text().len(),
        textarea_has_selection: agent.prompt.textarea.selection_range().is_some(),
        entry_count,
        time_span_ms,
        entries,
    })
}

pub(super) fn dispatch_dump_input_log(app: &mut AppView) -> Vec<Effect> {
    let Some(agent) = input_dump_target(app) else { return vec![]; };
    let now = chrono::Utc::now();
    let Some(dump) = build_input_dump(agent, now) else {
        agent.show_toast("No input events recorded yet.");
        return vec![];
    };
    let entry_count = dump.entry_count;
    let time_span_ms = dump.time_span_ms;
    let session_id = dump.session_id.clone();

    let json = match serde_json::to_string_pretty(&dump) {
        Ok(j) => j,
        Err(e) => {
            agent.show_toast(&format!("Failed to serialize input log: {e}"));
            return vec![];
        }
    };

    let grow_home = tools::util::grow_home::grow_home();
    let logs_dir = grow_home.join("logs");
    match crate::input_log::write_input_dump(&logs_dir, now, &json) {
        Ok(path) => {
            let display_path = path.display();
            agent.show_toast(&format!(
                "Input log ({entry_count} events) → {display_path}"
            ));
            crate::unified_log::info(
                &format!("input debug dump: {entry_count} events, {time_span_ms}ms span"),
                session_id.as_deref(),
                None,
            );
        }
        Err(e) => {
            agent.show_toast(&format!("Failed to write input log: {e}"));
        }
    }
    vec![]
}

// TaskResult handlers.

pub(super) fn handle_hooks_list_loaded(
    app: &mut AppView,
    agent_id: AgentId,
    result: Result<extension_types::HooksListResponse, String>,
) -> Vec<Effect> {
    use crate::views::extensions_modal::TabDataState;
    if let Some(agent) = app.agents.get_mut(&agent_id)
        && let Some(ref mut modal) = agent.extensions_modal
    {
        modal.hooks_data = match result {
            Ok(response) => {
                // Default all groups to collapsed.
                let mut seen = std::collections::HashSet::new();
                for hook in &response.hooks {
                    seen.insert(hook.source_dir.clone());
                }
                modal.hooks_collapsed_groups = seen;
                TabDataState::Loaded(response)
            }
            Err(e) => TabDataState::Error(e),
        };
    }
    vec![]
}

pub(super) fn handle_plugins_list_loaded(
    app: &mut AppView,
    agent_id: AgentId,
    result: Result<extension_types::PluginsListResponse, String>,
) -> Vec<Effect> {
    use crate::views::extensions_modal::TabDataState;
    if let Some(agent) = app.agents.get_mut(&agent_id)
        && let Some(ref mut modal) = agent.extensions_modal
    {
        modal.plugins_data = match result {
            Ok(response) => {
                modal.seed_plugin_groups_once(&response.plugins);
                TabDataState::Loaded(response)
            }
            Err(e) => TabDataState::Error(e),
        };
        // Clear pending_action so the UI unblocks as soon as the
        // plugins list arrives. Marketplace can continue loading
        // independently via its own TabDataState::Loading.
        modal.pending_action = None;
        modal.pending_entry_index = None;
    }
    vec![]
}

pub(super) fn handle_mcp_toggle_done(
    app: &mut AppView,
    agent_id: AgentId,
    result: Result<(), String>,
) -> Vec<Effect> {
    let Some(agent) = app.agents.get_mut(&agent_id) else {
        return vec![];
    };
    if let Some(ref mut modal) = agent.extensions_modal
        && let Err(e) = result
    {
        modal.pending_action = None;
        modal.pending_entry_index = None;
        modal.modal_message = Some(crate::views::extensions_modal::ModalMessage::Error(e));
        return vec![];
    }
    let Some(session_id) = agent.session.session_id.clone() else {
        return vec![];
    };
    vec![Effect::FetchMcpsList {
        agent_id,
        session_id,
    }]
}

pub(super) fn handle_marketplace_updates_available(
    app: &mut AppView,
    agent_id: AgentId,
    // (name, installed_ver, latest_ver)
    updates: Vec<(String, String, String)>,
) -> Vec<Effect> {
    if !updates.is_empty()
        && let Some(agent) = app.agents.get_mut(&agent_id)
    {
        let names: Vec<String> = updates
            .iter()
            .map(|(name, old, new)| format!("{name} (v{old} \u{2192} v{new})"))
            .collect();
        let summary = if names.len() <= 2 {
            names.join(", ")
        } else {
            format!("{} and {} more", names[..2].join(", "), names.len() - 2)
        };
        agent
            .scrollback
            .push_block(crate::scrollback::block::RenderBlock::notice(format!(
                "{} Plugins auto-updated: {summary}.",
                crate::glyphs::diamond_filled()
            )));
    }
    vec![]
}

pub(super) fn handle_marketplace_list_loaded(
    app: &mut AppView,
    agent_id: AgentId,
    result: Result<extension_types::MarketplaceListResponse, String>,
) -> Vec<Effect> {
    use crate::views::extensions_modal::TabDataState;
    let mut effects = Vec::new();
    if let Some(agent) = app.agents.get_mut(&agent_id) {
        let session_id = agent.session.session_id.clone();
        let Some(ref mut modal) = agent.extensions_modal else {
            return effects;
        };
        modal.marketplace_fetch_inflight = false;
        if std::mem::take(&mut modal.marketplace_refetch_queued)
            && let Some(session_id) = session_id
        {
            push_marketplace_fetch(modal, &mut effects, agent_id, session_id);
        }
        modal.marketplace_data = match result {
            Ok(mut response) => {
                response.sanitize();
                // Only default to collapsed on first load (when state is Loading).
                // On reloads (after install/uninstall/refresh), preserve the user's
                // expand/collapse choices.
                let is_first_load = matches!(modal.marketplace_data, TabDataState::Loading);
                if is_first_load {
                    // All sources start collapsed, so mark every plugin
                    // index as collapsed using the same index math as
                    // the renderer / navigation helpers.
                    let mut idx = 0usize;
                    for source in &response.sources {
                        idx += 1; // header
                        for _ in &source.plugins {
                            modal.marketplace_collapsed.insert(idx);
                            idx += 1;
                        }
                        // Empty / error sources still occupy at least 1 slot.
                        if source.plugins.is_empty() {
                            idx += 1;
                        }
                    }
                    modal.marketplace_collapsed_sources = (0..response.sources.len()).collect();
                }
                TabDataState::Loaded(response)
            }
            Err(e) => TabDataState::Error(e),
        };
        modal.pending_action = None;
        modal.pending_entry_index = None;
    }
    effects
}

pub(super) fn handle_skills_toggle_done(
    app: &mut AppView,
    agent_id: AgentId,
    result: Result<Vec<tools::implementations::skills::types::SkillInfo>, String>,
) -> Vec<Effect> {
    use crate::views::extensions_modal::TabDataState;
    if let Some(agent) = app.agents.get_mut(&agent_id)
        && let Some(ref mut modal) = agent.extensions_modal
    {
        modal.pending_action = None;
        modal.pending_entry_index = None;
        match result {
            Ok(skills) => {
                let len = skills.len();
                modal.skills_data = TabDataState::Loaded(skills);
                if len > 0 && modal.picker_state.selected >= len {
                    modal.picker_state.selected = len.saturating_sub(1);
                }
            }
            Err(e) => {
                modal.modal_message = Some(crate::views::extensions_modal::ModalMessage::Error(e));
            }
        }
    }
    // The toggle effect already called grow/skills/refresh-baseline
    // which triggers the session to reload skills and push an
    // AvailableCommandsUpdate notification with the updated list.
    vec![]
}

#[cfg(test)]
mod input_diagnostic_tests {
    use super::*;
    use crate::app::agent_view::AgentPane;
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    fn child_app() -> AppView {
        let mut app = crate::app::root::tests::test_app_with_agent();
        let mut child_app = crate::app::root::tests::test_app_with_agent();
        let mut child = child_app.agents.shift_remove(&AgentId(0)).unwrap();
        child.session.session_id = Some(acp::SessionId::new("diagnostic-child"));
        child.force_active_pane(AgentPane::Prompt);
        child.prompt.set_text("child text");
        child.prompt.set_cursor(2);
        let parent = app.agents.get_mut(&AgentId(0)).unwrap();
        parent.session.session_id = Some(acp::SessionId::new("diagnostic-parent"));
        parent.force_active_pane(AgentPane::Prompt);
        parent.prompt.set_text("parent with different length");
        parent.prompt.set_cursor(5);
        parent.subagent_views.insert("child".into(), Box::new(child));
        parent.active_subagent = Some("child".into());
        app
    }

    #[test]
    fn input_diagnostic_child_records_and_dump_resolves_same_surface() {
        let mut app = child_app();
        let _ = app.handle_input(&Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)));
        let parent = &app.agents[&AgentId(0)];
        assert_eq!(parent.input_log.entry_count(), 0);
        let child = &parent.subagent_views["child"];
        assert_eq!(child.input_log.entry_count(), 1);
        let expected_cursor = child.prompt.cursor();
        let expected_len = child.prompt.text().len();
        let target = input_dump_target(&mut app).unwrap();
        let dump = build_input_dump(target, chrono::Utc::now()).unwrap();
        assert_eq!(dump.session_id.as_deref(), Some("diagnostic-child"));
        assert_eq!(dump.textarea_cursor, expected_cursor);
        assert_eq!(dump.textarea_text_len, expected_len);
        assert_eq!(dump.entries[0].key, "Char");
        assert_eq!(dump.entries[0].pane, "Prompt");
        // Resolve the attached surface without claiming Esc reaches it: the
        // dashboard consumes Esc to close its popup before agent input routing.
        app.dashboard = Some(crate::views::dashboard::DashboardState::new());
        app.dashboard.as_mut().unwrap().attached_agent = Some(AgentId(0));
        app.active_view = ActiveView::AgentDashboard;
        let dump = build_input_dump(input_dump_target(&mut app).unwrap(), chrono::Utc::now()).unwrap();
        assert_eq!(dump.session_id.as_deref(), Some("diagnostic-child"));
        app.dashboard.as_mut().unwrap().attached_agent = None;
        assert!(input_dump_target(&mut app).is_none());
    }

    #[test]
    fn input_diagnostic_parent_close_records_parent_without_stale_delta() {
        let mut app = child_app();
        let parent = app.agents.get_mut(&AgentId(0)).unwrap();
        parent.prompt.last_input_delta.cursor_before = Some(999);
        parent.subagent_views.get_mut("child").unwrap().force_active_pane(AgentPane::Scrollback);
        let _ = app.handle_input(&Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        let parent = &app.agents[&AgentId(0)];
        assert!(parent.active_subagent.is_none());
        assert_eq!(parent.input_log.entry_count(), 1);
        assert_eq!(parent.subagent_views["child"].input_log.entry_count(), 0);
        let dump = build_input_dump(parent, chrono::Utc::now()).unwrap();
        assert_eq!(dump.session_id.as_deref(), Some("diagnostic-parent"));
        assert_eq!(dump.entries[0].cursor_before, None);
        assert_eq!(dump.entries[0].key, "Esc");
    }

    #[test]
    fn input_diagnostic_empty_surface_has_no_dump_model() {
        let mut app = child_app();
        assert!(build_input_dump(input_dump_target(&mut app).unwrap(), chrono::Utc::now()).is_none());
    }
}
