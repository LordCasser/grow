use std::{future::Future, pin::Pin, rc::Rc};

use futures::{AsyncRead, AsyncWrite, FutureExt as _};

use crate::{
    AcpAgentHandler, AcpClientHandler, AcpGatewayReceiver, AcpGatewaySender, protocol as acp,
};

/// Grow's agent-side connection handle.
pub type AgentSideConnection = AcpGatewaySender<acp::AgentSide>;

/// Grow's client-side connection handle.
pub type ClientSideConnection = AcpGatewaySender<acp::ClientSide>;

type SpawnLocal = Rc<dyn Fn(Pin<Box<dyn Future<Output = ()>>>)>;

enum AgentInbound {
    Request {
        request: acp::ClientRequest,
        responder: acp::Responder<serde_json::Value>,
        cancellation: acp::RequestCancellation,
    },
    Notification(acp::ClientNotification),
}

enum ClientInbound {
    Request {
        request: acp::AgentRequest,
        responder: acp::Responder<serde_json::Value>,
        cancellation: acp::RequestCancellation,
    },
    Notification(acp::AgentNotification),
}

/// Connect a Grow v1 agent handler to a byte transport using ACP SDK 2.x.
pub fn connect_agent_v1(
    agent: impl AcpAgentHandler + 'static,
    outgoing: impl AsyncWrite + Unpin + Send + 'static,
    incoming: impl AsyncRead + Unpin + Send + 'static,
    spawn: impl Fn(Pin<Box<dyn Future<Output = ()>>>) + 'static,
) -> (AgentSideConnection, impl Future<Output = acp::Result<()>>) {
    let spawn: SpawnLocal = Rc::new(spawn);
    let agent = Rc::new(agent);
    let (incoming_tx, mut incoming_rx) = tokio::sync::mpsc::unbounded_channel();
    let inbound_spawn = spawn.clone();
    spawn(Box::pin(async move {
        while let Some(inbound) = incoming_rx.recv().await {
            let agent = agent.clone();
            match inbound {
                AgentInbound::Request {
                    request,
                    responder,
                    cancellation,
                } => inbound_spawn(Box::pin(async move {
                    let result =
                        dispatch_agent_request(agent.as_ref(), request, cancellation).await;
                    let _ = match result {
                        Ok(value) => responder.respond(value),
                        Err(error) => responder.respond_with_error(error),
                    };
                })),
                AgentInbound::Notification(notification) => {
                    inbound_spawn(Box::pin(async move {
                        let _ = dispatch_agent_notification(agent.as_ref(), notification).await;
                    }));
                }
            }
        }
    }));

    let (outgoing_tx, outgoing_rx) = tokio::sync::mpsc::unbounded_channel();
    let connection = AcpGatewaySender::<acp::AgentSide>::new(outgoing_tx);
    let transport = agent_client_protocol::ByteStreams::new(outgoing, incoming);
    let request_sender = incoming_tx.clone();
    let notification_sender = incoming_tx;

    let future = agent_client_protocol::Agent
        .builder()
        .name("grow-agent-v1")
        .on_receive_request(
            move |request: acp::ClientRequest,
                  responder: agent_client_protocol::Responder<serde_json::Value>,
                  _cx: acp::ConnectionTo<agent_client_protocol::Client>| {
                let request_sender = request_sender.clone();
                async move {
                    let cancellation = responder.cancellation();
                    request_sender
                        .send(AgentInbound::Request {
                            request,
                            responder,
                            cancellation,
                        })
                        .map_err(|_| acp::Error::internal_error().data("agent handler closed"))?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            move |notification: acp::ClientNotification, _cx| {
                let notification_sender = notification_sender.clone();
                async move {
                    notification_sender
                        .send(AgentInbound::Notification(notification))
                        .map_err(|_| acp::Error::internal_error().data("agent handler closed"))
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_with(transport, async move |cx| {
            let gateway = AcpGatewayReceiver::new(outgoing_rx, AgentPeer { cx: cx.clone() });
            let mut gateway = Box::pin(gateway.run()).fuse();
            let mut closed = Box::pin(cx.incoming_closed()).fuse();
            futures::select! {
                () = gateway => Ok(()),
                () = closed => Ok(()),
            }
        });

    (connection, future)
}

/// Connect a Grow v1 client handler to a byte transport using ACP SDK 2.x.
pub fn connect_client_v1(
    client: impl AcpClientHandler + 'static,
    outgoing: impl AsyncWrite + Unpin + Send + 'static,
    incoming: impl AsyncRead + Unpin + Send + 'static,
    spawn: impl Fn(Pin<Box<dyn Future<Output = ()>>>) + 'static,
) -> (ClientSideConnection, impl Future<Output = acp::Result<()>>) {
    let spawn: SpawnLocal = Rc::new(spawn);
    let client = Rc::new(client);
    let (incoming_tx, mut incoming_rx) = tokio::sync::mpsc::unbounded_channel();
    let inbound_spawn = spawn.clone();
    spawn(Box::pin(async move {
        while let Some(inbound) = incoming_rx.recv().await {
            let client = client.clone();
            match inbound {
                ClientInbound::Request {
                    request,
                    responder,
                    cancellation,
                } => inbound_spawn(Box::pin(async move {
                    let result =
                        dispatch_client_request(client.as_ref(), request, cancellation).await;
                    let _ = match result {
                        Ok(value) => responder.respond(value),
                        Err(error) => responder.respond_with_error(error),
                    };
                })),
                ClientInbound::Notification(notification) => {
                    inbound_spawn(Box::pin(async move {
                        let _ = dispatch_client_notification(client.as_ref(), notification).await;
                    }));
                }
            }
        }
    }));

    let (outgoing_tx, outgoing_rx) = tokio::sync::mpsc::unbounded_channel();
    let connection = AcpGatewaySender::<acp::ClientSide>::new(outgoing_tx);
    let transport = agent_client_protocol::ByteStreams::new(outgoing, incoming);
    let request_sender = incoming_tx.clone();
    let notification_sender = incoming_tx;

    let future = agent_client_protocol::Client
        .builder()
        .name("grow-client-v1")
        .on_receive_request(
            move |request: acp::AgentRequest,
                  responder: agent_client_protocol::Responder<serde_json::Value>,
                  _cx: acp::ConnectionTo<agent_client_protocol::Agent>| {
                let request_sender = request_sender.clone();
                async move {
                    let cancellation = responder.cancellation();
                    request_sender
                        .send(ClientInbound::Request {
                            request,
                            responder,
                            cancellation,
                        })
                        .map_err(|_| acp::Error::internal_error().data("client handler closed"))?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            move |notification: acp::AgentNotification, _cx| {
                let notification_sender = notification_sender.clone();
                async move {
                    notification_sender
                        .send(ClientInbound::Notification(notification))
                        .map_err(|_| acp::Error::internal_error().data("client handler closed"))
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_with(transport, async move |cx| {
            let gateway = AcpGatewayReceiver::new(outgoing_rx, ClientPeer { cx: cx.clone() });
            let mut gateway = Box::pin(gateway.run()).fuse();
            let mut closed = Box::pin(cx.incoming_closed()).fuse();
            futures::select! {
                () = gateway => Ok(()),
                () = closed => Ok(()),
            }
        });

    (connection, future)
}

fn json_response<T: serde::Serialize>(response: T) -> acp::Result<serde_json::Value> {
    serde_json::to_value(response)
        .map_err(|error| acp::Error::internal_error().data(error.to_string()))
}

fn wire_ext_request(mut request: acp::ExtRequest) -> acp::ExtRequest {
    request.method = format!("_{}", request.method).into();
    request
}

fn wire_ext_notification(mut notification: acp::ExtNotification) -> acp::ExtNotification {
    notification.method = format!("_{}", notification.method).into();
    notification
}

async fn dispatch_agent_request<A: AcpAgentHandler + ?Sized>(
    agent: &A,
    request: acp::ClientRequest,
    cancellation: acp::RequestCancellation,
) -> acp::Result<serde_json::Value> {
    let prompt_session_id = match &request {
        acp::ClientRequest::PromptRequest(request) => Some(request.session_id.clone()),
        _ => None,
    };
    let result = cancellation
        .run_until_cancelled(dispatch_agent_request_uncancelled(agent, request))
        .await;
    if cancellation.is_cancelled()
        && let Some(session_id) = prompt_session_id
    {
        let _ = agent.cancel(acp::CancelNotification::new(session_id)).await;
    }
    result
}

async fn dispatch_agent_request_uncancelled<A: AcpAgentHandler + ?Sized>(
    agent: &A,
    request: acp::ClientRequest,
) -> acp::Result<serde_json::Value> {
    match request {
        acp::ClientRequest::InitializeRequest(request) => {
            json_response(agent.initialize(request).await?)
        }
        acp::ClientRequest::AuthenticateRequest(request) => {
            json_response(agent.authenticate(request).await?)
        }
        acp::ClientRequest::NewSessionRequest(request) => {
            json_response(agent.new_session(request).await?)
        }
        acp::ClientRequest::LoadSessionRequest(request) => {
            json_response(agent.load_session(request).await?)
        }
        acp::ClientRequest::SetSessionModeRequest(request) => {
            json_response(agent.set_session_mode(request).await?)
        }
        acp::ClientRequest::SetSessionConfigOptionRequest(request) => {
            json_response(agent.set_session_config_option(request).await?)
        }
        acp::ClientRequest::PromptRequest(request) => json_response(agent.prompt(request).await?),
        acp::ClientRequest::ListSessionsRequest(request) => {
            json_response(agent.list_sessions(request).await?)
        }
        acp::ClientRequest::ExtMethodRequest(request) => {
            json_response(agent.ext_method(request).await?)
        }
        _ => Err(acp::Error::method_not_found()),
    }
}

async fn dispatch_agent_notification<A: AcpAgentHandler + ?Sized>(
    agent: &A,
    notification: acp::ClientNotification,
) -> acp::Result<()> {
    match notification {
        acp::ClientNotification::CancelNotification(notification) => {
            agent.cancel(notification).await
        }
        acp::ClientNotification::ExtNotification(notification) => {
            agent.ext_notification(notification).await
        }
        _ => Ok(()),
    }
}

async fn dispatch_client_request<C: AcpClientHandler + ?Sized>(
    client: &C,
    request: acp::AgentRequest,
    cancellation: acp::RequestCancellation,
) -> acp::Result<serde_json::Value> {
    cancellation
        .run_until_cancelled(dispatch_client_request_uncancelled(client, request))
        .await
}

async fn dispatch_client_request_uncancelled<C: AcpClientHandler + ?Sized>(
    client: &C,
    request: acp::AgentRequest,
) -> acp::Result<serde_json::Value> {
    match request {
        acp::AgentRequest::RequestPermissionRequest(request) => {
            json_response(client.request_permission(request).await?)
        }
        acp::AgentRequest::ReadTextFileRequest(request) => {
            json_response(client.read_text_file(request).await?)
        }
        acp::AgentRequest::WriteTextFileRequest(request) => {
            json_response(client.write_text_file(request).await?)
        }
        acp::AgentRequest::CreateTerminalRequest(request) => {
            json_response(client.create_terminal(request).await?)
        }
        acp::AgentRequest::TerminalOutputRequest(request) => {
            json_response(client.terminal_output(request).await?)
        }
        acp::AgentRequest::ReleaseTerminalRequest(request) => {
            json_response(client.release_terminal(request).await?)
        }
        acp::AgentRequest::WaitForTerminalExitRequest(request) => {
            json_response(client.wait_for_terminal_exit(request).await?)
        }
        acp::AgentRequest::KillTerminalRequest(request) => {
            json_response(client.kill_terminal(request).await?)
        }
        acp::AgentRequest::ExtMethodRequest(request) => {
            json_response(client.ext_method(request).await?)
        }
        _ => Err(acp::Error::method_not_found()),
    }
}

async fn dispatch_client_notification<C: AcpClientHandler + ?Sized>(
    client: &C,
    notification: acp::AgentNotification,
) -> acp::Result<()> {
    match notification {
        acp::AgentNotification::SessionNotification(notification) => {
            client.session_notification(notification).await
        }
        acp::AgentNotification::ExtNotification(notification) => {
            client.ext_notification(notification).await
        }
        _ => Ok(()),
    }
}

#[derive(Clone)]
struct AgentPeer {
    cx: acp::ConnectionTo<agent_client_protocol::Client>,
}

#[async_trait::async_trait(?Send)]
impl AcpClientHandler for AgentPeer {
    async fn request_permission(
        &self,
        request: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        self.cx.send_request(request).block_task().await
    }
    async fn session_notification(
        &self,
        notification: acp::SessionNotification,
    ) -> acp::Result<()> {
        self.cx.send_notification(notification)
    }
    async fn write_text_file(
        &self,
        request: acp::WriteTextFileRequest,
    ) -> acp::Result<acp::WriteTextFileResponse> {
        self.cx.send_request(request).block_task().await
    }
    async fn read_text_file(
        &self,
        request: acp::ReadTextFileRequest,
    ) -> acp::Result<acp::ReadTextFileResponse> {
        self.cx.send_request(request).block_task().await
    }
    async fn create_terminal(
        &self,
        request: acp::CreateTerminalRequest,
    ) -> acp::Result<acp::CreateTerminalResponse> {
        self.cx.send_request(request).block_task().await
    }
    async fn terminal_output(
        &self,
        request: acp::TerminalOutputRequest,
    ) -> acp::Result<acp::TerminalOutputResponse> {
        self.cx.send_request(request).block_task().await
    }
    async fn release_terminal(
        &self,
        request: acp::ReleaseTerminalRequest,
    ) -> acp::Result<acp::ReleaseTerminalResponse> {
        self.cx.send_request(request).block_task().await
    }
    async fn wait_for_terminal_exit(
        &self,
        request: acp::WaitForTerminalExitRequest,
    ) -> acp::Result<acp::WaitForTerminalExitResponse> {
        self.cx.send_request(request).block_task().await
    }
    async fn kill_terminal(
        &self,
        request: acp::KillTerminalRequest,
    ) -> acp::Result<acp::KillTerminalResponse> {
        self.cx.send_request(request).block_task().await
    }
    async fn ext_method(&self, request: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        let value = self
            .cx
            .send_request(acp::AgentRequest::ExtMethodRequest(wire_ext_request(
                request,
            )))
            .block_task()
            .await?;
        serde_json::from_value(value)
            .map_err(|error| acp::Error::internal_error().data(error.to_string()))
    }
    async fn ext_notification(&self, notification: acp::ExtNotification) -> acp::Result<()> {
        self.cx
            .send_notification(acp::AgentNotification::ExtNotification(
                wire_ext_notification(notification),
            ))
    }
}

#[derive(Clone)]
struct ClientPeer {
    cx: acp::ConnectionTo<agent_client_protocol::Agent>,
}

#[async_trait::async_trait(?Send)]
impl AcpAgentHandler for ClientPeer {
    async fn initialize(
        &self,
        request: acp::InitializeRequest,
    ) -> acp::Result<acp::InitializeResponse> {
        self.cx.send_request(request).block_task().await
    }
    async fn authenticate(
        &self,
        request: acp::AuthenticateRequest,
    ) -> acp::Result<acp::AuthenticateResponse> {
        self.cx.send_request(request).block_task().await
    }
    async fn new_session(
        &self,
        request: acp::NewSessionRequest,
    ) -> acp::Result<acp::NewSessionResponse> {
        self.cx.send_request(request).block_task().await
    }
    async fn prompt(&self, request: acp::PromptRequest) -> acp::Result<acp::PromptResponse> {
        self.cx.send_request(request).block_task().await
    }
    async fn cancel(&self, notification: acp::CancelNotification) -> acp::Result<()> {
        self.cx.send_notification(notification)
    }
    async fn load_session(
        &self,
        request: acp::LoadSessionRequest,
    ) -> acp::Result<acp::LoadSessionResponse> {
        self.cx.send_request(request).block_task().await
    }
    async fn set_session_mode(
        &self,
        request: acp::SetSessionModeRequest,
    ) -> acp::Result<acp::SetSessionModeResponse> {
        self.cx.send_request(request).block_task().await
    }
    async fn set_session_config_option(
        &self,
        request: acp::SetSessionConfigOptionRequest,
    ) -> acp::Result<acp::SetSessionConfigOptionResponse> {
        self.cx.send_request(request).block_task().await
    }
    async fn list_sessions(
        &self,
        request: acp::ListSessionsRequest,
    ) -> acp::Result<acp::ListSessionsResponse> {
        self.cx.send_request(request).block_task().await
    }
    async fn ext_method(&self, request: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        let value = self
            .cx
            .send_request(acp::ClientRequest::ExtMethodRequest(wire_ext_request(
                request,
            )))
            .block_task()
            .await?;
        serde_json::from_value(value)
            .map_err(|error| acp::Error::internal_error().data(error.to_string()))
    }
    async fn ext_notification(&self, notification: acp::ExtNotification) -> acp::Result<()> {
        self.cx
            .send_notification(acp::ClientNotification::ExtNotification(
                wire_ext_notification(notification),
            ))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    use futures::future;
    use serde_json::json;
    use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader, DuplexStream};
    use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};

    use super::*;
    use crate::LineBufferedRead;

    struct DropMarker(Rc<Cell<bool>>);

    impl Drop for DropMarker {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    struct TestAgent {
        prompt_started: RefCell<Option<tokio::sync::oneshot::Sender<()>>>,
        prompt_dropped: Rc<Cell<bool>>,
        cancel_count: Rc<Cell<usize>>,
        ext_methods: Rc<RefCell<Vec<String>>>,
    }

    impl TestAgent {
        fn new() -> Self {
            Self {
                prompt_started: RefCell::new(None),
                prompt_dropped: Rc::new(Cell::new(false)),
                cancel_count: Rc::new(Cell::new(0)),
                ext_methods: Rc::new(RefCell::new(Vec::new())),
            }
        }
    }

    #[async_trait::async_trait(?Send)]
    impl AcpAgentHandler for TestAgent {
        async fn initialize(
            &self,
            request: acp::InitializeRequest,
        ) -> acp::Result<acp::InitializeResponse> {
            Ok(acp::InitializeResponse::new(request.protocol_version))
        }

        async fn authenticate(
            &self,
            _request: acp::AuthenticateRequest,
        ) -> acp::Result<acp::AuthenticateResponse> {
            Ok(acp::AuthenticateResponse::new())
        }

        async fn new_session(
            &self,
            _request: acp::NewSessionRequest,
        ) -> acp::Result<acp::NewSessionResponse> {
            Ok(acp::NewSessionResponse::new("test-session"))
        }

        async fn prompt(&self, _request: acp::PromptRequest) -> acp::Result<acp::PromptResponse> {
            let _drop_marker = DropMarker(self.prompt_dropped.clone());
            if let Some(started) = self.prompt_started.borrow_mut().take() {
                let _ = started.send(());
            }
            future::pending().await
        }

        async fn cancel(&self, _request: acp::CancelNotification) -> acp::Result<()> {
            self.cancel_count.set(self.cancel_count.get() + 1);
            Ok(())
        }

        async fn list_sessions(
            &self,
            _request: acp::ListSessionsRequest,
        ) -> acp::Result<acp::ListSessionsResponse> {
            Ok(acp::ListSessionsResponse::new(Vec::new()))
        }

        async fn ext_method(&self, request: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
            self.ext_methods
                .borrow_mut()
                .push(request.method.to_string());
            Ok(acp::ExtResponse::new(
                serde_json::value::to_raw_value(&json!({ "ok": true }))?.into(),
            ))
        }
    }

    struct TestClient;

    #[async_trait::async_trait(?Send)]
    impl AcpClientHandler for TestClient {
        async fn request_permission(
            &self,
            _request: acp::RequestPermissionRequest,
        ) -> acp::Result<acp::RequestPermissionResponse> {
            Err(acp::Error::method_not_found())
        }

        async fn session_notification(
            &self,
            _notification: acp::SessionNotification,
        ) -> acp::Result<()> {
            Ok(())
        }
    }

    fn start_agent(
        agent: TestAgent,
    ) -> (AgentSideConnection, DuplexStream, BufReader<DuplexStream>) {
        let (peer_writer, sdk_reader) = tokio::io::duplex(64 * 1024);
        let (sdk_writer, peer_reader) = tokio::io::duplex(64 * 1024);
        let incoming = LineBufferedRead::spawn_local(sdk_reader.compat());
        let (connection, io) =
            connect_agent_v1(agent, sdk_writer.compat_write(), incoming, |future| {
                tokio::task::spawn_local(future);
            });
        tokio::task::spawn_local(async move {
            io.await.expect("agent connection remains open");
        });
        (connection, peer_writer, BufReader::new(peer_reader))
    }

    fn start_client() -> (ClientSideConnection, DuplexStream, BufReader<DuplexStream>) {
        let (peer_writer, sdk_reader) = tokio::io::duplex(64 * 1024);
        let (sdk_writer, peer_reader) = tokio::io::duplex(64 * 1024);
        let incoming = LineBufferedRead::spawn_local(sdk_reader.compat());
        let (connection, io) =
            connect_client_v1(TestClient, sdk_writer.compat_write(), incoming, |future| {
                tokio::task::spawn_local(future);
            });
        tokio::task::spawn_local(async move {
            io.await.expect("client connection remains open");
        });
        (connection, peer_writer, BufReader::new(peer_reader))
    }

    async fn write_json(writer: &mut DuplexStream, value: &serde_json::Value) {
        writer
            .write_all(format!("{value}\n").as_bytes())
            .await
            .expect("write ACP frame");
        writer.flush().await.expect("flush ACP frame");
    }

    async fn write_json_fragmented(writer: &mut DuplexStream, value: &serde_json::Value) {
        let frame = format!("{value}\n");
        for chunk in frame.as_bytes().chunks(3) {
            writer.write_all(chunk).await.expect("write ACP fragment");
            tokio::task::yield_now().await;
        }
        writer.flush().await.expect("flush ACP frame");
    }

    async fn read_json(reader: &mut BufReader<DuplexStream>) -> serde_json::Value {
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            reader.read_line(&mut line),
        )
        .await
        .expect("timed out reading ACP frame")
        .expect("read ACP frame");
        serde_json::from_str(&line).expect("valid JSON-RPC frame")
    }

    async fn initialize_peer(writer: &mut DuplexStream, reader: &mut BufReader<DuplexStream>) {
        let request = acp::InitializeRequest::new(acp::ProtocolVersion::V1);
        write_json_fragmented(
            writer,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": serde_json::to_value(request).expect("serialize initialize"),
            }),
        )
        .await;
        let response = read_json(reader).await;
        assert_eq!(
            response,
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": serde_json::to_value(acp::InitializeResponse::new(
                    acp::ProtocolVersion::V1
                ))
                .expect("serialize initialize response"),
            })
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stable_v1_initialize_batch_and_unknown_notification() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let agent = TestAgent::new();
                let cancel_count = agent.cancel_count.clone();
                let (_connection, mut writer, mut reader) = start_agent(agent);
                initialize_peer(&mut writer, &mut reader).await;

                write_json(
                    &mut writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "method": "unknown/notification",
                        "params": { "ignored": true },
                    }),
                )
                .await;
                write_json(
                    &mut writer,
                    &json!([
                        {
                            "jsonrpc": "2.0",
                            "method": "session/cancel",
                            "params": { "sessionId": "batch-session" },
                        },
                        {
                            "jsonrpc": "2.0",
                            "id": 2,
                            "method": "session/list",
                            "params": {},
                        },
                    ]),
                )
                .await;

                let response = read_json(&mut reader).await;
                assert_eq!(
                    response,
                    json!([{
                        "jsonrpc": "2.0",
                        "id": 2,
                        "result": { "sessions": [] },
                    }])
                );
                for _ in 0..10 {
                    if cancel_count.get() == 1 {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
                assert_eq!(cancel_count.get(), 1);
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn extension_methods_use_prefixed_wire_names_and_logical_handler_names() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let agent = TestAgent::new();
                let ext_methods = agent.ext_methods.clone();
                let (_connection, mut writer, mut reader) = start_agent(agent);
                initialize_peer(&mut writer, &mut reader).await;

                write_json(
                    &mut writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": 2,
                        "method": "_grow/coordination/list",
                        "params": { "sourceSessionId": "source" },
                    }),
                )
                .await;
                let response = read_json(&mut reader).await;
                assert_eq!(response["result"], json!({ "ok": true }));
                assert_eq!(ext_methods.borrow().as_slice(), ["grow/coordination/list"]);

                let (client, mut agent_writer, mut agent_reader) = start_client();
                let initialize = async {
                    client
                        .initialize(acp::InitializeRequest::new(acp::ProtocolVersion::V1))
                        .await
                        .expect("client initialize")
                };
                let serve_initialize = async {
                    let request = read_json(&mut agent_reader).await;
                    assert_eq!(request["method"], "initialize");
                    write_json(
                        &mut agent_writer,
                        &json!({
                            "jsonrpc": "2.0",
                            "id": request["id"].clone(),
                            "result": serde_json::to_value(acp::InitializeResponse::new(
                                acp::ProtocolVersion::V1
                            ))
                            .expect("serialize initialize response"),
                        }),
                    )
                    .await;
                };
                let _ = future::join(initialize, serve_initialize).await;

                let extension = async {
                    client
                        .ext_method(acp::ExtRequest::new(
                            "grow/coordination/list",
                            serde_json::value::to_raw_value(&json!({})).unwrap().into(),
                        ))
                        .await
                        .expect("extension response")
                };
                let serve_extension = async {
                    let request = read_json(&mut agent_reader).await;
                    assert_eq!(request["method"], "_grow/coordination/list");
                    write_json(
                        &mut agent_writer,
                        &json!({
                            "jsonrpc": "2.0",
                            "id": request["id"].clone(),
                            "result": { "ok": true },
                        }),
                    )
                    .await;
                };
                let (response, ()) = future::join(extension, serve_extension).await;
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(response.0.get()).unwrap(),
                    json!({ "ok": true })
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn online_request_cancellation_reaches_prompt_handler() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let mut agent = TestAgent::new();
                let prompt_dropped = agent.prompt_dropped.clone();
                let cancel_count = agent.cancel_count.clone();
                let (started_tx, started_rx) = tokio::sync::oneshot::channel();
                agent.prompt_started = RefCell::new(Some(started_tx));
                let (_connection, mut writer, mut reader) = start_agent(agent);
                initialize_peer(&mut writer, &mut reader).await;

                write_json(
                    &mut writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": 7,
                        "method": "session/prompt",
                        "params": { "sessionId": "cancel-me", "prompt": [] },
                    }),
                )
                .await;
                started_rx.await.expect("prompt handler started");
                write_json(
                    &mut writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "method": "$/cancel_request",
                        "params": { "requestId": 7 },
                    }),
                )
                .await;

                let response = read_json(&mut reader).await;
                assert_eq!(response["id"], 7);
                assert_eq!(response["error"]["code"], -32800);
                assert_eq!(response["error"]["message"], "Request cancelled");
                assert!(
                    prompt_dropped.get(),
                    "cancelled handler future must be dropped"
                );
                assert_eq!(cancel_count.get(), 1, "session/cancel must be forwarded");
            })
            .await;
    }
}
