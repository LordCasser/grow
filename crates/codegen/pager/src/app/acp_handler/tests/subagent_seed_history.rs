use super::*;
use crate::app::actions::{Action, Effect, TaskResult};
use crate::app::root::dispatch::dispatch;
use crate::app::subagent::{ChildSeedHistoryRequest, read_child_seed_history};
use shell::extensions::notification::{SubagentSpawnDetails, SubagentSpawnHistory};
use shell::session::storage::SessionUpdate;
use std::path::Path;

const CHILD: &str = "resumed-child";
const PARENT: &str = "parent-of-resumed-child";

fn text_update(text: &str, user: bool) -> SessionUpdate {
    let chunk = acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(text)));
    let update = if user {
        acp::SessionUpdate::UserMessageChunk(chunk)
    } else {
        acp::SessionUpdate::AgentMessageChunk(chunk)
    };
    SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
        acp::SessionId::new(CHILD),
        update,
    )))
}

fn seed_fixture(home: &Path, turns: usize, reply: &str) -> u64 {
    let directory = write_session_summary(home, "/tmp", CHILD, Some("subagent"));
    let path = directory.join("summary.json");
    let mut summary: shell::session::persistence::Summary =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    summary.parent_session_id = Some(PARENT.into());
    std::fs::write(path, serde_json::to_vec(&summary).unwrap()).unwrap();
    let mut timeline = chat_state::Timeline::default();
    timeline
        .record(chat_state::TimelineEventKind::SubagentSeed(
            chat_state::SubagentSeedEvent {
                parent_timeline_id: PARENT.into(),
                parent_spawn_seq: 1,
                subagent_id: CHILD.into(),
                security_parent_session_id: PARENT.into(),
                context_source: chat_state::SubagentContextSource::Resumed,
                source_ref: None,
                normalized: false,
            },
        ))
        .unwrap();
    let mut updates = Vec::new();
    for turn in 0..turns {
        updates.push(text_update(&format!("old question {turn}"), true));
        updates.push(text_update(reply, false));
    }
    shell::session::testkit::write_timeline_history_fixture(
        &directory,
        timeline.events(),
        &updates,
    )
    .unwrap();
    let file = directory.join("timeline.jsonl");
    let bytes = file.metadata().unwrap().len();
    // The first prompt can reach disk before the UI's worker opens the file.
    updates.push(text_update("first live prompt", true));
    shell::session::testkit::write_timeline_history_fixture(
        &directory,
        timeline.events(),
        &updates,
    )
    .unwrap();
    bytes
}

fn spawn(app: &mut AppView, history: SubagentSpawnHistory) {
    let mut update = test_subagent_spawned(PARENT, CHILD);
    let GrowSessionUpdate::SubagentSpawned {
        spawn_details,
        resumed_from,
        ..
    } = &mut update
    else {
        unreachable!()
    };
    *resumed_from = Some("source-child".into());
    *spawn_details = Some(SubagentSpawnDetails {
        prompt: "first live prompt".into(),
        child_cwd: "/tmp".into(),
        worktree_path: None,
        history,
    });
    handle(make_ext_session_notification(PARENT, update), app);
}

fn request(app: &mut AppView) -> ChildSeedHistoryRequest {
    std::mem::take(&mut app.pending_effects)
        .into_iter()
        .find_map(|effect| match effect {
            Effect::ReadChildSeedHistory { request, .. } => Some(request),
            _ => None,
        })
        .expect("the spawn must yield the inherited transcript to a worker")
}

fn finish(app: &mut AppView, request: ChildSeedHistoryRequest) {
    let result = read_child_seed_history(&request);
    assert!(
        result.is_ok(),
        "seed history read failed: {:?}",
        result.err()
    );
    dispatch(
        Action::TaskComplete(TaskResult::ChildSeedHistoryLoaded { request, result }),
        app,
    );
}

fn live_prompt(app: &mut AppView) {
    let line: serde_json::Value =
        serde_json::from_str(&child_user_message_line(CHILD, "first live prompt")).unwrap();
    let (response_tx, _) = tokio::sync::oneshot::channel();
    handle(
        AcpClientMessage::SessionNotification(acp_transport::AcpArgs {
            request: serde_json::from_value(line["params"].clone()).unwrap(),
            response_tx,
        }),
        app,
    );
    handle(make_agent_chunk_message(CHILD, "live answer"), app);
}

fn text_rows(app: &AppView, owner: AgentId) -> Vec<(EntryId, String)> {
    app.agents[&owner].subagent_views[CHILD]
        .scrollback
        .iter_entries()
        .filter_map(|(id, entry)| {
            let text = match &entry.block {
                RenderBlock::UserPrompt(prompt) => prompt.text.to_string(),
                RenderBlock::AgentMessage(answer) => answer.text(),
                _ => return None,
            };
            Some((id, text))
        })
        .collect()
}

