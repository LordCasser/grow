//! Concurrency-safe, field-correct writes to a session's `summary.json`.
//!
//! The same `summary.json` is mutated by several writers and, on reconnect, by
//! more than one persistence actor. A whole-summary read-modify-write with no
//! lock loses updates: a writer holding a stale read overwrites a concurrent
//! writer's field on write-back, which silently reverted `last_active_at` and
//! `num_messages` (the active session then sank in the `/resume` picker).
//!
//! [`SummaryPatch`] expresses *intent* (a partial update) rather than a
//! whole-struct snapshot, and [`apply_patch_locked`] applies it under an
//! exclusive lock on a sidecar `summary.json.lock` (never renamed, so the lock
//! spans the entire read-modify-write). All writers funnel through it, so the
//! read-modify-writes serialize across actors and processes.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

use agent_client_protocol as acp;
use chrono::{DateTime, Utc};
use fs2::FileExt;
use sampling_types::ReasoningEffort;

use crate::session::persistence::Summary;

/// How a counter field changes. `Increment` is applied to the in-lock fresh
/// read (never precomputed by the caller, which would re-open the race); `Set`
/// is an absolute rewrite.
#[derive(Debug, Clone)]
pub(crate) enum CounterOp {
    Increment(usize),
    Set(usize),
}

impl CounterOp {
    fn apply(&self, current: usize) -> usize {
        match self {
            CounterOp::Increment(n) => current.saturating_add(*n),
            CounterOp::Set(n) => *n,
        }
    }
}

/// Model / agent / reasoning-effort update. Each `None` leaves the existing
/// value unchanged (matches the legacy `update_current_model` semantics).
#[derive(Debug, Clone)]
pub(crate) struct ModelPatch {
    pub model_id: acp::ModelId,
    pub agent_name: Option<String>,
    pub reasoning_effort: Option<Option<ReasoningEffort>>,
}

/// Persisted git HEAD. `commit` and `branch` are last-writer-wins, including
/// being cleared to `None`.
#[derive(Debug, Clone)]
pub(crate) struct GitHeadPatch {
    pub commit: Option<String>,
    pub branch: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionTitleProjection {
    pub event_seq: u64,
    pub title: String,
    pub source: chat_state::SessionTitleSource,
}

/// A typed, partial mutation of a `Summary`. Only the set fields change; the
/// rest are read fresh under the lock and preserved. Per-field merge rules
/// (see [`Summary::apply_patch`]): `last_active_at` /
/// `session_format_version` is monotonic (never lowered), counters apply to the
/// fresh read, everything else is last-writer-wins on that field alone.
#[derive(Debug, Clone, Default)]
pub(crate) struct SummaryPatch {
    pub record_activity: bool,
    pub messages: Option<CounterOp>,
    pub session_format_version: Option<u8>,
    pub model: Option<ModelPatch>,
    pub git_head: Option<GitHeadPatch>,
    pub session_title: Option<SessionTitleProjection>,
    pub cwd_switch_bookkeeping_generation: Option<u64>,
    pub lineage: Option<crate::session::persistence::SessionLineage>,
}

impl Summary {
    /// Apply `patch` in place using the per-field merge rules. `now` is the
    /// single timestamp used for both `last_active_at` (when activity is
    /// recorded) and `updated_at`.
    ///
    /// Returns `true` iff a newer canonical session-title projection applied.
    pub(crate) fn apply_patch(&mut self, patch: &SummaryPatch, now: DateTime<Utc>) -> bool {
        if patch.record_activity {
            // Monotonic: a stale concurrent writer can never move it backwards.
            self.last_active_at = Some(
                self.last_active_at
                    .map_or(now, |existing| existing.max(now)),
            );
        }
        if let Some(op) = &patch.messages {
            self.num_messages = op.apply(self.num_messages);
        }
        if let Some(version) = patch.session_format_version {
            self.session_format_version = self.session_format_version.max(version);
        }
        if let Some(generation) = patch.cwd_switch_bookkeeping_generation
            && generation > self.cwd_switch_bookkeeping_generation
        {
            self.cwd_switch_bookkeeping_generation = generation;
            if self
                .pending_cwd_switch_reminder
                .as_ref()
                .is_some_and(|pending| pending.cwd_generation <= generation)
            {
                self.pending_cwd_switch_reminder = None;
            }
        }
        if let Some(lineage) = &patch.lineage {
            lineage.apply_to(self);
        }
        if let Some(model) = &patch.model {
            self.current_model_id = model.model_id.clone();
            if let Some(agent_name) = &model.agent_name {
                self.agent_name = Some(agent_name.clone());
            }
            if let Some(reasoning_effort) = &model.reasoning_effort {
                self.reasoning_effort = *reasoning_effort;
            }
        }
        if let Some(git_head) = &patch.git_head {
            self.head_commit = git_head.commit.clone();
            self.head_branch = git_head.branch.clone();
        }
        let mut title_applied = false;
        if let Some(title) = &patch.session_title
            && self
                .title_event_seq
                .is_none_or(|current| title.event_seq > current)
        {
            self.title = Some(title.title.clone());
            self.title_source = Some(title.source.clone());
            self.title_event_seq = Some(title.event_seq);
            title_applied = true;
        }
        self.updated_at = now;
        title_applied
    }
}

/// Read → apply `patch` → write `summary_path`, serialized by an exclusive lock
/// on the sidecar `lock_path`. The lock is held across the whole read-modify-
/// write so concurrent writers cannot lose each other's updates. Synchronous:
/// callers run it on `spawn_blocking` because the lock acquisition blocks.
///
/// Returns whether a newer canonical title projection was applied.
pub(crate) fn apply_patch_locked(
    summary_path: &Path,
    lock_path: &Path,
    patch: &SummaryPatch,
) -> io::Result<bool> {
    let lock = open_lock_file(lock_path)?;
    lock.lock_exclusive()?;
    let result = read_modify_write(summary_path, patch);
    let _ = lock.unlock();
    result
}

fn read_modify_write(summary_path: &Path, patch: &SummaryPatch) -> io::Result<bool> {
    let mut summary = read_summary(summary_path)?;
    let title_applied = summary.apply_patch(patch, Utc::now());
    write_summary_atomic(summary_path, &summary)?;
    Ok(title_applied)
}

fn open_lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

fn read_summary(path: &Path) -> io::Result<Summary> {
    let bytes = std::fs::read(path)?;
    if bytes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("summary.json is empty (0 bytes): {}", path.display()),
        ));
    }
    let summary = serde_json::from_slice::<Summary>(&bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    summary.validate_current_format()?;
    Ok(summary)
}

