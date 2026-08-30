//! In-process `session/load` harness: a real `MvpAgent` wired to a client over
//! ACP duplex pipes, so a test can time a real load round-trip without a
//! subprocess.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use acp_transport::protocol as acp;
use acp_transport::{
    AcpAgentGatewayReceiver as GatewayReceiver, AcpAgentGatewaySender as GatewaySender,
    LineBufferedRead,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::agent::config::Config as AgentConfig;
use crate::agent::mvp_agent::MvpAgent;

const DUPLEX_BUFFER_BYTES: usize = 16 * 1024 * 1024;
const INIT_TIMEOUT: Duration = Duration::from_secs(60);
const LOAD_TIMEOUT: Duration = Duration::from_secs(180);

/// A completed `session/load` over the shared in-process harness. `client_conn`
/// is returned so the caller keeps the connection alive for any post-load
/// notifications (e.g. the re-advertise) it still wants to observe.
pub struct LoadedAgent {
    pub client_conn: acp_transport::ClientSideConnection,
    pub load_started: Instant,
    pub load_elapsed: Duration,
}

/// Stand up a real `MvpAgent` over in-process ACP pipes wired to `client`, run
/// the initialize and authenticate handshake, then time one `session/load`
/// round-trip. Must run inside a `LocalSet`, since it spawns local tasks.
pub async fn load_session_via_agent<C: acp_transport::AcpClientHandler + 'static>(
    client: C,
    client_type: &str,
    session_id: acp::SessionId,
    cwd: PathBuf,
) -> LoadedAgent {
    let agent_config = AgentConfig::default();
    let (gw_tx, gw_rx) = tokio::sync::mpsc::unbounded_channel();
    let gateway = GatewaySender::new(gw_tx);
    let agent = MvpAgent::new(gateway, &agent_config).expect("valid config");

    let (c2a_a, c2a_b) = tokio::io::duplex(DUPLEX_BUFFER_BYTES);
    let (a2c_a, a2c_b) = tokio::io::duplex(DUPLEX_BUFFER_BYTES);

    // Agent side.
    let agent_incoming = LineBufferedRead::spawn_local(c2a_b.compat());
    let (agent_conn, agent_io) =
        acp_transport::connect_agent_v1(agent, a2c_a.compat_write(), agent_incoming, |fut| {
            tokio::task::spawn_local(fut);
        });
    tokio::task::spawn_local(GatewayReceiver::new(gw_rx, agent_conn).run());
    tokio::task::spawn_local(agent_io);

    // Client side.
    let client_incoming = LineBufferedRead::spawn_local(a2c_b.compat());
    let (client_conn, client_io) =
        acp_transport::connect_client_v1(client, c2a_a.compat_write(), client_incoming, |fut| {
            tokio::task::spawn_local(fut);
        });
    tokio::task::spawn_local(client_io);

    use acp_transport::AcpAgentHandler as _;

    let init = tokio::time::timeout(
        INIT_TIMEOUT,
        client_conn.initialize(
            acp::InitializeRequest::new(acp::ProtocolVersion::V1)
                .client_capabilities(
                    acp::ClientCapabilities::new()
                        .fs(acp::FileSystemCapabilities::new())
                        .terminal(false),
                )
                .meta(
                    serde_json::json!({
                        "startupHints": { "nonInteractive": true, "skipGitStatus": true, "skipProjectLayout": true },
                        "clientType": client_type,
                        "clientVersion": "0.0-test",
                    })
                    .as_object()
                    .cloned(),
                ),
        ),
    )
    .await
    .expect("initialize timed out")
    .expect("initialize failed");

    // Best-effort auth: the mock backend accepts anything and `load` does not
    // require a prior success, so ignore any failure here.
    if let Some(method) = init
        .auth_methods
        .iter()
        .find(|m| &*m.id().0 == "provider.api_key")
    {
        let _ = client_conn
            .authenticate(
                acp::AuthenticateRequest::new(method.id().clone())
                    .meta(serde_json::json!({ "headless": true }).as_object().cloned()),
            )
            .await;
    }

    let load_started = Instant::now();
    tokio::time::timeout(
        LOAD_TIMEOUT,
        client_conn.load_session(acp::LoadSessionRequest::new(session_id, cwd)),
    )
    .await
    .expect("session/load timed out (>180s)")
    .expect("session/load failed");
    let load_elapsed = load_started.elapsed();

    LoadedAgent {
        client_conn,
        load_started,
        load_elapsed,
    }
}
