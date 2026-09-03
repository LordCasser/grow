use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::local_ipc::frame::{read_json, write_json};
use crate::local_ipc::transport::{LocalListener, LocalStream, PrivateEndpoint};

use super::inquiry::{
    CoordinationError, CoordinationErrorCode, INQUIRY_DEADLINE, InboundInquiry,
    InquiryCancellation, InquiryCancellationReason, InquiryOutcome, InquiryPhase, InquiryState,
    InquiryStatus, MAX_QUESTION_BYTES, TERMINAL_CACHE_TTL,
};
use super::manifest::{
    DiscoveredSession, HEARTBEAT_INTERVAL, LEASE_DURATION, LocalSessionSnapshot, PeerDescription,
    PeerManifest, PeerSession, SCHEMA_VERSION, conflicted_session_ids, ensure_private_runtime_dirs,
    merge_local_sessions, now_unix_ms, peers_dir, read_all_manifests, read_manifest,
    write_manifest,
};
use super::protocol::{ClientHello, PROTOCOL_VERSION, Request, Response, ServerHello};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const SETUP_TIMEOUT: Duration = Duration::from_secs(3);
const RECONNECT_MIN_DELAY: Duration = Duration::from_millis(250);
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(2);
const CANCEL_SETTLE_GRACE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
struct InquiryPayload {
    source_incarnation: String,
    source_session_id: String,
    source_cwd: String,
    target_session_id: String,
    question: String,
}

#[derive(Debug)]
struct InquiryRecord {
    payload: InquiryPayload,
    cancellation: InquiryCancellation,
    progress: watch::Sender<InquiryPhase>,
    result: watch::Sender<Option<InquiryOutcome>>,
    terminal_at: std::sync::Mutex<Option<Instant>>,
    target: std::sync::Mutex<Option<PeerManifest>>,
}

impl InquiryRecord {
    fn new(payload: InquiryPayload, phase: InquiryPhase) -> Self {
        Self {
            payload,
            cancellation: InquiryCancellation::new(),
            progress: watch::channel(phase).0,
            result: watch::channel(None).0,
            terminal_at: std::sync::Mutex::new(None),
            target: std::sync::Mutex::new(None),
        }
    }

    fn expired(&self) -> bool {
        self.terminal_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some_and(|at| at.elapsed() >= TERMINAL_CACHE_TTL)
    }

    fn complete(&self, outcome: InquiryOutcome) {
        self.result.send_replace(Some(outcome));
        self.progress.send_replace(InquiryPhase::Finished);
        *self.terminal_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    }
}

#[derive(Debug, thiserror::Error)]
enum InquiryRequestError {
    #[error("{0}")]
    Transport(CoordinationError),
    #[error("{0}")]
    Remote(CoordinationError),
}

#[derive(Debug, thiserror::Error)]
pub enum CoordinationStartError {
    #[error("failed to prepare coordination runtime: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug)]
struct Shared {
    grow_home: PathBuf,
    peer_id: String,
    incarnation: String,
    endpoint: OnceLock<PrivateEndpoint>,
    manifest_path: PathBuf,
    token: String,
    started_at: i64,
    sessions: RwLock<HashMap<String, PeerSession>>,
    inquiry_tx: mpsc::Sender<InboundInquiry>,
    inquiries: tokio::sync::Mutex<HashMap<(String, String), Arc<InquiryRecord>>>,
    outgoing: tokio::sync::Mutex<HashMap<(String, String), Arc<InquiryRecord>>>,
    started: AtomicBool,
    publication: std::sync::Mutex<()>,
    lease_lock: OnceLock<std::fs::File>,
    startup_error: std::sync::Mutex<Option<String>>,
    cancel: CancellationToken,
}

impl Shared {
    fn require_ready(&self) -> Result<(), CoordinationError> {
        if !self.started.load(Ordering::Acquire) || self.cancel.is_cancelled() {
            return Err(CoordinationError::new(
                CoordinationErrorCode::RuntimeUnavailable,
                self.startup_error
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone()
                    .unwrap_or_else(|| "local coordination runtime is not running".to_owned()),
            ));
        }
        let manifest = read_manifest(&self.manifest_path).map_err(|error| {
            CoordinationError::retry(
                CoordinationErrorCode::RuntimeUnavailable,
                format!("local coordination lease cannot be read: {error}"),
                1000,
            )
        })?;
        if manifest.incarnation != self.incarnation || manifest.expires_at < now_unix_ms() {
            return Err(CoordinationError::retry(
                CoordinationErrorCode::RuntimeUnavailable,
                "local coordination lease is not valid",
                1000,
            ));
        }
        Ok(())
    }

    fn publish(&self) -> io::Result<()> {
        let _guard = self.publication.lock().unwrap_or_else(|e| e.into_inner());
        if self.cancel.is_cancelled() {
            return Ok(());
        }
        // Serialize snapshot creation AND replacement, not just temporary names.
        write_manifest(&self.manifest_path, &self.current_manifest())
    }

    fn current_manifest(&self) -> PeerManifest {
        let now = now_unix_ms();
        PeerManifest {
            schema_version: SCHEMA_VERSION,
            peer_id: self.peer_id.clone(),
            pid: std::process::id(),
            incarnation: self.incarnation.clone(),
            endpoint: self
                .endpoint
                .get()
                .map(|endpoint| endpoint.path().to_path_buf())
                .unwrap_or_default(),
            transport: super::manifest::TRANSPORT_KIND.to_owned(),
            token: self.token.clone(),
            started_at: self.started_at,
            heartbeat_at: now,
            expires_at: now + LEASE_DURATION.as_millis() as i64,
            capabilities: vec![
                "ping".to_owned(),
                "describe".to_owned(),
                "ask".to_owned(),
                "cancel".to_owned(),
            ],
            sessions: self
                .sessions
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .values()
                .cloned()
                .collect(),
        }
    }

