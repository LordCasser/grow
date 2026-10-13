//! Manager-local admission barrier for a storage domain whose durable writer
//! has failed. The gate is deliberately small: it keeps the failure owner and
//! the exact unacknowledged Timeline event until a new writer proves a durable
//! recovery prefix.

use std::{collections::BTreeMap, io, path::PathBuf};

use crate::session::{
    info::Info,
    storage::{JsonlStorageAdapter, StorageAdapter},
};

const MAX_FAILURE_OWNERS: usize = 128;

#[derive(Clone)]
struct FailureOwner {
    epoch: String,
    event: Option<chat_state::PendingTimelineCommit>,
    shared_domain: bool,
    reason: String,
}

#[derive(Default)]
struct State {
    owners: BTreeMap<String, FailureOwner>,
    fail_closed: bool,
}

/// Admission state shared by sessions created by one MvpAgent manager.
///
/// A separate manager gets a separate instance, which keeps test fixtures and
/// explicitly isolated storage roots independent.
pub(crate) struct StorageAdmission {
    domain_root: PathBuf,
    state: parking_lot::Mutex<State>,
}

pub(crate) struct RecoveryToken {
    session_id: String,
    failed_epoch: String,
}

impl StorageAdmission {
    pub(crate) fn new(domain_root: PathBuf) -> Self {
        Self {
            domain_root,
            state: parking_lot::Mutex::new(State::default()),
        }
    }

    pub(crate) fn check(&self, session_id: &str) -> Result<(), String> {
        let state = self.state.lock();
        if state.fail_closed {
            return Err("storage admission is closed because failure tracking is full".into());
        }
        if let Some(owner) = state.owners.get(session_id) {
            return Err(format!("session storage is unavailable: {}", owner.reason));
        }
        if let Some(owner) = state.owners.values().find(|owner| owner.shared_domain) {
            return Err(format!("storage domain is unavailable: {}", owner.reason));
        }
        Ok(())
    }

    pub(crate) fn record_failure(
        &self,
        session_id: &str,
        epoch: &str,
        event: Option<chat_state::PendingTimelineCommit>,
        error_kind: Option<io::ErrorKind>,
        reason: impl Into<String>,
    ) {
        let reason = reason.into();
        let shared_domain = matches!(
            error_kind,
            Some(io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded)
        );
        let mut state = self.state.lock();
        if !state.owners.contains_key(session_id) && state.owners.len() >= MAX_FAILURE_OWNERS {
            state.fail_closed = true;
            return;
        }
        let previous = state.owners.get(session_id).cloned();
        let event = event.or_else(|| {
            previous.as_ref().and_then(|owner| {
                (owner.epoch == epoch)
                    .then(|| owner.event.clone())
                    .flatten()
            })
        });
        let shared_domain =
            shared_domain || previous.as_ref().is_some_and(|owner| owner.shared_domain);
        state.owners.insert(
            session_id.to_owned(),
            FailureOwner {
                epoch: epoch.to_owned(),
                event,
                shared_domain,
                reason,
            },
        );
    }

    /// Claim a fresh writer lease, replay the exact unacknowledged event when
    /// one exists, then sync the resulting prefix before any bootstrap facts.
    pub(crate) async fn prepare_recovery(&self, info: &Info) -> io::Result<Option<RecoveryToken>> {
        self.prepare_recovery_until(info, None).await
    }

    pub(crate) async fn prepare_recovery_before(
        &self,
        info: &Info,
        deadline: tokio::time::Instant,
    ) -> io::Result<Option<RecoveryToken>> {
        self.prepare_recovery_until(info, Some(deadline)).await
    }

