//! Tests for the block viewer and transcript dispatchers.

use super::*;

fn make_test_png(width: u32, height: u32) -> Vec<u8> {
    use image::{ImageBuffer, Rgba};
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(width, height, Rgba([128, 64, 32, 255]));
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .unwrap();
    buf
}

fn make_test_jpeg(width: u32, height: u32) -> Vec<u8> {
    use image::{ImageBuffer, Rgb};
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(width, height, Rgb([128, 64, 32]));
    let mut buf = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut buf),
        image::ImageFormat::Jpeg,
    )
    .unwrap();
    buf
}

#[test]
fn open_block_viewer_on_group_header_toggles_group() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    {
        let agent = app.agents.get_mut(&id).unwrap();
        let mut appearance = crate::appearance::AppearanceConfig::default();
        appearance.scrollback.display.group_max_visible = 3;
        agent.scrollback.set_appearance(appearance);
        for i in 0..6 {
            agent
                .scrollback
                .push_block(crate::scrollback::block::RenderBlock::tool_call(
                    format!("Tool{i}"),
                    "info",
                    true,
                ));
        }
        agent.scrollback.prepare_layout(80, 40);
        agent.scrollback.set_selected(Some(0));
        assert!(agent.scrollback.is_selected_group_header());
    }

    // Enter on the "N more" header expands the group instead of opening
    // the hidden first entry in the block viewer.
    dispatch(Action::OpenBlockViewer, &mut app);
    {
        let agent = app.agents.get_mut(&id).unwrap();
        assert!(
            agent.block_viewer.is_none(),
            "viewer must not open on a group header"
        );
        assert_eq!(
            agent.scrollback.selected(),
            None,
            "expanding a group clears the selection"
        );
        agent.scrollback.prepare_layout(80, 40);
        agent.scrollback.set_selected(Some(0));
        assert_eq!(
            agent.scrollback.selected_group_header_fold_label(),
            Some("collapse"),
            "entry 0 should now be the expanded group's collapse header"
        );
    }

    // Enter on the collapse header collapses the group back.
    dispatch(Action::OpenBlockViewer, &mut app);
    {
        let agent = app.agents.get_mut(&id).unwrap();
        assert!(agent.block_viewer.is_none());
        agent.scrollback.prepare_layout(80, 40);
        assert_eq!(
            agent.scrollback.selected_group_header_fold_label(),
            Some("expand"),
            "group should be truncated again ('N more' header)"
        );
    }
}

#[test]
fn open_block_viewer_opens_grep_search_block() {
    use crate::scrollback::blocks::{SearchFileMatch, SearchLineMatch};

    let mut app = test_app_with_agent();
    let id = AgentId(0);
    let agent = app.agents.get_mut(&id).unwrap();
    agent.scrollback.push_block(RenderBlock::search(
        "fn main",
        1,
        vec![SearchFileMatch {
            path: "src/main.rs".into(),
            matches: vec![SearchLineMatch {
                line_number: 1,
                content: "fn main() {}".into(),
            }],
        }],
    ));
    agent.scrollback.set_selected(Some(0));

    let entry = agent.scrollback.entry(0).unwrap();
    assert!(entry.block.has_normal_fullscreen_viewer());

    let effects = dispatch(Action::OpenBlockViewer, &mut app);
    assert!(effects.is_empty());
    let agent = app.agents.get(&id).unwrap();
    assert!(agent.block_viewer.is_some());
    assert_eq!(
        agent.block_viewer.as_ref().unwrap().kind,
        crate::views::block_viewer::ViewerKind::Grep
    );
}

#[test]
fn open_block_viewer_opens_list_dir_block() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    let agent = app.agents.get_mut(&id).unwrap();
    agent
        .scrollback
        .push_block(RenderBlock::list_dir_with_output("/tmp", "a.txt\nb.txt"));
    agent.scrollback.set_selected(Some(0));

    let entry = agent.scrollback.entry(0).unwrap();
    assert!(entry.block.has_normal_fullscreen_viewer());

    let effects = dispatch(Action::OpenBlockViewer, &mut app);
    assert!(effects.is_empty());
    let agent = app.agents.get(&id).unwrap();
    assert!(agent.block_viewer.is_some());
    assert_eq!(
        agent.block_viewer.as_ref().unwrap().kind,
        crate::views::block_viewer::ViewerKind::PlainText
    );
}