    fn owns_session(&self, session_id: &str) -> bool {
        self.sessions
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(session_id)
    }
}

pub struct CoordinationRuntime {
    shared: Arc<Shared>,
    inquiry_rx: std::sync::Mutex<Option<mpsc::Receiver<InboundInquiry>>>,
    start_lock: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
pub struct CoordinationHandle {
    shared: Arc<Shared>,
}

impl CoordinationRuntime {
    pub fn new(grow_home: PathBuf) -> Self {
        let peer_id = uuid::Uuid::new_v4().to_string();
        let incarnation = uuid::Uuid::new_v4().to_string();
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let manifest_path = peers_dir(&grow_home).join(format!("{peer_id}.json"));
        let (inquiry_tx, inquiry_rx) = mpsc::channel(256);
        Self {
            shared: Arc::new(Shared {
                grow_home,
                peer_id,
                incarnation,
                endpoint: OnceLock::new(),
                manifest_path,
                token,
                started_at: now_unix_ms(),
                sessions: RwLock::new(HashMap::new()),
                inquiry_tx,
                inquiries: tokio::sync::Mutex::new(HashMap::new()),
                outgoing: tokio::sync::Mutex::new(HashMap::new()),
                started: AtomicBool::new(false),
                publication: std::sync::Mutex::new(()),
                lease_lock: OnceLock::new(),
                startup_error: std::sync::Mutex::new(None),
                cancel: CancellationToken::new(),
            }),
            inquiry_rx: std::sync::Mutex::new(Some(inquiry_rx)),
            start_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn ensure_started(&self) -> Result<(), CoordinationStartError> {
        if self.shared.started.load(Ordering::Acquire) {
            return Ok(());
        }
        let _guard = self.start_lock.lock().await;
        if self.shared.started.load(Ordering::Acquire) {
            return Ok(());
        }

        let prepared = (|| -> io::Result<_> {
            ensure_private_runtime_dirs(&self.shared.grow_home)?;
            if self.shared.lease_lock.get().is_none() {
                let lock = super::manifest::lock_peer(&self.shared.manifest_path)?;
                let _ = self.shared.lease_lock.set(lock);
            }
            if self.shared.endpoint.get().is_none() {
                let _ = self.shared.endpoint.set(PrivateEndpoint::new()?);
            }
            let endpoint = self.shared.endpoint.get().expect("allocated endpoint");
            #[cfg(unix)]
            if endpoint.path().exists() {
                std::fs::remove_file(endpoint.path())?;
            }
            let listener = LocalListener::bind(endpoint.path())?;
            self.shared.publish()?;
            Ok(listener)
        })();
        let listener = prepared.map_err(|error| {
            *self
                .shared
                .startup_error
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(error.to_string());
            error
        })?;
        self.shared.started.store(true, Ordering::Release);
        tokio::spawn(run_heartbeat(Arc::clone(&self.shared)));
        tokio::spawn(run_server(listener, Arc::clone(&self.shared)));
        Ok(())
    }

    pub fn is_started(&self) -> bool {
        self.shared.started.load(Ordering::Acquire)
    }

    pub(crate) fn take_inquiry_receiver(&self) -> Option<mpsc::Receiver<InboundInquiry>> {
        self.inquiry_rx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }

    pub fn publish_sessions(&self, snapshots: Vec<LocalSessionSnapshot>) {
        let mut sessions = self
            .shared
            .sessions
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *sessions = merge_local_sessions(&sessions, snapshots, now_unix_ms());
        drop(sessions);
        if self.shared.started.load(Ordering::Acquire)
            && let Err(error) = self.shared.publish()
        {
            tracing::warn!(error = %error, "failed to publish coordination session snapshot");
        }
    }

    pub async fn cancelled(&self) {
        self.shared.cancel.cancelled().await;
    }

    pub fn handle(&self) -> CoordinationHandle {
        CoordinationHandle {
            shared: Arc::clone(&self.shared),
        }
    }

    pub async fn list_active_sessions(
        &self,
        source_session_id: &str,
    ) -> Result<Vec<DiscoveredSession>, CoordinationError> {
        self.handle().list_active_sessions(source_session_id).await
    }

    pub async fn ask_session(
        &self,
        inquiry_id: &str,
        source_session_id: &str,
        target_session_id: &str,
        question: &str,
        progress: Option<mpsc::UnboundedSender<InquiryPhase>>,
        cancellation: CancellationToken,
    ) -> Result<InquiryOutcome, CoordinationError> {
        self.handle()
            .ask_session(
                inquiry_id,
                source_session_id,
                target_session_id,
                question,
                progress,
                cancellation,
            )
            .await
    }

    pub async fn cancel_session(
        &self,
        inquiry_id: &str,
        source_session_id: &str,
        target_session_id: &str,
    ) -> Result<bool, CoordinationError> {
        self.handle()
            .cancel_session(inquiry_id, source_session_id, target_session_id)
            .await
    }
}

impl CoordinationHandle {
    pub fn require_ready(&self) -> Result<(), CoordinationError> {
        self.shared.require_ready()
    }

    fn require_source(&self, source: &str) -> Result<(), CoordinationError> {
        if !self.shared.owns_session(source) {
            return Err(CoordinationError::new(
                CoordinationErrorCode::PermissionDenied,
                "source session is not owned by this Grow process",
            ));
        }
        Ok(())
    }

    pub async fn list_active_sessions(
        &self,
        source_session_id: &str,
    ) -> Result<Vec<DiscoveredSession>, CoordinationError> {
        self.require_ready()?;
        self.require_source(source_session_id)?;
        let manifests = self.live_manifests(source_session_id).await?;
        let conflicts = conflicted_session_ids(&manifests);
        let mut discovered = Vec::new();
        for manifest in manifests {
            for session in manifest.sessions {
                if session.session_id == source_session_id
                    || conflicts.contains(&session.session_id)
                {
                    continue;
                }
                discovered.push(DiscoveredSession {
                    session_id: session.session_id,
                    canonical_cwd: session.canonical_cwd,
                    main_agent: session.main_agent,
                    activity: session.activity,
                    subagents: session.subagents,
                    started_at: session.started_at,
                    process_started_at: manifest.started_at,
                    last_heartbeat: manifest.heartbeat_at,
                });
            }
        }
        discovered.sort_by(|left, right| {
            right
                .started_at
                .cmp(&left.started_at)
                .then_with(|| left.session_id.cmp(&right.session_id))
        });
        Ok(discovered)
    }

    pub async fn ask_session(
        &self,
        inquiry_id: &str,
        source_session_id: &str,
        target_session_id: &str,
        question: &str,
        progress: Option<mpsc::UnboundedSender<InquiryPhase>>,
        cancellation: CancellationToken,
    ) -> Result<InquiryOutcome, CoordinationError> {
        validate_inquiry_id(inquiry_id)
            .map_err(|e| CoordinationError::new(CoordinationErrorCode::InvalidRequest, e))?;
        self.require_source(source_session_id)?;
        if question.len() > MAX_QUESTION_BYTES || question.trim().is_empty() {
            return Err(CoordinationError::new(
                CoordinationErrorCode::InvalidRequest,
                format!("question must be nonempty and at most {MAX_QUESTION_BYTES} bytes"),
            ));
        }
        let payload = InquiryPayload {
            source_incarnation: self.shared.incarnation.clone(),
            source_session_id: source_session_id.to_owned(),
            source_cwd: self
                .shared
                .sessions
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .get(source_session_id)
                .expect("owned source")
                .canonical_cwd
                .clone(),
            target_session_id: target_session_id.to_owned(),
            question: question.to_owned(),
        };
        let key = (source_session_id.to_owned(), inquiry_id.to_owned());
        let mut outgoing = self.shared.outgoing.lock().await;
        outgoing.retain(|_, record| !record.expired());
        let record = if let Some(record) = outgoing.get(&key) {
            if record.payload != payload {
                return Err(CoordinationError::new(
                    CoordinationErrorCode::Conflict,
                    "inquiry id already belongs to a different payload",
                ));
            }
            Arc::clone(record)
        } else {
            if let Some(audit) = self.durable_inquiry(inquiry_id, source_session_id).await? {
                if audit.target_session_id != target_session_id || audit.question != question {
                    return Err(CoordinationError::new(
                        CoordinationErrorCode::Conflict,
                        "inquiry id already belongs to a different durable request",
                    ));
                }
                return Ok(audit.outcome);
            }
            let record = Arc::new(InquiryRecord::new(payload, InquiryPhase::Discovering));
            outgoing.insert(key, Arc::clone(&record));
            let sender = self.clone();
            let task_record = Arc::clone(&record);
            let task_id = inquiry_id.to_owned();
            tokio::spawn(async move {
                sender.run_outgoing(task_id, task_record).await;
            });
            record
        };
        drop(outgoing);
        let mut result = record.result.subscribe();
        let mut phases = record.progress.subscribe();
        send_progress(&progress, *phases.borrow());
        loop {
            if let Some(outcome) = result.borrow().clone() {
                return Ok(outcome);
            }
            tokio::select! {
                _ = cancellation.cancelled(), if !record.cancellation.is_cancelled() => {
                    record.cancellation.cancel(InquiryCancellationReason::Explicit);
                }
                changed = result.changed() => {
                    changed.map_err(|_| CoordinationError::new(CoordinationErrorCode::Failed, "inquiry result channel closed"))?;
                }
                changed = phases.changed() => {
                    changed.map_err(|_| CoordinationError::new(CoordinationErrorCode::Failed, "inquiry progress channel closed"))?;
                    send_progress(&progress, *phases.borrow_and_update());
                }
            }
        }
    }

    async fn run_outgoing(&self, id: String, record: Arc<InquiryRecord>) {
        let (progress, mut phases) = mpsc::unbounded_channel();
        let observed = Arc::clone(&record);
        let relay = tokio::spawn(async move {
            while let Some(phase) = phases.recv().await {
                observed.progress.send_replace(phase);
            }
        });
        let result = tokio::select! {
            biased;
            _ = self.shared.cancel.cancelled() => Err(CoordinationError::new(
                CoordinationErrorCode::SourceUnavailable, "source runtime stopped")),
            _ = record.cancellation.cancelled() => Ok(record.cancellation.outcome(&id)),
            result = timeout(INQUIRY_DEADLINE, self.deliver_inquiry(&id, &record, progress)) => {
                result.unwrap_or_else(|_| Err(CoordinationError::new(
                    CoordinationErrorCode::TimedOut, "coordination inquiry deadline exceeded")))
            }
        };
        // deliver_inquiry owns the only sender, including when its future is dropped.
        let _ = relay.await;
        let outcome = result.unwrap_or_else(|error| {
            let status = match error.code {
                CoordinationErrorCode::TimedOut => InquiryStatus::TimedOut,
                CoordinationErrorCode::PermissionDenied => InquiryStatus::Rejected,
                CoordinationErrorCode::NotFound
                | CoordinationErrorCode::TargetRestarted
                | CoordinationErrorCode::RuntimeUnavailable
                | CoordinationErrorCode::SourceUnavailable => InquiryStatus::Unavailable,
                _ => InquiryStatus::Failed,
            };
            InquiryOutcome::terminal(&id, status, error)
        });
        if matches!(
            outcome.status,
            InquiryStatus::Cancelled | InquiryStatus::TimedOut
        ) {
            let target = record
                .target
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if let Some(target) = target {
                // Cancel the pinned instance, never re-resolve onto a restarted peer.
                let _ = request_peer(
                    &self.shared,
                    &target,
                    &record.payload.source_session_id,
                    Request::Cancel {
                        inquiry_id: id.clone(),
                        target_session_id: record.payload.target_session_id.clone(),
                    },
                )
                .await;
            }
        }
        record.complete(outcome);
    }

    async fn deliver_inquiry(
        &self,
        id: &str,
        record: &InquiryRecord,
        progress: mpsc::UnboundedSender<InquiryPhase>,
    ) -> Result<InquiryOutcome, CoordinationError> {
        self.require_ready()?;
        let payload = &record.payload;
        let mut target = self
            .resolve_target(&payload.source_session_id, &payload.target_session_id)
            .await?;
        *record.target.lock().unwrap_or_else(|e| e.into_inner()) = Some(target.clone());
        let request = Request::Ask {
            inquiry_id: id.to_owned(),
            target_session_id: payload.target_session_id.clone(),
            question: payload.question.clone(),
        };
        let mut delay = RECONNECT_MIN_DELAY;
        loop {
            match request_inquiry(
                &self.shared,
                &target,
                &payload.source_session_id,
                &request,
                Some(&progress),
            )
            .await
            {
                Ok(outcome) => return Ok(outcome),
                Err(InquiryRequestError::Remote(error)) => return Err(error),
                Err(InquiryRequestError::Transport(error)) if !error.retryable => {
                    return Err(error);
                }
                Err(InquiryRequestError::Transport(_)) => {
                    let _ = progress.send(InquiryPhase::Reconnecting);
                    // Never log question, answer, endpoint or token.
                    tracing::debug!(inquiry_id = id, "coordination transport interrupted");
                }
            }
            tokio::time::sleep(delay).await;
            delay = std::cmp::min(delay.saturating_mul(2), RECONNECT_MAX_DELAY);
            match self
                .resolve_target(&payload.source_session_id, &payload.target_session_id)
                .await
            {
                Ok(current) => {
                    if current.peer_id != target.peer_id
                        || current.incarnation != target.incarnation
                    {
                        return Err(CoordinationError::new(
                            CoordinationErrorCode::TargetRestarted,
                            "target process restarted; the accepted inquiry will not be replayed",
                        ));
                    }
                    target = current;
                }
                Err(error) if target.expires_at < now_unix_ms() => return Err(error),
                Err(_) => {}
            }
        }
    }

    pub async fn get_inquiry(
        &self,
        inquiry_id: &str,
        source_session_id: &str,
    ) -> Result<InquiryState, CoordinationError> {
        self.require_source(source_session_id)?;
        validate_inquiry_id(inquiry_id)
            .map_err(|e| CoordinationError::new(CoordinationErrorCode::InvalidRequest, e))?;
        let mut outgoing = self.shared.outgoing.lock().await;
        outgoing.retain(|_, record| !record.expired());
        if let Some(record) = outgoing.get(&(source_session_id.to_owned(), inquiry_id.to_owned())) {
            // Hold the result snapshot while reading progress: complete() writes
            // result first, so a query cannot pair an old phase with a new result.
            let outcome = record.result.borrow();
            return Ok(InquiryState {
                inquiry_id: inquiry_id.to_owned(),
                phase: if outcome.is_some() {
                    InquiryPhase::Finished
                } else {
                    *record.progress.borrow()
                },
                outcome: outcome.clone(),
            });
        }
        drop(outgoing);
        let audit = self
            .durable_inquiry(inquiry_id, source_session_id)
            .await?
            .ok_or_else(|| {
                CoordinationError::new(
                    CoordinationErrorCode::NotFound,
                    "inquiry not found in this source session",
                )
            })?;
        Ok(InquiryState {
            inquiry_id: inquiry_id.to_owned(),
            phase: InquiryPhase::Finished,
            outcome: Some(audit.outcome),
        })
    }

    /// Do not let queries report success after source audit persistence fails.
    pub(crate) async fn audit_failed(&self, id: &str, source: &str, error: CoordinationError) {
        if let Some(record) = self
            .shared
            .outgoing
            .lock()
            .await
            .get(&(source.to_owned(), id.to_owned()))
        {
            record.complete(InquiryOutcome::terminal(id, InquiryStatus::Failed, error));
        }
    }

    async fn durable_inquiry(
        &self,
        id: &str,
        source: &str,
    ) -> Result<Option<super::inquiry::InquiryAudit>, CoordinationError> {
        let id = id.to_owned();
        let source = source.to_owned();
        let home = self.shared.grow_home.clone();
        tokio::task::spawn_blocking(move || {
            let mut found = None;
            let _ = crate::session::storage::stream_replay_grow_notifications_at(
                &source,
                &home,
                |notification| {
                    if let crate::extensions::notification::SessionUpdate::UiNotice(notice) =
                        notification.update
                        && notice.category
                            == crate::extensions::notification::UiNoticeCategory::Coordination
                        && notice.correlation_id == id
                        && notice.subject.as_deref() == Some("outgoing inquiry completed")
                        && let Some(details) = notice.details
                        && let Ok(audit) =
                            serde_json::from_str::<super::inquiry::InquiryAudit>(&details)
                        && audit.outcome.inquiry_id == id
                    {
                        found = Some(audit);
                    }
                },
            )
            .map_err(|error| {
                CoordinationError::new(CoordinationErrorCode::AuditFailure, error.to_string())
            })?;
            Ok(found)
        })
        .await
        .map_err(|_| {
            CoordinationError::new(
                CoordinationErrorCode::AuditFailure,
                "inquiry history reader failed",
            )
        })?
    }

    pub async fn cancel_session(
        &self,
        inquiry_id: &str,
        source_session_id: &str,
        target_session_id: &str,
    ) -> Result<bool, CoordinationError> {
        self.require_source(source_session_id)?;
        validate_inquiry_id(inquiry_id)
            .map_err(|e| CoordinationError::new(CoordinationErrorCode::InvalidRequest, e))?;
        {
            let outgoing = self.shared.outgoing.lock().await;
            if let Some(record) =
                outgoing.get(&(source_session_id.to_owned(), inquiry_id.to_owned()))
            {
                if record.payload.target_session_id != target_session_id {
                    return Err(CoordinationError::new(
                        CoordinationErrorCode::Conflict,
                        "inquiry target does not match",
                    ));
                }
                if record.result.borrow().is_some() {
                    return Ok(false);
                }
                record
                    .cancellation
                    .cancel(InquiryCancellationReason::Explicit);
                return Ok(true);
            }
        }
        self.require_ready()?;
        let target = self
            .resolve_target(source_session_id, target_session_id)
            .await?;
        match request_peer(
            &self.shared,
            &target,
            source_session_id,
            Request::Cancel {
                inquiry_id: inquiry_id.to_owned(),
                target_session_id: target_session_id.to_owned(),
            },
        )
        .await?
        {
            Response::Cancellation { accepted, .. } => Ok(accepted),
            Response::Error { code, message } => Err(remote_error(&code, message)),
            _ => Err(CoordinationError::new(
                CoordinationErrorCode::TransportError,
                "invalid cancel response",
            )),
        }
    }

    async fn resolve_target(
        &self,
        source_session_id: &str,
        target_session_id: &str,
    ) -> Result<PeerManifest, CoordinationError> {
        self.require_ready()?;
        self.require_source(source_session_id)?;
        if source_session_id == target_session_id {
            return Err(CoordinationError::new(
                CoordinationErrorCode::InvalidRequest,
                "source and target sessions must differ",
            ));
        }
        let manifests = self.live_manifests(source_session_id).await?;
        if conflicted_session_ids(&manifests).contains(target_session_id) {
            return Err(CoordinationError::new(
                CoordinationErrorCode::Conflict,
                "target session identity is conflicted",
            ));
        }
        let mut matches = manifests
            .into_iter()
            .filter(|m| m.sessions.iter().any(|s| s.session_id == target_session_id));
        let target = matches.next().ok_or_else(|| CoordinationError::new(CoordinationErrorCode::NotFound,
            "target primary session is not online; rediscover targets (subagents are not independently addressable)"))?;
        if matches.next().is_some() {
            return Err(CoordinationError::new(
                CoordinationErrorCode::Conflict,
                "target session identity is ambiguous",
            ));
        }
        Ok(target)
    }

    async fn live_manifests(
        &self,
        source_session_id: &str,
    ) -> Result<Vec<PeerManifest>, CoordinationError> {
        timeout(SETUP_TIMEOUT, async {
            let now = now_unix_ms();
            let manifests = read_all_manifests(&self.shared.grow_home).map_err(|e| {
                CoordinationError::new(CoordinationErrorCode::DiscoveryError, e.to_string())
            })?;
            let mut live = Vec::new();
            for manifest in manifests {
                if manifest.peer_id == self.shared.peer_id {
                    live.push(self.shared.current_manifest());
                } else if manifest.expires_at >= now {
                    live.push(manifest);
                } else {
                    match request_peer(
                        &self.shared,
                        &manifest,
                        source_session_id,
                        Request::Describe,
                    )
                    .await
                    {
                        Ok(Response::Description { peer }) => {
                            live.push(manifest_from_description(&manifest, peer))
                        }
                        _ => {
                            cleanup_stale_manifest(&self.shared.grow_home, &manifest);
                        }
                    }
                }
            }
            Ok(live)
        })
        .await
        .map_err(|_| {
            CoordinationError::retry(
                CoordinationErrorCode::DiscoveryError,
                "peer discovery timed out; no empty-online-set was inferred",
                1000,
            )
        })?
    }
}

impl Drop for CoordinationRuntime {
    fn drop(&mut self) {
        self.shared.cancel.cancel();
        let _guard = self
            .shared
            .publication
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _ = std::fs::remove_file(&self.shared.manifest_path);
        #[cfg(unix)]
        if let Some(endpoint) = self.shared.endpoint.get() {
            let _ = std::fs::remove_file(endpoint.path());
        }
    }
}

async fn run_heartbeat(shared: Arc<Shared>) {
    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = shared.cancel.cancelled() => break,
            _ = heartbeat.tick() => {
                let publisher = Arc::clone(&shared);
                if let Ok(Err(error)) = tokio::task::spawn_blocking(move || publisher.publish()).await {
                    tracing::warn!(error = %error, "failed to refresh coordination lease");
                }
            }
        }
    }
}

async fn run_server(listener: LocalListener, shared: Arc<Shared>) {
    loop {
        tokio::select! {
            _ = shared.cancel.cancelled() => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    let shared = Arc::clone(&shared);
                    tokio::spawn(async move {
                        if serve_connection(stream, shared).await.is_err() {
                            tracing::debug!(error_code = "connection_error", "coordination connection ended");
                        }
                    });
                }
                Err(error) => {
                    tracing::warn!(error = %error, "coordination listener accept failed");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        }
    }
}

async fn serve_connection(mut stream: LocalStream, shared: Arc<Shared>) -> Result<(), String> {
    let hello: ClientHello = timeout(SETUP_TIMEOUT, read_json(&mut stream))
        .await
        .map_err(|_| "coordination handshake timed out".to_owned())?
        .map_err(|error| error.to_string())?;
    let source = validate_hello(&shared, &hello);
    let rejection = source.as_ref().err().cloned();
    let server_hello = ServerHello {
        protocol_version: PROTOCOL_VERSION,
        peer_id: shared.peer_id.clone(),
        incarnation: shared.incarnation.clone(),
        accepted: rejection.is_none(),
        error: rejection,
    };
    write_json(&mut stream, &server_hello)
        .await
        .map_err(|error| error.to_string())?;
    if !server_hello.accepted {
        return Ok(());
    }
    let source = source.expect("accepted coordination handshake has a source manifest");

    let request: Request = timeout(SETUP_TIMEOUT, read_json(&mut stream))
        .await
        .map_err(|_| "coordination request timed out".to_owned())?
        .map_err(|error| error.to_string())?;
    match request {
        Request::Ping => write_response(&mut stream, Response::Pong).await,
        Request::Describe => {
            write_response(
                &mut stream,
                Response::Description {
                    peer: PeerDescription::from(&shared.current_manifest()),
                },
            )
            .await
        }
        Request::Ask {
            inquiry_id,
            target_session_id,
            question,
        } => {
            let source_cwd = source
                .sessions
                .iter()
                .find(|session| session.session_id == hello.source_session_id)
                .expect("validated source session is present")
                .canonical_cwd
                .clone();
            let response_inquiry_id = inquiry_id.clone();
            let accepted = accept_inquiry(
                Arc::clone(&shared),
                source,
                InquiryPayload {
                    source_incarnation: hello.incarnation,
                    source_session_id: hello.source_session_id,
                    source_cwd,
                    target_session_id,
                    question,
                },
                inquiry_id,
            )
            .await;
            let (mut progress, mut result) = match accepted {
                Ok(accepted) => accepted,
                Err((code, message)) => {
                    return write_response(&mut stream, Response::Error { code, message }).await;
                }
            };
            let initial_phase = *progress.borrow();
            write_response(
                &mut stream,
                Response::Progress {
                    inquiry_id: response_inquiry_id.clone(),
                    phase: initial_phase,
                },
            )
            .await?;
            loop {
                let terminal = result.borrow().clone();
                if let Some(outcome) = terminal {
                    return write_response(&mut stream, Response::Inquiry { outcome }).await;
                }
                tokio::select! {
                    changed = result.changed() => {
                        changed.map_err(|_| "coordination inquiry result channel closed".to_owned())?;
                    }
                    changed = progress.changed() => {
                        changed.map_err(|_| "coordination inquiry progress channel closed".to_owned())?;
                        let phase = *progress.borrow_and_update();
                        write_response(
                            &mut stream,
                            Response::Progress {
                                inquiry_id: response_inquiry_id.clone(),
                                phase,
                            },
                        )
                        .await?;
                    }
                }
            }
        }
        Request::Cancel {
            inquiry_id,
            target_session_id,
        } => {
            let accepted = cancel_inquiry(
                &shared,
                &hello.peer_id,
                &hello.source_session_id,
                &inquiry_id,
                &target_session_id,
            )
            .await;
            write_response(
                &mut stream,
                Response::Cancellation {
                    inquiry_id,
                    accepted,
                },
            )
            .await
        }
    }
}

async fn write_response(stream: &mut LocalStream, response: Response) -> Result<(), String> {
    write_json(stream, &response)
        .await
        .map_err(|error| error.to_string())
}

fn validate_hello(shared: &Shared, hello: &ClientHello) -> Result<PeerManifest, String> {
    if uuid::Uuid::parse_str(&hello.peer_id).is_err() {
        return Err("invalid source peer identity".to_owned());
    }
    if hello.protocol_version != PROTOCOL_VERSION {
        return Err("unsupported coordination protocol version".to_owned());
    }
    if !constant_time_eq(hello.bearer_token.as_bytes(), shared.token.as_bytes()) {
        return Err("invalid coordination bearer token".to_owned());
    }
    let path = peers_dir(&shared.grow_home).join(format!("{}.json", hello.peer_id));
    let source = read_manifest(&path).map_err(|_| "source peer is unavailable".to_owned())?;
    if source.incarnation != hello.incarnation
        || source.expires_at < now_unix_ms()
        || !source
            .sessions
            .iter()
            .any(|session| session.session_id == hello.source_session_id)
    {
        return Err("source session identity is invalid".to_owned());
    }
    Ok(source)
}

async fn accept_inquiry(
    shared: Arc<Shared>,
    source: PeerManifest,
    payload: InquiryPayload,
    inquiry_id: String,
) -> Result<
    (
        watch::Receiver<InquiryPhase>,
        watch::Receiver<Option<InquiryOutcome>>,
    ),
    (String, String),
> {
    validate_inquiry_id(&inquiry_id)
        .map_err(|message| ("invalid_inquiry_id".to_owned(), message))?;
    if payload.question.as_bytes().len() > MAX_QUESTION_BYTES {
        return Err((
            "question_too_large".to_owned(),
            format!("coordination question exceeds the {MAX_QUESTION_BYTES}-byte limit"),
        ));
    }
    if payload.question.trim().is_empty() {
        return Err((
            "invalid_question".to_owned(),
            "coordination question must not be empty".to_owned(),
        ));
    }
    if !shared.owns_session(&payload.target_session_id) {
        return Err((
            "target_unavailable".to_owned(),
            "target session is unavailable".to_owned(),
        ));
    }

    let key = (source.peer_id.clone(), inquiry_id.clone());
    let mut inquiries = shared.inquiries.lock().await;
    inquiries.retain(|_, record| {
        record
            .terminal_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_none_or(|terminal_at| terminal_at.elapsed() < TERMINAL_CACHE_TTL)
    });
    if let Some(existing) = inquiries.get(&key) {
        if existing.payload != payload {
            return Err((
                "inquiry_payload_conflict".to_owned(),
                "the inquiry id was already used with a different payload".to_owned(),
            ));
        }
        return Ok((existing.progress.subscribe(), existing.result.subscribe()));
    }

    let cancellation = InquiryCancellation::new();
    let (progress, progress_rx) = watch::channel(InquiryPhase::Receiving);
    let (result, result_rx) = watch::channel(None);
    let record = Arc::new(InquiryRecord {
        payload: payload.clone(),
        cancellation,
        progress,
        result,
        terminal_at: std::sync::Mutex::new(None),
        target: std::sync::Mutex::new(None),
    });
    inquiries.insert(key, Arc::clone(&record));
    drop(inquiries);

    tokio::spawn(run_inquiry(shared, source, inquiry_id, record));
    Ok((progress_rx, result_rx))
}

async fn run_inquiry(
    shared: Arc<Shared>,
    source: PeerManifest,
    inquiry_id: String,
    record: Arc<InquiryRecord>,
) {
    let deadline_cancellation = record.cancellation.clone();
    let deadline_result = record.result.subscribe();
    tokio::spawn(async move {
        tokio::select! {
            _ = tokio::time::sleep(INQUIRY_DEADLINE) => {
                deadline_cancellation.cancel(InquiryCancellationReason::TimedOut);
            }
            _ = wait_for_terminal(deadline_result) => {}
        }
    });
    let lease_cancellation = record.cancellation.clone();
    let lease_result = record.result.subscribe();
    let grow_home = shared.grow_home.clone();
    let source_peer_id = source.peer_id;
    let lease_source_peer_id = source_peer_id.clone();
    let source_incarnation = source.incarnation;
    let mut verified_expiry = source.expires_at;
    let source_session_id = record.payload.source_session_id.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = wait_for_terminal(lease_result.clone()) => break,
                _ = interval.tick() => {
                    let path = peers_dir(&grow_home).join(format!("{lease_source_peer_id}.json"));
                    let source_is_live = match read_manifest(&path) {
                        Ok(manifest) => {
                            let valid_identity = manifest.incarnation == source_incarnation
                                && manifest.sessions.iter().any(|session| session.session_id == source_session_id);
                            if valid_identity { verified_expiry = manifest.expires_at; }
                            valid_identity && verified_expiry >= now_unix_ms()
                        }
                        // A transient sharing/read failure is not a SessionClosed event.
                        // Never extend the last authenticated lease.
                        Err(_) => verified_expiry >= now_unix_ms(),
                    };
                    if !source_is_live {
                        lease_cancellation.cancel(InquiryCancellationReason::SourceUnavailable);
                        break;
                    }
                }
            }
        }
    });

    let (respond_to, response) = tokio::sync::oneshot::channel();
    let inbound = InboundInquiry {
        inquiry_id: inquiry_id.clone(),
        source_peer_id,
        source_session_id: record.payload.source_session_id.clone(),
        source_cwd: record.payload.source_cwd.clone(),
        target_session_id: record.payload.target_session_id.clone(),
        question: record.payload.question.clone(),
        cancellation: record.cancellation.clone(),
        progress: record.progress.clone(),
        respond_to,
    };
    let outcome = if shared.inquiry_tx.send(inbound).await.is_err() {
        InquiryOutcome::terminal(
            &inquiry_id,
            InquiryStatus::Unavailable,
            "target coordination dispatcher is unavailable",
        )
    } else {
        let mut response = response;
        tokio::select! {
            response = &mut response => response.unwrap_or_else(|_| InquiryOutcome::terminal(
                &inquiry_id,
                InquiryStatus::Unavailable,
                "target session stopped before answering",
            )),
            _ = record.cancellation.cancelled() => {
                match timeout(CANCEL_SETTLE_GRACE, &mut response).await {
                    Ok(Ok(outcome)) => outcome,
                    _ => record.cancellation.outcome(&inquiry_id),
                }
            }
        }
    };
    record.result.send_replace(Some(outcome));
    *record
        .terminal_at
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
}

