//! End-to-end test for subagent orphan reconciliation on session resume.
//!
//! When a process dies after the parent Timeline commits a subagent spawn but
//! before it commits the terminal, resume must close that open spawn before it
//! emits a finished projection.
//!
//! This test spawns a real `grow agent stdio` process, seeds an orphaned parent
//! spawn fact, resumes the session, and asserts a terminal fact was appended to
//! the same canonical Timeline.
//!
//! Run locally (needs a pre-built binary):
//! ```bash
//! cargo test -p shell --features test-support --test test_subagent_orphan_reconcile -- --ignored
//! ```

use futures::FutureExt;
use std::future::Future;
use std::path::{Path, PathBuf};

use acp_transport::protocol as acp;
use test_support::*;

async fn with_local_set<F, Fut>(f: F)
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    tokio::task::LocalSet::new().run_until(f()).await;
}

/// Find `<home>/sessions/<enc-cwd>/<id>` without depending on the internal cwd
/// encoder: scan the one level of cwd dirs for a child named `<id>`.
fn locate_session_dir(home: &Path, id: &str) -> PathBuf {
    let sessions = home.join("sessions");
    for entry in std::fs::read_dir(&sessions)
        .expect("read sessions dir")
        .flatten()
    {
        let candidate = entry.path().join(id);
        if candidate.is_dir() {
            return candidate;
        }
    }
    panic!(
        "session dir for {id} not found under {}",
        sessions.display()
    );
}

