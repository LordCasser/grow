//! Dispatch-time eligibility revalidation for MCP tool calls (backlog MCP-F1).
//!
//! Every test drives a real entry point — the model's `McpErasedTool::run` or the
//! direct `McpClient::call_tool` — against an in-process MCP server reached over
//! the production ACP reverse bridge. The server parks its `initialize`
//! handshake on a barrier, so a test can revoke eligibility while that call's
//! preparation is still in flight. Wire-level counters then prove whether a
//! `tools/call` actually left the client: no sleeps, no inference from state.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tool_runtime::Tool as _;

/// In-process MCP server over the real [`crate::acp_transport`] bridge.
///
/// `hold_init` selects one handshake by ordinal (1 = first initialization) to
/// park on the barrier, so a test can gate the first init and a later recovery
/// handshake independently.
///
/// A `tools/call` is counted as *received* the moment its branch is entered,
/// before any argument check or delay — so "0 received" really means the request
/// never reached this server, and a call whose response is still outstanding is
/// still reported as received. `completed` separately records replies produced,
/// which keeps "we never sent it" distinct from "it was sent and we lost the race".
struct BridgeServer {
    tool: &'static str,
    hold_init: usize,
    call_delay_ms: u64,
    inits: AtomicUsize,
    received: AtomicUsize,
    completed: AtomicUsize,
    started: Notify,
    release: Notify,
}

impl BridgeServer {
    fn new(tool: &'static str) -> Self {
        Self {
            tool,
            hold_init: 0,
            call_delay_ms: 0,
            inits: AtomicUsize::new(0),
            received: AtomicUsize::new(0),
            completed: AtomicUsize::new(0),
            started: Notify::new(),
            release: Notify::new(),
        }
    }

    fn holding_init(tool: &'static str, ordinal: usize) -> Self {
        Self {
            hold_init: ordinal,
            ..Self::new(tool)
        }
    }

    /// `tools/call` requests that reached this server.
    fn calls(&self) -> usize {
        self.received.load(Ordering::SeqCst)
    }

    fn replies(&self) -> usize {
        self.completed.load(Ordering::SeqCst)
    }

    fn inits(&self) -> usize {
        self.inits.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl crate::acp_transport::AcpReverseInvoker for BridgeServer {
    async fn invoke(
        &self,
        _server_id: &str,
        message: serde_json::Value,
        _timeout: std::time::Duration,
    ) -> Result<serde_json::Value, String> {
        let result = match message["method"].as_str().unwrap_or_default() {
            "initialize" => {
                let ordinal = self.inits.fetch_add(1, Ordering::SeqCst) + 1;
                if self.hold_init == ordinal {
                    self.started.notify_one();
                    self.release.notified().await;
                }
                serde_json::json!({
                    "protocolVersion": message["params"]["protocolVersion"],
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "bridge", "version": "0.0.0"},
                })
            }
            "tools/list" => serde_json::json!({
                "tools": [{
                    "name": self.tool,
                    "description": "bridge tool",
                    "inputSchema": {"type": "object", "properties": {}},
                }],
            }),
            "tools/call" => {
                // Count receipt immediately: this is the causal barrier that
                // proves the client actually sent the request.
                self.received.fetch_add(1, Ordering::SeqCst);
                let name = message["params"]["name"].as_str().unwrap_or_default();
                if name != self.tool {
                    return Err(format!("unexpected tool {name}"));
                }
                if self.call_delay_ms > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(self.call_delay_ms)).await;
                }
                self.completed.fetch_add(1, Ordering::SeqCst);
                serde_json::json!({
                    "content": [{"type": "text", "text": "ok"}],
                    "isError": false,
                })
            }
            other => return Err(format!("unexpected method {other}")),
        };
        Ok(serde_json::json!({"jsonrpc": "2.0", "id": message["id"], "result": result}))
    }
}

const SERVER: &str = "bridge";
const TOOL: &str = "read";