#[test]
fn enter_opens_command_coordination_notice_and_failed_tool_details() {
    use crate::scrollback::blocks::{
        NoticeCategory, NoticeTone, OtherToolCallBlock, ToolCallBlock,
    };
    let blocks = [
        RenderBlock::terminal_notice(
            "command-event",
            NoticeTone::Success,
            NoticeCategory::Command,
            "Goal cleared.",
            Some("Command: /goal clear".into()),
        ),
        RenderBlock::terminal_notice(
            "event",
            NoticeTone::Error,
            NoticeCategory::Coordination,
            "Inquiry failed",
            Some("Inquiry ID: id\nnot_found".into()),
        ),
        RenderBlock::ToolCall(ToolCallBlock::Other(
            OtherToolCallBlock::new("ask_session", "target").with_error("not_found"),
        )),
    ];
    for block in blocks {
        let mut app = test_app_with_agent();
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        assert!(block.has_normal_fullscreen_viewer());
        agent.scrollback.push_block(block);
        agent.scrollback.set_selected(Some(0));
        dispatch(Action::OpenBlockViewer, &mut app);
        assert!(app.agents.get(&AgentId(0)).unwrap().block_viewer.is_some());
    }
}

#[test]
fn open_block_viewer_prefers_markdown_viewer_over_image_refs() {
    use crate::terminal::image::{GraphicsProtocol, set_protocol_for_test};

    let mut app = test_app_with_agent();
    let id = AgentId(0);
    let dir = tempfile::tempdir().unwrap();
    let image_path = dir.path().join("referenced.png");
    std::fs::write(&image_path, make_test_png(20, 10)).unwrap();

    let agent = app.agents.get_mut(&id).unwrap();
    agent
        .scrollback
        .push_block(RenderBlock::agent_message(format!(
            "Here is an image: ![ref]({})",
            image_path.display()
        )));
    agent.scrollback.set_selected(Some(0));

    // Need a graphics protocol so the top-level media guard doesn't
    // short-circuit before reaching the block viewer.
    let _guard = set_protocol_for_test(GraphicsProtocol::Kitty);
    let effects = dispatch(Action::OpenBlockViewer, &mut app);

    assert!(effects.is_empty());
    let agent = app.agents.get(&id).unwrap();
    assert!(agent.block_viewer.is_some());
    assert!(agent.image_viewer.is_none());
}

#[test]
fn open_block_viewer_uses_markdown_viewer_for_agent_message_with_image_ref() {
    use crate::terminal::image::{GraphicsProtocol, set_protocol_for_test};

    let mut app = test_app_with_agent();
    let id = AgentId(0);
    let dir = tempfile::tempdir().unwrap();
    let jpg_path = dir.path().join("generated.jpg");
    std::fs::write(&jpg_path, make_test_jpeg(20, 10)).unwrap();

    let agent = app.agents.get_mut(&id).unwrap();
    agent
        .scrollback
        .push_block(RenderBlock::agent_message(format!(
            "![generated]({})",
            jpg_path.display()
        )));
    agent.scrollback.set_selected(Some(0));

    let _guard = set_protocol_for_test(GraphicsProtocol::Kitty);
    let effects = dispatch(Action::OpenBlockViewer, &mut app);

    assert!(effects.is_empty());
    let agent = app.agents.get(&id).unwrap();
    // Agent messages with image refs now open the normal markdown viewer
    // (inline media rendering moved to the tool call block).
    assert!(agent.block_viewer.is_some());
}