#[tokio::test]
#[ignore] // requires pre-built binary
async fn resume_reconciles_orphaned_running_subagent() {
    with_local_set(|| async {
        let server = MockInferenceServer::start_with_models(vec![
            MockModelEntry::with_agent_type("test-model", "grow-build")
                .with_api_backend("responses"),
        ])
        .await
        .expect("start mock server");
        server.set_response("parent session initialized before child spawn");
        let workdir = git_workdir();

        // Phase 1: create a real session, then take its home so we can seed it.
        let diagnostic_dir = tempfile::tempdir().expect("create diagnostic log directory");
        let diagnostic_log = diagnostic_dir.path().join("grow-orphan-reconcile.log");
        let mut sandbox = TestSandbox::builder().mock_url(server.url()).build();
        sandbox.set_env("GROW_LOG_FILE", &diagnostic_log);
        std::fs::write(
            sandbox.grow_home().join("config.toml"),
            format!(
                r#"[models]
default = "mock/test-model"

[provider.mock.options]
base_url = "{}"
env_key = "GROW_API_KEY"

[provider.mock.models.test-model]
model = "test-model"
api_backend = "responses"
context_window = 200000
agent_type = "grow-build"
"#,
                server.url()
            ),
        )
        .expect("write explicit mock provider config");
        let mut writer =
            GrowStdioClient::spawn_with_sandbox(&server, workdir.workspace(), sandbox).await;
        writer.initialize_with_timeout().await;
        let session_id = writer
            .create_session_with_timeout(workdir.workspace())
            .await;
        let parent_turn = writer
            .prompt_with_timeout(&session_id, "Initialize this parent session.")
            .await
            .expect("complete parent initialization turn");
        assert!(
            matches!(parent_turn.stop_reason, acp::StopReason::EndTurn),
            "parent initialization turn must complete before orphan spawn, got {:?}",
            parent_turn.stop_reason
        );
        let shared_sandbox = writer.take_sandbox();
        drop(writer);

        // Create a real child entity in the same grow_home, then stop its
        // writer. The crash window below is after the parent Spawn is durable
        // but before its display notification/terminal is delivered.
        let grow_home = shared_sandbox.grow_home().to_path_buf();
        let session_dir = locate_session_dir(&grow_home, session_id.0.as_ref());
        let mut child_writer =
            GrowStdioClient::spawn_with_sandbox(&server, workdir.workspace(), shared_sandbox).await;
        child_writer.initialize_with_timeout().await;
        let child_session_id = child_writer
            .create_session_with_timeout(workdir.workspace())
            .await;
        let shared_sandbox = child_writer.take_sandbox();
        drop(child_writer);
        let child_session_dir = locate_session_dir(&grow_home, child_session_id.0.as_ref());

        // The child must be a real member of this parent tree. The normal
        // subagent spawn path writes these lineage fields alongside its seed.
        let summary_path = child_session_dir.join("summary.json");
        let mut summary: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&summary_path).expect("read child summary"))
                .expect("parse child summary");
        summary["parent_session_id"] = serde_json::Value::String(session_id.0.to_string());
        summary["session_kind"] = serde_json::Value::String("subagent".into());
        std::fs::write(
            &summary_path,
            serde_json::to_vec(&summary).expect("serialize child summary"),
        )
        .expect("write child lineage summary");

        let sub_id = "sa-orphan";
        let mut timeline =
            shell::session::storage::load_timeline_by_id_at(session_id.0.as_ref(), &grow_home)
                .expect("read parent Timeline")
                .expect("parent session exists");
        let spawn = timeline
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Spawned(chat_state::SubagentSpawnEvent {
                    subagent_id: sub_id.into(),
                    child_session_id: child_session_id.0.to_string(),
                    origin_tool_call_id: None,
                    security_parent_session_id: session_id.to_string(),
                    parent_step_id: None,
                    subagent_type: "general-purpose".into(),
                    description: "stuck task".into(),
                    prompt: "do work".into(),
                    execution_recipe: serde_json::json!({}),
                    context_source: chat_state::SubagentContextSource::New,
                    source_ref: None,
                    context_normalized: false,
                    resumed_from: None,
                    parent_prompt_id: None,
                    capability_mode: None,
                    permission_mode: None,
                    effective_permission_mode: None,
                    workflow_run_id: None,
                    goal_id: None,
                    goal_definition_revision: None,
                    surface_completion: true,
                    child_cwd: workdir.workspace().to_string_lossy().into_owned(),
                    worktree_path: None,
                    effective_model_id: "test-model".into(),
                    model_transport_key: sampling_types::ModelImageInputKey::new(
                        "test-model",
                        "responses",
                        "test-endpoint",
                    ),
                    reasoning_effort: None,
                }),
            ))
            .expect("record orphan spawn");
        shell::session::testkit::append_timeline_domain_fixture(
            &session_dir.join("timeline.jsonl"),
            std::slice::from_ref(&spawn),
        )
        .expect("append checksummed orphan spawn fixture");

        // The crash window is after the SubagentStart hook completed but
        // before its SubagentSpawned presentation was delivered. Recovery
        // must restore the display projection without running the hook again.
        let hook_cause = chat_state::HookCause::Subagent {
            subagent_id: sub_id.into(),
        };
        let hook_triggered = timeline
            .record(chat_state::TimelineEventKind::Hook(
                chat_state::HookEvent::Triggered {
                    occurrence_id: "orphan-subagent-start".into(),
                    event: chat_state::HookEventType::SubagentStart,
                    gate: chat_state::HookGateKind::Observe,
                    cause: hook_cause.clone(),
                    config_generation: 1,
                    handlers: Vec::new(),
                },
            ))
            .expect("record completed spawn hook trigger");
        let hook_completed = timeline
            .record(chat_state::TimelineEventKind::Hook(
                chat_state::HookEvent::Completed {
                    occurrence_id: "orphan-subagent-start".into(),
                    decision: chat_state::HookAggregateDecision::Observe,
                },
            ))
            .expect("record completed spawn hook outcome");
        shell::session::testkit::append_timeline_domain_fixture(
            &session_dir.join("timeline.jsonl"),
            &[hook_triggered, hook_completed],
        )
        .expect("append completed SubagentStart hook fixture");

        // Write the matching seed after its owning Spawn has been committed.
        let mut child_timeline = shell::session::storage::load_timeline_by_id_at(
            child_session_id.0.as_ref(),
            &grow_home,
        )
        .expect("read child Timeline")
        .expect("child session exists");
        let seed = child_timeline
            .record(chat_state::TimelineEventKind::SubagentSeed(
                chat_state::SubagentSeedEvent {
                    parent_timeline_id: session_id.0.to_string(),
                    parent_spawn_seq: spawn.seq.get(),
                    subagent_id: sub_id.into(),
                    security_parent_session_id: session_id.0.to_string(),
                    context_source: chat_state::SubagentContextSource::New,
                    source_ref: None,
                    normalized: false,
                },
            ))
            .expect("record child seed");
        shell::session::testkit::append_timeline_domain_fixture(
            &child_session_dir.join("timeline.jsonl"),
            std::slice::from_ref(&seed),
        )
        .expect("append checksummed child seed fixture");
        // Phase 2: resume in a fresh process. `load_session` runs the reconcile.
        eprintln!(
            "orphan reconciliation diagnostic log: {}",
            diagnostic_log.display()
        );
        let reader =
            GrowStdioClient::spawn_with_sandbox(&server, workdir.workspace(), shared_sandbox).await;
        reader.initialize_with_timeout().await;
        let load = std::panic::AssertUnwindSafe(
            reader.load_session_with_timeout(&session_id, workdir.workspace()),
        )
        .catch_unwind()
        .await;
        if let Err(payload) = load {
            let timeline_context = match shell::session::storage::load_timeline_by_id_at(
                session_id.0.as_ref(),
                &grow_home,
            ) {
                Ok(Some(timeline)) => timeline
                    .events()
                    .iter()
                    .rev()
                    .take(20)
                    .map(|event| {
                        let summary = format!("seq={} {:?}", event.seq.get(), event.kind);
                        if summary.chars().count() > 400 {
                            format!("{}…", summary.chars().take(400).collect::<String>())
                        } else {
                            summary
                        }
                    })
                    .collect::<Vec<_>>(),
                Ok(None) => vec!["parent Timeline is missing".into()],
                Err(error) => vec![format!("parent Timeline read failed: {error}")],
            };
            let log_context = std::fs::read_to_string(&diagnostic_log)
                .unwrap_or_else(|error| format!("cannot read diagnostic log: {error}"))
                .lines()
                .filter(|line| {
                    line.contains("ERROR")
                        || line.contains("fatal Timeline")
                        || line.contains("turn-boundary")
                        || line.contains("synthetic")
                })
                .rev()
                .take(30)
                .map(|line| {
                    if line.chars().count() > 600 {
                        format!("{}…", line.chars().take(600).collect::<String>())
                    } else {
                        line.to_owned()
                    }
                })
                .collect::<Vec<_>>();
            eprintln!(
                "session/load failed; recent parent Domain events (reverse order):\n{}\nrecent diagnostic errors:\n{}",
                timeline_context.join("\n"),
                log_context.join("\n")
            );
            std::panic::resume_unwind(payload);
        }

        let reread =
            shell::session::storage::load_timeline_by_id_at(session_id.0.as_ref(), &grow_home)
                .expect("read reconciled Timeline")
                .expect("parent session exists");
        let matching_hook_triggers = reread
            .events()
            .iter()
            .filter(|event| matches!(
                &event.kind,
                chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                    event: chat_state::HookEventType::SubagentStart,
                    cause: chat_state::HookCause::Subagent { subagent_id },
                    ..
                }) if subagent_id == sub_id
            ))
            .count();
        let matching_hook_completions = reread
            .events()
            .iter()
            .filter(|event| matches!(
                &event.kind,
                chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Completed {
                    occurrence_id,
                    decision: chat_state::HookAggregateDecision::Observe,
                }) if occurrence_id == "orphan-subagent-start"
            ))
            .count();
        assert_eq!(matching_hook_triggers, 1, "restore must not rerun SubagentStart");
        assert_eq!(matching_hook_completions, 1, "original hook completion remains singular");
        assert!(
            reread.events().iter().any(|event| matches!(
                &event.kind,
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(end))
                    if end.subagent_id == sub_id
                        && matches!(
                            end.outcome,
                            chat_state::SubagentOutcome::Failed
                                | chat_state::SubagentOutcome::Cancelled
                        )
            )),
            "resume must close the orphaned spawn in Timeline\nstderr:\n{}",
            stderr_tail(&reader.stderr(), 2000),
        );
        let parent_terminal =
            reread
                .events()
                .iter()
                .find_map(|event| match &event.kind {
                    chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(
                        end,
                    )) if end.subagent_id == sub_id => Some(end),
                    _ => None,
                })
                .expect("recovery writes a parent terminal");
        let result_ref = parent_terminal
            .result_ref
            .as_ref()
            .expect("parent terminal references the durable child result");
        assert_eq!(result_ref.timeline_id, child_session_id.0.as_ref());
        assert_eq!(result_ref.first_seq, result_ref.last_seq);
        let child_timeline = shell::session::storage::load_timeline_by_id_at(
            child_session_id.0.as_ref(),
            &grow_home,
        )
        .expect("read recovered child Timeline")
        .expect("child session remains available");
        let child_result = child_timeline
            .events()
            .iter()
            .find(|event| event.seq.get() == result_ref.first_seq)
            .expect("parent result reference resolves to a child event");
        assert!(
            matches!(
                &child_result.kind,
                chat_state::TimelineEventKind::SubagentResult(result)
                    if result.subagent_id == sub_id
                        && result.outcome == parent_terminal.outcome
                        && matches!(
                            result.outcome,
                            chat_state::SubagentOutcome::Failed
                                | chat_state::SubagentOutcome::Cancelled
                        )
            ),
            "parent result_ref must point to the matching canonical child result"
        );
    })
    .await;
}