fn acp_client(name: &str, server: &Arc<BridgeServer>) -> Arc<McpClient> {
    let invoker: Arc<dyn crate::acp_transport::AcpReverseInvoker> = server.clone();
    Arc::new(McpClient::new_acp(
        name.to_string(),
        "srv_0".to_string(),
        invoker,
        None,
        None,
    ))
}

fn timeout_overrides(tool_timeout_sec: u64) -> McpClientTimeoutOverrides {
    McpClientTimeoutOverrides {
        startup_timeout_sec: Some(10),
        tool_timeout_sec: Some(tool_timeout_sec),
        ..Default::default()
    }
}

/// The session state that owns `client` for [`SERVER`], with the tool published
/// as eligible — the unchanged baseline every revocation departs from.
async fn owning_state(client: &Arc<McpClient>) -> Arc<Mutex<McpState>> {
    let mut state = McpState::new(vec![]);
    state
        .owned_clients
        .insert(SERVER.to_string(), Arc::clone(client));
    state.publish_eligibility(HashSet::from([format!("{SERVER}__{TOOL}")]));
    Arc::new(Mutex::new(state))
}

fn erased_tool(mcp_state: &Arc<Mutex<McpState>>) -> McpErasedTool {
    McpErasedTool {
        tool: McpTool::new(
            TOOL.to_string(),
            "read description".to_string(),
            SERVER.to_string(),
            Arc::clone(mcp_state),
            serde_json::json!({"type": "object"}),
            None,
        ),
    }
}

/// Fail with a concrete message instead of hanging if a barrier never arrives.
async fn wait_for_barrier(server: &BridgeServer) {
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        server.started.notified(),
    )
    .await
    .expect("the initialization handshake never reached the barrier");
}

/// Run a model tool call up to its initialization barrier and hand the pending
/// call back, so the test can revoke eligibility before releasing preparation.
macro_rules! park_model_call {
    ($tool:expr, $server:expr) => {{
        let mut call = Box::pin($tool.run(
            tool_runtime::ToolCallContext::default(),
            serde_json::json!({}),
        ));
        tokio::select! {
            _ = wait_for_barrier(&$server) => {}
            result = &mut call => {
                panic!("the tool call ended before the handshake barrier: {result:?}")
            }
        }
        call
    }};
}

/// A pre-dispatch rejection is an explicit, known-outcome error that sends
/// nothing further to the retired client and never re-handshakes it.
///
/// `calls_before` / `inits_before` are the counters observed once the test's
/// barrier is reached, so a scenario that already dispatched an earlier, still
/// legitimate call asserts against that baseline instead of zero.
fn assert_no_dispatch_since(
    error: &tool_runtime::ToolError,
    retired: &BridgeServer,
    calls_before: usize,
    inits_before: usize,
) {
    assert!(
        !error.detail.contains("outcome is unknown"),
        "an undispatched call has a known outcome: {error}"
    );
    assert_eq!(
        retired.calls(),
        calls_before,
        "the retired client received a tools/call after losing eligibility: {error}"
    );
    assert_eq!(
        retired.inits(),
        inits_before,
        "a pre-dispatch rejection must not handshake the retired client again: {error}"
    );
}

// ── Model path ──────────────────────────────────────────────────────

#[tokio::test]
async fn model_call_rejected_when_tool_disabled_during_initialization() {
    let server = Arc::new(BridgeServer::holding_init(TOOL, 1));
    let client = acp_client(SERVER, &server);
    let state = owning_state(&client).await;
    let tool = erased_tool(&state);

    let call = park_model_call!(tool, server);
    state
        .lock()
        .await
        .disabled_tools
        .insert(SERVER.to_string(), HashSet::from([TOOL.to_string()]));
    server.release.notify_one();

    let error = call
        .await
        .expect_err("a tool disabled during initialization must not be dispatched");
    assert!(
        error.detail.contains("disabled"),
        "expected a disabled-tool rejection, got: {error}"
    );
    assert_no_dispatch_since(&error, &server, 0, 1);
}