async fn wait_for_terminal(mut result: watch::Receiver<Option<InquiryOutcome>>) {
    while result.borrow().is_none() {
        if result.changed().await.is_err() {
            break;
        }
    }
}

async fn cancel_inquiry(
    shared: &Shared,
    source_peer_id: &str,
    source_session_id: &str,
    inquiry_id: &str,
    target_session_id: &str,
) -> bool {
    let mut inquiries = shared.inquiries.lock().await;
    inquiries.retain(|_, record| {
        record
            .terminal_at
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_none_or(|terminal_at| terminal_at.elapsed() < TERMINAL_CACHE_TTL)
    });
    let Some(record) = inquiries.get(&(source_peer_id.to_owned(), inquiry_id.to_owned())) else {
        return false;
    };
    if record.payload.source_session_id != source_session_id
        || record.payload.target_session_id != target_session_id
    {
        return false;
    }
    record
        .cancellation
        .cancel(InquiryCancellationReason::Explicit);
    true
}

fn validate_inquiry_id(inquiry_id: &str) -> Result<(), String> {
    let parsed =
        uuid::Uuid::parse_str(inquiry_id).map_err(|_| "inquiryId must be a UUIDv7".to_owned())?;
    if parsed.get_version() != Some(uuid::Version::SortRand) {
        return Err("inquiryId must be a UUIDv7".to_owned());
    }
    Ok(())
}