#[test]
fn resumed_child_seed_finishes_after_live_output_and_terminal() {
    with_replay_disk_home(|home| {
        let bytes = seed_fixture(home, 2, "old answer");
        let owner = AgentId(7);
        let mut app = make_app_with_agent(PARENT);
        let mut root = app.agents.shift_remove(&AgentId(0)).unwrap();
        root.session.id = owner;
        app.agents.insert(owner, root);
        crate::app::root::dispatch::switch_to_agent(
            &mut app,
            owner,
            crate::app::root::dispatch::SwitchCause::New,
        );
        spawn(
            &mut app,
            SubagentSpawnHistory::Inherited {
                seed_history_bytes: bytes,
            },
        );
        let pending = request(&mut app);
        // Occupy the only blocking thread so the real effect cannot finish
        // until the test has delivered the live prompt and terminal event.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        let (release, held) = std::sync::mpsc::channel::<()>();
        let _hold = runtime.spawn_blocking(move || {
            let _ = held.recv();
        });
        let mut tasks = tokio::task::JoinSet::new();
        runtime.block_on(async {
            crate::app::root::effects::execute(
                Effect::ReadChildSeedHistory { request: pending },
                &mut tasks,
                &app.acp_tx,
                &app.cwd,
                &Default::default(),
            );
            tokio::task::yield_now().await;
        });
        assert!(tasks.try_join_next().is_none());
        app.agents
            .get_mut(&owner)
            .unwrap()
            .open_subagent_fullscreen(CHILD.into());
        live_prompt(&mut app);
        handle(
            make_ext_session_notification(PARENT, test_subagent_finished(CHILD)),
            &mut app,
        );
        let tail = text_rows(&app, owner);
        assert_eq!(
            tail.iter().map(|row| row.1.as_str()).collect::<Vec<_>>(),
            ["first live prompt", "live answer"]
        );
        drop(release);
        let result = runtime.block_on(tasks.join_next()).unwrap().unwrap();
        dispatch(Action::TaskComplete(result), &mut app);
        let all = text_rows(&app, owner);
        let scrollback = &app.agents[&owner].subagent_views[CHILD].scrollback;
        assert!(
            scrollback
                .get_by_id(all[1].0)
                .unwrap()
                .has_cached_line_widths(),
            "the first foreground layout should reuse the worker's source measurements"
        );
        assert_eq!(&all[4..], &tail, "selected live rows keep their IDs");
        assert_eq!(
            all[..4]
                .iter()
                .map(|row| row.1.as_str())
                .collect::<Vec<_>>(),
            [
                "old question 0",
                "old answer",
                "old question 1",
                "old answer"
            ]
        );
        assert!(app.agents[&owner].session.subagent_sessions[CHILD].finished);
    });
}

#[test]
fn resumed_child_seed_cannot_install_on_replaced_child() {
    with_replay_disk_home(|home| {
        let bytes = seed_fixture(home, 1, "old answer");
        let mut app = make_app_with_agent(PARENT);
        spawn(
            &mut app,
            SubagentSpawnHistory::Inherited {
                seed_history_bytes: bytes,
            },
        );
        let old = request(&mut app);
        *app.agents.get_mut(&AgentId(0)).unwrap() = make_agent(Some(PARENT));
        spawn(&mut app, SubagentSpawnHistory::Empty);
        live_prompt(&mut app);
        let fresh = text_rows(&app, AgentId(0));
        finish(&mut app, old);
        assert_eq!(text_rows(&app, AgentId(0)), fresh);
    });
}

#[test]
fn resumed_child_seed_during_parent_load_retries_for_the_current_binding() {
    with_replay_disk_home(|home| {
        let bytes = seed_fixture(home, 1, "old answer");
        let mut app = make_app_with_agent(PARENT);
        let owner = AgentId(0);
        app.agents.get_mut(&owner).unwrap().session.loading_replay = true;
        spawn(
            &mut app,
            SubagentSpawnHistory::Inherited {
                seed_history_bytes: bytes,
            },
        );
        let pending = request(&mut app);
        let root = app.agents.get_mut(&owner).unwrap();
        root.session.loading_replay = false;
        root.open_subagent_fullscreen(CHILD.into());
        root.session_binding_epoch += 1;
        live_prompt(&mut app);
        let tail = text_rows(&app, owner);
        assert!(crate::minimal_api::agent_native_history_pending(
            &app.agents[&owner].subagent_views[CHILD]
        ));
        finish(&mut app, pending);
        assert_eq!(text_rows(&app, owner), tail);
        let Some(Effect::ReadChildSeedHistory { request: retry }) =
            crate::app::subagent::take_open_child_seed_retry(&mut app)
        else {
            panic!("visible child should reacquire its seed in the current binding");
        };
        finish(&mut app, retry);
        assert!(!crate::minimal_api::agent_native_history_pending(
            &app.agents[&owner].subagent_views[CHILD]
        ));
        assert_eq!(text_rows(&app, owner)[2..], tail);
    });
}