#[tokio::test]
async fn model_call_rejected_when_server_removed_during_initialization() {
    let server = Arc::new(BridgeServer::holding_init(TOOL, 1));
    let client = acp_client(SERVER, &server);
    let state = owning_state(&client).await;
    let tool = erased_tool(&state);

    let call = park_model_call!(tool, server);
    state.lock().await.owned_clients.remove(SERVER);
    server.release.notify_one();

    let error = call
        .await
        .expect_err("a server removed during initialization must not be dispatched");
    assert!(
        error.detail.contains("not found") || error.detail.contains("no longer"),
        "expected a removal rejection, got: {error}"
    );
    assert_no_dispatch_since(&error, &server, 0, 1);
}

#[tokio::test]
async fn model_call_rejected_without_falling_back_to_same_name_replacement() {
    let server = Arc::new(BridgeServer::holding_init(TOOL, 1));
    let client = acp_client(SERVER, &server);
    let replacement_server = Arc::new(BridgeServer::new(TOOL));
    let replacement = acp_client(SERVER, &replacement_server);
    let state = owning_state(&client).await;
    let tool = erased_tool(&state);

    let call = park_model_call!(tool, server);
    state
        .lock()
        .await
        .owned_clients
        .insert(SERVER.to_string(), Arc::clone(&replacement));
    server.release.notify_one();

    let error = call
        .await
        .expect_err("a replaced server must not be dispatched from the retired client");
    assert_no_dispatch_since(&error, &server, 0, 1);
    assert_eq!(
        replacement_server.calls(),
        0,
        "the replacement must not inherit the retired invocation"
    );
    assert_eq!(
        replacement_server.inits(),
        0,
        "the replacement must not be initialized by the retired invocation"
    );
}

#[tokio::test]
async fn model_call_rejected_when_ancestor_revokes_tool_before_child_dispatch() {
    let server = Arc::new(BridgeServer::holding_init(TOOL, 1));
    let client = acp_client(SERVER, &server);

    // Parent authorizes the client + tool; the child inherits both and holds the
    // same transport.
    let mut parent = McpState::new(vec![]);
    parent
        .owned_clients
        .insert(SERVER.to_string(), Arc::clone(&client));
    parent.publish_eligibility(HashSet::from([format!("{SERVER}__{TOOL}")]));
    let pool = SharedMcpPool::from_state(&parent);
    let mut child_state = McpState::new(vec![]);
    child_state.import_shared_clients(&pool);
    assert!(
        Arc::ptr_eq(child_state.get_client(SERVER).unwrap(), &client),
        "the child must hold the inherited transport"
    );
    let state = Arc::new(Mutex::new(child_state));
    let tool = erased_tool(&state);

    let call = park_model_call!(tool, server);
    // The ancestor withdraws the tool ceiling. The child map is deliberately not
    // reconciled, so only the live chain can observe this.
    parent.publish_eligibility(HashSet::new());
    server.release.notify_one();

    let error = call
        .await
        .expect_err("an upstream tool revocation must reach the pending child call");
    assert!(
        error.detail.contains("not")
            || error.detail.contains("denied")
            || error.detail.contains("eligible"),
        "expected an eligibility rejection, got: {error}"
    );
    assert_no_dispatch_since(&error, &server, 0, 1);
}

