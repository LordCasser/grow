//! `NotificationSender` — transport layer for session notifications.
//!
//! Owns the gateway handle, gateway-enabled gate, and persistence
//! channel needed to emit notifications.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tokio::sync::mpsc;

use acp_transport::AcpAgentGatewaySender as GatewaySender;

use crate::session::persistence::PersistenceMsg;
use crate::session::storage::SessionUpdate;

/// Transport layer for delivering session notifications to the client
/// and persistence layer.
pub struct NotificationSender {
    /// Gateway handle for forwarding notifications to the client.
    pub gateway: GatewaySender,
    /// When false, notifications are persisted but NOT forwarded to the
    /// client. Opened by `MvpAgent::load_session` when the client
    /// explicitly loads the session.
    pub gateway_enabled: Arc<AtomicBool>,
    /// Persistence channel for writing updates to disk.
    pub persistence_tx: mpsc::UnboundedSender<PersistenceMsg>,
}

impl NotificationSender {
    /// Append one update after all previously queued persistence work and wait
    /// for the storage actor's durable result.
    pub async fn append_update_durably(
        &self,
        update: SessionUpdate,
    ) -> Result<(), crate::session::persistence::DurableAppendError> {
        use crate::session::persistence::DurableAppendError;
        let (respond_to, response) = tokio::sync::oneshot::channel();
        self.persistence_tx
            .send(PersistenceMsg::AppendUpdateDurablyAndAck { update, respond_to })
            .map_err(|_| {
                DurableAppendError::NotCommitted(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "session persistence actor stopped before durable notification append",
                ))
            })?;
        response
            .await
            .map_err(|_| {
                DurableAppendError::AcknowledgementLost(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "session persistence actor dropped durable notification acknowledgement",
                ))
            })?
            .map_err(Into::into)
    }
}