#[test]
fn open_block_viewer_opens_image_only_blocks_natively() {
    use crate::terminal::image::{GraphicsProtocol, set_protocol_for_test};

    let mut app = test_app_with_agent();
    let id = AgentId(0);
    let dir = tempfile::tempdir().unwrap();
    let image_path = dir.path().join("referenced.png");
    std::fs::write(&image_path, make_test_png(20, 10)).unwrap();

    let agent = app.agents.get_mut(&id).unwrap();
    agent
        .scrollback
        .push_block(RenderBlock::ToolCall(ToolCallBlock::Other(
            crate::scrollback::blocks::OtherToolCallBlock::new("image_tool", "saved image")
                .with_output(format!("Saved image: {}", image_path.display())),
        )));
    agent.scrollback.set_selected(Some(0));

    let entry = agent.scrollback.entry(0).unwrap();
    assert!(entry.block.supports_fullscreen());
    assert!(!entry.block.has_normal_fullscreen_viewer());

    // Pretend the host terminal speaks Kitty graphics so the media
    // short-circuit guard (`guard_image_support`) doesn't fire and the
    // dispatch reaches the image branch, which opens the file natively
    // rather than an in-app viewer.
    let _guard = set_protocol_for_test(GraphicsProtocol::Kitty);
    let effects = dispatch(Action::OpenBlockViewer, &mut app);

    // A tool-produced local image opens in the OS-native viewer
    // (fire-and-forget), so no in-app viewer is shown.
    assert!(effects.is_empty());
    let agent = app.agents.get(&id).unwrap();
    assert!(agent.block_viewer.is_none());
    assert!(agent.image_viewer.is_none());
}

// -- Plugins tab: group-collapse seeding on PluginsListLoaded --------------

fn plugins_list_response() -> extension_types::PluginsListResponse {
    use crate::views::extensions_modal::test_plugin_info;
    extension_types::PluginsListResponse {
        plugins: vec![
            test_plugin_info("user-tool", extension_types::PluginOrigin::UserGrow),
            test_plugin_info("custom-tool", extension_types::PluginOrigin::ConfigPath),
        ],
    }
}

fn open_plugins_modal(app: &mut AppView, id: AgentId) {
    app.agents.get_mut(&id).unwrap().extensions_modal =
        Some(crate::views::extensions_modal::ExtensionsModalState::new(
            crate::views::extensions_modal::ExtensionsTab::Plugins,
        ));
}

fn deliver_plugins_list(app: &mut AppView, id: AgentId) {
    dispatch(
        Action::TaskComplete(TaskResult::PluginsListLoaded {
            agent_id: id,
            result: Ok(plugins_list_response()),
        }),
        app,
    );
}

fn plugins_collapsed_keys(app: &AppView, id: AgentId) -> Vec<String> {
    let modal = app.agents[&id].extensions_modal.as_ref().unwrap();
    let mut keys: Vec<String> = modal.plugins_collapsed_groups.iter().cloned().collect();
    keys.sort();
    keys
}

#[test]
fn plugins_list_loaded_seeds_all_groups_collapsed_on_first_load() {
    use crate::views::extensions_modal::TabDataState;

    let mut app = test_app_with_agent();
    let id = AgentId(0);
    open_plugins_modal(&mut app, id);

    deliver_plugins_list(&mut app, id);

    assert_eq!(
        plugins_collapsed_keys(&app, id),
        vec!["origin:config".to_string(), "origin:user".to_string()]
    );
    let modal = app.agents[&id].extensions_modal.as_ref().unwrap();
    match &modal.plugins_data {
        TabDataState::Loaded(response) => assert_eq!(response.plugins.len(), 2),
        other => panic!("expected Loaded plugins data, got {other:?}"),
    }
}

#[test]
fn plugins_list_delivery_seeds_once_then_always_preserves() {
    use crate::views::extensions_modal::TabDataState;

    let mut app = test_app_with_agent();
    let id = AgentId(0);
    open_plugins_modal(&mut app, id);
    deliver_plugins_list(&mut app, id);

    // User expands a group, then the post-action refetch arrives.
    app.agents
        .get_mut(&id)
        .unwrap()
        .extensions_modal
        .as_mut()
        .unwrap()
        .plugins_collapsed_groups
        .remove("origin:user");
    deliver_plugins_list(&mut app, id);

    assert_eq!(
        plugins_collapsed_keys(&app, id),
        vec!["origin:config".to_string()],
        "post-action refetch must not re-collapse an expanded group"
    );

    // Reload sets Loading, but seeding is once-per-modal: still preserves.
    app.agents
        .get_mut(&id)
        .unwrap()
        .extensions_modal
        .as_mut()
        .unwrap()
        .plugins_data = TabDataState::Loading;
    deliver_plugins_list(&mut app, id);

    assert_eq!(
        plugins_collapsed_keys(&app, id),
        vec!["origin:config".to_string()],
        "reload must not re-collapse groups the user expanded"
    );
}

