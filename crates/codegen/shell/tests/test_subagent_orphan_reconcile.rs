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
//! cargo test -p shell --test test_subagent_orphan_reconcile -- --ignored
//! ```

use std::future::Future;
use std::path::{Path, PathBuf};

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
        let server = MockInferenceServer::start()
            .await
            .expect("start mock server");
        let workdir = git_workdir();

        // Phase 1: create a real session, then take its home so we can seed it.
        let mut writer = GrowStdioClient::spawn(&server, workdir.workspace()).await;
        writer.initialize_with_timeout().await;
        let session_id = writer
            .create_session_with_timeout(workdir.workspace())
            .await;
        let shared_sandbox = writer.take_sandbox();
        drop(writer);

        // Simulate a crash: append a parent spawn with no child result or
        // parent terminal.
        let grow_home = shared_sandbox.grow_home().to_path_buf();
        let session_dir = locate_session_dir(&grow_home, session_id.0.as_ref());
        let sub_id = "sa-orphan";
        let mut timeline =
            shell::session::storage::load_timeline_by_id_at(session_id.0.as_ref(), &grow_home)
                .expect("read parent Timeline")
                .expect("parent session exists");
        let spawn = timeline
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Spawned(chat_state::SubagentSpawnEvent {
                    subagent_id: sub_id.into(),
                    child_session_id: "child-orphan".into(),
                    subagent_type: "general-purpose".into(),
                    description: "stuck task".into(),
                    prompt: "do work".into(),
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
                    surface_completion: true,
                    child_cwd: workdir.workspace().to_string_lossy().into_owned(),
                    worktree_path: None,
                    effective_model_id: "test-model".into(),
                }),
            ))
            .expect("record orphan spawn");
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(session_dir.join("timeline.jsonl"))
            .unwrap();
        serde_json::to_writer(&mut file, &spawn).unwrap();
        file.write_all(b"\n").unwrap();
        file.sync_all().unwrap();

        // Phase 2: resume in a fresh process. `load_session` runs the reconcile.
        let reader =
            GrowStdioClient::spawn_with_sandbox(&server, workdir.workspace(), shared_sandbox).await;
        reader.initialize_with_timeout().await;
        let _ = reader
            .load_session_with_timeout(&session_id, workdir.workspace())
            .await;

        let reread =
            shell::session::storage::load_timeline_by_id_at(session_id.0.as_ref(), &grow_home)
                .expect("read reconciled Timeline")
                .expect("parent session exists");
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
    })
    .await;
}