#[test]
fn resumed_child_seed_read_error_keeps_live_tail_and_retries_on_open() {
    with_replay_disk_home(|home| {
        let bytes = seed_fixture(home, 1, "restored reply");
        let mut app = make_app_with_agent(PARENT);
        let owner = AgentId(0);
        spawn(
            &mut app,
            SubagentSpawnHistory::Inherited {
                seed_history_bytes: bytes,
            },
        );
        let pending = request(&mut app);
        live_prompt(&mut app);
        let tail = text_rows(&app, owner);

        let empty_home = tempfile::tempdir().unwrap();
        let mut missing = pending.clone();
        missing.grow_home = empty_home.path().into();
        let result = read_child_seed_history(&missing);
        assert!(result.is_err());
        dispatch(
            Action::TaskComplete(TaskResult::ChildSeedHistoryLoaded {
                request: pending,
                result,
            }),
            &mut app,
        );
        assert_eq!(text_rows(&app, owner), tail);
        let has_error = |app: &AppView| {
            app.agents[&owner].subagent_views[CHILD]
                .scrollback
                .iter_entries()
                .any(|(_, entry)| {
                    entry.block.immutable_event_id() == Some("child-history-error:resumed-child")
                })
        };
        assert!(has_error(&app));
        assert!(crate::app::subagent::take_open_child_seed_retry(&mut app).is_none());
        app.agents
            .get_mut(&owner)
            .unwrap()
            .open_subagent_fullscreen(CHILD.into());
        let Some(Effect::ReadChildSeedHistory { request: retry }) =
            crate::app::subagent::take_open_child_seed_retry(&mut app)
        else {
            panic!("reopening must retry the immutable prefix");
        };
        assert!(crate::app::subagent::take_open_child_seed_retry(&mut app).is_none());
        finish(&mut app, retry);
        assert!(!has_error(&app));
        let rows = text_rows(&app, owner);
        assert_eq!(rows[1].1, "restored reply");
        assert_eq!(rows[2..], tail);
    });
}

#[test]
#[ignore = "manual foreground latency comparison on a generated long child transcript"]
fn measure_resumed_child_seed_foreground() {
    with_replay_disk_home(|home| {
        let bytes = seed_fixture(home, 1024, &"x".repeat(16 * 1024));
        let mut legacy = make_app_with_agent(PARENT);
        let start = Instant::now();
        spawn(&mut legacy, SubagentSpawnHistory::Durable);
        let synchronous = start.elapsed();
        drop(legacy);
        let mut app = make_app_with_agent(PARENT);
        let start = Instant::now();
        spawn(
            &mut app,
            SubagentSpawnHistory::Inherited {
                seed_history_bytes: bytes,
            },
        );
        let launch = start.elapsed();
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .open_subagent_fullscreen(CHILD.into());
        let pending = request(&mut app);
        let ((result, worker), live) = std::thread::scope(|scope| {
            let reader = scope.spawn(|| {
                let start = Instant::now();
                let result = read_child_seed_history(&pending);
                (result, start.elapsed())
            });
            let start = Instant::now();
            live_prompt(&mut app);
            let live = start.elapsed();
            assert!(
                !reader.is_finished(),
                "live rows should already be routable while the worker runs"
            );
            (reader.join().unwrap(), live)
        });
        assert!(result.is_ok(), "worker failed: {:?}", result.err());
        let install = Instant::now();
        dispatch(
            Action::TaskComplete(TaskResult::ChildSeedHistoryLoaded {
                request: pending,
                result,
            }),
            &mut app,
        );
        let install = install.elapsed();
        let layout = Instant::now();
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .subagent_views
            .get_mut(CHILD)
            .unwrap()
            .scrollback
            .prepare_layout(100, 30);
        let layout = layout.elapsed();
        assert_eq!(text_rows(&app, AgentId(0)).len(), 2 * 1024 + 2);
        eprintln!(
            "seed_bytes={bytes} old_sync={synchronous:?} new_spawn={launch:?} live={live:?} worker={worker:?} install={install:?} layout={layout:?}"
        );
    });
}
