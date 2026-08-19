use std::io;
use std::path::PathBuf;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::cpu_profile::ControlError;

const MAX_MESSAGE_SIZE: u32 = 64 * 1024 * 1024; // 64MB

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("Message too large: {0} bytes (max: {MAX_MESSAGE_SIZE})")]
    MessageTooLarge(u32),
    #[error("Invalid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("Connection closed")]
    ConnectionClosed,
}

pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Vec<u8>, ProtocolError> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(ProtocolError::ConnectionClosed);
        }
        Err(e) => return Err(ProtocolError::Io(e)),
    }

    let len = u32::from_be_bytes(len_buf);
    if len > MAX_MESSAGE_SIZE {
        return Err(ProtocolError::MessageTooLarge(len));
    }

    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}

pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    data: &[u8],
) -> Result<(), ProtocolError> {
    let len = data.len() as u32;
    if len > MAX_MESSAGE_SIZE {
        return Err(ProtocolError::MessageTooLarge(len));
    }

    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(data).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn read_message<R, T>(reader: &mut R) -> Result<T, ProtocolError>
where
    R: AsyncRead + Unpin,
    T: serde::de::DeserializeOwned,
{
    let data = read_frame(reader).await?;
    Ok(serde_json::from_slice(&data)?)
}

pub async fn write_message<W, T>(writer: &mut W, msg: &T) -> Result<(), ProtocolError>
where
    W: AsyncWrite + Unpin,
    T: serde::Serialize,
{
    let data = serde_json::to_vec(msg)?;
    write_frame(writer, &data).await
}

/// Unique identifier for a connected client.
///
/// Each client gets a unique ID when connecting to the leader server.
/// IDs are monotonically increasing and wrap around at u64::MAX.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(pub u64);

impl ClientId {
    /// Generate a new unique client ID.
    ///
    /// Uses an atomic counter that wraps around at u64::MAX.
    /// While collisions are theoretically possible after 2^64 IDs,
    /// this is practically impossible in real-world usage.
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        // Use wrapping_add to handle overflow gracefully
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(if id == 0 {
            COUNTER.fetch_add(1, Ordering::Relaxed)
        } else {
            id
        })
    }
}

impl Default for ClientId {
    fn default() -> Self {
        Self::new()
    }
}

/// Local leader transport used by ACP clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientMode {
    /// Clients send and receive ACP messages through the local IPC leader.
    Stdio,
}