#[tokio::test]
async fn model_call_rejected_when_ancestor_rebinds_the_same_name_during_initialization() {
    let server = Arc::new(BridgeServer::holding_init(TOOL, 1));
    let client = acp_client(SERVER, &server);
    let replacement_server = Arc::new(BridgeServer::new(TOOL));
    let replacement = acp_client(SERVER, &replacement_server);

    let mut parent = McpState::new(vec![]);
    parent
        .owned_clients
        .insert(SERVER.to_string(), Arc::clone(&client));
    parent.publish_eligibility(HashSet::from([format!("{SERVER}__{TOOL}")]));
    let pool = SharedMcpPool::from_state(&parent);
    let mut child_state = McpState::new(vec![]);
    child_state.import_shared_clients(&pool);
    let state = Arc::new(Mutex::new(child_state));
    let tool = erased_tool(&state);

    let call = park_model_call!(tool, server);
    // Same name, new incarnation. The child keeps the old transport and does not
    // reconcile, so the stale binding must still be refused.
    parent
        .owned_clients
        .insert(SERVER.to_string(), Arc::clone(&replacement));
    parent.publish_eligibility(HashSet::from([format!("{SERVER}__{TOOL}")]));
    server.release.notify_one();

    let error = call
        .await
        .expect_err("an upstream rebind must refuse the retired inherited client");
    assert_no_dispatch_since(&error, &server, 0, 1);
    assert_eq!(
        replacement_server.calls(),
        0,
        "the rebound service must not inherit the retired invocation"
    );
}

#[tokio::test]
async fn model_call_to_owned_override_survives_unrelated_ancestor_revocation() {
    let server = Arc::new(BridgeServer::holding_init(TOOL, 1));
    let owned = acp_client(SERVER, &server);
    let inherited_server = Arc::new(BridgeServer::new(TOOL));
    let inherited = acp_client(SERVER, &inherited_server);

    // The ancestor authorizes its own client for the same name; the child's
    // owned client overrides it and must stay dispatchable.
    let mut parent = McpState::new(vec![]);
    parent
        .owned_clients
        .insert(SERVER.to_string(), Arc::clone(&inherited));
    parent.publish_eligibility(HashSet::from([format!("{SERVER}__{TOOL}")]));
    let pool = SharedMcpPool::from_state(&parent);
    let mut child_state = McpState::new(vec![]);
    child_state.import_shared_clients(&pool);
    child_state
        .owned_clients
        .insert(SERVER.to_string(), Arc::clone(&owned));
    child_state.publish_eligibility(HashSet::from([format!("{SERVER}__{TOOL}")]));
    let state = Arc::new(Mutex::new(child_state));
    let tool = erased_tool(&state);

    let call = park_model_call!(tool, server);
    parent.publish_eligibility(HashSet::new());
    server.release.notify_one();

    call.await
        .expect("an owned override is independent of the inherited authority");
    assert_eq!(
        server.calls(),
        1,
        "the owned client must dispatch exactly once"
    );
    assert_eq!(
        inherited_server.calls(),
        0,
        "the inherited transport must not receive the owned client's call"
    );
}

#[tokio::test]
async fn model_call_dispatches_once_when_eligibility_survives_initialization() {
    let server = Arc::new(BridgeServer::holding_init(TOOL, 1));
    let client = acp_client(SERVER, &server);
    let state = owning_state(&client).await;
    let tool = erased_tool(&state);

    let call = park_model_call!(tool, server);
    // An unrelated server changes the catalog; this call's server does not.
    state.lock().await.publish_eligibility(HashSet::from([
        format!("{SERVER}__{TOOL}"),
        "other__list".to_string(),
    ]));
    server.release.notify_one();

    let output = call
        .await
        .expect("an unchanged eligible client dispatches normally");
    assert!(matches!(output, ToolOutput::MCP(_)));
    assert_eq!(
        server.calls(),
        1,
        "the captured client must be called exactly once, without replay"
    );
}

