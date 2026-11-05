//! Real Shell extension -> WorkspaceOps -> hunk actor rejection contract.
//!
//! Run with GROW_HOME pointing to an existing directory under the system temp
//! root. The fixture deliberately has no watcher, provider request or ACP session.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use acp_transport::{AcpAgentGatewaySender, AcpClientMessage, protocol as acp};
use hunk_tracker::{
    FileHunkData, Hunk, HunkEvent, HunkId, HunkRemovalReason, HunkTrackerActor, HunkTrackerHandle,
    HunkTrackerSnapshot, TrackingMode,
};
use serde_json::{Value, json};
use shell::agent::{MvpAgent, config::Config};
use shell::extensions::hunk_tracker as hunk_extension;
use tempfile::TempDir;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tools::registry::types::FinalizedToolset;
use workspace::WorkspaceOps;

const TURN: usize = 7;
const BASELINE: &str = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";
const MODIFIED: &str = "1\nCHANGED_2\n3\n4\n5\n6\n7\n8\nCHANGED_9\n10\n";

#[derive(Clone, Copy, Debug)]
enum Scope {
    Hunk,
    File,
    Turn,
    All,
}

impl Scope {
    const ALL: [Self; 4] = [Self::Hunk, Self::File, Self::Turn, Self::All];

    fn method(self) -> &'static str {
        match self {
            Self::Hunk => "grow/hunk-tracker/hunk-action",
            Self::File => "grow/hunk-tracker/file-action",
            Self::Turn => "grow/hunk-tracker/turn-action",
            Self::All => "grow/hunk-tracker/all-action",
        }
    }
}

fn temp_root() -> PathBuf {
    if cfg!(unix) {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    }
}

fn agent_config() -> Config {
    // Agent bootstrap extracts builtins. Refuse to do that in a normal Grow
    // home; the runner supplies this process's isolated home before startup.
    let home = PathBuf::from(std::env::var_os("GROW_HOME").expect("set an isolated GROW_HOME"));
    let home = home.canonicalize().expect("GROW_HOME must already exist");
    let temp = temp_root().canonicalize().unwrap();
    assert!(
        home.starts_with(&temp) && home != temp,
        "GROW_HOME must be below {temp:?}"
    );

    let raw: toml::Value = toml::from_str(
        r#"
        [models]
        default = "test/default"

        [provider.test]
        api_backend = "chat_completions"

        [provider.test.options]
        base_url = "http://127.0.0.1:1/v1"

        [provider.test.models.default]
        context_window = 128000
        "#,
    )
    .unwrap();
    let mut config = Config::new_from_toml_cfg(&raw).expect("explicit local model config");
    config.models.default = Some("test/default".to_owned());
    config.default_model_override = None;
    config.remote_settings = Some(shell::util::config::RemoteSettings::default());
    config
}

struct Fixture {
    agent: MvpAgent,
    ops: WorkspaceOps,
    tracker: HunkTrackerHandle,
    events: mpsc::UnboundedReceiver<HunkEvent>,
    cancel: CancellationToken,
    session_id: String,
    path: PathBuf,
    _gateway_rx: mpsc::UnboundedReceiver<AcpClientMessage>,
    _repo: TempDir,
}

struct PendingState {
    snapshot: HunkTrackerSnapshot,
    summary: Value,
    displayed: FileHunkData,
}

impl Fixture {
    async fn new(baseline: Option<&str>) -> Self {
        let config = agent_config();
        let repo = tempfile::Builder::new()
            .prefix("grow-hunk-stale-shell-")
            .tempdir_in(temp_root())
            .unwrap();
        let root = repo.path();
        let path = root.join("review.rs");
        std::fs::create_dir(root.join("empty-hooks")).unwrap();
        std::fs::create_dir(root.join("empty-template")).unwrap();
        git(root, &["init"]);
        if let Some(content) = baseline {
            std::fs::write(&path, content).unwrap();
            git(root, &["add", "review.rs"]);
        }
        git(root, &["commit", "--allow-empty", "-m", "fixture baseline"]);

        let (gateway_tx, gateway_rx) = mpsc::unbounded_channel();
        let agent = MvpAgent::new(AcpAgentGatewaySender::new(gateway_tx), &config)
            .expect("construct real agent without authenticating or prompting");
        let session_id = format!("hunk-stale-{}", uuid::Uuid::new_v4());
        let (event_tx, events) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let tracker = HunkTrackerActor::spawn(
            session_id.clone(),
            root.to_path_buf(),
            event_tx,
            TrackingMode::AgentOnly,
            cancel.clone(),
        );
        let ops = WorkspaceOps::for_test_in(root);
        ops.bind_local_session(
            &session_id,
            root.to_path_buf(),
            tracker.clone(),
            Arc::new(FinalizedToolset::empty_for_test()),
            None,
        )
        .await
        .expect("bind real tracker to the session dispatched by the extension");

        Self {
            agent,
            ops,
            tracker,
            events,
            cancel,
            session_id,
            path,
            _gateway_rx: gateway_rx,
            _repo: repo,
        }
    }

