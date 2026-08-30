use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::local_ipc::frame::{read_json, write_json};
use crate::local_ipc::transport::{LocalListener, LocalStream};

use super::manifest::{
    DiscoveredSession, HEARTBEAT_INTERVAL, LEASE_DURATION, LocalSessionSnapshot, PeerDescription,
    PeerManifest, PeerSession, SCHEMA_VERSION, conflicted_session_ids, ensure_private_runtime_dirs,
    merge_local_sessions, now_unix_ms, peers_dir, read_all_manifests, read_manifest,
    write_manifest,
};
use super::protocol::{ClientHello, PROTOCOL_VERSION, Request, Response, ServerHello};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

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
    endpoint: PathBuf,
    manifest_path: PathBuf,
    token: String,
    started_at: i64,
    sessions: RwLock<HashMap<String, PeerSession>>,
    cancel: CancellationToken,
}

impl Shared {
    fn current_manifest(&self) -> PeerManifest {
        let now = now_unix_ms();
        PeerManifest {
            schema_version: SCHEMA_VERSION,
            peer_id: self.peer_id.clone(),
            pid: std::process::id(),
            incarnation: self.incarnation.clone(),
            endpoint: self.endpoint.clone(),
            token: self.token.clone(),
            started_at: self.started_at,
            heartbeat_at: now,
            expires_at: now + LEASE_DURATION.as_millis() as i64,
            capabilities: vec!["ping".to_owned(), "describe".to_owned()],
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
    start_lock: tokio::sync::Mutex<()>,
    started: AtomicBool,
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
        let endpoint = std::env::temp_dir().join(format!("grow-coordination-{peer_id}.sock"));
        let manifest_path = peers_dir(&grow_home).join(format!("{peer_id}.json"));
        Self {
            shared: Arc::new(Shared {
                grow_home,
                peer_id,
                incarnation,
                endpoint,
                manifest_path,
                token,
                started_at: now_unix_ms(),
                sessions: RwLock::new(HashMap::new()),
                cancel: CancellationToken::new(),
            }),
            start_lock: tokio::sync::Mutex::new(()),
            started: AtomicBool::new(false),
        }
    }

    pub async fn ensure_started(&self) -> Result<(), CoordinationStartError> {
        if self.started.load(Ordering::Acquire) {
            return Ok(());
        }
        let _guard = self.start_lock.lock().await;
        if self.started.load(Ordering::Acquire) {
            return Ok(());
        }

        ensure_private_runtime_dirs(&self.shared.grow_home)?;
        #[cfg(unix)]
        if self.shared.endpoint.exists() {
            std::fs::remove_file(&self.shared.endpoint)?;
        }
        let listener = LocalListener::bind(&self.shared.endpoint)?;
        write_manifest(&self.shared.manifest_path, &self.shared.current_manifest())?;
        self.started.store(true, Ordering::Release);
        tokio::spawn(run_server(listener, Arc::clone(&self.shared)));
        Ok(())
    }

    pub fn is_started(&self) -> bool {
        self.started.load(Ordering::Acquire)
    }

    pub fn publish_sessions(&self, snapshots: Vec<LocalSessionSnapshot>) {
        let mut sessions = self
            .shared
            .sessions
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *sessions = merge_local_sessions(&sessions, snapshots, now_unix_ms());
        drop(sessions);
        if self.started.load(Ordering::Acquire)
            && let Err(error) =
                write_manifest(&self.shared.manifest_path, &self.shared.current_manifest())
        {
            tracing::warn!(error = %error, "failed to publish coordination session snapshot");
        }
    }

    pub async fn cancelled(&self) {
        self.shared.cancel.cancelled().await;
    }

    pub async fn list_active_sessions(
        &self,
        source_session_id: &str,
    ) -> Result<Vec<DiscoveredSession>, String> {
        if !self.shared.owns_session(source_session_id) {
            return Err("source session is not owned by this Grow process".to_owned());
        }
        let manifests = self.live_manifests(source_session_id).await;
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

    async fn live_manifests(&self, source_session_id: &str) -> Vec<PeerManifest> {
        let now = now_unix_ms();
        let mut manifests = read_all_manifests(&self.shared.grow_home);
        if !manifests
            .iter()
            .any(|manifest| manifest.peer_id == self.shared.peer_id)
        {
            manifests.push(self.shared.current_manifest());
        }
        let mut live = Vec::new();
        for manifest in manifests {
            if manifest.peer_id == self.shared.peer_id {
                live.push(self.shared.current_manifest());
                continue;
            }
            if manifest.expires_at >= now {
                live.push(manifest);
                continue;
            }
            match request_peer(
                &self.shared,
                &manifest,
                source_session_id,
                Request::Describe,
            )
            .await
            {
                Ok(Response::Description { peer }) => {
                    live.push(manifest_from_description(&manifest, peer));
                }
                _ => cleanup_stale_manifest(&self.shared.grow_home, &manifest),
            }
        }
        live
    }
}

impl Drop for CoordinationRuntime {
    fn drop(&mut self) {
        self.shared.cancel.cancel();
        let _ = std::fs::remove_file(&self.shared.manifest_path);
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.shared.endpoint);
    }
}

async fn run_server(listener: LocalListener, shared: Arc<Shared>) {
    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = shared.cancel.cancelled() => break,
            _ = heartbeat.tick() => {
                if let Err(error) = write_manifest(&shared.manifest_path, &shared.current_manifest()) {
                    tracing::warn!(error = %error, "failed to refresh coordination lease");
                }
            }
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    let shared = Arc::clone(&shared);
                    tokio::spawn(async move {
                        if let Err(error) = timeout(REQUEST_TIMEOUT, serve_connection(stream, shared)).await {
                            tracing::debug!(error = %error, "coordination connection timed out");
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
    let hello: ClientHello = read_json(&mut stream)
        .await
        .map_err(|error| error.to_string())?;
    let rejection = validate_hello(&shared, &hello).err();
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

    let request: Request = read_json(&mut stream)
        .await
        .map_err(|error| error.to_string())?;
    let response = match request {
        Request::Ping => Response::Pong,
        Request::Describe => Response::Description {
            peer: PeerDescription::from(&shared.current_manifest()),
        },
        Request::Ask { .. } | Request::Cancel { .. } => Response::Error {
            code: "not_implemented".to_owned(),
            message: "coordination inquiries are not enabled yet".to_owned(),
        },
    };
    write_json(&mut stream, &response)
        .await
        .map_err(|error| error.to_string())
}

fn validate_hello(shared: &Shared, hello: &ClientHello) -> Result<(), String> {
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
    Ok(())
}

async fn request_peer(
    source: &Shared,
    target: &PeerManifest,
    source_session_id: &str,
    request: Request,
) -> Result<Response, String> {
    let mut stream = timeout(CONNECT_TIMEOUT, LocalStream::connect(&target.endpoint))
        .await
        .map_err(|_| "coordination connect timed out".to_owned())?
        .map_err(|error| error.to_string())?;
    let hello = ClientHello {
        protocol_version: PROTOCOL_VERSION,
        peer_id: source.peer_id.clone(),
        incarnation: source.incarnation.clone(),
        bearer_token: target.token.clone(),
        source_session_id: source_session_id.to_owned(),
    };
    write_json(&mut stream, &hello)
        .await
        .map_err(|error| error.to_string())?;
    let server: ServerHello = read_json(&mut stream)
        .await
        .map_err(|error| error.to_string())?;
    if !server.accepted
        || server.protocol_version != PROTOCOL_VERSION
        || server.peer_id != target.peer_id
        || server.incarnation != target.incarnation
    {
        return Err(server
            .error
            .unwrap_or_else(|| "coordination handshake rejected".to_owned()));
    }
    write_json(&mut stream, &request)
        .await
        .map_err(|error| error.to_string())?;
    read_json(&mut stream)
        .await
        .map_err(|error| error.to_string())
}

fn manifest_from_description(base: &PeerManifest, description: PeerDescription) -> PeerManifest {
    let now = now_unix_ms();
    PeerManifest {
        schema_version: SCHEMA_VERSION,
        peer_id: description.peer_id,
        pid: base.pid,
        incarnation: description.incarnation,
        endpoint: base.endpoint.clone(),
        token: base.token.clone(),
        started_at: description.started_at,
        heartbeat_at: now,
        expires_at: now + LEASE_DURATION.as_millis() as i64,
        capabilities: description.capabilities,
        sessions: description.sessions,
    }
}

fn cleanup_stale_manifest(grow_home: &Path, manifest: &PeerManifest) {
    let expected = peers_dir(grow_home).join(format!("{}.json", manifest.peer_id));
    let _ = std::fs::remove_file(expected);
    #[cfg(unix)]
    if manifest.endpoint.parent() == Some(std::env::temp_dir().as_path())
        && manifest
            .endpoint
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == format!("grow-coordination-{}.sock", manifest.peer_id))
    {
        let _ = std::fs::remove_file(&manifest.endpoint);
    }
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
}