fn send_progress(progress: &Option<mpsc::UnboundedSender<InquiryPhase>>, phase: InquiryPhase) {
    if let Some(progress) = progress {
        let _ = progress.send(phase);
    }
}

fn transport_error(error: impl std::fmt::Display) -> CoordinationError {
    CoordinationError::retry(
        CoordinationErrorCode::TransportError,
        error.to_string(),
        250,
    )
}

fn io_error(error: io::Error) -> CoordinationError {
    if error.kind() == io::ErrorKind::PermissionDenied {
        CoordinationError::new(CoordinationErrorCode::PermissionDenied, error.to_string())
    } else {
        transport_error(error)
    }
}

fn remote_error(code: &str, message: String) -> CoordinationError {
    let code = match code {
        "inquiry_payload_conflict" => CoordinationErrorCode::Conflict,
        "target_unavailable" => CoordinationErrorCode::NotFound,
        "invalid_inquiry_id" | "question_too_large" | "invalid_question" => {
            CoordinationErrorCode::InvalidRequest
        }
        _ => CoordinationErrorCode::Failed,
    };
    CoordinationError::new(code, message)
}

async fn request_peer(
    source: &Shared,
    target: &PeerManifest,
    source_session_id: &str,
    request: Request,
) -> Result<Response, CoordinationError> {
    timeout(SETUP_TIMEOUT, async {
        let mut stream = connect_peer(source, target, source_session_id).await?;
        write_json(&mut stream, &request)
            .await
            .map_err(transport_error)?;
        read_json(&mut stream).await.map_err(transport_error)
    })
    .await
    .map_err(|_| transport_error("coordination RPC timed out"))?
}