#[test]
fn open_block_viewer_skips_image_viewer_when_no_graphics() {
    use crate::terminal::image::{GraphicsProtocol, set_protocol_for_test};

    let mut app = test_app_with_agent();
    let id = AgentId(0);
    let dir = tempfile::tempdir().unwrap();
    let image_path = dir.path().join("referenced.png");
    std::fs::write(&image_path, make_test_png(20, 10)).unwrap();

    let agent = app.agents.get_mut(&id).unwrap();
    agent
        .scrollback
        .push_block(RenderBlock::ToolCall(ToolCallBlock::Other(
            crate::scrollback::blocks::OtherToolCallBlock::new("image_tool", "saved image")
                .with_output(format!("Saved image: {}", image_path.display())),
        )));
    agent.scrollback.set_selected(Some(0));

    // Terminal has no inline-image protocol (e.g. Windows / ConPTY).
    // The dispatch should refuse to open the image-viewer modal and
    // surface the situation via a toast instead.
    let _guard = set_protocol_for_test(GraphicsProtocol::None);
    let effects = dispatch(Action::OpenBlockViewer, &mut app);

    assert!(effects.is_empty());
    let agent = app.agents.get(&id).unwrap();
    assert!(agent.block_viewer.is_none());
    assert!(
        agent.image_viewer.is_none(),
        "image_viewer modal should not open on terminals without a graphics protocol"
    );
}

#[test]
fn export_file_uses_session_cwd_and_preserves_absolute_targets() {
    let session_dir = tempfile::tempdir().unwrap();
    // Capture the buggy process-relative destination inside an owned temp dir.
    let process_relative = tempfile::tempdir_in(".").unwrap();
    let relative = std::path::PathBuf::from(process_relative.path().file_name().unwrap())
        .join("nested/transcript.md");
    assert!(relative.is_relative());
    let mut app = test_app_with_agent();
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    agent.session.cwd = session_dir.path().to_path_buf();
    agent.scrollback.push_block(RenderBlock::user_prompt("Export this conversation"));
    dispatch_file_action(Action::ExportConversation { file_path: Some(relative.clone()) }, &mut app);
    let expected = session_dir.path().join(&relative);
    assert!(expected.is_file(), "export must land at {}", expected.display());
    assert!(!relative.exists(), "must not write under process cwd");
    assert_eq!(std::fs::read_to_string(expected).unwrap(), "## User\n\nExport this conversation");

    let absolute = process_relative.path().canonicalize().unwrap().join("absolute.md");
    dispatch_file_action(Action::ExportConversation { file_path: Some(absolute.clone()) }, &mut app);
    assert_eq!(std::fs::read_to_string(absolute).unwrap(), "## User\n\nExport this conversation");
}

#[test]
fn export_waits_until_history_replay_completes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("transcript.md");
    let mut app = test_app_with_agent();
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    agent.session.loading_replay = true;
    agent.scrollback.push_block(RenderBlock::user_prompt("First part"));
    dispatch_file_action(Action::ExportConversation { file_path: Some(path.clone()) }, &mut app);
    assert!(!path.exists(), "partial history must not be exported");
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    agent.scrollback.push_block(RenderBlock::user_prompt("Remaining history"));
    agent.session.loading_replay = false;
    dispatch_file_action(Action::ExportConversation { file_path: Some(path.clone()) }, &mut app);
    let text = std::fs::read_to_string(path).unwrap();
    assert_eq!(text, "## User\n\nFirst part\n\n## User\n\nRemaining history");
}