#[tokio::test]
async fn model_call_does_not_refund_preparation_time_to_its_deadline() {
    let mut server = BridgeServer::holding_init(TOOL, 1);
    // Longer than what is left of the admitted budget once preparation ends, but
    // short enough to complete if the call were handed a fresh full timeout.
    server.call_delay_ms = 400;
    let server = Arc::new(server);
    // A 1s tool budget is admitted before the handshake barrier is released.
    let invoker: Arc<dyn crate::acp_transport::AcpReverseInvoker> = server.clone();
    let client = Arc::new(McpClient::new_acp(
        SERVER.to_string(),
        "srv_0".to_string(),
        invoker,
        Some(&timeout_overrides(1)),
        None,
    ));
    let state = owning_state(&client).await;
    let tool = erased_tool(&state);

    let started = std::time::Instant::now();
    let call = park_model_call!(tool, server);
    // Spend most of the admitted budget inside preparation.
    tokio::time::sleep(std::time::Duration::from_millis(700)).await;
    server.release.notify_one();

    let error = call
        .await
        .expect_err("the admitted deadline must still bound the dispatched call");
    let elapsed = started.elapsed();

    // The budget is measured from admission, so preparation is not refunded: the
    // call cannot have run to a fresh full second *after* the barrier.
    assert!(
        elapsed < std::time::Duration::from_millis(1400),
        "the deadline was re-granted after preparation ({elapsed:?}); received={} replies={} error={error}",
        server.calls(),
        server.replies()
    );
    assert!(
        error.detail.contains("timed out") || error.detail.contains("outcome is unknown"),
        "expected a timeout, got: {error} (received={} replies={})",
        server.calls(),
        server.replies()
    );
    // A timed-out dispatch is never replayed, whether or not the request had
    // already reached the wire when the budget ran out.
    assert!(
        server.calls() <= 1,
        "the invocation was replayed: received={} replies={} error={error}",
        server.calls(),
        server.replies()
    );
    assert_eq!(
        server.inits(),
        1,
        "a timed-out call must not handshake the client again"
    );
}

#[tokio::test]
async fn model_call_rejection_after_recovery_handshake_does_not_replay() {
    let server = Arc::new(BridgeServer::holding_init(TOOL, 2));
    let client = acp_client(SERVER, &server);
    let state = owning_state(&client).await;
    let tool = erased_tool(&state);

    // First initialization completes normally: source is a warm transport.
    let first = tool
        .run(
            tool_runtime::ToolCallContext::default(),
            serde_json::json!({}),
        )
        .await
        .expect("the first call initializes and dispatches");
    assert!(matches!(first, ToolOutput::MCP(_)));
    assert_eq!(server.calls(), 1);
    assert_eq!(server.inits(), 1);

    // Recovery resets the transport; the *second* handshake is the barrier.
    client.reset_transport(None).await;
    let call = park_model_call!(tool, server);
    // Baseline: the first call legitimately dispatched once, and this recovery
    // handshake has already been entered.
    let calls_before = server.calls();
    let inits_before = server.inits();
    assert_eq!((calls_before, inits_before), (1, 2));
    state.lock().await.owned_clients.remove(SERVER);
    server.release.notify_one();

    let error = call
        .await
        .expect_err("a removal during recovery preparation must not be dispatched");
    assert_no_dispatch_since(&error, &server, calls_before, inits_before);
    assert_eq!(
        server.calls(),
        1,
        "the recovered client must not receive a second tools/call"
    );
}

// ── Direct path ─────────────────────────────────────────────────────
//
// `McpClient::call_tool` is the entry the Shell's `grow/mcp/call` handler uses.
// The captured client is resolved by the caller before preparation, so the same
// initialization window exists here and must be closed by the same owner.

/// Capture the direct call's client exactly as the Shell handler does — from the
/// session state before preparation — then park on that client's handshake.
macro_rules! park_direct_call {
    ($client:expr, $state:expr, $server:expr) => {{
        let client = Arc::clone(&$client);
        let state = Arc::clone(&$state);
        let mut call = Box::pin(async move {
            client
                .call_tool(
                    &state,
                    TOOL,
                    serde_json::json!({}),
                    std::time::Duration::from_secs(10),
                )
                .await
        });
        tokio::select! {
            _ = wait_for_barrier(&$server) => {}
            result = &mut call => {
                panic!("the direct call ended before the handshake barrier: {result:?}")
            }
        }
        call
    }};
}