async fn request_inquiry(
    source: &Shared,
    target: &PeerManifest,
    source_session_id: &str,
    request: &Request,
    progress: Option<&mpsc::UnboundedSender<InquiryPhase>>,
) -> Result<InquiryOutcome, InquiryRequestError> {
    let mut stream = connect_peer(source, target, source_session_id)
        .await
        .map_err(InquiryRequestError::Transport)?;
    timeout(SETUP_TIMEOUT, write_json(&mut stream, request))
        .await
        .map_err(|_| InquiryRequestError::Transport(transport_error("inquiry send timed out")))?
        .map_err(|error| InquiryRequestError::Transport(transport_error(error)))?;
    loop {
        let response: Response = read_json(&mut stream)
            .await
            .map_err(|error| InquiryRequestError::Transport(transport_error(error)))?;
        match response {
            Response::Progress { phase, .. } => {
                if let Some(progress) = progress {
                    let _ = progress.send(phase);
                }
            }
            Response::Inquiry { outcome } => return Ok(outcome),
            Response::Error { code, message } => {
                return Err(InquiryRequestError::Remote(remote_error(&code, message)));
            }
            _ => {
                return Err(InquiryRequestError::Transport(transport_error(
                    "invalid inquiry response",
                )));
            }
        }
    }
}

