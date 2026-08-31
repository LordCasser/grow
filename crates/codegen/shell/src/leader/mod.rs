//! Leader-follower IPC architecture for shell.
//!
//! This module implements a single-leader-per-machine architecture where one leader
//! process manages the agent state while multiple local clients (TUI, IDE extensions, scripts)
//! communicate via Unix domain sockets.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                        Leader Process                        │
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                      Agent (MvpAgent)                    ││
//! │  │   - Shared state across all clients                      ││
//! │  │   - Persists to ~/.grow/                                 ││
//! │  └─────────────────────────────────────────────────────────┘│
//! │                           ▲                                  │
//! │                           │ ACP                              │
//! │  ┌────────────────────────┴────────────────────────────────┐│
//! │  │                   IPC Server (Unix Socket)               ││
//! │  │   - Routes messages between clients and agent            ││
//! │  │   - Namespaces request IDs to avoid collisions           ││
//! │  │   - Tracks session ownership for routing                 ││
//! │  └────────────────────────┬────────────────────────────────┘│
//! └───────────────────────────┼──────────────────────────────────┘
//!                             │ IPC (Unix socket at ~/.grow/leader.sock)
//!         ┌───────────────────┼───────────────────┐
//!         ▼                   ▼                   ▼
//! ┌───────────────┐   ┌───────────────┐   ┌───────────────┐
//! │   TUI Client  │   │  IDE Extension │   │  Script/SDK   │
//! │   (local IPC) │   │   (local IPC)  │   │  (local IPC)  │
//! └───────────────┘   └───────────────┘   └───────────────┘
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use shell::leader::{connect_or_spawn, ClientCapabilities, ClientMode};
//!
//! // Connect to existing leader or spawn a new one
//! let caps = ClientCapabilities {
//!     permission_mode: diagnostics::enums::PermissionMode::AlwaysApprove,
//!     default_model: Some("grow-3-fast".to_string()),
//! };
//! let conn = connect_or_spawn("my-client", ClientMode::Stdio, caps).await?;
//!
//! // Send/receive ACP messages
//! conn.send(r#"{"jsonrpc":"2.0","method":"test","id":1}"#.to_string())?;
//! if let Some(response) = conn.recv().await {
//!     println!("Got response: {}", response);
//! }
//! ```
mod client;
#[cfg(feature = "test-support")]
pub mod in_process;
mod lock;
pub mod protocol;
mod server;
#[cfg(test)]
pub(crate) mod test_support;
pub use crate::local_ipc::transport::listener_is_ready;
pub use client::{ClientError, DisconnectReason, LeaderClient, LeaderRegistration};
pub use lock::{
    LEADER_SOCKET_ENV, LeaderLock, LockError, lock_path, lock_path_in, socket_path, socket_path_in,
};
pub use protocol::{
    ClientCapabilities, ClientId, ClientMode, ControlCommand, ControlPayload,
    LEADER_PROTOCOL_VERSION, ShutdownReason,
};
use serde::{Deserialize, Serialize};
pub use server::{
    LeaderServerControlState, LeaderServerMetadata, ServerError, ServerHandle, run_leader_server,
    spawn_leader_server,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
const SPAWN_WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const SPAWN_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// Max wait for an evicted leader to exit before force-killing (relaunch drain ~5s).
const EVICT_WAIT_TIMEOUT: Duration = Duration::from_secs(8);
/// How long the SAME live grow flock-holder may stay unconnectable before
/// `connect_or_spawn` treats it as a "zombie leader" and evicts it.
const ZOMBIE_EVICT_DEADLINE: Duration = Duration::from_secs(30);
/// Whether `leader_version` is a strictly-older parseable semver than `baseline`.
/// Unparseable versions (e.g. dev `"unknown"`) return `false` — leave them alone.
pub fn leader_is_older_than(leader_version: &str, baseline: &str) -> bool {
    match (
        semver::Version::parse(leader_version),
        semver::Version::parse(baseline),
    ) {
        (Ok(leader), Ok(baseline)) => leader < baseline,
        _ => false,
    }
}
/// Base delay between reconnection attempts.
const RECONNECT_BASE_DELAY: Duration = Duration::from_secs(1);
/// Maximum delay between reconnection attempts (caps exponential backoff).
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(30);
/// Maximum reconnection attempts for bounded mode (headless/`grow -p`).
/// TUI mode uses unlimited retries controlled by a cancellation token.
const RECONNECT_MAX_ATTEMPTS_BOUNDED: u32 = 5;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaderDiscoveryState {
    Reachable,
    Stale,
    Unreachable,
    UnsupportedProtocol,
    Ambiguous,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaderTargetErrorCode {
    LeaderNotFound,
    SocketUnreachable,
    PidVerificationFailed,
    UnsupportedProtocol,
    AmbiguousTarget,
}
impl std::fmt::Display for LeaderTargetErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let code = match self {
            Self::LeaderNotFound => "leader_not_found",
            Self::SocketUnreachable => "socket_unreachable",
            Self::PidVerificationFailed => "pid_verification_failed",
            Self::UnsupportedProtocol => "unsupported_protocol",
            Self::AmbiguousTarget => "ambiguous_target",
        };
        f.write_str(code)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[error("{message}")]
pub struct LeaderTargetError {
    pub code: LeaderTargetErrorCode,
    pub message: String,
}
impl LeaderTargetError {
    fn new(code: LeaderTargetErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveLeaderInfo {
    pub pid: u32,
    pub socket_path: PathBuf,
    pub lock_path: PathBuf,
    pub leader_protocol_version: u32,
    pub leader_binary_version: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaderDescriptor {
    pub pid_from_lock: Option<u32>,
    pub lock_path: Option<PathBuf>,
    pub socket_path: Option<PathBuf>,
    pub classification: LeaderDiscoveryState,
    pub live_info: Option<LiveLeaderInfo>,
    pub target_error: Option<LeaderTargetErrorCode>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaderTargetSelection {
    pub descriptor: LeaderDescriptor,
}
impl LeaderTargetSelection {
    pub fn socket_path(&self) -> Option<&Path> {
        self.descriptor.socket_path.as_deref()
    }
    pub fn lock_path(&self) -> Option<&Path> {
        self.descriptor.lock_path.as_deref()
    }
    pub fn live_info(&self) -> Option<&LiveLeaderInfo> {
        self.descriptor.live_info.as_ref()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaderTarget {
    Local,
    Pid(u32),
}
fn build_live_leader_info(payload: ControlPayload) -> Result<LiveLeaderInfo, LeaderTargetError> {
    match payload {
        ControlPayload::LeaderInfo {
            pid,
            socket_path,
            lock_path,
            leader_protocol_version,
            leader_binary_version,
            ..
        } => Ok(LiveLeaderInfo {
            pid,
            socket_path,
            lock_path,
            leader_protocol_version,
            leader_binary_version,
        }),
        _ => Err(LeaderTargetError::new(
            LeaderTargetErrorCode::UnsupportedProtocol,
            "leader returned an unexpected control payload for GetLeaderInfo",
        )),
    }
}
async fn fetch_live_leader_info(socket_path: &Path) -> Result<LiveLeaderInfo, LeaderTargetError> {
    let client = LeaderClient::connect(
        socket_path.to_path_buf(),
        "grow-leader-discovery",
        ClientMode::Stdio,
        ClientCapabilities::default(),
    )
    .await
    .map_err(|error| {
        LeaderTargetError::new(
            LeaderTargetErrorCode::SocketUnreachable,
            format!(
                "failed to connect to leader socket {}: {}",
                socket_path.display(),
                error
            ),
        )
    })?;
    let result = async {
        let registration = client.registration();
        let protocol_version = registration.leader_protocol_version;
        if protocol_version != LEADER_PROTOCOL_VERSION {
            return Err(LeaderTargetError::new(
                LeaderTargetErrorCode::UnsupportedProtocol,
                format!(
                    "leader at {} uses unsupported protocol version {}",
                    socket_path.display(),
                    protocol_version
                ),
            ));
        }
        let payload = client
            .send_control(ControlCommand::GetLeaderInfo)
            .await
            .map_err(|error| {
                LeaderTargetError::new(
                    LeaderTargetErrorCode::SocketUnreachable,
                    format!(
                        "failed to query live leader info from {}: {}",
                        socket_path.display(),
                        error
                    ),
                )
            })?
            .map_err(|error| {
                LeaderTargetError::new(
                    LeaderTargetErrorCode::UnsupportedProtocol,
                    format!(
                        "leader at {} rejected GetLeaderInfo: {}",
                        socket_path.display(),
                        error
                    ),
                )
            })?;
        build_live_leader_info(payload)
    }
    .await;
    client.cancel();
    result
}
fn descriptor_from_paths(
    lock_path: Option<PathBuf>,
    socket_path: Option<PathBuf>,
    pid_from_lock: Option<u32>,
    live_info: Option<LiveLeaderInfo>,
    classification: LeaderDiscoveryState,
    target_error: Option<LeaderTargetErrorCode>,
) -> LeaderDescriptor {
    LeaderDescriptor {
        pid_from_lock,
        lock_path,
        socket_path,
        classification,
        live_info,
        target_error,
    }
}
async fn discover_leaders_in(root: &Path) -> Vec<LeaderDescriptor> {
    let lock_path = root.join("leader.lock");
    let socket_path = root.join("leader.sock");
    let lock_path = lock_path.exists().then_some(lock_path);
    let socket_path = socket_path.exists().then_some(socket_path);
    if lock_path.is_none() && socket_path.is_none() {
        return Vec::new();
    }
    let mut entries = Vec::new();
    {
        let pid_from_lock = lock_path
            .as_deref()
            .and_then(LeaderLock::read_pid_from_path);
        match (lock_path.clone(), socket_path.clone()) {
            (Some(lock_path), None) => entries.push(descriptor_from_paths(
                Some(lock_path),
                None,
                pid_from_lock,
                None,
                LeaderDiscoveryState::Stale,
                None,
            )),
            (None, Some(socket_path)) => match fetch_live_leader_info(&socket_path).await {
                Ok(live_info) => entries.push(descriptor_from_paths(
                    None,
                    Some(socket_path),
                    None,
                    Some(live_info),
                    LeaderDiscoveryState::Reachable,
                    None,
                )),
                Err(error) if error.code == LeaderTargetErrorCode::SocketUnreachable => {
                    entries.push(descriptor_from_paths(
                        None,
                        Some(socket_path),
                        None,
                        None,
                        LeaderDiscoveryState::Unreachable,
                        Some(LeaderTargetErrorCode::SocketUnreachable),
                    ));
                }
                Err(error) if error.code == LeaderTargetErrorCode::UnsupportedProtocol => {
                    entries.push(descriptor_from_paths(
                        None,
                        Some(socket_path),
                        None,
                        None,
                        LeaderDiscoveryState::UnsupportedProtocol,
                        Some(LeaderTargetErrorCode::UnsupportedProtocol),
                    ));
                }
                Err(error) => entries.push(descriptor_from_paths(
                    None,
                    Some(socket_path),
                    None,
                    None,
                    LeaderDiscoveryState::Ambiguous,
                    Some(error.code),
                )),
            },
            (Some(lock_path), Some(socket_path)) => {
                match fetch_live_leader_info(&socket_path).await {
                    Ok(live_info) => entries.push(descriptor_from_paths(
                        Some(lock_path),
                        Some(socket_path),
                        pid_from_lock,
                        Some(live_info),
                        LeaderDiscoveryState::Reachable,
                        None,
                    )),
                    Err(error) if error.code == LeaderTargetErrorCode::SocketUnreachable => {
                        entries.push(descriptor_from_paths(
                            Some(lock_path),
                            Some(socket_path),
                            pid_from_lock,
                            None,
                            LeaderDiscoveryState::Unreachable,
                            Some(LeaderTargetErrorCode::SocketUnreachable),
                        ));
                    }
                    Err(error) if error.code == LeaderTargetErrorCode::UnsupportedProtocol => {
                        entries.push(descriptor_from_paths(
                            Some(lock_path),
                            Some(socket_path),
                            pid_from_lock,
                            None,
                            LeaderDiscoveryState::UnsupportedProtocol,
                            Some(LeaderTargetErrorCode::UnsupportedProtocol),
                        ));
                    }
                    Err(error) => entries.push(descriptor_from_paths(
                        Some(lock_path),
                        Some(socket_path),
                        pid_from_lock,
                        None,
                        LeaderDiscoveryState::Ambiguous,
                        Some(error.code),
                    )),
                }
            }
            (None, None) => {}
        }
    }
    entries
}
pub async fn discover_leaders() -> Vec<LeaderDescriptor> {
    discover_leaders_in(&crate::util::grow_home::grow_home()).await
}
/// (pid, leader_binary_version) of socket-verified (Reachable) leaders; a
/// stale-lock-only descriptor is skipped (its `pid_from_lock` may be recycled).
fn reachable_leader_pids(leaders: &[LeaderDescriptor]) -> Vec<(u32, String)> {
    leaders
        .iter()
        .filter_map(|d| {
            d.live_info
                .as_ref()
                .map(|li| (li.pid, li.leader_binary_version.clone()))
        })
        .collect()
}
/// Best-effort, time-boxed kill of reachable leaders — reclaims a leader still
/// running after leader mode was disabled by policy (`reason`). Emits unified_log
/// (captured in unified.jsonl) so operators can attribute eviction kills; the `tracing`
/// lines are kept for local debug. Errors are logged, never fatal.
pub async fn kill_stale_reachable_leaders(reason: &'static str) {
    let targets = reachable_leader_pids(&discover_leaders().await);
    let discovered = targets.len();
    crate::unified_log::info(
        "leader.startup_kill.begin",
        None,
        Some(serde_json::json!({ "reason": reason, "discovered": discovered })),
    );
    let mut killed = 0usize;
    let mut failed = 0usize;
    let timed_out = tokio::time::timeout(Duration::from_secs(5), async {
        for (pid, dead_leader_ver) in &targets {
            match crate::util::kill_process_by_pid(*pid) {
                Ok(()) => {
                    killed += 1;
                    info!(pid = *pid, "killed stale reachable leader");
                    crate::unified_log::warn(
                        "leader.startup_kill.killed",
                        None,
                        Some(serde_json::json!({
                            "pid": *pid,
                            "dead_leader_ver": dead_leader_ver,
                            "reason": reason,
                            "killer_ver": version::VERSION,
                        })),
                    );
                }
                Err(e) => {
                    failed += 1;
                    warn!(pid = *pid, error = %e, "failed to kill stale leader");
                    crate::unified_log::warn(
                        "leader.startup_kill.failed",
                        None,
                        Some(serde_json::json!({
                            "pid": *pid,
                            "dead_leader_ver": dead_leader_ver,
                            "error": e.to_string(),
                        })),
                    );
                }
            }
        }
    })
    .await
    .is_err();
    crate::unified_log::info(
        "leader.startup_kill.done",
        None,
        Some(serde_json::json!({
            "reason": reason,
            "discovered": discovered,
            "killed": killed,
            "failed": failed,
            "timed_out": timed_out,
        })),
    );
}
fn resolve_target_from_descriptors(
    target: LeaderTarget,
    leaders: Vec<LeaderDescriptor>,
) -> Result<LeaderTargetSelection, LeaderTargetError> {
    match target {
        LeaderTarget::Local => {
            let reachable: Vec<_> = leaders
                .into_iter()
                .filter(|descriptor| descriptor.classification == LeaderDiscoveryState::Reachable)
                .collect();
            match reachable.len() {
                1 => Ok(LeaderTargetSelection {
                    descriptor: reachable.into_iter().next().expect("length checked"),
                }),
                0 => Err(LeaderTargetError::new(
                    LeaderTargetErrorCode::LeaderNotFound,
                    "no reachable local leader found",
                )),
                _ => Err(LeaderTargetError::new(
                    LeaderTargetErrorCode::AmbiguousTarget,
                    "multiple reachable local leaders found",
                )),
            }
        }
        LeaderTarget::Pid(pid) => {
            let matching: Vec<_> = leaders
                .into_iter()
                .filter(|descriptor| {
                    descriptor.pid_from_lock == Some(pid)
                        || descriptor
                            .live_info
                            .as_ref()
                            .is_some_and(|info| info.pid == pid)
                })
                .collect();
            if matching.is_empty() {
                return Err(LeaderTargetError::new(
                    LeaderTargetErrorCode::LeaderNotFound,
                    format!("no leader candidate found for pid {}", pid),
                ));
            }
            let reachable: Vec<_> = matching
                .iter()
                .filter(|descriptor| descriptor.classification == LeaderDiscoveryState::Reachable)
                .cloned()
                .collect();
            if reachable.len() != 1 {
                return Err(LeaderTargetError::new(
                    LeaderTargetErrorCode::PidVerificationFailed,
                    format!(
                        "pid {} did not resolve to exactly one reachable leader candidate",
                        pid
                    ),
                ));
            }
            let Some(descriptor) = reachable.into_iter().next() else {
                return Err(LeaderTargetError::new(
                    LeaderTargetErrorCode::PidVerificationFailed,
                    format!(
                        "pid {} did not resolve to a reachable leader candidate",
                        pid
                    ),
                ));
            };
            let live_pid = descriptor.live_info.as_ref().map(|info| info.pid);
            if live_pid != Some(pid) {
                return Err(LeaderTargetError::new(
                    LeaderTargetErrorCode::PidVerificationFailed,
                    format!(
                        "leader pid verification failed: lock file recorded {:?}, live leader reported {:?}",
                        descriptor.pid_from_lock, live_pid
                    ),
                ));
            }
            Ok(LeaderTargetSelection { descriptor })
        }
    }
}
pub async fn resolve_leader_target(
    target: LeaderTarget,
) -> Result<LeaderTargetSelection, LeaderTargetError> {
    let leaders = discover_leaders().await;
    resolve_target_from_descriptors(target, leaders)
}
#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("Lock error: {0}")]
    Lock(#[from] LockError),
    #[error("Client error: {0}")]
    Client(#[from] ClientError),
    #[error("Server error: {0}")]
    Server(#[from] ServerError),
    #[error("Failed to spawn leader: {0}")]
    SpawnFailed(String),
    #[error("Timeout waiting for leader to start")]
    Timeout,
    #[error("Reconnection cancelled")]
    Cancelled,
    #[error(
        "leader mode is unavailable under sandbox profile '{0}': the leader is a \
         separate, shared process this client cannot prove is confined by that \
         profile, so tools are not guaranteed to stay inside it. Disable the \
         profile at the source that selected it (CLI, env, config, or a managed \
         requirement)"
    )]
    SandboxConfinement(&'static str),
}
/// Handle for a connection to the leader process.
///
/// Provides send/receive methods for ACP message payloads.
/// The connection is automatically cleaned up when dropped.
pub struct LeaderConnection {
    client: LeaderClient,
}
impl LeaderConnection {
    /// Send an ACP message payload to the leader.
    ///
    /// The payload should be a valid JSON-RPC message. Request IDs will be
    /// namespaced by the leader to avoid collisions with other clients.
    pub fn send(&self, payload: String) -> Result<(), ConnectionError> {
        self.client.send(payload).map_err(ConnectionError::Client)
    }
    /// Send a leader control request over the existing IPC connection.
    ///
    /// This exposes the same capability-aware control surface as [`LeaderClient`],
    /// so callers using the public `connect_or_spawn` facade can issue process-level
    /// commands without reimplementing leader discovery or socket selection.
    pub async fn send_control(
        &self,
        command: ControlCommand,
    ) -> Result<Result<ControlPayload, crate::cpu_profile::ControlError>, ConnectionError> {
        self.client
            .send_control(command)
            .await
            .map_err(ConnectionError::Client)
    }
    /// Returns the negotiated registration metadata for this connection.
    pub fn registration(&self) -> &LeaderRegistration {
        self.client.registration()
    }
    /// Receive the next ACP message from the leader.
    ///
    /// Returns `None` if the connection is closed.
    pub async fn recv(&mut self) -> Option<String> {
        self.client.recv().await
    }
    /// Returns a receiver for the most recent `ShuttingDown` reason sent by the
    /// server before a planned shutdown.
    ///
    /// - `None` — no `ShuttingDown` message received yet (still connected or
    ///   connection ended without a planned shutdown announcement).
    /// - `Some(AutoUpdate)` — leader is restarting to install a binary update;
    ///   safe to reconnect immediately via `connect_or_spawn`.
    /// - `Some(Manual)` — deliberately stopped or unspecified shutdown.
    ///
    /// This is the primary entry point for first-party callers (TUI bridge,
    /// headless path, reconnection logic) because `connect_or_spawn` returns
    /// `LeaderConnection`, not `LeaderClient` directly.
    pub fn shutting_down_reason(&self) -> watch::Receiver<Option<protocol::ShutdownReason>> {
        self.client.shutting_down_reason()
    }
    /// Decompose this connection into raw channels.
    ///
    /// Useful for integration with other async code that needs direct channel access.
    pub fn into_channels(
        self,
    ) -> (
        mpsc::UnboundedSender<String>,
        mpsc::UnboundedReceiver<String>,
    ) {
        self.client.into_channels()
    }
    /// Decompose into raw channels plus the disconnect reason receiver.
    ///
    /// Like [`into_channels()`](Self::into_channels) but also returns a
    /// [`watch::Receiver<DisconnectReason>`] so the caller can observe
    /// why the connection ended (e.g., `LeaderShutdown` vs `ConnectionLost`).
    pub fn into_channels_with_disconnect(
        self,
    ) -> (
        mpsc::UnboundedSender<String>,
        mpsc::UnboundedReceiver<String>,
        watch::Receiver<DisconnectReason>,
    ) {
        self.client.into_channels_with_disconnect()
    }
}
/// Status of a reconnection attempt, observable by callers (e.g., TUI banner).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionStatus {
    /// Connected to the leader.
    ///
    /// `generation` is 0 for the initial connection and increments on every
    /// successful reconnect. Observers compare it against the last generation
    /// they handled, so a fast `Reconnecting -> Connected` flip coalesced by
    /// the watch channel still registers as a reconnect.
    Connected { generation: u64 },
    /// Attempting to reconnect (includes current attempt number).
    Reconnecting { attempt: u32 },
    /// Reconnection failed permanently.
    Failed { error: String },
}
/// Controls how many reconnection attempts are made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconnectPolicy {
    /// Retry indefinitely until the cancellation token fires.
    /// Suitable for interactive TUI sessions where the user expects persistence.
    Unbounded,
    /// Retry up to a fixed number of attempts, then fail.
    /// Suitable for headless/`grow -p` where hanging forever is unacceptable.
    Bounded { max_attempts: u32 },
}
impl ReconnectPolicy {
    /// Default bounded policy for headless/non-interactive modes.
    pub fn bounded() -> Self {
        Self::Bounded {
            max_attempts: RECONNECT_MAX_ATTEMPTS_BOUNDED,
        }
    }
    /// Default unbounded policy for interactive TUI mode.
    pub fn unbounded() -> Self {
        Self::Unbounded
    }
}
/// Holds the parameters needed to reconnect to a leader process.
///
/// Does **not** own the live channels — the caller (bridge) owns those directly
/// and swaps them on reconnect. This matches how `connect_or_spawn()` →
/// `conn.into_channels()` works in `run_via_leader()`.
///
/// # Usage
///
/// ```ignore
/// let (status_tx, status_rx) = LeaderReconnector::status_channel();
/// let reconnector = LeaderReconnector::new(
///     "grow-tui", ClientMode::Stdio, caps, status_tx,
/// );
///
/// // When connection dies:
/// let (new_tx, new_rx, _disconnect_rx) = reconnector.reconnect(
///     ReconnectPolicy::unbounded(), &cancel,
/// ).await?;
/// // ... install new_tx/new_rx, then:
/// reconnector.notify_connected();
/// ```
pub struct LeaderReconnector {
    client_type: String,
    mode: ClientMode,
    capabilities: ClientCapabilities,
    status_tx: watch::Sender<ConnectionStatus>,
    /// Generation [`notify_connected`](Self::notify_connected) publishes next.
    /// Starts at 1: generation 0 is the initial connection, pre-seeded by
    /// [`status_channel`](Self::status_channel). Atomic because
    /// `notify_connected` takes `&self`.
    next_generation: std::sync::atomic::AtomicU64,
}
impl LeaderReconnector {
    /// Create a new reconnector with the given connection parameters.
    ///
    /// The `status_tx` channel is used to broadcast reconnection status
    /// to observers (e.g., TUI banner).
    pub fn new(
        client_type: impl Into<String>,
        mode: ClientMode,
        capabilities: ClientCapabilities,
        status_tx: watch::Sender<ConnectionStatus>,
    ) -> Self {
        Self {
            client_type: client_type.into(),
            mode,
            capabilities,
            status_tx,
            next_generation: std::sync::atomic::AtomicU64::new(1),
        }
    }
    /// Publish `ConnectionStatus::Connected` with the next reconnect generation.
    ///
    /// Deliberately NOT called by [`reconnect`](Self::reconnect): the caller
    /// must first install the fresh channels it returned, then notify — so an
    /// observer that reacts to `Connected` by sending requests cannot race the
    /// channel swap and write into the dead pre-reconnect sender.
    pub fn notify_connected(&self) {
        let generation = self
            .next_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let _ = self
            .status_tx
            .send(ConnectionStatus::Connected { generation });
    }
    /// Attempt to reconnect to the leader (or spawn a new one).
    ///
    /// Returns fresh `(tx, rx, disconnect_rx)` on success. The caller is
    /// responsible for swapping these into its local state, calling
    /// [`notify_connected`](Self::notify_connected), and replaying
    /// initialization (e.g., `initialize` + `session/load`).
    ///
    /// The `disconnect_rx` allows the caller to observe *why* the new
    /// connection ends (e.g., `LeaderShutdown` vs `ConnectionLost`),
    /// preserving the signal from step 1b across reconnection cycles.
    ///
    /// Uses exponential backoff: 1s → 2s → 4s → ... → max 30s.
    ///
    /// # Retry policy
    ///
    /// - [`ReconnectPolicy::Unbounded`]: retries until `cancel` fires (for TUI).
    /// - [`ReconnectPolicy::Bounded`]: retries up to `max_attempts`, then returns an error.
    pub async fn reconnect(
        &self,
        policy: ReconnectPolicy,
        cancel: &CancellationToken,
    ) -> Result<
        (
            mpsc::UnboundedSender<String>,
            mpsc::UnboundedReceiver<String>,
            watch::Receiver<DisconnectReason>,
        ),
        ConnectionError,
    > {
        self.reconnect_with(policy, cancel, || {
            connect_or_spawn(&self.client_type, self.mode, self.capabilities.clone())
        })
        .await
    }
    async fn reconnect_with<F, Fut>(
        &self,
        policy: ReconnectPolicy,
        cancel: &CancellationToken,
        mut connect_attempt: F,
    ) -> Result<
        (
            mpsc::UnboundedSender<String>,
            mpsc::UnboundedReceiver<String>,
            watch::Receiver<DisconnectReason>,
        ),
        ConnectionError,
    >
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<LeaderConnection, ConnectionError>>,
    {
        let mut attempt: u32 = 0;
        let mut delay = RECONNECT_BASE_DELAY;
        loop {
            if cancel.is_cancelled() {
                let _ = self.status_tx.send(ConnectionStatus::Failed {
                    error: "Cancelled".into(),
                });
                return Err(ConnectionError::Cancelled);
            }
            attempt += 1;
            let _ = self
                .status_tx
                .send(ConnectionStatus::Reconnecting { attempt });
            info!(
                attempt,
                delay_ms = delay.as_millis(),
                "Attempting to reconnect to leader"
            );
            match connect_attempt().await {
                Ok(conn) => {
                    info!(attempt, "Reconnected to leader");
                    return Ok(conn.into_channels_with_disconnect());
                }
                Err(e) if is_terminal_refusal(&e) => {
                    warn!(attempt, error = %e, "Reconnection refused (terminal)");
                    let _ = self.status_tx.send(ConnectionStatus::Failed {
                        error: e.to_string(),
                    });
                    return Err(e);
                }
                Err(e) => {
                    warn!(attempt, error = %e, "Reconnection attempt failed");
                    if let ReconnectPolicy::Bounded { max_attempts } = policy
                        && attempt >= max_attempts
                    {
                        let error_msg = format!("Failed after {} attempts: {}", max_attempts, e);
                        let _ = self.status_tx.send(ConnectionStatus::Failed {
                            error: error_msg.clone(),
                        });
                        return Err(ConnectionError::SpawnFailed(error_msg));
                    }
                }
            }
            tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = self.status_tx.send(ConnectionStatus::Failed {
                        error: "Cancelled".into(),
                    });
                    return Err(ConnectionError::Cancelled);
                }
                _ = tokio::time::sleep(delay) => {}
            }
            delay = std::cmp::min(delay * 2, RECONNECT_MAX_DELAY);
        }
    }
    /// Create a `watch` channel pair for connection status.
    ///
    /// Convenience helper — returns `(tx, rx)` initialized to the
    /// pre-reconnect `Connected { generation: 0 }` state.
    /// Pass `tx` to [`LeaderReconnector::new()`], keep `rx` for observing status.
    pub fn status_channel() -> (
        watch::Sender<ConnectionStatus>,
        watch::Receiver<ConnectionStatus>,
    ) {
        watch::channel(ConnectionStatus::Connected { generation: 0 })
    }
}
/// Poll until `pid` is no longer alive or `timeout` elapses.
async fn wait_for_pid_exit(pid: u32, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if !crate::util::is_process_alive(pid) {
            return;
        }
        tokio::time::sleep(SPAWN_POLL_INTERVAL).await;
    }
    debug!(
        pid,
        "Evicted leader still alive after grace; reclaiming socket anyway"
    );
}
/// PID-keyed timer state: the holder PID being timed and when we first saw it
/// live-but-unconnectable.
type ZombieTimer = Option<(u32, Instant)>;
/// Decision produced by [`zombie_evict_decision`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ZombieAction {
    /// Not a zombie candidate this round; timer cleared.
    Clear,
    /// A live grow holder is still unconnectable; timer (re)armed, keep waiting.
    Wait,
    /// The SAME holder PID has been unconnectable for the full deadline — evict.
    Evict { pid: u32, waited: Duration },
}
/// Pure decision for the zombie-eviction net. The timer is keyed to the PID so a
/// timer accrued against an old zombie can never evict a freshly-spawned leader.
fn zombie_evict_decision(
    holder: Option<u32>,
    now: Instant,
    deadline: Duration,
    timer: &mut ZombieTimer,
) -> ZombieAction {
    let Some(pid) = holder else {
        *timer = None;
        return ZombieAction::Clear;
    };
    match *timer {
        Some((tracked_pid, since)) if tracked_pid == pid => {
            let waited = now.saturating_duration_since(since);
            if waited >= deadline {
                *timer = None;
                ZombieAction::Evict { pid, waited }
            } else {
                ZombieAction::Wait
            }
        }
        _ => {
            *timer = Some((pid, now));
            ZombieAction::Wait
        }
    }
}
/// The live *grow* PID that ACTUALLY holds the flock on the lock file, if any.
/// `None` for a dead / non-grow PID, OR when the file PID can't be confirmed to be
/// the real flock holder — so the auto-kill zombie net never SIGKILLs a process
/// that does not hold the flock (a stale-but-live PID left in `leader.lock`, or a
/// brief spawner that held the flock without rewriting the file). Uses the
/// stricter (name-matching) grow check since this drives the auto-kill path.
///
/// Linux confirms the holder via `/proc/locks`. macOS/BSD have no `/proc/locks`,
/// so the holder is unconfirmable and this returns `None` (eviction skipped),
/// accepting that a genuine zombie there is not auto-killed.
fn live_grow_lock_holder(lock: &LeaderLock) -> Option<u32> {
    let file_pid = live_grow_pid_from_lock(lock)?;
    let pid = evictable_holder(file_pid, confirmed_flock_holder(lock.lock_path()))?;
    Some(pid)
}
/// Read a live grow PID from a contended leader lock. This is used only after
/// the caller failed to acquire that exact lock and completed a socket handshake
/// proving the current generation speaks an incompatible protocol.
fn live_grow_pid_from_lock(lock: &LeaderLock) -> Option<u32> {
    let pid = lock.read_pid()?;
    (crate::util::is_process_alive(pid) && crate::util::is_grow_process_strict(pid)).then_some(pid)
}
/// Safety gate: a file PID is evictable only when the confirmed flock `holder` is
/// known AND equals it. An unknown holder or a mismatch (file PID ≠ real holder)
/// is NOT evictable. Pure so the "do not evict" invariant is unit-testable.
fn evictable_holder(file_pid: u32, holder: Option<u32>) -> Option<u32> {
    match holder {
        Some(h) if h == file_pid => Some(file_pid),
        _ => None,
    }
}
/// PID that actually holds the exclusive flock on the lock file, or `None` when it
/// can't be determined. Linux reads `/proc/locks`; other platforms lack that
/// interface, so the holder is unknowable there and we return `None` (callers must
/// not auto-kill a PID they can't confirm holds the flock).
fn confirmed_flock_holder(lock_path: &Path) -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        flock_holder_pid(lock_path)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = lock_path;
        None
    }
}
/// The flock-holder PID for `lock_path` per `/proc/locks`: stat the path for its
/// device:inode, then find the matching `FLOCK`/`WRITE` (fs2's exclusive lock)
/// entry. Linux-only.
#[cfg(target_os = "linux")]
fn flock_holder_pid(lock_path: &Path) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(lock_path).ok()?;
    let proc_locks = std::fs::read_to_string("/proc/locks").ok()?;
    let (major, minor) = glibc_dev_major_minor(meta.dev());
    parse_flock_holder(&proc_locks, major, minor, meta.ino())
}
/// Decode a glibc 64-bit `dev_t` into (major, minor) — the same bit layout glibc's
/// `gnu_dev_major`/`gnu_dev_minor` use, matching the numbers the kernel prints in
/// `/proc/locks`. (libc 0.2 dropped `major`/`minor` for the gnu target.) Pure, so
/// it (and the parser below) compile+test on all hosts even though only Linux
/// consumes them.
#[cfg(any(target_os = "linux", test))]
fn glibc_dev_major_minor(dev: u64) -> (u64, u64) {
    let major = ((dev & 0x0000_0000_000f_ff00) >> 8) | ((dev & 0xffff_f000_0000_0000) >> 32);
    let minor = (dev & 0x0000_0000_0000_00ff) | ((dev & 0x0000_0fff_fff0_0000) >> 12);
    (major, minor)
}
/// Parse `/proc/locks` for the PID holding an exclusive `flock` on the file
/// identified by `major:minor:inode`. Skips blocked waiters (lines whose second
/// field is `->`, which does not hold the lock and shifts the field layout).
/// Returns `None` if no matching `FLOCK`/`WRITE` holder is present. Pure (parses a
/// string) so it is unit-testable without real kernel locks.
#[cfg(any(target_os = "linux", test))]
fn parse_flock_holder(proc_locks: &str, major: u64, minor: u64, inode: u64) -> Option<u32> {
    for line in proc_locks.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.get(1) == Some(&"->") {
            continue;
        }
        if f.len() < 6 || f[1] != "FLOCK" || f[3] != "WRITE" {
            continue;
        }
        let mut dev_inode = f[5].split(':');
        let (Some(maj), Some(min), Some(ino)) =
            (dev_inode.next(), dev_inode.next(), dev_inode.next())
        else {
            continue;
        };
        let (Ok(maj), Ok(min), Ok(ino)) = (
            u64::from_str_radix(maj, 16),
            u64::from_str_radix(min, 16),
            ino.parse::<u64>(),
        ) else {
            continue;
        };
        if maj == major && min == minor && ino == inode {
            return f[4].parse::<u32>().ok();
        }
    }
    None
}
/// Max eviction attempts against the same unusable leader PID before
/// `connect_or_spawn` surfaces an error instead of looping forever.
const MAX_UNUSABLE_EVICT_ATTEMPTS: u32 = 3;
/// Max times `connect_or_spawn` will self-spawn a leader that fails to become
/// connectable before surfacing an error. Bounds a persistent spawn/bind failure
/// (bad socket-dir perms, exec fault in `run_leader`) that would otherwise
/// re-fork every `SPAWN_WAIT_TIMEOUT` forever.
const MAX_SELF_SPAWN_ATTEMPTS: u32 = 3;
/// Records an eviction attempt against `pid`; returns `false` once the per-PID
/// budget is exhausted. Attempts reset when the target PID changes.
fn register_evict_attempt(state: &mut Option<(u32, u32)>, pid: u32, max_attempts: u32) -> bool {
    let count = match *state {
        Some((tracked, n)) if tracked == pid => n + 1,
        _ => 1,
    };
    *state = Some((pid, count));
    count <= max_attempts
}
/// A connect-level failure: never became connectable (`Timeout`) or the socket
/// file exists but refuses connections (`Connect`, e.g. ECONNREFUSED against a
/// stale socket / dead IPC task). Both drive the zombie net. Registration- and
/// protocol-level errors mean the socket ANSWERED and must surface instead.
fn is_connect_level_failure(error: &ConnectionError) -> bool {
    matches!(
        error,
        ConnectionError::Timeout | ConnectionError::Client(ClientError::Connect(_, _))
    )
}
fn is_incompatible_protocol_failure(error: &ConnectionError) -> bool {
    matches!(
        error,
        ConnectionError::Client(ClientError::IncompatibleProtocol(_))
    )
}
/// Policy refusals that can never succeed on reconnect retry (not zombie-evictable).
fn is_terminal_refusal(error: &ConnectionError) -> bool {
    matches!(error, ConnectionError::SandboxConfinement(_))
}
/// Evict a verified grow process that owns an unusable leader generation.
/// SIGTERM, wait, then escalate to SIGKILL if it overran the grace window.
async fn evict_unusable_leader(pid: u32, sock_path: &Path, reason: &str, waited: Duration) {
    use crate::util::KillSignal;
    warn!(
        pid,
        socket = %sock_path.display(),
        reason,
        "Evicting unusable leader"
    );
    if let Err(e) = crate::util::kill_process_with_signal(pid, KillSignal::Term) {
        warn!(error = %e, pid, reason, "Failed to SIGTERM unusable leader");
    }
    wait_for_pid_exit(pid, EVICT_WAIT_TIMEOUT).await;
    let outcome = if !crate::util::is_process_alive(pid) {
        "exited"
    } else if let Err(e) = crate::util::kill_process_with_signal(pid, KillSignal::Kill) {
        warn!(error = %e, pid, reason, "Failed to SIGKILL unusable leader");
        "sigkill_failed"
    } else {
        wait_for_pid_exit(pid, EVICT_WAIT_TIMEOUT).await;
        if crate::util::is_process_alive(pid) {
            "survived_sigkill"
        } else {
            "sigkilled"
        }
    };
    ::diagnostics::unified_log::warn(
        "leader.unusable.evicted",
        None,
        Some(serde_json::json!({
            "leader_pid": pid,
            "socket_path": sock_path.display().to_string(),
            "reason": reason,
            "outcome": outcome,
            "client_version": version::VERSION,
            "waited_ms": waited.as_millis() as u64,
        })),
    );
}
/// Connect to existing leader or spawn a new one.
///
/// Uses OS-level file locking (flock) to coordinate:
/// 1. Try to connect to existing socket (fast path)
/// 2. If connection fails, try to acquire exclusive lock
/// 3. If lock acquired, we are responsible for spawning the leader
/// 4. If lock not acquired, another process is leader/spawning - wait and retry
///
/// # Arguments
///
/// * `client_type` - Identifier for the client type (e.g., "grow-tui", "vscode")
/// * `mode` - Communication mode (Stdio or Headless)
/// * `capabilities` - Client capabilities (e.g., always_approve_mode) to register with the leader
pub async fn connect_or_spawn(
    client_type: &str,
    mode: ClientMode,
    capabilities: ClientCapabilities,
) -> Result<LeaderConnection, ConnectionError> {
    if let Some(profile) = sandbox::requested_confinement_profile() {
        return Err(ConnectionError::SandboxConfinement(profile));
    }
    let start = std::time::Instant::now();
    let mut lock = LeaderLock::new();
    let sock_path = lock.socket_path().clone();
    let mut replacing_stale = false;
    if crate::local_ipc::transport::listener_is_ready(&sock_path) {
        let skip_connect = if let Some(pid) = lock.read_pid() {
            if crate::util::is_process_alive(pid) {
                debug!(pid, "Leader PID is alive, attempting connection");
                false
            } else {
                debug!(pid, "Leader PID is dead, skipping socket connect");
                true
            }
        } else {
            debug!("Socket exists but no PID in lock, attempting connection");
            false
        };
        if !skip_connect {
            match connect_to_leader(&sock_path, client_type, mode, capabilities.clone()).await {
                Ok(conn) => {
                    info!(
                        elapsed_ms = start.elapsed().as_millis() as u64,
                        "Adopted leader"
                    );
                    return Ok(conn);
                }
                Err(e) => {
                    debug!(error = %e, "Connection to existing socket failed");
                }
            }
        }
    }
    let mut zombie_timer: ZombieTimer = None;
    let mut evict_attempts: Option<(u32, u32)> = None;
    let mut self_spawn_attempts: u32 = 0;
    loop {
        match lock.try_acquire() {
            Ok(true) => {
                if let Err(error) = lock.cleanup_socket() {
                    warn!(error = %error, "Failed to remove stale leader socket");
                }
                info!("Acquired lock, spawning leader subprocess");
                if let Err(e) = lock.release() {
                    warn!(error = %e, "Failed to release lock before spawning leader");
                }
                spawn_leader_subprocess()?;
                let conn = match wait_for_socket_connectable(
                    &sock_path,
                    client_type,
                    mode,
                    capabilities.clone(),
                )
                .await
                {
                    Ok(conn) => conn,
                    Err(ConnectionError::Timeout) => {
                        self_spawn_attempts += 1;
                        if self_spawn_attempts >= MAX_SELF_SPAWN_ATTEMPTS {
                            return Err(ConnectionError::SpawnFailed(format!(
                                "spawned leader did not become connectable after \
                                 {MAX_SELF_SPAWN_ATTEMPTS} attempts"
                            )));
                        }
                        debug!(
                            attempt = self_spawn_attempts,
                            "Spawned leader not connectable yet, retrying"
                        );
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                let elapsed_ms = start.elapsed().as_millis() as u64;
                info!(elapsed_ms, "Spawned and connected to leader");
                if replacing_stale {
                    ::diagnostics::unified_log::info(
                        "leader.spawn.replacement",
                        None,
                        Some(serde_json::json!({
                            "reason": "incompatible_protocol",
                            "client_version": version::VERSION,
                            "elapsed_ms": elapsed_ms,
                        })),
                    );
                }
                return Ok(conn);
            }
            Ok(false) => {
                debug!("Lock held by another process, probing socket connectability");
            }
            Err(e) => {
                return Err(e.into());
            }
        }
        match wait_for_socket_connectable(&sock_path, client_type, mode, capabilities.clone()).await
        {
            Ok(conn) => {
                info!(
                    elapsed_ms = start.elapsed().as_millis() as u64,
                    "Adopted leader"
                );
                return Ok(conn);
            }
            Err(e) if is_incompatible_protocol_failure(&e) => {
                let Some(pid) = live_grow_pid_from_lock(&lock) else {
                    return Err(e);
                };
                if !register_evict_attempt(&mut evict_attempts, pid, MAX_UNUSABLE_EVICT_ATTEMPTS) {
                    return Err(ConnectionError::SpawnFailed(format!(
                        "incompatible leader pid {pid} could not be evicted after \
                         {MAX_UNUSABLE_EVICT_ATTEMPTS} attempts"
                    )));
                }
                evict_unusable_leader(pid, &sock_path, "incompatible_protocol", Duration::ZERO)
                    .await;
                replacing_stale = true;
                continue;
            }
            Err(e) if is_connect_level_failure(&e) => {
                let holder = live_grow_lock_holder(&lock);
                match zombie_evict_decision(
                    holder,
                    Instant::now(),
                    ZOMBIE_EVICT_DEADLINE,
                    &mut zombie_timer,
                ) {
                    ZombieAction::Evict { pid, waited } => {
                        if !register_evict_attempt(
                            &mut evict_attempts,
                            pid,
                            MAX_UNUSABLE_EVICT_ATTEMPTS,
                        ) {
                            return Err(ConnectionError::SpawnFailed(format!(
                                "zombie leader pid {pid} could not be evicted after \
                                 {MAX_UNUSABLE_EVICT_ATTEMPTS} attempts"
                            )));
                        }
                        evict_unusable_leader(pid, &sock_path, "unreachable", waited).await;
                        continue;
                    }
                    ZombieAction::Wait => {
                        debug!("Flock-holder not connectable yet, waiting");
                        continue;
                    }
                    ZombieAction::Clear => {
                        debug!("Timeout waiting for socket, retrying lock acquisition");
                        continue;
                    }
                }
            }
            Err(e) => return Err(e),
        }
    }
}
/// Resolve the binary to spawn as the leader subprocess.
///
/// For a **managed install** — the running binary lives under `grow_home`
/// (e.g. `~/.grow/...`) — prefer the managed `~/.grow/bin/grow` symlink. After an
/// auto-update or `grow update` atomically swaps that symlink, `current_exe()`
/// still resolves (via `/proc/self/exe` on Linux) to the *old* versioned target,
/// so spawning it would relaunch the stale binary. The symlink always points to
/// the freshly-installed version.
///
/// This mirrors `update::auto_update::resolve_restart_exe` in that shared
/// "managed installs relaunch on the managed link, not the stale
/// current_exe" core, with one difference: the updater's resolver first
/// prefers the `grow` found on `PATH` (the binary the user actually runs),
/// while the leader must stay on the binary that spawned it, so this
/// resolver never consults PATH.
///
/// For a **dev / out-of-tree binary** (`cargo run`, integration tests, installs
/// not under `grow_home`), keep `current_exe()` so the spawned leader matches the
/// calling binary.
///
/// Falls back to `~/.grow/bin/grow` only when `current_exe()` is unavailable.
fn resolve_exe_for_spawn() -> Result<std::path::PathBuf, ConnectionError> {
    resolve_binary_with_home(&crate::util::grow_home::grow_home())
}
fn resolve_binary_with_home(grow_home: &Path) -> Result<std::path::PathBuf, ConnectionError> {
    resolve_binary_impl(grow_home, std::env::current_exe().ok())
}
/// Binary file name for the managed grow install (`grow` / `grow.exe`).
fn managed_grow_bin_name() -> &'static str {
    if cfg!(windows) { "grow.exe" } else { "grow" }
}
/// Core leader-binary resolution with the current-exe path injected, for testability.
fn resolve_binary_impl(
    grow_home: &Path,
    current_exe: Option<std::path::PathBuf>,
) -> Result<std::path::PathBuf, ConnectionError> {
    let managed_bin = grow_home.join("bin").join(managed_grow_bin_name());
    if let Some(ref exe) = current_exe
        && path_is_under(exe, grow_home)
        && managed_bin.exists()
    {
        return Ok(managed_bin);
    }
    if let Some(exe) = current_exe {
        return Ok(exe);
    }
    if managed_bin.exists() {
        return Ok(managed_bin);
    }
    Err(ConnectionError::SpawnFailed(
        "could not determine binary path for leader spawn".into(),
    ))
}
/// Whether `path` is located within `dir`, canonicalizing both where possible so
/// symlinked / relative paths compare correctly.
fn path_is_under(path: &Path, dir: &Path) -> bool {
    let path = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = dunce::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    path.starts_with(&dir)
}
fn spawn_leader_subprocess() -> Result<u32, ConnectionError> {
    let exe = resolve_exe_for_spawn()?;
    let mut cmd = Command::new(exe);
    cmd.arg("agent").arg("leader");
    cmd.arg("--no-exit-on-disconnect");
    if let Some(socket) = std::env::var_os(crate::leader::LEADER_SOCKET_ENV) {
        cmd.env(crate::leader::LEADER_SOCKET_ENV, socket);
    }
    for key in [
        "GROW_DEBUG_LOG",
        "GROW_HOOKS_LOG",
        "GROW_LOG_SAMPLING",
        "GROW_INSTRUMENTATION",
    ] {
        if let Some(v) = std::env::var_os(key) {
            cmd.env(key, v);
        }
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null());
    let log_path = crate::util::grow_home::grow_home().join("leader.log");
    match std::fs::File::create(&log_path) {
        Ok(log_file) => {
            info!("Leader stderr → log file");
            cmd.stderr(std::process::Stdio::from(log_file));
        }
        Err(e) => {
            warn!(error = %e, "Failed to create leader log file, using /dev/null");
            cmd.stderr(std::process::Stdio::null());
        }
    }
    let leader_log = std::env::var("GROW_LEADER_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .unwrap_or_else(|_| "shell=info,acp=warn,mcp=warn".into());
    cmd.env("RUST_LOG", leader_log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP.0);
    }
    #[allow(clippy::disallowed_methods)]
    let mut child = cmd
        .spawn()
        .map_err(|e| ConnectionError::SpawnFailed(e.to_string()))?;
    let pid = child.id();
    info!(pid, "Spawned leader subprocess");
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}
async fn connect_to_leader(
    sock_path: &Path,
    client_type: &str,
    mode: ClientMode,
    capabilities: ClientCapabilities,
) -> Result<LeaderConnection, ConnectionError> {
    let client =
        LeaderClient::connect(sock_path.to_path_buf(), client_type, mode, capabilities).await?;
    Ok(LeaderConnection { client })
}
/// Wait for socket to appear and successfully connect.
///
/// Polls the socket path until it becomes connectable or timeout is reached.
/// Uses exponential backoff starting from SPAWN_POLL_INTERVAL.
pub(crate) async fn wait_for_socket_connectable(
    sock_path: &Path,
    client_type: &str,
    mode: ClientMode,
    capabilities: ClientCapabilities,
) -> Result<LeaderConnection, ConnectionError> {
    let deadline = tokio::time::Instant::now() + SPAWN_WAIT_TIMEOUT;
    let mut last_error = None;
    while tokio::time::Instant::now() < deadline {
        if crate::local_ipc::transport::listener_is_ready(sock_path) {
            match connect_to_leader(sock_path, client_type, mode, capabilities.clone()).await {
                Ok(conn) => return Ok(conn),
                Err(error) if is_incompatible_protocol_failure(&error) => return Err(error),
                Err(e) => {
                    debug!(error = %e, "Connection attempt failed, retrying");
                    last_error = Some(e);
                }
            }
        }
        tokio::time::sleep(SPAWN_POLL_INTERVAL).await;
    }
    match last_error {
        Some(e) => Err(e),
        None => Err(ConnectionError::Timeout),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::leader::test_support::{FakeLeaderBehavior, spawn_fake_leader};
    use std::fs;
    use tempfile::TempDir;
    const TEST_DEADLINE: Duration = Duration::from_secs(30);
    /// No live grow holder → `Clear`, and any pending timer is reset.
    #[test]
    fn zombie_decision_clears_when_no_holder() {
        let mut timer: ZombieTimer = Some((100, Instant::now()));
        assert_eq!(
            zombie_evict_decision(None, Instant::now(), TEST_DEADLINE, &mut timer),
            ZombieAction::Clear
        );
        assert_eq!(timer, None, "timer must be cleared when there is no holder");
    }
    /// First sighting of a holder arms the timer and waits (never evicts).
    #[test]
    fn zombie_decision_arms_timer_on_first_sighting() {
        let mut timer: ZombieTimer = None;
        let t0 = Instant::now();
        assert_eq!(
            zombie_evict_decision(Some(100), t0, TEST_DEADLINE, &mut timer),
            ZombieAction::Wait
        );
        assert_eq!(timer, Some((100, t0)));
    }
    /// The SAME holder is evicted only after staying unconnectable for the deadline.
    #[test]
    fn zombie_decision_evicts_same_pid_after_deadline() {
        let mut timer: ZombieTimer = None;
        let t0 = Instant::now();
        assert_eq!(
            zombie_evict_decision(Some(100), t0, TEST_DEADLINE, &mut timer),
            ZombieAction::Wait
        );
        let t_mid = t0 + Duration::from_secs(29);
        assert_eq!(
            zombie_evict_decision(Some(100), t_mid, TEST_DEADLINE, &mut timer),
            ZombieAction::Wait
        );
        let t_end = t0 + Duration::from_secs(30);
        assert_eq!(
            zombie_evict_decision(Some(100), t_end, TEST_DEADLINE, &mut timer),
            ZombieAction::Evict {
                pid: 100,
                waited: Duration::from_secs(30),
            }
        );
        assert_eq!(timer, None);
    }
    /// A holder PID change re-keys the timer, so time accrued against an old zombie
    /// can never evict a fresh leader.
    #[test]
    fn zombie_decision_resets_timer_when_pid_changes() {
        let mut timer: ZombieTimer = None;
        let t0 = Instant::now();
        assert_eq!(
            zombie_evict_decision(Some(100), t0, TEST_DEADLINE, &mut timer),
            ZombieAction::Wait
        );
        let t1 = t0 + Duration::from_secs(40);
        assert_eq!(
            zombie_evict_decision(Some(200), t1, TEST_DEADLINE, &mut timer),
            ZombieAction::Wait
        );
        assert_eq!(timer, Some((200, t1)), "timer must re-key to the new PID");
        let t2 = t1 + Duration::from_secs(1);
        assert_eq!(
            zombie_evict_decision(None, t2, TEST_DEADLINE, &mut timer),
            ZombieAction::Clear
        );
        assert_eq!(timer, None);
    }
    /// Eviction safety gate: a file PID is evictable only when the confirmed flock
    /// holder is known AND equals it. Unknown holder or a mismatch → do not evict.
    #[test]
    fn evictable_holder_requires_confirmed_matching_holder() {
        assert_eq!(evictable_holder(100, Some(100)), Some(100));
        assert_eq!(evictable_holder(100, Some(200)), None);
        assert_eq!(evictable_holder(100, None), None);
    }
    /// glibc `dev_t` decode matches the logical major:minor the kernel prints.
    /// makedev(253, 1) == 0xfd01 → (253, 1).
    #[test]
    fn glibc_dev_major_minor_decodes_makedev() {
        assert_eq!(glibc_dev_major_minor(0xfd01), (253, 1));
        assert_eq!(glibc_dev_major_minor(0), (0, 0));
    }
    /// `/proc/locks` parsing: match an exclusive FLOCK holder by device:inode and
    /// return its PID; skip waiters, POSIX locks, and non-matching dev/inode.
    #[test]
    fn parse_flock_holder_matches_dev_inode_and_pid() {
        let sample = "\
1: POSIX  ADVISORY  WRITE 111 fd:01:2000 0 EOF
2: FLOCK  ADVISORY  WRITE 592 fd:01:1000 0 EOF
3: FLOCK  ADVISORY  WRITE 700 fd:01:3000 0 EOF
";
        assert_eq!(parse_flock_holder(sample, 253, 1, 1000), Some(592));
        assert_eq!(parse_flock_holder(sample, 253, 1, 9999), None);
        assert_eq!(parse_flock_holder(sample, 8, 1, 1000), None);
    }
    /// Blocked waiters (`->`) do not hold the lock and shift the field layout, so
    /// they must be skipped even when their dev:inode matches.
    #[test]
    fn parse_flock_holder_skips_waiters() {
        let sample = "\
1: FLOCK  ADVISORY  WRITE 592 fd:01:1000 0 EOF
1: -> FLOCK ADVISORY WRITE 800 fd:01:1000 0 EOF
";
        assert_eq!(parse_flock_holder(sample, 253, 1, 1000), Some(592));
    }
    /// A stale-but-live PID in the lock file (file PID ≠ real flock holder) is
    /// classified "do not evict" end-to-end through the parse + gate helpers.
    #[test]
    fn stale_file_pid_not_matching_holder_is_not_evictable() {
        let sample = "1: FLOCK  ADVISORY  WRITE 592 fd:01:1000 0 EOF\n";
        let holder = parse_flock_holder(sample, 253, 1, 1000);
        assert_eq!(holder, Some(592));
        assert_eq!(evictable_holder(12345, holder), None);
    }
    /// Connect-level failures (timeout / connection-refused) drive the zombie
    /// net; registration/protocol errors (socket answered) surface instead.
    #[test]
    fn connect_level_failure_classification() {
        use std::io::{Error, ErrorKind};
        assert!(is_connect_level_failure(&ConnectionError::Timeout));
        assert!(is_connect_level_failure(&ConnectionError::Client(
            ClientError::Connect(3, Error::from(ErrorKind::ConnectionRefused))
        )));
        assert!(!is_connect_level_failure(&ConnectionError::Client(
            ClientError::Registration("rejected".into())
        )));
        assert!(!is_connect_level_failure(&ConnectionError::Client(
            ClientError::ConnectionClosed
        )));
        assert!(!is_connect_level_failure(
            &ConnectionError::SandboxConfinement("strict")
        ));
        let incompatible = ConnectionError::Client(ClientError::IncompatibleProtocol(
            "protocol mismatch".into(),
        ));
        assert!(!is_connect_level_failure(&incompatible));
        assert!(is_incompatible_protocol_failure(&incompatible));
    }
    #[test]
    fn terminal_refusal_classification() {
        assert!(is_terminal_refusal(&ConnectionError::SandboxConfinement(
            "strict"
        )));
        assert!(!is_terminal_refusal(&ConnectionError::Timeout));
        assert!(!is_terminal_refusal(&ConnectionError::SpawnFailed(
            "boom".into()
        )));
        assert!(!is_terminal_refusal(&ConnectionError::Cancelled));
    }
    /// Per-PID eviction budget: allows `max` attempts, then denies; a PID change
    /// resets the counter so a fresh zombie gets its own budget.
    #[test]
    fn register_evict_attempt_bounds_per_pid() {
        let mut state: Option<(u32, u32)> = None;
        assert!(register_evict_attempt(&mut state, 100, 3));
        assert!(register_evict_attempt(&mut state, 100, 3));
        assert!(register_evict_attempt(&mut state, 100, 3));
        assert!(!register_evict_attempt(&mut state, 100, 3));
        assert!(register_evict_attempt(&mut state, 200, 3));
        assert_eq!(state, Some((200, 1)));
    }
    /// `live_grow_lock_holder` returns `None` for a missing or dead PID, so the
    /// zombie net never times/kills a recycled or unrelated PID.
    #[test]
    fn live_grow_lock_holder_none_for_missing_or_dead_pid() {
        let temp = TempDir::new().unwrap();
        let lock = LeaderLock::from_paths(
            temp.path().join("leader.lock"),
            temp.path().join("leader.sock"),
        );
        assert_eq!(live_grow_lock_holder(&lock), None);
        fs::write(lock.lock_path(), "4000000000").unwrap();
        assert_eq!(live_grow_lock_holder(&lock), None);
    }
    #[test]
    fn reachable_leader_pids_skips_stale_locks() {
        let reachable = LeaderDescriptor {
            pid_from_lock: Some(111),
            lock_path: None,
            socket_path: None,
            classification: LeaderDiscoveryState::Reachable,
            live_info: Some(LiveLeaderInfo {
                pid: 222,
                socket_path: PathBuf::new(),
                lock_path: PathBuf::new(),
                leader_protocol_version: 0,
                leader_binary_version: "0.2.52".to_string(),
            }),
            target_error: None,
        };
        let stale = LeaderDescriptor {
            pid_from_lock: Some(333),
            lock_path: None,
            socket_path: None,
            classification: LeaderDiscoveryState::Stale,
            live_info: None,
            target_error: None,
        };
        assert_eq!(
            reachable_leader_pids(&[reachable, stale]),
            vec![(222, "0.2.52".to_string())]
        );
    }
    #[test]
    fn leader_is_older_than_directional() {
        assert!(leader_is_older_than("0.1.0", "0.2.0"));
        assert!(leader_is_older_than("0.1.219", "0.1.220"));
        assert!(leader_is_older_than("0.1.220-alpha.1", "0.1.220"));
        assert!(leader_is_older_than("0.1.9", "0.1.10"));
        assert!(!leader_is_older_than("0.1.10", "0.1.9"));
        assert!(!leader_is_older_than("0.2.0", "0.1.0"));
        assert!(!leader_is_older_than("0.2.0", "0.2.0"));
        assert!(!leader_is_older_than("unknown", "0.2.0"));
        assert!(!leader_is_older_than("0.1.0", "not-a-version"));
    }
    #[tokio::test]
    async fn wait_for_pid_exit_returns_immediately_for_dead_pid() {
        let start = tokio::time::Instant::now();
        wait_for_pid_exit(4_000_000_000, Duration::from_secs(30)).await;
        assert!(start.elapsed() < Duration::from_secs(1));
    }
    #[tokio::test(start_paused = true)]
    async fn wait_for_pid_exit_honors_timeout_for_live_pid() {
        let timeout = Duration::from_secs(8);
        let start = tokio::time::Instant::now();
        wait_for_pid_exit(std::process::id(), timeout).await;
        assert!(start.elapsed() >= timeout);
    }
    /// A leader that accepts but never registers must surface a hard timeout
    /// error — today there is no eviction/respawn fallback on this path, so
    /// every client adopting the hung leader parks and then errors.
    #[tokio::test(start_paused = true)]
    async fn connect_to_hung_leader_times_out_with_no_fallback() {
        let temp = TempDir::new().unwrap();
        let sock_path = temp.path().join("hung.sock");
        let fake =
            spawn_fake_leader(sock_path.clone(), FakeLeaderBehavior::SilentAfterAccept).await;
        let result = connect_to_leader(
            &sock_path,
            "test",
            ClientMode::Stdio,
            ClientCapabilities::default(),
        )
        .await;
        let Err(err) = result else {
            panic!("a silent leader must not yield a connection");
        };
        assert!(
            matches!(err, ConnectionError::Client(ClientError::Timeout(_))),
            "expected registration timeout, got {err:?}"
        );
        fake.cancel();
    }
    /// Reconnect attempts against a hung leader exhaust the bounded policy and
    /// publish `Failed` — the reconnector never falls back to evicting the hung
    /// leader and spawning a healthy one.
    #[tokio::test(start_paused = true)]
    async fn reconnect_against_hung_leader_exhausts_attempts_without_respawn() {
        let temp = TempDir::new().unwrap();
        let sock_path = temp.path().join("hung.sock");
        let fake =
            spawn_fake_leader(sock_path.clone(), FakeLeaderBehavior::SilentAfterAccept).await;
        let (status_tx, mut status_rx) = LeaderReconnector::status_channel();
        let reconnector = LeaderReconnector::new(
            "test",
            ClientMode::Stdio,
            ClientCapabilities::default(),
            status_tx,
        );
        let cancel = CancellationToken::new();
        let sock = sock_path.clone();
        let result = reconnector
            .reconnect_with(
                ReconnectPolicy::Bounded { max_attempts: 2 },
                &cancel,
                || {
                    connect_to_leader(
                        &sock,
                        "test",
                        ClientMode::Stdio,
                        ClientCapabilities::default(),
                    )
                },
            )
            .await;
        assert!(result.is_err(), "hung leader must exhaust bounded attempts");
        assert!(
            matches!(
                status_rx.borrow_and_update().clone(),
                ConnectionStatus::Failed { .. }
            ),
            "status must land on Failed after exhaustion"
        );
        fake.cancel();
    }
    /// A leader that closes right after `Registered` still yields a usable
    /// registration; disconnect is observed after the handshake.
    #[tokio::test]
    async fn close_after_register_still_exposes_registration_metadata() {
        let temp = TempDir::new().unwrap();
        let sock_path = temp.path().join("close.sock");
        let fake =
            spawn_fake_leader(sock_path.clone(), FakeLeaderBehavior::CloseAfterRegister).await;
        let conn = connect_to_leader(
            &sock_path,
            "test",
            ClientMode::Stdio,
            ClientCapabilities::default(),
        )
        .await
        .unwrap();
        assert_eq!(conn.registration().leader_binary_version, version::VERSION);
        fake.cancel();
    }
    #[tokio::test]
    async fn spawn_server_and_connect() {
        use protocol::ClientMode;
        let temp = TempDir::new().unwrap();
        let sock_path = temp.path().join("test.sock");
        let handle = spawn_leader_server(sock_path.clone()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let client = LeaderClient::connect(
            sock_path,
            "test",
            ClientMode::Stdio,
            ClientCapabilities::default(),
        )
        .await
        .unwrap();
        client.cancel();
        handle.cancel.cancel();
    }
    #[test]
    fn reconnect_policy_bounded_default() {
        let policy = ReconnectPolicy::bounded();
        assert_eq!(
            policy,
            ReconnectPolicy::Bounded {
                max_attempts: RECONNECT_MAX_ATTEMPTS_BOUNDED
            }
        );
    }
    #[test]
    fn reconnect_policy_unbounded() {
        let policy = ReconnectPolicy::unbounded();
        assert_eq!(policy, ReconnectPolicy::Unbounded);
    }
    #[test]
    fn status_channel_initial_value() {
        let (_tx, rx) = LeaderReconnector::status_channel();
        assert_eq!(*rx.borrow(), ConnectionStatus::Connected { generation: 0 });
    }
    /// Each `notify_connected` publishes a strictly increasing generation, so
    /// an observer that only sees the latest watch value still detects every
    /// reconnect (including a coalesced `Reconnecting -> Connected` flip).
    #[test]
    fn notify_connected_increments_generation() {
        let (status_tx, status_rx) = LeaderReconnector::status_channel();
        let reconnector = LeaderReconnector::new(
            "test",
            ClientMode::Stdio,
            ClientCapabilities::default(),
            status_tx,
        );
        reconnector.notify_connected();
        assert_eq!(
            *status_rx.borrow(),
            ConnectionStatus::Connected { generation: 1 }
        );
        reconnector.notify_connected();
        assert_eq!(
            *status_rx.borrow(),
            ConnectionStatus::Connected { generation: 2 }
        );
    }
    /// A successful `reconnect_with` must NOT publish `Connected` itself — the
    /// caller installs the new channels first, then calls `notify_connected`.
    /// Publishing early lets an observer send requests into the dead
    /// pre-reconnect channel.
    #[tokio::test]
    async fn reconnect_with_does_not_publish_connected_before_swap() {
        let temp = TempDir::new().unwrap();
        let sock_path = temp.path().join("test.sock");
        let handle = spawn_leader_server(sock_path.clone()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let (status_tx, mut status_rx) = LeaderReconnector::status_channel();
        let reconnector = LeaderReconnector::new(
            "test",
            ClientMode::Stdio,
            ClientCapabilities::default(),
            status_tx,
        );
        let _ = status_rx.borrow_and_update();
        let cancel = CancellationToken::new();
        let sock = sock_path.clone();
        let result = reconnector
            .reconnect_with(ReconnectPolicy::bounded(), &cancel, || {
                connect_to_leader(
                    &sock,
                    "test",
                    ClientMode::Stdio,
                    ClientCapabilities::default(),
                )
            })
            .await;
        assert!(result.is_ok(), "reconnect should succeed");
        assert_eq!(
            status_rx.borrow_and_update().clone(),
            ConnectionStatus::Reconnecting { attempt: 1 }
        );
        assert!(
            !status_rx.has_changed().unwrap(),
            "Connected must not be published before notify_connected()"
        );
        reconnector.notify_connected();
        assert_eq!(
            status_rx.borrow_and_update().clone(),
            ConnectionStatus::Connected { generation: 1 }
        );
        handle.cancel.cancel();
    }
    #[tokio::test]
    async fn reconnector_bounded_fails_after_max_attempts() {
        let (status_tx, mut status_rx) = LeaderReconnector::status_channel();
        let reconnector = LeaderReconnector::new(
            "test",
            ClientMode::Stdio,
            ClientCapabilities::default(),
            status_tx,
        );
        let cancel = CancellationToken::new();
        let policy = ReconnectPolicy::Bounded { max_attempts: 2 };
        let mut attempts = 0;
        let result = reconnector
            .reconnect_with(policy, &cancel, || {
                attempts += 1;
                async move {
                    Err(ConnectionError::SpawnFailed(format!(
                        "synthetic failure #{attempts}"
                    )))
                }
            })
            .await;
        assert!(result.is_err(), "Should fail after 2 attempts");
        let status = status_rx.borrow_and_update().clone();
        assert!(
            matches!(status, ConnectionStatus::Failed { .. }),
            "Expected Failed status, got {:?}",
            status
        );
    }
    #[tokio::test]
    async fn reconnector_cancelled_returns_error() {
        let (status_tx, _status_rx) = LeaderReconnector::status_channel();
        let reconnector = LeaderReconnector::new(
            "test",
            ClientMode::Stdio,
            ClientCapabilities::default(),
            status_tx,
        );
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = reconnector
            .reconnect(ReconnectPolicy::unbounded(), &cancel)
            .await;
        assert!(result.is_err(), "Should fail when cancelled");
    }
    #[tokio::test]
    async fn reconnector_succeeds_when_server_exists() {
        let temp = TempDir::new().unwrap();
        let sock_path = temp.path().join("test.sock");
        let handle = spawn_leader_server(sock_path.clone()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let (status_tx, status_rx) = LeaderReconnector::status_channel();
        let conn = connect_to_leader(
            &sock_path,
            "test",
            ClientMode::Stdio,
            ClientCapabilities::default(),
        )
        .await
        .unwrap();
        let _ = status_tx.send(ConnectionStatus::Connected { generation: 1 });
        assert_eq!(
            *status_rx.borrow(),
            ConnectionStatus::Connected { generation: 1 }
        );
        let (tx, _rx) = conn.into_channels();
        assert!(
            tx.send(r#"{"jsonrpc":"2.0","method":"test","id":1}"#.into())
                .is_ok()
        );
        handle.cancel.cancel();
    }
    #[tokio::test]
    async fn reconnector_status_transitions_on_failure_then_success() {
        let (status_tx, mut status_rx) = LeaderReconnector::status_channel();
        assert_eq!(
            *status_rx.borrow(),
            ConnectionStatus::Connected { generation: 0 }
        );
        let _ = status_tx.send(ConnectionStatus::Reconnecting { attempt: 1 });
        assert!(status_rx.has_changed().unwrap());
        let status = status_rx.borrow_and_update().clone();
        assert_eq!(status, ConnectionStatus::Reconnecting { attempt: 1 });
        let _ = status_tx.send(ConnectionStatus::Reconnecting { attempt: 2 });
        let status = status_rx.borrow_and_update().clone();
        assert_eq!(status, ConnectionStatus::Reconnecting { attempt: 2 });
        let _ = status_tx.send(ConnectionStatus::Connected { generation: 1 });
        let status = status_rx.borrow_and_update().clone();
        assert_eq!(status, ConnectionStatus::Connected { generation: 1 });
    }
    #[tokio::test]
    async fn reconnect_to_new_server_after_old_dies() {
        let temp = TempDir::new().unwrap();
        let sock_path = temp.path().join("test.sock");
        let handle_a = spawn_leader_server(sock_path.clone()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let client_a = LeaderClient::connect(
            sock_path.clone(),
            "test",
            ClientMode::Stdio,
            ClientCapabilities::default(),
        )
        .await
        .unwrap();
        let (tx_a, _rx_a) = client_a.into_channels();
        assert!(
            tx_a.send(r#"{"jsonrpc":"2.0","method":"test","id":1}"#.into())
                .is_ok()
        );
        handle_a.cancel.cancel();
        for _ in 0..50 {
            if tx_a.send("probe".into()).is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            tx_a.send("dead".into()).is_err(),
            "Old channel should be dead after server kill"
        );
        let _ = std::fs::remove_file(&sock_path);
        let handle_b = spawn_leader_server(sock_path.clone()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let client_b = LeaderClient::connect(
            sock_path,
            "test",
            ClientMode::Stdio,
            ClientCapabilities::default(),
        )
        .await
        .unwrap();
        let (tx_b, _rx_b) = client_b.into_channels();
        assert!(
            tx_b.send(r#"{"jsonrpc":"2.0","method":"test","id":2}"#.into())
                .is_ok()
        );
        handle_b.cancel.cancel();
    }
    #[tokio::test]
    async fn double_reconnect_server_a_dies_b_dies_c_works() {
        let temp = TempDir::new().unwrap();
        let sock_path = temp.path().join("test.sock");
        let handle_a = spawn_leader_server(sock_path.clone()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let client_a = LeaderClient::connect(
            sock_path.clone(),
            "test",
            ClientMode::Stdio,
            ClientCapabilities::default(),
        )
        .await
        .unwrap();
        let mut disconnect_rx_a = client_a.disconnect_reason();
        let (tx_a, _rx_a) = client_a.into_channels();
        assert!(
            tx_a.send(r#"{"jsonrpc":"2.0","method":"test","id":1}"#.into())
                .is_ok()
        );
        handle_a.cancel.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(5), disconnect_rx_a.changed()).await;
        let reason_a = disconnect_rx_a.borrow().clone();
        assert!(
            reason_a == DisconnectReason::LeaderShutdown
                || reason_a == DisconnectReason::ConnectionLost,
            "First disconnect: expected LeaderShutdown or ConnectionLost, got {:?}",
            reason_a
        );
        let _ = std::fs::remove_file(&sock_path);
        let handle_b = spawn_leader_server(sock_path.clone()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let client_b = LeaderClient::connect(
            sock_path.clone(),
            "test",
            ClientMode::Stdio,
            ClientCapabilities::default(),
        )
        .await
        .unwrap();
        let mut disconnect_rx_b = client_b.disconnect_reason();
        let (tx_b, _rx_b) = client_b.into_channels();
        assert!(
            tx_b.send(r#"{"jsonrpc":"2.0","method":"test","id":2}"#.into())
                .is_ok()
        );
        handle_b.cancel.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(5), disconnect_rx_b.changed()).await;
        let reason_b = disconnect_rx_b.borrow().clone();
        assert!(
            reason_b == DisconnectReason::LeaderShutdown
                || reason_b == DisconnectReason::ConnectionLost,
            "Second disconnect: expected LeaderShutdown or ConnectionLost, got {:?}",
            reason_b
        );
        let _ = std::fs::remove_file(&sock_path);
        let handle_c = spawn_leader_server(sock_path.clone()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let client_c = LeaderClient::connect(
            sock_path,
            "test",
            ClientMode::Stdio,
            ClientCapabilities::default(),
        )
        .await
        .unwrap();
        let disconnect_rx_c = client_c.disconnect_reason();
        let (tx_c, _rx_c) = client_c.into_channels();
        assert!(
            tx_c.send(r#"{"jsonrpc":"2.0","method":"test","id":3}"#.into())
                .is_ok()
        );
        assert_eq!(*disconnect_rx_c.borrow(), DisconnectReason::Connected);
        handle_c.cancel.cancel();
    }
    #[test]
    fn resolve_binary_prefers_current_exe() {
        let temp = TempDir::new().unwrap();
        let bin_dir = temp.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join("grow"), "fake-binary").unwrap();
        let result = resolve_binary_with_home(temp.path()).unwrap();
        let current = std::env::current_exe().unwrap();
        assert_eq!(result, current);
    }
    #[test]
    fn resolve_binary_succeeds_without_managed_bin() {
        let temp = TempDir::new().unwrap();
        let result = resolve_binary_with_home(temp.path()).unwrap();
        assert!(result.exists());
    }
    #[cfg(unix)]
    #[test]
    fn resolve_binary_prefers_current_exe_over_symlink() {
        let temp = TempDir::new().unwrap();
        let bin_dir = temp.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let target_v2 = bin_dir.join("grow-v2");
        std::fs::write(&target_v2, "new-binary").unwrap();
        std::os::unix::fs::symlink(&target_v2, bin_dir.join("grow")).unwrap();
        let result = resolve_binary_with_home(temp.path()).unwrap();
        let current = std::env::current_exe().unwrap();
        assert_eq!(result, current);
    }
    #[cfg(unix)]
    #[test]
    fn resolve_binary_prefers_managed_symlink_for_managed_install() {
        let temp = TempDir::new().unwrap();
        let bin_dir = temp.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let new_target = bin_dir.join("grow-v2");
        std::fs::write(&new_target, "new-binary").unwrap();
        let managed = bin_dir.join("grow");
        std::os::unix::fs::symlink(&new_target, &managed).unwrap();
        let stale_target = bin_dir.join("grow-v1");
        std::fs::write(&stale_target, "old-binary").unwrap();
        let result = resolve_binary_impl(temp.path(), Some(stale_target)).unwrap();
        assert_eq!(result, managed);
    }
    #[test]
    fn resolve_binary_prefers_current_exe_for_out_of_tree_install() {
        let temp = TempDir::new().unwrap();
        let bin_dir = temp.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join(managed_grow_bin_name()), "managed").unwrap();
        let dev_exe = std::env::current_exe().unwrap();
        let result = resolve_binary_impl(temp.path(), Some(dev_exe.clone())).unwrap();
        assert_eq!(result, dev_exe);
    }
    #[test]
    fn resolve_binary_falls_back_to_managed_when_no_current_exe() {
        let temp = TempDir::new().unwrap();
        let bin_dir = temp.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let managed = bin_dir.join(managed_grow_bin_name());
        std::fs::write(&managed, "managed").unwrap();
        let result = resolve_binary_impl(temp.path(), None).unwrap();
        assert_eq!(result, managed);
    }
    #[test]
    fn pid_check_identifies_dead_leader() {
        let temp = TempDir::new().unwrap();
        let lock_path = temp.path().join("leader.lock");
        fs::write(&lock_path, "4000000000").unwrap();
        let pid = LeaderLock::read_pid_from_path(&lock_path);
        assert_eq!(pid, Some(4_000_000_000));
        assert!(!crate::util::is_process_alive(4_000_000_000));
        fs::write(&lock_path, format!("{}", std::process::id())).unwrap();
        let pid = LeaderLock::read_pid_from_path(&lock_path).unwrap();
        assert_eq!(pid, std::process::id());
        assert!(crate::util::is_process_alive(pid));
    }
    #[tokio::test]
    async fn pid_alive_and_server_reachable_allows_connection() {
        let temp = TempDir::new().unwrap();
        let sock_path = temp.path().join("leader.sock");
        let lock_path = temp.path().join("leader.lock");
        let handle = spawn_leader_server(sock_path.clone()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        fs::write(&lock_path, format!("{}", std::process::id())).unwrap();
        let pid = LeaderLock::read_pid_from_path(&lock_path).unwrap();
        assert!(crate::util::is_process_alive(pid));
        let conn = connect_to_leader(
            &sock_path,
            "test",
            ClientMode::Stdio,
            ClientCapabilities::default(),
        )
        .await
        .unwrap();
        let (tx, _rx) = conn.into_channels();
        assert!(
            tx.send(r#"{"jsonrpc":"2.0","method":"test","id":1}"#.into())
                .is_ok()
        );
        handle.cancel.cancel();
    }
}