#[test]
fn copy_file_uses_session_cwd_and_keeps_private_permissions() {
    let session_dir = tempfile::tempdir().unwrap();
    let process_dir = tempfile::tempdir_in(".").unwrap();
    let relative = std::path::PathBuf::from(process_dir.path().file_name().unwrap()).join("reply.txt");
    let mut app = test_app_with_agent();
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    agent.session.cwd = session_dir.path().to_path_buf();
    agent.scrollback.push_block(RenderBlock::agent_message("Copy this reply"));
    dispatch_file_action(Action::CopyAssistantMessage { n: 1, file_path: Some(relative.clone()) }, &mut app);
    let expected = session_dir.path().join(relative.clone());
    assert!(expected.is_file(), "copy must use session cwd");
    assert!(!relative.exists(), "must not write under process cwd");
    assert_eq!(std::fs::read_to_string(&expected).unwrap(), "Copy this reply");
    let absolute = process_dir.path().join("absolute.txt");
    dispatch_file_action(Action::CopyAssistantMessage { n: 1, file_path: Some(absolute.clone()) }, &mut app);
    assert_eq!(std::fs::read_to_string(&absolute).unwrap(), "Copy this reply");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [expected, absolute] {
            assert_eq!(std::fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}

#[test]
fn copy_selection_uses_assistant_order_and_rejects_invalid_indices() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = test_app_with_agent();
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    for text in ["oldest", "middle", "latest"] {
        agent.scrollback.push_block(RenderBlock::agent_message(text));
        agent.scrollback.push_block(RenderBlock::user_prompt("not an assistant message"));
    }
    for (n, expected) in [(1, "latest"), (2, "middle"), (3, "oldest")] {
        let path = dir.path().join(format!("{n}.txt"));
        dispatch_file_action(Action::CopyAssistantMessage { n, file_path: Some(path.clone()) }, &mut app);
        assert_eq!(std::fs::read_to_string(path).unwrap(), expected);
    }
    for n in [0, 4, usize::MAX] {
        let path = dir.path().join(format!("invalid-{n}.txt"));
        dispatch_file_action(Action::CopyAssistantMessage { n, file_path: Some(path.clone()) }, &mut app);
        assert!(!path.exists(), "invalid index must not write a file");
    }
}

fn complete_file_effect(effect: Effect, app: &mut AppView) -> Vec<Effect> {
    let Effect::WriteTranscriptFile { id, request } = effect else { panic!("expected file effect") };
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let result = runtime.block_on(crate::app::transcript_file_writes::execute(id, request));
    dispatch(Action::TaskComplete(result), app)
}
fn dispatch_file_action(action: Action, app: &mut AppView) {
    let mut effects = dispatch(action, app);
    while let Some(effect) = effects.pop() {
        effects.extend(complete_file_effect(effect, app));
    }
}

#[test]
fn file_writes_are_deferred_ordered_and_ignore_rebound_session_feedback() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("transcript.md");
    let mut app = test_app_with_agent();
    app.agents.get_mut(&AgentId(0)).unwrap().scrollback.push_block(RenderBlock::user_prompt("first"));
    let mut first = dispatch(Action::ExportConversation { file_path: Some(path.clone()) }, &mut app);
    assert!(!path.exists(), "dispatcher must not write files");
    app.agents.get_mut(&AgentId(0)).unwrap().scrollback.push_block(RenderBlock::agent_message("second"));
    assert!(dispatch(Action::CopyAssistantMessage { n: 1, file_path: Some(path.clone()) }, &mut app).is_empty());
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    agent.session.session_id = Some(acp::SessionId::from("different-session".to_string()));
    let visible = agent.scrollback.len();
    let mut second = complete_file_effect(first.pop().unwrap(), &mut app);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "## User\n\nfirst");
    assert_eq!(app.agents[&AgentId(0)].scrollback.len(), visible);
    assert_eq!(second.len(), 1);
    assert!(complete_file_effect(second.pop().unwrap(), &mut app).is_empty());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "second");
    assert_eq!(app.agents[&AgentId(0)].scrollback.len(), visible);
}