async fn connect_peer(
    source: &Shared,
    target: &PeerManifest,
    source_session_id: &str,
) -> Result<LocalStream, CoordinationError> {
    let mut stream = timeout(CONNECT_TIMEOUT, LocalStream::connect(&target.endpoint))
        .await
        .map_err(|_| transport_error("coordination connect timed out"))?
        .map_err(io_error)?;
    let hello = ClientHello {
        protocol_version: PROTOCOL_VERSION,
        peer_id: source.peer_id.clone(),
        incarnation: source.incarnation.clone(),
        bearer_token: target.token.clone(),
        source_session_id: source_session_id.to_owned(),
    };
    let server: ServerHello = timeout(SETUP_TIMEOUT, async {
        write_json(&mut stream, &hello)
            .await
            .map_err(transport_error)?;
        read_json(&mut stream).await.map_err(transport_error)
    })
    .await
    .map_err(|_| transport_error("coordination handshake timed out"))??;
    if server.protocol_version != PROTOCOL_VERSION
        || server.peer_id != target.peer_id
        || server.incarnation != target.incarnation
    {
        return Err(CoordinationError::new(
            CoordinationErrorCode::Conflict,
            "coordination peer identity mismatch",
        ));
    }
    if !server.accepted {
        return Err(CoordinationError::new(
            CoordinationErrorCode::PermissionDenied,
            server
                .error
                .unwrap_or_else(|| "coordination handshake rejected".to_owned()),
        ));
    }
    Ok(stream)
}

fn manifest_from_description(base: &PeerManifest, description: PeerDescription) -> PeerManifest {
    let now = now_unix_ms();
    PeerManifest {
        schema_version: SCHEMA_VERSION,
        peer_id: description.peer_id,
        pid: base.pid,
        incarnation: description.incarnation,
        endpoint: base.endpoint.clone(),
        transport: base.transport.clone(),
        token: base.token.clone(),
        started_at: description.started_at,
        heartbeat_at: now,
        expires_at: now + LEASE_DURATION.as_millis() as i64,
        capabilities: description.capabilities,
        sessions: description.sessions,
    }
}