fn write_summary_atomic(summary_path: &Path, summary: &Summary) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(summary)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    crate::session::storage::write_bytes_atomic(summary_path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::info::Info;
    use crate::session::storage::StorageAdapter;
    use crate::session::storage::jsonl::JsonlStorageAdapter;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::Barrier;

    fn test_info() -> Info {
        Info {
            id: acp::SessionId::new("concurrent-summary-test"),
            cwd: "/test".into(),
        }
    }

    /// Regression guard for the `/resume` "frozen `last_active_at`" lost-update
    /// race. Two adapters (standing in for two persistence actors) hammer the
    /// SAME `summary.json` concurrently: one appends, the other writes metadata.
    /// Every write is a whole-summary read-modify-write, so without the sidecar
    /// lock the metadata writer reverts the appender's `num_messages` /
    /// `last_active_at` (and vice versa). The invariants below are exact, so a
    /// regression that drops the lock fails this deterministically: the counter
    /// must equal the number of appends and the monotonic field must not regress.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_writes_do_not_lose_updates() {
        const N: usize = 300;
        let dir = TempDir::new().unwrap();
        let session_dir = dir.path().join("session");
        let info = test_info();

        let init = JsonlStorageAdapter::with_explicit_session_dir(session_dir.clone());
        init.init_session(&info, acp::ModelId::new("test-model"))
            .await
            .unwrap();

        let appender = JsonlStorageAdapter::with_explicit_session_dir(session_dir.clone());
        let metadata = JsonlStorageAdapter::with_explicit_session_dir(session_dir.clone());
        let barrier = Arc::new(Barrier::new(2));

        let info_a = info.clone();
        let barrier_a = barrier.clone();
        let task_a = tokio::spawn(async move {
            barrier_a.wait().await;
            for _ in 0..N {
                appender
                    .apply_summary_patch(
                        &info_a,
                        SummaryPatch {
                            record_activity: true,
                            messages: Some(CounterOp::Increment(1)),
                            ..Default::default()
                        },
                    )
                    .await
                    .unwrap();
            }
        });

        let info_b = info.clone();
        let barrier_b = barrier.clone();
        let task_b = tokio::spawn(async move {
            barrier_b.wait().await;
            for turn in 0..N {
                metadata
                    .apply_summary_patch(
                        &info_b,
                        SummaryPatch {
                            git_head: Some(GitHeadPatch {
                                commit: Some(format!("commit-{turn}")),
                                branch: Some("main".to_string()),
                            }),
                            ..Default::default()
                        },
                    )
                    .await
                    .unwrap();
            }
        });

        task_a.await.unwrap();
        task_b.await.unwrap();

        let summary = read_summary(&session_dir.join("summary.json")).unwrap();
        assert_eq!(
            summary.num_messages, N,
            "lost an append increment to a racing metadata write",
        );
        assert_eq!(summary.head_branch.as_deref(), Some("main"));
        assert!(
            summary.last_active_at.is_some(),
            "activity timestamp was lost",
        );
    }

    /// A freshly-initialized (untitled) session: returns its adapter and the
    /// path to the on-disk `summary.json`.
    async fn new_session(dir: &TempDir) -> (JsonlStorageAdapter, Info, std::path::PathBuf) {
        let session_dir = dir.path().join("session");
        let info = test_info();
        let adapter = JsonlStorageAdapter::with_explicit_session_dir(session_dir.clone());
        adapter
            .init_session(&info, acp::ModelId::new("test-model"))
            .await
            .unwrap();
        (adapter, info, session_dir.join("summary.json"))
    }

    #[tokio::test]
    async fn canonical_title_projection_advances_by_event_seq() {
        let dir = TempDir::new().unwrap();
        let (adapter, info, summary_path) = new_session(&dir).await;

        let applied = adapter
            .repair_session_title_projection(
                &info,
                7,
                "Auto Title".into(),
                chat_state::SessionTitleSource::Generated {
                    sideband_id: uuid::Uuid::now_v7().to_string(),
                    result_seq: 2,
                },
            )
            .await
            .unwrap();

        assert!(applied);
        let summary = read_summary(&summary_path).unwrap();
        assert_eq!(summary.display_title(), "Auto Title");
        assert_eq!(summary.title_event_seq, Some(7));
        assert!(summary.manual_title_opt().is_none());
    }

    #[tokio::test]
    async fn stale_title_projection_is_ignored() {
        let dir = TempDir::new().unwrap();
        let (adapter, info, summary_path) = new_session(&dir).await;

        adapter
            .repair_session_title_projection(
                &info,
                9,
                "Manual Title".into(),
                chat_state::SessionTitleSource::User,
            )
            .await
            .unwrap();
        let applied = adapter
            .repair_session_title_projection(
                &info,
                8,
                "Stale Auto Title".into(),
                chat_state::SessionTitleSource::Generated {
                    sideband_id: uuid::Uuid::now_v7().to_string(),
                    result_seq: 2,
                },
            )
            .await
            .unwrap();

        assert!(!applied);
        let summary = read_summary(&summary_path).unwrap();
        assert_eq!(summary.display_title(), "Manual Title");
        assert_eq!(summary.manual_title_opt().as_deref(), Some("Manual Title"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_title_projection_converges_to_highest_seq() {
        for _ in 0..25 {
            let dir = TempDir::new().unwrap();
            let (adapter, info, summary_path) = new_session(&dir).await;
            let barrier = Arc::new(Barrier::new(2));

            let newer = adapter.clone();
            let info_m = info.clone();
            let barrier_m = barrier.clone();
            let task_m = tokio::spawn(async move {
                barrier_m.wait().await;
                newer
                    .repair_session_title_projection(
                        &info_m,
                        11,
                        "Manual Title".into(),
                        chat_state::SessionTitleSource::User,
                    )
                    .await
                    .unwrap();
            });

            let older = adapter.clone();
            let info_a = info.clone();
            let barrier_a = barrier.clone();
            let task_a = tokio::spawn(async move {
                barrier_a.wait().await;
                older
                    .repair_session_title_projection(
                        &info_a,
                        10,
                        "Auto Title".into(),
                        chat_state::SessionTitleSource::Generated {
                            sideband_id: uuid::Uuid::now_v7().to_string(),
                            result_seq: 2,
                        },
                    )
                    .await
                    .unwrap();
            });

            task_m.await.unwrap();
            task_a.await.unwrap();

            let summary = read_summary(&summary_path).unwrap();
            assert_eq!(summary.display_title(), "Manual Title");
            assert_eq!(summary.title_event_seq, Some(11));
        }
    }
}