#[test]
fn failed_file_write_advances_queue_after_origin_is_removed() {
    let dir = tempfile::tempdir().unwrap();
    let good = dir.path().join("good.md");
    let mut app = test_app_with_agent();
    app.agents.get_mut(&AgentId(0)).unwrap().scrollback.push_block(RenderBlock::user_prompt("content"));
    let mut first = dispatch(Action::ExportConversation { file_path: Some(dir.path().to_path_buf()) }, &mut app);
    assert!(dispatch(Action::ExportConversation { file_path: Some(good.clone()) }, &mut app).is_empty());
    app.agents.shift_remove(&AgentId(0));
    let mut next = complete_file_effect(first.pop().unwrap(), &mut app);
    assert_eq!(next.len(), 1);
    assert!(complete_file_effect(next.pop().unwrap(), &mut app).is_empty());
    assert_eq!(std::fs::read_to_string(good).unwrap(), "## User\n\ncontent");
    assert!(dir.path().is_dir());
}

#[test]
fn full_file_write_queue_rejects_new_request_without_dropping_accepted_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = test_app_with_agent();
    app.agents.get_mut(&AgentId(0)).unwrap().scrollback.push_block(RenderBlock::user_prompt("content"));
    let mut effects = dispatch(Action::ExportConversation { file_path: Some(dir.path().join("0.md")) }, &mut app);
    for i in 1..=8 {
        assert!(dispatch(Action::ExportConversation { file_path: Some(dir.path().join(format!("{i}.md"))) }, &mut app).is_empty());
    }
    let rejected = dir.path().join("rejected.md");
    assert!(dispatch(Action::ExportConversation { file_path: Some(rejected.clone()) }, &mut app).is_empty());
    let agent = &app.agents[&AgentId(0)];
    let RenderBlock::Notice(notice) = &agent.scrollback.entry(agent.scrollback.len() - 1).unwrap().block else { panic!() };
    assert!(notice.text.contains("Too many file writes pending"));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    while let Some(effect) = effects.pop() {
        effects.extend(complete_file_effect(effect, &mut app));
    }
    assert!(!rejected.exists());
    for i in 0..=8 {
        assert_eq!(std::fs::read_to_string(dir.path().join(format!("{i}.md"))).unwrap(), "## User\n\ncontent");
    }
    let agent = &app.agents[&AgentId(0)];
    let RenderBlock::Notice(notice) = &agent.scrollback.entry(agent.scrollback.len() - 1).unwrap().block else { panic!() };
    assert!(notice.text.contains("Conversation exported to"));
}

#[test]
fn pager_transcript_files_are_private_and_owned_across_replacement() {
    let mut app = test_app_with_agent();
    app.screen_mode = crate::app::ScreenMode::Fullscreen;
    assert!(!app.screen_mode.is_minimal());
    app.agents.get_mut(&AgentId(0)).unwrap().scrollback.push_block(
        RenderBlock::user_prompt("private transcript"),
    );
    dispatch(Action::OpenTranscriptPager, &mut app);
    let markdown = app.pending_pager.as_ref().unwrap().path.to_path_buf();
    assert_eq!(markdown.extension().unwrap(), "md");
    assert!(std::fs::read_to_string(&markdown).unwrap().contains("private transcript"));
    assert!(!app.pending_pager.as_ref().unwrap().ansi);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&markdown).unwrap().permissions().mode() & 0o777, 0o600);
    }
    crate::minimal_api::app_set_pending_pager(&mut app, AgentId(0), "\x1b[31mprivate ANSI\x1b[0m", true).unwrap();
    assert!(!markdown.exists(), "replacing a queued request releases its file");
    let ansi = app.pending_pager.as_ref().unwrap().path.to_path_buf();
    assert_eq!(ansi.extension().unwrap(), "ansi");
    assert_eq!(std::fs::read_to_string(&ansi).unwrap(), "\x1b[31mprivate ANSI\x1b[0m");
    assert!(app.pending_pager.as_ref().unwrap().ansi);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&ansi).unwrap().permissions().mode() & 0o777, 0o600);
    }
    drop(app);
    assert!(!ansi.exists(), "dropping the app releases the pending snapshot");
}

