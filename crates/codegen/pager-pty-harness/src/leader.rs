//! Multi-client leader cluster: one shared leader, N pager clients, plus
//! inspection of the durable session log so reattach tests can assert on
//! what actually persisted.
//!
//! Every other leader test is single-client/single-leader; [`LeaderCluster`]
//! is the missing abstraction for "one leader, several pager clients sharing
//! its session". One [`ContentController`] gives one shared `$HOME` (hence one
//! elected leader) plus a fixed leader socket beneath its `GROW_HOME`; clients
//! spawn with the `--leader`/`--leader-socket` flags so they all attach to the
//! SAME leader. It also exposes the leader's durable Timeline history so a
//! reattach test can assert on the persisted, replayable turn-completion
//! records — the genuine end-to-end signal behind durable turn completion.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::Value;

use test_support::process::{TestProcess, TestProcessConfig};

use crate::{ContentController, PtyHarness, pager_binary};

/// One shared mock-backed leader plus the pager clients attached to it.
pub struct LeaderCluster {
    // Declared first so this concrete owner shuts down before content drops.
    _leader_owner: TestProcess,
    content: ContentController,
    binary: PathBuf,
    socket: PathBuf,
    rows: u16,
    cols: u16,
}

impl LeaderCluster {
    /// Start the cluster: one [`ContentController`] (one shared `$HOME` =>
    /// one leader) and a fixed leader socket under its `GROW_HOME`.
    pub async fn start(rows: u16, cols: u16) -> Result<Self> {
        let content = ContentController::start()
            .await
            .context("start content controller")?;
        content.seed_llm_config().context("seed mock LLM config")?;
        let binary = pager_binary().context("resolve pager binary")?;
        let cwd = std::env::current_dir().context("resolve leader working directory")?;
        let socket = content.sandbox().grow_home().join("leader.sock");
        let mut command = tokio::process::Command::new(&binary);
        command
            .args([
                "agent",
                "leader",
                "--no-exit-on-disconnect",
                "--no-auto-update",
            ])
            .current_dir(&cwd);
        let mut leader_owner = TestProcess::spawn(
            command,
            content.sandbox(),
            TestProcessConfig::new()
                .label("dedicated PTY test leader")
                .env("GROW_LEADER_SOCKET", &socket),
        )
        .context("spawn owned persistent leader")?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        while !socket.exists() {
            if let Some(status) = leader_owner.try_wait().context("poll dedicated leader")? {
                anyhow::bail!(
                    "dedicated PTY test leader exited with {status} before binding {}: {}",
                    socket.display(),
                    leader_owner.stderr_tail().text,
                );
            }
            if tokio::time::Instant::now() >= deadline {
                anyhow::bail!(
                    "dedicated PTY test leader did not bind {} within 30 seconds",
                    socket.display(),
                );
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok(Self {
            _leader_owner: leader_owner,
            content,
            binary,
            socket,
            rows,
            cols,
        })
    }

    /// Spawn a client attached to the dedicated leader
    /// (`--leader --leader-socket <S>` plus `extra_args`); it starts a fresh
    /// session through the already-owned daemon.
    pub fn spawn_leader(&self, extra_args: &[&str]) -> Result<PtyHarness> {
        self.spawn_client(&[], extra_args)
    }

    /// Attach another client that resumes the shared session through the SAME
    /// leader (`--leader --leader-socket <S> --resume` plus `extra_args`).
    pub fn attach(&self, extra_args: &[&str]) -> Result<PtyHarness> {
        self.spawn_client(&["--resume"], extra_args)
    }

    /// Spawn a client wired to the shared leader socket. `mode_args` carries
    /// the per-role flag (`--resume` for attachers); `extra_args` is the
    /// caller's.
    fn spawn_client(&self, mode_args: &[&str], extra_args: &[&str]) -> Result<PtyHarness> {
        let socket = self.socket.to_str().context("socket path is utf-8")?;
        let mut args: Vec<&str> = vec!["--leader", "--leader-socket", socket];
        args.extend_from_slice(mode_args);
        args.extend_from_slice(extra_args);
        PtyHarness::spawn_with_content(&self.binary, self.rows, self.cols, &self.content, &args)
            .context("spawn pager client on shared leader")
    }

    /// The shared content controller (mock inference server + sandbox env).
    pub fn content(&self) -> &ContentController {
        &self.content
    }

    /// The cluster's sessions root: `GROW_HOME/sessions` (layout below is
    /// `sessions/<encoded-cwd>/<session-id>/timeline.jsonl`).
    fn sessions_dir(&self) -> PathBuf {
        self.content.home().join(".grow").join("sessions")
    }

    /// The durable Grow notifications projected from each Timeline under the
    /// cluster's [`sessions_dir`](Self::sessions_dir). Scans all sessions under
    /// the cluster; callers use this to observe Grow terminal notifications.
    ///
    /// Infallible by design: a file that vanishes or fails production journal
    /// validation during a poll is skipped and retried on the next one.
    pub fn session_updates(&self) -> Vec<Value> {
        let mut files = Vec::new();
        collect_timeline_files(&self.sessions_dir(), &mut files);
        let mut out = Vec::new();
        for file in files {
            let Some(session_id) = file
                .parent()
                .and_then(Path::file_name)
                .and_then(std::ffi::OsStr::to_str)
            else {
                continue;
            };
            if let Ok(updates) = replay_grow_updates(session_id, &self.content.home().join(".grow"))
            {
                out.extend(updates);
            }
        }
        out
    }

    /// Poll [`session_updates`](Self::session_updates) until a record with
    /// `sessionUpdate == "turn_completed"` appears, returning that (inner)
    /// update payload, or error on timeout. Scans ALL sessions under the
    /// cluster (fine for the single-session clusters these tests build).
    pub fn wait_for_turn_completed(&self, timeout: Duration) -> Result<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            let updates = self.session_updates();
            if let Some(rec) = updates.iter().find(|u| is_turn_completed(u)) {
                return Ok(rec.clone());
            }
            if Instant::now() >= deadline {
                // Surface what WAS persisted so "zero records / env problem" is
                // distinguishable from "records present but no turn_completed /
                // producer regression".
                let tags: std::collections::BTreeSet<&str> = updates
                    .iter()
                    .filter_map(|u| u.get("sessionUpdate").and_then(Value::as_str))
                    .collect();
                anyhow::bail!(
                    "timed out after {timeout:?} waiting for a turn_completed record under {}; \
                     saw {} update record(s) with sessionUpdate tags {tags:?}",
                    self.sessions_dir().display(),
                    updates.len(),
                );
            }
            // Sync FS poll mirrors the harness's blocking wait_for_text; a stat
            // every 150ms is cheap and fine on a multi_thread runtime worker.
            std::thread::sleep(Duration::from_millis(150));
        }
    }
}