    async fn agent_write(&self, content: &str) -> FileHunkData {
        std::fs::write(&self.path, content).unwrap();
        self.tracker
            .record_agent_write(self.path.clone(), content.to_owned(), TURN, None);
        // This public query is the command barrier before the external edit.
        self.tracker.get_file_hunk_data(self.path.clone()).await
    }

    async fn pending_state(&self) -> PendingState {
        let displayed = self.tracker.get_file_hunk_data(self.path.clone()).await;
        let ids = hunk_ids(&displayed.hunks);
        assert!(!ids.is_empty());
        assert_eq!(hunk_ids(&self.tracker.get_turn_hunks(TURN).await), ids);
        PendingState {
            snapshot: self.tracker.snapshot_state().await.unwrap(),
            summary: serde_json::to_value(self.tracker.get_session_summary().await).unwrap(),
            displayed,
        }
    }

    async fn reject(&self, scope: Scope, hunk_id: &HunkId) -> Value {
        let mut params = json!({"sessionId": self.session_id, "action": "reject"});
        match scope {
            Scope::Hunk => params["hunkId"] = json!(hunk_id.as_str()),
            Scope::File => params["path"] = json!(self.path),
            Scope::Turn => params["promptIndex"] = json!(TURN),
            Scope::All => {}
        }
        let raw = serde_json::value::to_raw_value(&params).unwrap();
        let request = acp::ExtRequest::new(scope.method(), Arc::from(raw));
        let response = tokio::time::timeout(
            Duration::from_secs(10),
            hunk_extension::handle(&self.agent, &self.ops, &request),
        )
        .await
        .expect("extension action timed out")
        .expect("action failure must be represented by the product response");
        let envelope: Value = serde_json::from_str(response.0.get()).unwrap();
        assert!(
            envelope.get("error").is_none(),
            "unexpected envelope error: {envelope}"
        );
        let result = envelope
            .get("result")
            .expect("ExtMethodResult result field");
        assert!(result.is_object(), "missing ActionResponse: {envelope}");
        result.clone()
    }

    async fn assert_unchanged(&mut self, before: &PendingState) {
        let after = self.tracker.snapshot_state().await.unwrap();
        assert_eq!(
            serde_json::to_value(after.file_states).unwrap(),
            serde_json::to_value(&before.snapshot.file_states).unwrap()
        );
        assert_eq!(after.turn_index, before.snapshot.turn_index);
        assert_eq!(
            serde_json::to_value(after.session_stats).unwrap(),
            serde_json::to_value(&before.snapshot.session_stats).unwrap()
        );
        assert_eq!(
            serde_json::to_value(self.tracker.get_session_summary().await).unwrap(),
            before.summary
        );
        let ids = hunk_ids(&before.displayed.hunks);
        assert_eq!(hunk_ids(&self.tracker.get_all_hunks().await), ids);
        assert_eq!(hunk_ids(&self.tracker.get_turn_hunks(TURN).await), ids);
        assert_eq!(
            serde_json::to_value(self.tracker.get_file_hunk_data(self.path.clone()).await).unwrap(),
            serde_json::to_value(&before.displayed).unwrap()
        );
        assert!(
            self.drain_events().is_empty(),
            "failed action emitted a success projection"
        );
    }

    fn drain_events(&mut self) -> Vec<HunkEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            events.push(event);
        }
        events
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.ops.end_local_session(&self.session_id);
        self.cancel.cancel();
    }
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-c")
        .arg(format!(
            "core.hooksPath={}",
            root.join("empty-hooks").display()
        ))
        .arg("-c")
        .arg(format!(
            "init.templateDir={}",
            root.join("empty-template").display()
        ))
        .args(["-c", "commit.gpgSign=false", "-c", "tag.gpgSign=false"])
        .args(args)
        .current_dir(root)
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "Hunk Test")
        .env("GIT_AUTHOR_EMAIL", "hunk@example.invalid")
        .env("GIT_COMMITTER_NAME", "Hunk Test")
        .env("GIT_COMMITTER_EMAIL", "hunk@example.invalid")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn hunk_ids(hunks: &[Arc<Hunk>]) -> HashSet<HunkId> {
    hunks.iter().map(|hunk| hunk.id.clone()).collect()
}

fn assert_failed_response(response: &Value, message: &str) {
    assert_eq!(response["success"], false, "{response}");
    let error = response["error"]
        .as_str()
        .expect("failure must explain the rejected action");
    assert!(error.contains(message), "{response}");
    assert!(
        response.get("affectedCount").is_none(),
        "failed action reported success count: {response}"
    );
}

fn assert_success_response(response: &Value, count: usize) {
    assert_eq!(response["success"], true, "{response}");
    assert_eq!(response["affectedCount"], count, "{response}");
    assert!(response.get("error").is_none(), "{response}");
}