    async fn prepare_recovery_until(
        &self,
        info: &Info,
        deadline: Option<tokio::time::Instant>,
    ) -> io::Result<Option<RecoveryToken>> {
        let owner = self.state.lock().owners.get(info.id.0.as_ref()).cloned();
        let Some(owner) = owner else { return Ok(None) };
        if deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "recovery deadline elapsed before writer claim",
            ));
        }
        let storage = JsonlStorageAdapter::with_root(self.domain_root.clone());
        storage.claim_recovery_writer(info)?;
        if deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "recovery deadline elapsed after writer claim",
            ));
        }
        if let Some(pending) = owner.event {
            let append =
                StorageAdapter::append_timeline_event_durable(&storage, info, &pending.event);
            match deadline {
                Some(deadline) => {
                    tokio::time::timeout_at(deadline, append)
                        .await
                        .map_err(|_| {
                            io::Error::new(
                                io::ErrorKind::TimedOut,
                                "recovery deadline elapsed waiting for pending append",
                            )
                        })??
                }
                None => append.await?,
            }
        }
        let sync = storage.sync_recovery_prefix(info);
        match deadline {
            Some(deadline) => tokio::time::timeout_at(deadline, sync)
                .await
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        "recovery deadline elapsed waiting for prefix sync",
                    )
                })??,
            None => sync.await?,
        }
        drop(storage);
        Ok(Some(RecoveryToken {
            session_id: info.id.to_string(),
            failed_epoch: owner.epoch,
        }))
    }

    /// Clear only the owner that was recovered by this token and only after a
    /// distinct healthy writer epoch has started.
    pub(crate) fn complete_recovery(&self, token: RecoveryToken, new_epoch: &str) -> bool {
        if token.failed_epoch == new_epoch {
            return false;
        }
        let mut state = self.state.lock();
        let Some(owner) = state.owners.get(&token.session_id) else {
            return false;
        };
        if owner.epoch != token.failed_epoch {
            return false;
        }
        state.owners.remove(&token.session_id);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate() -> StorageAdmission {
        StorageAdmission::new(std::env::temp_dir().join("storage-admission-test"))
    }

    #[test]
    fn integrity_failure_blocks_only_its_session() {
        let gate = gate();
        gate.record_failure(
            "owner",
            "epoch-1",
            None,
            Some(io::ErrorKind::InvalidData),
            "invalid timeline",
        );

        assert!(gate.check("owner").is_err());
        assert!(gate.check("other").is_ok());
    }

    #[test]
    fn storage_full_closes_the_shared_domain() {
        let gate = gate();
        gate.record_failure(
            "owner",
            "epoch-1",
            None,
            Some(io::ErrorKind::StorageFull),
            "disk full",
        );

        assert!(gate.check("owner").is_err());
        assert!(gate.check("other").is_err());
    }

    #[test]
    fn cloned_manager_gate_observes_the_same_shared_storage_failure() {
        let gate = std::sync::Arc::new(gate());
        let manager_clone = gate.clone();
        manager_clone.record_failure(
            "owner",
            "epoch-1",
            None,
            Some(io::ErrorKind::QuotaExceeded),
            "quota exceeded",
        );

        assert!(gate.check("owner").is_err());
        assert!(gate.check("another-session-in-the-domain").is_err());
    }

    #[test]
    fn failure_owner_overflow_fails_closed() {
        let gate = gate();
        for index in 0..=MAX_FAILURE_OWNERS {
            gate.record_failure(
                &format!("session-{index}"),
                "epoch-1",
                None,
                Some(io::ErrorKind::InvalidData),
                "invalid timeline",
            );
        }

        assert!(gate.check("unrelated-session").is_err());
    }

    #[test]
    fn recovery_requires_the_same_owner_and_a_new_epoch() {
        let gate = gate();
        gate.record_failure(
            "owner",
            "epoch-1",
            None,
            Some(io::ErrorKind::InvalidData),
            "invalid timeline",
        );

        assert!(!gate.complete_recovery(
            RecoveryToken {
                session_id: "other-owner".into(),
                failed_epoch: "epoch-1".into(),
            },
            "epoch-2",
        ));
        assert!(gate.check("owner").is_err());

        assert!(!gate.complete_recovery(
            RecoveryToken {
                session_id: "owner".into(),
                failed_epoch: "epoch-1".into(),
            },
            "epoch-1",
        ));
        assert!(gate.check("owner").is_err());

        assert!(gate.complete_recovery(
            RecoveryToken {
                session_id: "owner".into(),
                failed_epoch: "epoch-1".into(),
            },
            "epoch-2",
        ));
        assert!(gate.check("owner").is_ok());
    }

    #[tokio::test]
    async fn failed_recovery_barrier_keeps_admission_closed() {
        let root = std::env::temp_dir().join(format!(
            "storage-admission-non-directory-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::write(&root, b"not a directory").expect("create invalid storage root");
        let gate = StorageAdmission::new(root.clone());
        gate.record_failure(
            "owner",
            "epoch-1",
            None,
            Some(io::ErrorKind::StorageFull),
            "disk full",
        );
        let info = Info {
            id: acp_transport::protocol::SessionId::new("owner"),
            cwd: std::env::temp_dir().to_string_lossy().into_owned(),
        };

        let recovery = gate.prepare_recovery(&info).await;
        let _ = std::fs::remove_file(root);

        assert!(recovery.is_err());
        assert!(gate.check("owner").is_err());
        assert!(gate.check("other").is_err());
    }
}