fn assert_direct_pre_dispatch_rejection(error: &McpError, retired: &BridgeServer) {
    assert_eq!(
        retired.calls(),
        0,
        "the retired client received a tools/call after losing eligibility: {error}"
    );
    assert_eq!(
        retired.inits(),
        1,
        "a pre-dispatch rejection must not handshake the retired client again"
    );
}

#[tokio::test]
async fn direct_call_rejected_when_tool_disabled_during_initialization() {
    let server = Arc::new(BridgeServer::holding_init(TOOL, 1));
    let client = acp_client(SERVER, &server);
    let state = owning_state(&client).await;

    let call = park_direct_call!(client, state, server);
    state
        .lock()
        .await
        .disabled_tools
        .insert(SERVER.to_string(), HashSet::from([TOOL.to_string()]));
    server.release.notify_one();

    let error = call
        .await
        .expect_err("a tool disabled during initialization must not be dispatched");
    assert_direct_pre_dispatch_rejection(&error, &server);
}

#[tokio::test]
async fn direct_call_rejected_when_server_removed_during_initialization() {
    let server = Arc::new(BridgeServer::holding_init(TOOL, 1));
    let client = acp_client(SERVER, &server);
    let state = owning_state(&client).await;

    let call = park_direct_call!(client, state, server);
    state.lock().await.owned_clients.remove(SERVER);
    server.release.notify_one();

    let error = call
        .await
        .expect_err("a server removed during initialization must not be dispatched");
    assert_direct_pre_dispatch_rejection(&error, &server);
}

#[tokio::test]
async fn direct_call_rejected_without_falling_back_to_same_name_replacement() {
    let server = Arc::new(BridgeServer::holding_init(TOOL, 1));
    let client = acp_client(SERVER, &server);
    let replacement_server = Arc::new(BridgeServer::new(TOOL));
    let replacement = acp_client(SERVER, &replacement_server);
    let state = owning_state(&client).await;

    let call = park_direct_call!(client, state, server);
    state
        .lock()
        .await
        .owned_clients
        .insert(SERVER.to_string(), Arc::clone(&replacement));
    server.release.notify_one();

    let error = call
        .await
        .expect_err("a replaced server must not be dispatched from the retired client");
    assert_direct_pre_dispatch_rejection(&error, &server);
    assert_eq!(
        replacement_server.calls(),
        0,
        "the replacement must not inherit the retired invocation"
    );
    assert_eq!(
        replacement_server.inits(),
        0,
        "the replacement must not be initialized by the retired invocation"
    );
}

#[tokio::test]
async fn direct_call_rejected_when_ancestor_revokes_tool_before_dispatch() {
    let server = Arc::new(BridgeServer::holding_init(TOOL, 1));
    let client = acp_client(SERVER, &server);

    let mut parent = McpState::new(vec![]);
    parent
        .owned_clients
        .insert(SERVER.to_string(), Arc::clone(&client));
    parent.publish_eligibility(HashSet::from([format!("{SERVER}__{TOOL}")]));
    let pool = SharedMcpPool::from_state(&parent);
    let mut child_state = McpState::new(vec![]);
    child_state.import_shared_clients(&pool);
    assert!(Arc::ptr_eq(
        child_state.get_client(SERVER).unwrap(),
        &client
    ));
    let state = Arc::new(Mutex::new(child_state));

    let call = park_direct_call!(client, state, server);
    parent.publish_eligibility(HashSet::new());
    server.release.notify_one();

    let error = call
        .await
        .expect_err("an upstream tool revocation must reach the pending direct call");
    assert_direct_pre_dispatch_rejection(&error, &server);
}