#[test]
fn minimal_transcript_waits_for_owner_reload() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    app.agents.get_mut(&id).unwrap().scrollback.push_block(RenderBlock::user_prompt("before reload"));
    crate::minimal_api::request_minimal_transcript(&mut app);
    app.agents.get_mut(&id).unwrap().begin_session_reload(1);
    assert!(crate::minimal_api::take_minimal_transcript(&mut app).is_none(),
        "a pump must not consume entry IDs while the owner transcript is stashed");
    assert!(crate::minimal_api::minimal_transcript_progress(&app).is_some());
}

#[test]
fn minimal_transcript_restarts_from_final_reload_state_even_between_frames() {
    for (success, full_replay) in [(true, true), (true, false), (false, true)] {
        for pump_during_reload in [true, false] {
            let mut app = test_app_with_agent();
            let id = AgentId(0);
            for text in ["old first", "old second"] {
                app.agents.get_mut(&id).unwrap().scrollback.push_block(RenderBlock::user_prompt(text));
            }
            crate::minimal_api::request_minimal_transcript(&mut app);
            let mut build = crate::minimal_api::take_minimal_transcript(&mut app).unwrap();
            build.next = 1;
            build.out = "rendered old prefix".to_string();
            crate::minimal_api::set_minimal_transcript(&mut app, Some(build));
            app.agents.get_mut(&id).unwrap().begin_session_reload(8);
            crate::minimal_api::restart_minimal_transcript_after_reload(&mut app, &[id]);
            if pump_during_reload {
                assert!(crate::minimal_api::take_minimal_transcript(&mut app).is_none());
            }
            let agent = app.agents.get_mut(&id).unwrap();
            agent.scrollback.push_block(RenderBlock::user_prompt("reload text"));
            if full_replay {
                agent.mark_reload_replay_seen();
            }
            assert!(agent.finish_session_reload(8, success));
            let expected: Vec<_> = (0..agent.scrollback.len())
                .map(|i| agent.scrollback.entry(i).unwrap().id).collect();
            assert!(!expected.is_empty());
            let rebuilt = crate::minimal_api::take_minimal_transcript(&mut app).unwrap();
            assert_eq!(rebuilt.agent, id);
            assert_eq!(rebuilt.ids, expected);
            assert_eq!(rebuilt.next, 0);
            assert!(rebuilt.out.is_empty());
            assert!(!rebuilt.restart_after_reload);
        }
    }
}

#[test]
fn minimal_transcript_new_request_waits_and_unrelated_reload_preserves_build() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    app.agents.get_mut(&id).unwrap().scrollback.push_block(RenderBlock::user_prompt("retained"));
    app.agents.get_mut(&id).unwrap().begin_session_reload(1);
    crate::minimal_api::request_minimal_transcript(&mut app);
    assert!(crate::minimal_api::minimal_transcript_progress(&app).is_some());
    assert!(crate::minimal_api::take_minimal_transcript(&mut app).is_none());
    assert!(app.agents.get_mut(&id).unwrap().finish_session_reload(1, false));
    let mut build = crate::minimal_api::take_minimal_transcript(&mut app).unwrap();
    assert!(!build.ids.is_empty());
    let expected_ids = build.ids.clone();
    build.next = 1;
    build.out = "keep prefix".to_string();
    crate::minimal_api::set_minimal_transcript(&mut app, Some(build));
    app.active_view = ActiveView::Agent(AgentId(999));
    crate::minimal_api::restart_minimal_transcript_after_reload(&mut app, &[AgentId(999)]);
    let unchanged = crate::minimal_api::take_minimal_transcript(&mut app).unwrap();
    assert_eq!(unchanged.agent, id);
    assert_eq!(unchanged.ids, expected_ids);
    assert_eq!(unchanged.next, 1);
    assert_eq!(unchanged.out, "keep prefix");
}