#[tokio::test(flavor = "current_thread")]
async fn shell_reject_stale_content_preserves_disk_and_pending_for_every_scope() {
    for scope in Scope::ALL {
        let mut fixture = Fixture::new(Some(BASELINE)).await;
        let displayed = fixture.agent_write(MODIFIED).await;
        assert_eq!(displayed.hunks.len(), 2);
        assert_eq!(displayed.current.content.as_deref(), Some(MODIFIED));
        let before = fixture.pending_state().await;
        fixture.drain_events();
        let external = format!("{MODIFIED}external append\n");
        // Withhold fsnotify; only disk changes after the displayed snapshot.
        std::fs::write(&fixture.path, &external).unwrap();
        let response = fixture.reject(scope, &displayed.hunks[0].id).await;
        assert_failed_response(&response, "stale content");
        assert!(
            response["error"]
                .as_str()
                .unwrap()
                .contains(fixture.path.to_str().unwrap())
        );
        assert_eq!(std::fs::read(&fixture.path).unwrap(), external.as_bytes());
        fixture.assert_unchanged(&before).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn shell_reject_matching_content_reports_actual_count_once() {
    for scope in Scope::ALL {
        let mut fixture = Fixture::new(Some(BASELINE)).await;
        let displayed = fixture.agent_write(MODIFIED).await;
        assert_eq!(displayed.hunks.len(), 2);
        let selected = displayed
            .hunks
            .iter()
            .find(|hunk| hunk.line_info.old_start == 2)
            .unwrap();
        let affected = if matches!(scope, Scope::Hunk) {
            HashSet::from([selected.id.clone()])
        } else {
            hunk_ids(&displayed.hunks)
        };
        fixture.drain_events();
        let response = fixture.reject(scope, &selected.id).await;
        assert_success_response(&response, affected.len());
        let expected = if matches!(scope, Scope::Hunk) {
            "1\n2\n3\n4\n5\n6\n7\n8\nCHANGED_9\n10\n"
        } else {
            BASELINE
        };
        assert_eq!(std::fs::read(&fixture.path).unwrap(), expected.as_bytes());
        let pending: HashSet<_> = displayed
            .hunks
            .iter()
            .filter(|hunk| !affected.contains(&hunk.id))
            .map(|hunk| hunk.id.clone())
            .collect();
        assert_eq!(hunk_ids(&fixture.tracker.get_all_hunks().await), pending);
        assert_eq!(
            hunk_ids(&fixture.tracker.get_turn_hunks(TURN).await),
            pending
        );
        let summary = fixture.tracker.get_session_summary().await;
        assert_eq!(summary.stats.rejected_hunks, affected.len());
        assert_eq!(summary.stats.rejected_lines_added, affected.len());
        assert_eq!(summary.stats.rejected_lines_removed, affected.len());
        assert_eq!(summary.stats.accepted_hunks, 0);
        assert_eq!(summary.pending_hunks, pending.len());
        let removed: Vec<_> = fixture
            .drain_events()
            .into_iter()
            .filter_map(|event| match event {
                HunkEvent::HunkRemoved {
                    hunk_id, reason, ..
                } => {
                    assert_eq!(reason, HunkRemovalReason::Rejected);
                    Some(hunk_id)
                }
                _ => None,
            })
            .collect();
        assert_eq!(removed.len(), affected.len());
        assert_eq!(removed.into_iter().collect::<HashSet<_>>(), affected);

        let response = fixture.reject(scope, &selected.id).await;
        if matches!(scope, Scope::Hunk) {
            assert_failed_response(&response, "Hunk not found");
        } else {
            assert_success_response(&response, 0);
        }
        assert_eq!(std::fs::read(&fixture.path).unwrap(), expected.as_bytes());
        assert_eq!(
            serde_json::to_value(fixture.tracker.get_session_summary().await).unwrap(),
            serde_json::to_value(summary).unwrap()
        );
        assert_eq!(
            hunk_ids(&fixture.tracker.get_turn_hunks(TURN).await),
            pending
        );
        assert!(fixture.drain_events().is_empty());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn shell_bulk_reject_preserves_edited_creation_and_recreated_deletion() {
    for created in [true, false] {
        let mut fixture = Fixture::new(if created { None } else { Some("original\n") }).await;
        assert_eq!(fixture.agent_write("agent content\n").await.hunks.len(), 1);
        if !created {
            std::fs::remove_file(&fixture.path).unwrap();
            fixture.tracker.handle_file_deleted(fixture.path.clone());
        }
        let before = fixture.pending_state().await;
        assert_eq!(before.displayed.hunks.len(), 1);
        if !created {
            assert_eq!(
                before.displayed.current.status,
                hunk_tracker::FileContentStatus::Missing
            );
        }
        fixture.drain_events();
        let external = if created {
            "agent content\nexternal append\n"
        } else {
            "external rebuild\n"
        };
        std::fs::write(&fixture.path, external).unwrap();
        let scope = if created { Scope::File } else { Scope::All };
        let response = fixture.reject(scope, &before.displayed.hunks[0].id).await;
        assert_failed_response(&response, "stale content");
        assert_eq!(std::fs::read(&fixture.path).unwrap(), external.as_bytes());
        fixture.assert_unchanged(&before).await;
    }
}