#[tokio::test]
async fn direct_call_rejected_when_ancestor_rebinds_same_name_during_initialization() {
    let server = Arc::new(BridgeServer::holding_init(TOOL, 1));
    let client = acp_client(SERVER, &server);
    let replacement_server = Arc::new(BridgeServer::new(TOOL));
    let replacement = acp_client(SERVER, &replacement_server);

    let mut parent = McpState::new(vec![]);
    parent
        .owned_clients
        .insert(SERVER.to_string(), Arc::clone(&client));
    parent.publish_eligibility(HashSet::from([format!("{SERVER}__{TOOL}")]));
    let pool = SharedMcpPool::from_state(&parent);
    let mut child_state = McpState::new(vec![]);
    child_state.import_shared_clients(&pool);
    let state = Arc::new(Mutex::new(child_state));

    let call = park_direct_call!(client, state, server);
    parent
        .owned_clients
        .insert(SERVER.to_string(), Arc::clone(&replacement));
    parent.publish_eligibility(HashSet::from([format!("{SERVER}__{TOOL}")]));
    server.release.notify_one();

    let error = call
        .await
        .expect_err("an upstream rebind must refuse the retired inherited client");
    assert_direct_pre_dispatch_rejection(&error, &server);
    assert_eq!(replacement_server.calls(), 0);
}

#[tokio::test]
async fn direct_call_does_not_refund_eligibility_lock_wait_to_the_send() {
    let mut server = BridgeServer::holding_init(TOOL, 1);
    // Longer than what remains of the admitted budget once the eligibility lock
    // wait is over, but short enough to complete if the send were handed a fresh
    // full timeout.
    server.call_delay_ms = 700;
    let server = Arc::new(server);
    let invoker: Arc<dyn crate::acp_transport::AcpReverseInvoker> = server.clone();
    let client = Arc::new(McpClient::new_acp(
        SERVER.to_string(),
        "srv_0".to_string(),
        invoker,
        Some(&timeout_overrides(1)),
        None,
    ));
    let state = owning_state(&client).await;

    // Hold the session state before the call runs, so its eligibility check has to
    // wait on this lock, and release it with only ~400 ms of the 1 s budget left.
    let guard = Arc::clone(&state).lock_owned().await;
    let call_state = Arc::clone(&state);
    let call_client = Arc::clone(&client);
    let started = std::time::Instant::now();
    let call = tokio::spawn(async move {
        call_client
            .call_tool(
                &call_state,
                TOOL,
                serde_json::json!({}),
                std::time::Duration::from_secs(1),
            )
            .await
    });
    wait_for_barrier(&server).await;
    server.release.notify_one();
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    drop(guard);

    let error = call
        .await
        .expect("the call task must not panic")
        .expect_err("the eligibility wait must be charged to the admitted 1s budget");
    let elapsed = started.elapsed();
    assert!(
        error.is_timeout(),
        "a refunded deadline would have had a full second left and completed: {error}"
    );
    assert!(
        elapsed < std::time::Duration::from_millis(1400),
        "the send was handed a fresh budget after the eligibility wait ({elapsed:?})"
    );
    assert_eq!(
        server.calls(),
        1,
        "the request did reach the wire once, then the budget expired"
    );
}

#[tokio::test]
async fn direct_call_dispatches_once_when_eligibility_survives_initialization() {
    let server = Arc::new(BridgeServer::holding_init(TOOL, 1));
    let client = acp_client(SERVER, &server);
    let state = owning_state(&client).await;

    let call = park_direct_call!(client, state, server);
    // Unrelated catalog change: this server keeps its binding and tool.
    state.lock().await.publish_eligibility(HashSet::from([
        format!("{SERVER}__{TOOL}"),
        "other__list".to_string(),
    ]));
    server.release.notify_one();

    let result = call
        .await
        .expect("an unchanged eligible client dispatches normally");
    assert_eq!(result.content.len(), 1);
    assert_eq!(
        server.calls(),
        1,
        "the captured client must be called exactly once, without replay"
    );
    assert_eq!(server.replies(), 1);
}