/// Whether a session-update payload is a `turn_completed` terminal.
fn is_turn_completed(update: &Value) -> bool {
    update.get("sessionUpdate").and_then(Value::as_str) == Some("turn_completed")
}

fn replay_grow_updates(session_id: &str, grow_home: &Path) -> std::io::Result<Vec<Value>> {
    let mut updates = Vec::new();
    let _ = shell::session::storage::stream_replay_grow_notifications_at(
        session_id,
        grow_home,
        |notification| {
            if let Ok(update) = serde_json::to_value(notification.update) {
                updates.push(update);
            }
        },
    )?;
    Ok(updates)
}

/// Recursively collect every physical Timeline beneath `dir` (a manual walk to
/// avoid a new crate dep). A missing/unreadable dir yields nothing — sessions
/// may not exist yet, and the walk is re-run on every poll.
fn collect_timeline_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        // No-follow file type: a symlinked directory has `is_dir() == false`,
        // so a symlink cycle can never recurse forever here.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            collect_timeline_files(&path, out);
        } else if path.file_name().and_then(|n| n.to_str()) == Some("timeline.jsonl") {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shell::session::storage::{JsonlStorageAdapter, SessionUpdate, StorageAdapter};
    use shell::session::testkit::synth::{SessionSpec, prepare_session};

    fn small_session_spec() -> SessionSpec {
        SessionSpec {
            turns: 1,
            acu_per_turn: 1,
            catalog_commands: 1,
            catalog_desc_len: 4,
            agent_chunks_per_turn: 1,
            agent_chunk_len: 4,
            rewind_points: 0,
            files_per_rewind: 0,
            file_content_len: 0,
        }
    }

    async fn session_with_turn_completed(root: &Path, cwd: &Path) -> (String, PathBuf) {
        let (info, directory) = prepare_session(root, cwd, &small_session_spec()).await;
        let adapter = JsonlStorageAdapter::with_root(root.to_path_buf());
        let notification = shell::extensions::notification::SessionNotification {
            session_id: info.id.clone(),
            update: shell::extensions::notification::SessionUpdate::TurnCompleted {
                prompt_id: "prompt-1".into(),
                identity: None,
                stop_reason: "end_turn".into(),
                agent_result: Some("fixture result".into()),
                usage: None,
            },
            meta: Some(serde_json::json!({"eventId": "turn-completed-prompt-1"})),
        };
        adapter
            .append_update_durable_commit_aware(&info, &SessionUpdate::Grow(Box::new(notification)))
            .await
            .expect("append durable TurnCompleted Grow fact");
        (info.id.0.to_string(), directory)
    }

    #[tokio::test]
    async fn production_replay_ignores_torn_trailing_physical_record() {
        let root = tempfile::tempdir().expect("create isolated grow home");
        let cwd = tempfile::tempdir().expect("create session cwd");
        let (session_id, directory) = session_with_turn_completed(root.path(), cwd.path()).await;
        let timeline = directory.join("timeline.jsonl");
        use std::io::Write as _;
        std::fs::OpenOptions::new()
            .append(true)
            .open(&timeline)
            .expect("open physical Timeline")
            .write_all(b"{\"version\":1,\"seq\":")
            .expect("append torn physical record");

        let updates = replay_grow_updates(&session_id, root.path())
            .expect("stream through production Grow replay projection");
        assert_eq!(
            updates.len(),
            1,
            "complete Grow fact survives the torn tail"
        );
        assert!(is_turn_completed(&updates[0]));
        assert_eq!(updates[0]["prompt_id"], "prompt-1");
        assert_eq!(updates[0]["agent_result"], "fixture result");
    }

    #[tokio::test]
    async fn production_grow_replay_returns_turn_terminal_without_acp_facts() {
        let root = tempfile::tempdir().expect("create isolated grow home");
        let cwd = tempfile::tempdir().expect("create session cwd");
        let (session_id, _) = session_with_turn_completed(root.path(), cwd.path()).await;
        let updates = replay_grow_updates(&session_id, root.path())
            .expect("stream through production Grow replay projection");
        assert_eq!(updates.len(), 1, "only the persisted Grow fact is emitted");
        assert!(is_turn_completed(&updates[0]));
        assert_eq!(updates[0]["stop_reason"], "end_turn");
    }
}

#[cfg(all(test, unix))]
mod ownership_tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "starts the built Grow daemon in an isolated test sandbox"]
    async fn leader_cluster_reaps_dedicated_daemon_on_drop_and_unwind() {
        for unwind in [false, true] {
            let cluster = LeaderCluster::start(40, 120).await.unwrap();
            let pid = cluster._leader_owner.pid().expect("owned daemon is live");
            assert!(
                !test_support::process::process_has_exited_without_reap(pid, "owned leader")
                    .unwrap()
            );
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _cluster = cluster;
                if unwind {
                    panic!("simulate PTY fixture assertion failure");
                }
            }));
            assert_eq!(result.is_err(), unwind);
            let error = test_support::process::process_has_exited_without_reap(pid, "owned leader")
                .expect_err("cluster Drop must wait/reap the dedicated daemon");
            assert_eq!(error.raw_os_error(), Some(libc::ECHILD));
        }
    }
}