/// Client capabilities reported during registration.
///
/// These capabilities are used by the leader to customize behavior for each client,
/// such as injecting settings into session requests.
pub const LEADER_PROTOCOL_VERSION: u32 = 2;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientCapabilities {
    /// Permission mode injected into session/new and session/load requests.
    pub permission_mode: ::diagnostics::enums::PermissionMode,

    /// Default model ID to use for new sessions.
    /// When set, the leader will inject `modelId` into session/new requests
    /// (only if the request doesn't already specify a modelId).
    pub default_model: Option<String>,

    /// Client binary version (e.g., "0.1.150").
    /// Used by the leader to detect version mismatches after client auto-updates.
    /// If the client version differs from the leader's version, a warning is logged.
    pub client_version: Option<String>,

    /// Whether this client has advertised `grow/codeNavigation.enabled`.
    /// When true, the leader injects `codeNavEnabled: true` into `session/new`
    /// and `session/load` requests so the agent can gate code-nav startup on a
    /// per-client basis rather than reading from shared last-initialized state.
    pub code_nav_enabled: bool,

    /// Whether the client handles terminal ACP messages (create, output, kill, etc.).
    /// When true, the leader injects `clientTerminal: true` into `session/new` and
    /// `session/load` so the agent routes terminal commands to the client via ACP
    /// instead of running them locally. Per-client so a TUI (`terminal: false`) and
    /// a web client (`terminal: true`) sharing the same leader get independent routing.
    pub terminal: bool,

    /// Whether the client handles filesystem ACP read/write messages.
    /// Same per-client isolation rationale as `terminal`.
    pub fs_read: bool,
    pub fs_write: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlCommand {
    GetLeaderInfo,
    CpuProfileStatus,
    StartCpuProfile {
        #[serde(default)]
        output: Option<String>,
        #[serde(default)]
        frequency_hz: Option<i32>,
    },
    StopCpuProfile,
    /// Ask the leader to relaunch onto a freshly-installed binary (driven by
    /// `grow update`). The leader stops admitting new turns, waits a bounded
    /// grace period for in-flight turns to finish, flushes session state, then
    /// exits with [`ShutdownReason::AutoUpdate`] so connected clients reconnect
    /// onto the new binary and restore their sessions via `session/load`.
    ///
    /// `to_version` is the version `grow update` just installed; the leader uses
    /// it to decline if it is already running that version or newer.
    RelaunchForUpdate {
        to_version: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlPayload {
    LeaderInfo {
        pid: u32,
        socket_path: PathBuf,
        lock_path: PathBuf,
        leader_protocol_version: u32,
        leader_binary_version: String,
        profiling_supported: bool,
        profiling_compiled_in: bool,
        cpu_profile_active: bool,
        cpu_profile_stopping: bool,
        profile_started_at: Option<String>,
    },
    CpuProfileStatus {
        active: bool,
        stopping: bool,
        started_at: Option<String>,
        artifact_path: Option<PathBuf>,
        frequency_hz: Option<i32>,
    },
    CpuProfileStarted {
        pid: u32,
        artifact_path: PathBuf,
        frequency_hz: i32,
        started_at: String,
    },
    CpuProfileStopped {
        pid: u32,
        artifact_path: PathBuf,
        started_at: String,
        stopped_at: String,
    },
    /// Ack for [`ControlCommand::RelaunchForUpdate`]: the leader accepted the
    /// request and will exit after a bounded grace period of `grace_ms`.
    Relaunching {
        from_version: String,
        to_version: String,
        grace_ms: u64,
    },
    /// Response to [`ControlCommand::RelaunchForUpdate`] when the leader will not
    /// relaunch — e.g. it is already running `to_version` or newer, or a relaunch
    /// is already in progress.
    RelaunchDeclined { reason: String },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientMessage {
    Register {
        client_type: String,
        /// Client mode determines how leader handles this client's communication
        mode: ClientMode,
        protocol_version: u32,
        capabilities: ClientCapabilities,
    },
    Acp {
        payload: String,
    },
    Control {
        request_id: String,
        command: ControlCommand,
    },
    Ping,
    Disconnect,
}

/// Reason for a planned leader shutdown, sent with [`ServerMessage::ShuttingDown`].
///
/// ## Runtime status
///
/// | Variant | Emitted today? | Notes |
/// |---------|---------------|-------|
/// | `AutoUpdate` | **Yes** — when `run_auto_update_checker` triggers shutdown | |
/// | `Manual` | **Yes** — default for SIGTERM, test cancellation, all other paths | |
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownReason {
    /// Leader is shutting down to install a downloaded binary auto-update.
    /// Clients should reconnect immediately via `connect_or_spawn`; the new binary
    /// will be picked up automatically.
    AutoUpdate,
    /// Unspecified or externally-triggered shutdown (SIGTERM, programmatic cancel, etc.).
    Manual,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServerMessage {
    /// Registration confirmation.
    ///
    /// `ready` indicates whether the leader has already completed its startup
    /// (auth + model prefetch). When `ready = false` the client **must** wait for a
    /// subsequent [`LeaderReady`](Self::LeaderReady) message before sending any ACP
    /// traffic — the server will hold the connection open until the leader is ready.
    Registered {
        client_id: u64,
        /// Whether the leader is fully initialised and ready to forward ACP traffic.
        ready: bool,
        leader_protocol_version: u32,
        leader_binary_version: String,
        runtime_cpu_profile: bool,
    },
    Acp {
        payload: String,
    },
    ControlResult {
        request_id: String,
        result: Result<ControlPayload, ControlError>,
    },
    Pong,
    Error {
        code: i32,
        message: String,
    },
    /// Advance notice of a planned shutdown. Sent before [`Shutdown`](Self::Shutdown)
    /// to give clients time to prepare for reconnection.
    ///
    /// Clients should treat this as a signal that [`Shutdown`](Self::Shutdown) is
    /// imminent and pre-arm their reconnection handlers (e.g. show a banner).
    ShuttingDown {
        reason: ShutdownReason,
        /// Milliseconds until the actual [`Shutdown`](Self::Shutdown) message.
        ///
        /// **Currently always `0`** — the server sends `Shutdown` immediately after
        /// `ShuttingDown` with no intervening sleep. Clients must not rely on this
        /// field providing a real grace window in the current implementation; treat
        /// `ShuttingDown` as equivalent to an imminent `Shutdown` regardless of this
        /// value.
        delay_ms: u64,
    },
    Shutdown,
    /// Sent by the server after a `Registered { ready: false }` once the leader
    /// finishes initialising. The client should treat this as the signal that
    /// ACP traffic will now be forwarded correctly.
    LeaderReady,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[tokio::test]
    async fn frame_roundtrip() {
        let (mut client, mut server) = duplex(1024);
        let data = b"hello world";

        write_frame(&mut client, data).await.unwrap();
        let received = read_frame(&mut server).await.unwrap();

        assert_eq!(received, data);
    }

    #[tokio::test]
    async fn message_roundtrip() {
        let (mut client, mut server) = duplex(1024);
        let msg = ClientMessage::Register {
            client_type: "test".into(),
            mode: ClientMode::Stdio,
            protocol_version: LEADER_PROTOCOL_VERSION,
            capabilities: ClientCapabilities::default(),
        };

        write_message(&mut client, &msg).await.unwrap();
        let received: ClientMessage = read_message(&mut server).await.unwrap();

        match received {
            ClientMessage::Register {
                client_type, mode, ..
            } => {
                assert_eq!(client_type, "test");
                assert_eq!(mode, ClientMode::Stdio);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[tokio::test]
    async fn control_message_roundtrip() {
        let (mut client, mut server) = duplex(1024);
        let msg = ClientMessage::Control {
            request_id: "req-1".into(),
            command: ControlCommand::StartCpuProfile {
                output: Some("/tmp/profile.folded".into()),
                frequency_hz: Some(250),
            },
        };

        write_message(&mut client, &msg).await.unwrap();
        let received: ClientMessage = read_message(&mut server).await.unwrap();

        assert!(matches!(
            received,
            ClientMessage::Control {
                request_id,
                command: ControlCommand::StartCpuProfile {
                    output: Some(output),
                    frequency_hz: Some(250),
                },
            } if request_id == "req-1" && output == "/tmp/profile.folded"
        ));
    }

    #[tokio::test]
    async fn connection_closed_on_eof() {
        let (client, mut server) = duplex(1024);
        drop(client);

        match read_frame(&mut server).await {
            Err(ProtocolError::ConnectionClosed) => {}
            other => panic!("expected ConnectionClosed, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn rejects_oversized_messages() {
        let (mut client, mut server) = duplex(1024);

        // Write a length header claiming a huge message
        client
            .write_all(&(MAX_MESSAGE_SIZE + 1).to_be_bytes())
            .await
            .unwrap();

        match read_frame(&mut server).await {
            Err(ProtocolError::MessageTooLarge(size)) => {
                assert_eq!(size, MAX_MESSAGE_SIZE + 1);
            }
            other => panic!("expected MessageTooLarge, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn multiple_frames_in_sequence() {
        let (mut client, mut server) = duplex(4096);

        for i in 0..10 {
            let data = format!("message {}", i);
            write_frame(&mut client, data.as_bytes()).await.unwrap();
        }
        drop(client);

        for i in 0..10 {
            let received = read_frame(&mut server).await.unwrap();
            assert_eq!(received, format!("message {}", i).as_bytes());
        }
    }

    #[test]
    fn registered_rejects_missing_protocol_metadata() {
        let json = r#"{"type":"registered","client_id":7}"#;
        assert!(serde_json::from_str::<ServerMessage>(json).is_err());
    }

    #[test]
    fn registered_roundtrips_required_protocol_metadata() {
        let msg = ServerMessage::Registered {
            client_id: 7,
            ready: true,
            leader_protocol_version: LEADER_PROTOCOL_VERSION,
            leader_binary_version: "1.2.3".into(),
            runtime_cpu_profile: true,
        };

        let json = serde_json::to_string(&msg).unwrap();
        let decoded: ServerMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            decoded,
            ServerMessage::Registered {
                client_id: 7,
                ready: true,
                leader_protocol_version: LEADER_PROTOCOL_VERSION,
                leader_binary_version: _,
                runtime_cpu_profile: true,
            }
        ));
    }

    #[test]
    fn control_payload_rejects_missing_stopping_flags() {
        let leader_info_json = r#"{
            "type":"leader_info",
            "pid":123,
            "socket_path":"/tmp/leader.sock",
            "lock_path":"/tmp/leader.lock",
            "leader_protocol_version":1,
            "leader_binary_version":"1.2.3",
            "profiling_supported":true,
            "profiling_compiled_in":true,
            "cpu_profile_active":false,
            "profile_started_at":null
        }"#;
        let status_json = r#"{
            "type":"cpu_profile_status",
            "active":false,
            "started_at":null,
            "artifact_path":null,
            "frequency_hz":null
        }"#;

        assert!(serde_json::from_str::<ControlPayload>(leader_info_json).is_err());
        assert!(serde_json::from_str::<ControlPayload>(status_json).is_err());
    }

    #[test]
    fn client_id_is_unique() {
        let ids: Vec<_> = (0..100).map(|_| ClientId::new()).collect();
        let unique: std::collections::HashSet<_> = ids.iter().map(|c| c.0).collect();
        assert_eq!(unique.len(), 100);
    }

    // --- ShuttingDown / ShutdownReason tests ---

    #[tokio::test]
    async fn shutting_down_message_roundtrip() {
        let (mut client, mut server) = duplex(1024);
        let msg = ServerMessage::ShuttingDown {
            reason: ShutdownReason::AutoUpdate,
            delay_ms: 2000,
        };

        write_message(&mut client, &msg).await.unwrap();
        let received: ServerMessage = read_message(&mut server).await.unwrap();

        match received {
            ServerMessage::ShuttingDown { reason, delay_ms } => {
                assert_eq!(reason, ShutdownReason::AutoUpdate);
                assert_eq!(delay_ms, 2000);
            }
            _ => panic!("Expected ShuttingDown, got {:?}", received),
        }
    }

    #[test]
    fn shutdown_reason_variants_serialize_correctly() {
        let auto = serde_json::to_string(&ShutdownReason::AutoUpdate).unwrap();
        assert_eq!(auto, "\"auto_update\"");

        let manual = serde_json::to_string(&ShutdownReason::Manual).unwrap();
        assert_eq!(manual, "\"manual\"");

        // Verify deserialization
        let parsed: ShutdownReason = serde_json::from_str("\"auto_update\"").unwrap();
        assert_eq!(parsed, ShutdownReason::AutoUpdate);
    }
}