fn cleanup_stale_manifest(grow_home: &Path, manifest: &PeerManifest) {
    let path = peers_dir(grow_home).join(format!("{}.json", manifest.peer_id));
    let Ok(_lock) = super::manifest::lock_peer(&path) else {
        return;
    };
    let Ok(current) = read_manifest(&path) else {
        return;
    };
    if current.incarnation != manifest.incarnation || current.expires_at >= now_unix_ms() {
        return;
    }
    let _ = std::fs::remove_file(&path);
    // Socket cleanup belongs to its owning endpoint, never to discovery.
    // A failed RPC is not authority to unlink another process's listener.
    let _ = std::fs::remove_file(path.with_extension("lock"));
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(id: &str, cwd: &str) -> LocalSessionSnapshot {
        LocalSessionSnapshot {
            session_id: id.to_owned(),
            canonical_cwd: cwd.to_owned(),
            main_agent: "Grow".to_owned(),
            activity: crate::agent::roster::RosterActivity::Idle,
            subagents: Default::default(),
        }
    }

    #[tokio::test]
    async fn unstarted_runtime_is_not_an_empty_online_set() {
        let home = tempfile::tempdir().unwrap();
        let runtime = CoordinationRuntime::new(home.path().to_owned());
        runtime.publish_sessions(vec![snapshot("source", "/repo")]);
        assert_eq!(
            runtime
                .list_active_sessions("source")
                .await
                .unwrap_err()
                .code,
            CoordinationErrorCode::RuntimeUnavailable
        );
        let id = uuid::Uuid::now_v7().to_string();
        let result = runtime
            .ask_session(
                &id,
                "source",
                "missing",
                "hello",
                None,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(
            result.error.as_ref().unwrap().code,
            CoordinationErrorCode::RuntimeUnavailable
        );
        assert_eq!(
            runtime
                .handle()
                .get_inquiry(&id, "source")
                .await
                .unwrap()
                .outcome,
            Some(result)
        );
    }

    #[tokio::test]
    async fn missing_target_has_structured_queryable_terminal_state() {
        let home = tempfile::tempdir().unwrap();
        let runtime = CoordinationRuntime::new(home.path().to_owned());
        runtime.ensure_started().await.unwrap();
        runtime.publish_sessions(vec![
            snapshot("source", "/repo"),
            snapshot("other", "/repo"),
        ]);
        let id = uuid::Uuid::now_v7().to_string();
        let result = runtime
            .ask_session(
                &id,
                "source",
                "missing",
                "hello",
                None,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(result.status, InquiryStatus::Unavailable);
        assert_eq!(
            result.error.as_ref().unwrap().code,
            CoordinationErrorCode::NotFound
        );
        assert!(
            runtime.handle().get_inquiry(&id, "other").await.is_err(),
            "another local session cannot read this inquiry"
        );
        assert_eq!(
            runtime
                .handle()
                .get_inquiry(&id, "source")
                .await
                .unwrap()
                .outcome,
            Some(result)
        );
    }

    #[tokio::test]
    async fn concurrent_retries_share_one_run_and_expose_running_state() {
        let home = tempfile::tempdir().unwrap();
        let source = CoordinationRuntime::new(home.path().to_owned());
        let target = CoordinationRuntime::new(home.path().to_owned());
        source.ensure_started().await.unwrap();
        target.ensure_started().await.unwrap();
        source.publish_sessions(vec![snapshot("source", "/repo")]);
        target.publish_sessions(vec![snapshot("target", "/repo")]);
        let mut incoming = target.take_inquiry_receiver().unwrap();
        let id = uuid::Uuid::now_v7().to_string();
        let first = source.ask_session(
            &id,
            "source",
            "target",
            "work?",
            None,
            CancellationToken::new(),
        );
        let retry = source.ask_session(
            &id,
            "source",
            "target",
            "work?",
            None,
            CancellationToken::new(),
        );
        let answer = async {
            let inquiry = incoming.recv().await.unwrap();
            inquiry.progress.send_replace(InquiryPhase::Running);
            timeout(Duration::from_secs(2), async {
                loop {
                    let state = source.handle().get_inquiry(&id, "source").await.unwrap();
                    assert!(state.outcome.is_none());
                    if state.phase == InquiryPhase::Running {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert!(incoming.try_recv().is_err());
            inquiry
                .respond_to
                .send(InquiryOutcome::answered(&id, "one answer".into()))
                .unwrap();
        };
        let (first, retry, ()) = tokio::join!(first, retry, answer);
        assert_eq!(first.unwrap(), retry.unwrap());
        assert_eq!(
            source
                .handle()
                .get_inquiry(&id, "source")
                .await
                .unwrap()
                .phase,
            InquiryPhase::Finished
        );
        let conflict = source
            .ask_session(
                &id,
                "source",
                "target",
                "different",
                None,
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(conflict.code, CoordinationErrorCode::Conflict);
    }

    #[tokio::test]
    async fn stale_probe_cannot_reap_a_live_peers_manifest_or_socket() {
        let home = tempfile::tempdir().unwrap();
        let runtime = CoordinationRuntime::new(home.path().to_owned());
        runtime.ensure_started().await.unwrap();
        let mut manifest = runtime.shared.current_manifest();
        manifest.expires_at = 0;
        write_manifest(&runtime.shared.manifest_path, &manifest).unwrap();
        cleanup_stale_manifest(home.path(), &manifest);
        assert!(runtime.shared.manifest_path.exists());
        #[cfg(unix)]
        assert!(manifest.endpoint.exists());
    }

    #[tokio::test]
    async fn reconnect_never_replays_to_a_new_target_incarnation() {
        let home = tempfile::tempdir().unwrap();
        let source = CoordinationRuntime::new(home.path().to_owned());
        source.ensure_started().await.unwrap();
        source.publish_sessions(vec![snapshot("source", "/repo")]);
        let endpoint = PrivateEndpoint::new().unwrap();
        let listener = LocalListener::bind(endpoint.path()).unwrap();
        let mut target = source.shared.current_manifest();
        target.peer_id = uuid::Uuid::new_v4().to_string();
        target.endpoint = endpoint.path().to_owned();
        target.sessions[0].session_id = "target".into();
        let path = peers_dir(home.path()).join(format!("{}.json", target.peer_id));
        write_manifest(&path, &target).unwrap();
        let replacement = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _: ClientHello = read_json(&mut stream).await.unwrap();
            write_json(
                &mut stream,
                &ServerHello {
                    protocol_version: PROTOCOL_VERSION,
                    peer_id: target.peer_id.clone(),
                    incarnation: target.incarnation.clone(),
                    accepted: true,
                    error: None,
                },
            )
            .await
            .unwrap();
            let request: Request = read_json(&mut stream).await.unwrap();
            assert!(matches!(request, Request::Ask { .. }));
            target.incarnation = uuid::Uuid::new_v4().to_string();
            write_manifest(&path, &target).unwrap();
            // Lost response + a new incarnation must never trigger a new Ask.
            drop(stream);
            assert!(
                timeout(Duration::from_secs(1), listener.accept())
                    .await
                    .is_err()
            );
        });
        let id = uuid::Uuid::now_v7().to_string();
        let result = timeout(
            Duration::from_secs(3),
            source.ask_session(
                &id,
                "source",
                "target",
                "work?",
                None,
                CancellationToken::new(),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            result.error.unwrap().code,
            CoordinationErrorCode::TargetRestarted
        );
        replacement.await.unwrap();
    }

    #[tokio::test]
    async fn transient_source_manifest_failure_preserves_only_the_last_verified_lease() {
        let home = tempfile::tempdir().unwrap();
        let source = CoordinationRuntime::new(home.path().to_owned());
        let target = CoordinationRuntime::new(home.path().to_owned());
        source.ensure_started().await.unwrap();
        target.ensure_started().await.unwrap();
        source.publish_sessions(vec![snapshot("source", "/repo")]);
        target.publish_sessions(vec![snapshot("target", "/repo")]);
        let mut incoming = target.take_inquiry_receiver().unwrap();
        let mut identity = source.shared.current_manifest();
        identity.expires_at = now_unix_ms() + 500;
        source.shared.cancel.cancel(); // Stop publication without changing identity.
        std::fs::remove_file(&source.shared.manifest_path).unwrap();
        let id = uuid::Uuid::now_v7().to_string();
        let _ = accept_inquiry(
            Arc::clone(&target.shared),
            identity.clone(),
            InquiryPayload {
                source_incarnation: identity.incarnation,
                source_session_id: "source".into(),
                source_cwd: "/repo".into(),
                target_session_id: "target".into(),
                question: "work?".into(),
            },
            id,
        )
        .await
        .unwrap();
        let inquiry = incoming.recv().await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!inquiry.cancellation.is_cancelled());
        timeout(Duration::from_secs(3), inquiry.cancellation.cancelled())
            .await
            .unwrap();
        assert_eq!(
            inquiry.cancellation.reason(),
            InquiryCancellationReason::SourceUnavailable
        );
        let _ = inquiry
            .respond_to
            .send(inquiry.cancellation.outcome(&inquiry.inquiry_id));
    }

    #[tokio::test]
    async fn describe_deadline_covers_a_peer_stalling_after_handshake() {
        let home = tempfile::tempdir().unwrap();
        let source = CoordinationRuntime::new(home.path().to_owned());
        source.ensure_started().await.unwrap();
        source.publish_sessions(vec![snapshot("source", "/repo")]);
        let endpoint = PrivateEndpoint::new().unwrap();
        let listener = LocalListener::bind(endpoint.path()).unwrap();
        let mut target = source.shared.current_manifest();
        target.endpoint = endpoint.path().to_owned();
        let expected = target.clone();
        let stalled = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let _: ClientHello = read_json(&mut stream).await.unwrap();
            write_json(
                &mut stream,
                &ServerHello {
                    protocol_version: PROTOCOL_VERSION,
                    peer_id: expected.peer_id,
                    incarnation: expected.incarnation,
                    accepted: true,
                    error: None,
                },
            )
            .await
            .unwrap();
            let _: Request = read_json(&mut stream).await.unwrap();
            std::future::pending::<()>().await;
        });
        let result = timeout(
            Duration::from_secs(5),
            request_peer(&source.shared, &target, "source", Request::Describe),
        )
        .await
        .unwrap();
        assert_eq!(
            result.unwrap_err().code,
            CoordinationErrorCode::TransportError
        );
        stalled.abort();
    }

    #[test]
    fn token_comparison_rejects_length_and_content_mismatch() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"short"));
        assert!(!constant_time_eq(b"same", b"diff"));
    }

    #[tokio::test]
    async fn one_runtime_lists_other_primary_sessions_but_not_source() {
        let home = tempfile::tempdir().unwrap();
        let runtime = CoordinationRuntime::new(home.path().to_path_buf());
        runtime.ensure_started().await.unwrap();
        runtime.publish_sessions(vec![snapshot("source", "/a"), snapshot("other", "/b")]);

        let sessions = runtime.list_active_sessions("source").await.unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "other");
    }

    #[tokio::test]
    async fn independent_runtimes_discover_each_other_through_manifest_and_ipc_identity() {
        let home = tempfile::tempdir().unwrap();
        let first = CoordinationRuntime::new(home.path().to_path_buf());
        let second = CoordinationRuntime::new(home.path().to_path_buf());
        first.ensure_started().await.unwrap();
        second.ensure_started().await.unwrap();
        first.publish_sessions(vec![snapshot("first", "/repo")]);
        second.publish_sessions(vec![snapshot("second", "/repo")]);

        let sessions = first.list_active_sessions("first").await.unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "second");
        assert_eq!(sessions[0].canonical_cwd, "/repo");
    }

    #[tokio::test]
    async fn independent_processes_discover_and_answer_over_local_ipc() {
        const CHILD_HOME: &str = "GROW_COORDINATION_PROCESS_TEST_HOME";
        const CHILD_READY: &str = "GROW_COORDINATION_PROCESS_TEST_READY";
        const TEST_NAME: &str = "coordination::runtime::tests::independent_processes_discover_and_answer_over_local_ipc";

        if let (Ok(home), Ok(ready)) = (std::env::var(CHILD_HOME), std::env::var(CHILD_READY)) {
            let runtime = CoordinationRuntime::new(PathBuf::from(home));
            runtime.ensure_started().await.unwrap();
            runtime.publish_sessions(vec![snapshot("process-target", "/repo")]);
            let mut inquiries = runtime.take_inquiry_receiver().unwrap();
            std::fs::write(ready, b"ready").unwrap();
            let inquiry = timeout(Duration::from_secs(10), inquiries.recv())
                .await
                .unwrap()
                .unwrap();
            inquiry.progress.send_replace(InquiryPhase::Running);
            let inquiry_id = inquiry.inquiry_id.clone();
            let _ = inquiry.respond_to.send(InquiryOutcome::answered(
                inquiry_id,
                "answer from an independent Grow process".to_owned(),
            ));
            tokio::time::sleep(Duration::from_millis(100)).await;
            return;
        }

        let home = tempfile::tempdir().unwrap();
        let ready = home.path().join("child.ready");
        let executable = std::env::current_exe().unwrap();
        let mut child = std::process::Command::new(executable)
            .arg(TEST_NAME)
            .arg("--exact")
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(CHILD_HOME, home.path())
            .env(CHILD_READY, &ready)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();

        timeout(Duration::from_secs(10), async {
            while !ready.exists() {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("independent target process did not publish its manifest");

        let source = CoordinationRuntime::new(home.path().to_path_buf());
        source.ensure_started().await.unwrap();
        source.publish_sessions(vec![snapshot("process-source", "/repo")]);
        let sessions = source.list_active_sessions("process-source").await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "process-target");

        let outcome = source
            .ask_session(
                &uuid::Uuid::now_v7().to_string(),
                "process-source",
                "process-target",
                "what are you doing?",
                None,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(outcome.status, InquiryStatus::Answered);
        assert_eq!(
            outcome.answer.as_deref(),
            Some("answer from an independent Grow process")
        );

        let output = tokio::task::spawn_blocking(move || child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            output.status.success(),
            "child process failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    #[tokio::test]
    async fn bad_target_token_and_forged_source_session_are_rejected() {
        let home = tempfile::tempdir().unwrap();
        let source = CoordinationRuntime::new(home.path().to_path_buf());
        let target = CoordinationRuntime::new(home.path().to_path_buf());
        source.ensure_started().await.unwrap();
        target.ensure_started().await.unwrap();
        source.publish_sessions(vec![snapshot("source", "/source")]);
        target.publish_sessions(vec![snapshot("target", "/target")]);
        let mut target_manifest = target.shared.current_manifest();

        target_manifest.token = "wrong".to_owned();
        assert!(
            request_peer(&source.shared, &target_manifest, "source", Request::Ping)
                .await
                .is_err()
        );

        target_manifest.token = target.shared.token.clone();
        assert!(
            request_peer(&source.shared, &target_manifest, "forged", Request::Ping)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn expired_live_peer_is_recovered_by_describe() {
        let home = tempfile::tempdir().unwrap();
        let source = CoordinationRuntime::new(home.path().to_path_buf());
        let target = CoordinationRuntime::new(home.path().to_path_buf());
        source.ensure_started().await.unwrap();
        target.ensure_started().await.unwrap();
        source.publish_sessions(vec![snapshot("source", "/source")]);
        target.publish_sessions(vec![snapshot("target", "/target")]);
        let mut expired = target.shared.current_manifest();
        expired.expires_at = 0;
        write_manifest(&target.shared.manifest_path, &expired).unwrap();

        let sessions = source.list_active_sessions("source").await.unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "target");
    }

    #[tokio::test]
    async fn expired_unreachable_peer_is_cleaned_even_if_pid_is_reused() {
        let home = tempfile::tempdir().unwrap();
        let source = CoordinationRuntime::new(home.path().to_path_buf());
        source.ensure_started().await.unwrap();
        source.publish_sessions(vec![snapshot("source", "/source")]);
        let ghost_id = uuid::Uuid::new_v4().to_string();
        let ghost_path = peers_dir(home.path()).join(format!("{ghost_id}.json"));
        let ghost = PeerManifest {
            schema_version: SCHEMA_VERSION,
            peer_id: ghost_id.clone(),
            pid: std::process::id(),
            incarnation: uuid::Uuid::new_v4().to_string(),
            endpoint: std::env::temp_dir().join(format!("grow-coordination-{ghost_id}.sock")),
            transport: super::super::manifest::TRANSPORT_KIND.to_owned(),
            token: "ghost".to_owned(),
            started_at: 0,
            heartbeat_at: 0,
            expires_at: 0,
            capabilities: vec!["ping".to_owned()],
            sessions: merge_local_sessions(&HashMap::new(), vec![snapshot("ghost", "/ghost")], 0)
                .into_values()
                .collect(),
        };
        write_manifest(&ghost_path, &ghost).unwrap();

        let sessions = source.list_active_sessions("source").await.unwrap();

        assert!(sessions.is_empty());
        assert!(!ghost_path.exists());
    }

    #[tokio::test]
    async fn conflicting_session_identity_is_hidden_fail_closed() {
        let home = tempfile::tempdir().unwrap();
        let source = CoordinationRuntime::new(home.path().to_path_buf());
        let first = CoordinationRuntime::new(home.path().to_path_buf());
        let second = CoordinationRuntime::new(home.path().to_path_buf());
        source.ensure_started().await.unwrap();
        first.ensure_started().await.unwrap();
        second.ensure_started().await.unwrap();
        source.publish_sessions(vec![snapshot("source", "/source")]);
        first.publish_sessions(vec![snapshot("duplicate", "/one")]);
        second.publish_sessions(vec![snapshot("duplicate", "/two")]);

        let sessions = source.list_active_sessions("source").await.unwrap();

        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn disconnected_ask_reconnects_to_same_running_inquiry_and_terminal_cache() {
        let home = tempfile::tempdir().unwrap();
        let source = CoordinationRuntime::new(home.path().to_path_buf());
        let target = CoordinationRuntime::new(home.path().to_path_buf());
        source.ensure_started().await.unwrap();
        target.ensure_started().await.unwrap();
        source.publish_sessions(vec![snapshot("source", "/repo")]);
        target.publish_sessions(vec![snapshot("target", "/repo")]);
        let mut inbound = target.take_inquiry_receiver().unwrap();
        let target_manifest = target.shared.current_manifest();
        let inquiry_id = uuid::Uuid::now_v7().to_string();
        let request = Request::Ask {
            inquiry_id: inquiry_id.clone(),
            target_session_id: "target".to_owned(),
            question: "what are you doing?".to_owned(),
        };

        let mut stream = connect_peer(&source.shared, &target_manifest, "source")
            .await
            .unwrap();
        write_json(&mut stream, &request).await.unwrap();
        assert!(matches!(
            read_json::<_, Response>(&mut stream).await.unwrap(),
            Response::Progress {
                phase: InquiryPhase::Receiving,
                ..
            }
        ));
        let inquiry = timeout(Duration::from_secs(1), inbound.recv())
            .await
            .unwrap()
            .unwrap();
        drop(stream);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!inquiry.cancellation.is_cancelled());
        let _ = inquiry.respond_to.send(InquiryOutcome::answered(
            &inquiry_id,
            "working on tests".to_owned(),
        ));

        let outcome = source
            .ask_session(
                &inquiry_id,
                "source",
                "target",
                "what are you doing?",
                None,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(outcome.status, InquiryStatus::Answered);
        assert_eq!(outcome.answer.as_deref(), Some("working on tests"));

        let cached = source
            .ask_session(
                &inquiry_id,
                "source",
                "target",
                "what are you doing?",
                None,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(cached, outcome);
        assert!(inbound.try_recv().is_err(), "retry must not dispatch twice");
    }

    #[tokio::test]
    async fn same_inquiry_id_with_different_payload_is_rejected() {
        let home = tempfile::tempdir().unwrap();
        let source = CoordinationRuntime::new(home.path().to_path_buf());
        let target = CoordinationRuntime::new(home.path().to_path_buf());
        source.ensure_started().await.unwrap();
        target.ensure_started().await.unwrap();
        source.publish_sessions(vec![snapshot("source", "/repo")]);
        target.publish_sessions(vec![snapshot("target", "/repo")]);
        let mut inbound = target.take_inquiry_receiver().unwrap();
        let target_manifest = target.shared.current_manifest();
        let inquiry_id = uuid::Uuid::now_v7().to_string();
        let request = Request::Ask {
            inquiry_id: inquiry_id.clone(),
            target_session_id: "target".to_owned(),
            question: "first".to_owned(),
        };
        let mut stream = connect_peer(&source.shared, &target_manifest, "source")
            .await
            .unwrap();
        write_json(&mut stream, &request).await.unwrap();
        let _: Response = read_json(&mut stream).await.unwrap();
        let inquiry = inbound.recv().await.unwrap();
        drop(stream);

        let error = source
            .ask_session(
                &inquiry_id,
                "source",
                "target",
                "different",
                None,
                CancellationToken::new(),
            )
            .await
            .unwrap()
            .error
            .unwrap();
        assert_eq!(error.code, CoordinationErrorCode::Conflict);
        let _ = inquiry.respond_to.send(InquiryOutcome::answered(
            &inquiry_id,
            "first answer".to_owned(),
        ));
    }

    #[tokio::test]
    async fn explicit_source_cancel_reaches_accepted_target_inquiry() {
        let home = tempfile::tempdir().unwrap();
        let source = CoordinationRuntime::new(home.path().to_path_buf());
        let target = CoordinationRuntime::new(home.path().to_path_buf());
        source.ensure_started().await.unwrap();
        target.ensure_started().await.unwrap();
        source.publish_sessions(vec![snapshot("source", "/repo")]);
        target.publish_sessions(vec![snapshot("target", "/repo")]);
        let mut inbound = target.take_inquiry_receiver().unwrap();
        let inquiry_id = uuid::Uuid::now_v7().to_string();
        let cancellation = CancellationToken::new();
        let cancel_for_target = cancellation.clone();
        let target_task = async move {
            let inquiry = inbound.recv().await.unwrap();
            cancel_for_target.cancel();
            timeout(Duration::from_secs(2), inquiry.cancellation.cancelled())
                .await
                .unwrap();
            let outcome = inquiry.cancellation.outcome(&inquiry.inquiry_id);
            let _ = inquiry.respond_to.send(outcome);
        };
        let ask = source.ask_session(
            &inquiry_id,
            "source",
            "target",
            "cancel me",
            None,
            cancellation,
        );

        let (outcome, ()) = tokio::join!(ask, target_task);
        assert_eq!(outcome.unwrap().status, InquiryStatus::Cancelled);
    }

    #[tokio::test]
    async fn source_session_disappearance_cancels_target_work() {
        let home = tempfile::tempdir().unwrap();
        let source = CoordinationRuntime::new(home.path().to_path_buf());
        let target = CoordinationRuntime::new(home.path().to_path_buf());
        source.ensure_started().await.unwrap();
        target.ensure_started().await.unwrap();
        source.publish_sessions(vec![snapshot("source", "/repo")]);
        target.publish_sessions(vec![snapshot("target", "/repo")]);
        let mut inbound = target.take_inquiry_receiver().unwrap();
        let inquiry_id = uuid::Uuid::now_v7().to_string();
        let target_task = async {
            let inquiry = inbound.recv().await.unwrap();
            source.publish_sessions(Vec::new());
            timeout(Duration::from_secs(4), inquiry.cancellation.cancelled())
                .await
                .unwrap();
            assert_eq!(
                inquiry.cancellation.reason(),
                InquiryCancellationReason::SourceUnavailable
            );
            let outcome = inquiry.cancellation.outcome(&inquiry.inquiry_id);
            let _ = inquiry.respond_to.send(outcome);
        };
        let ask = source.ask_session(
            &inquiry_id,
            "source",
            "target",
            "cancel when I close",
            None,
            CancellationToken::new(),
        );

        let (outcome, ()) = tokio::join!(ask, target_task);
        assert_eq!(outcome.unwrap().status, InquiryStatus::Cancelled);
    }
}