#[test]
fn transcript_initial_history_load_does_not_publish_partial_content() {
    for mode in [crate::app::ScreenMode::Inline, crate::app::ScreenMode::Fullscreen, crate::app::ScreenMode::Minimal] {
        let mut app = test_app_with_agent();
        app.screen_mode = mode;
        let id = AgentId(0);
        let agent = app.agents.get_mut(&id).unwrap();
        agent.session.loading_replay = true;
        agent.scrollback.push_block(RenderBlock::user_prompt("first part"));
        dispatch(Action::OpenTranscriptPager, &mut app);
        assert!(app.pending_pager.is_none(), "{mode:?} must not write partial history");
        assert!(crate::minimal_api::minimal_transcript_progress(&app).is_none());
        let agent = app.agents.get_mut(&id).unwrap();
        assert!(matches!(&agent.scrollback.last().unwrap().block,
            RenderBlock::Notice(notice) if notice.text.contains("history is still loading")));
        agent.session.loading_replay = false;
        agent.scrollback.push_block(RenderBlock::user_prompt("final part"));
        let expected_ids: Vec<_> = (0..agent.scrollback.len())
            .map(|i| agent.scrollback.entry(i).unwrap().id).collect();
        dispatch(Action::OpenTranscriptPager, &mut app);
        if mode.is_minimal() {
            let build = crate::minimal_api::take_minimal_transcript(&mut app).unwrap();
            assert_eq!(build.ids, expected_ids);
        } else {
            let content = std::fs::read_to_string(&app.pending_pager.as_ref().unwrap().path).unwrap();
            assert!(content.contains("first part"));
            assert!(content.contains("final part"));
        }
    }
}

#[test]
fn pager_child_feedback_preserves_origin_and_rejects_rebound_session() {
    let mut app = test_app_with_agent();
    app.screen_mode = crate::app::ScreenMode::Fullscreen;
    let mut child = crate::test_util::make_agent_view(Some("pager-child"), "/tmp");
    child.session.id = AgentId(99);
    child.scrollback.push_block(RenderBlock::user_prompt("child-only-transcript"));
    let root = app.agents.get_mut(&AgentId(0)).unwrap();
    root.insert_subagent_view("pager-child".into(), Box::new(child));
    root.active_subagent = Some("pager-child".into());
    dispatch(Action::OpenTranscriptPager, &mut app);
    let request = app.pending_pager.take().unwrap();
    assert!(std::fs::read_to_string(&request.path).unwrap().contains("child-only-transcript"));
    app.agents.get_mut(&AgentId(0)).unwrap().active_subagent = None;
    let root_len = app.agents[&AgentId(0)].scrollback.len();
    assert!(!request.report(&mut app, "child-pager-failure"));
    assert_eq!(app.agents[&AgentId(0)].scrollback.len(), root_len);
    let child = app.agents.get_mut(&AgentId(0)).unwrap().subagent_views.get_mut("pager-child").unwrap();
    assert!(matches!(&child.scrollback.last().unwrap().block, RenderBlock::Notice(block) if block.text == "child-pager-failure"));
    let child_len = child.scrollback.len();
    child.session.session_id = Some("replacement-session".into());
    assert!(!request.report(&mut app, "stale-pager-failure"));
    assert_eq!(app.agents[&AgentId(0)].subagent_views["pager-child"].scrollback.len(), child_len);
    app.agents.get_mut(&AgentId(0)).unwrap().subagent_views.remove("pager-child");
    assert!(!request.report(&mut app, "removed-child-failure"));
    assert_eq!(app.agents[&AgentId(0)].scrollback.len(), root_len);
}

#[test]
fn minimal_pager_capture_uses_build_owner_after_tab_switch() {
    let mut app = test_app_with_agent();
    let other = AgentId(1);
    let mut agent = crate::test_util::make_agent_view(Some("other-tab"), "/tmp");
    agent.session.id = other;
    app.agents.insert(other, agent);
    app.active_view = ActiveView::Agent(other);
    crate::minimal_api::app_set_pending_pager(&mut app, AgentId(0), "original-build", true).unwrap();
    let request = app.pending_pager.take().unwrap();
    assert!(request.ansi);
    let other_len = app.agents[&other].scrollback.len();
    assert!(!request.report(&mut app, "build-owner-notice"));
    assert_eq!(app.agents[&other].scrollback.len(), other_len);
    assert!(matches!(&app.agents[&AgentId(0)].scrollback.last().unwrap().block, RenderBlock::Notice(block) if block.text == "build-owner-notice"));
    app.agents.shift_remove(&AgentId(0));
    assert!(!request.report(&mut app, "removed-root-notice"));
    assert_eq!(app.agents[&other].scrollback.len(), other_len);
}
