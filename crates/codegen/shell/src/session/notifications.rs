//! `NotificationSender` — transport layer for session notifications.
//!
//! Owns the gateway handle, gateway-enabled gate, and persistence
//! channel needed to emit notifications.

use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use serde::Serialize;
use tokio::sync::mpsc;

use acp_transport::AcpAgentGatewaySender as GatewaySender;

use crate::extensions::notification::SessionNotification as GrowSessionNotification;
use crate::session::persistence::PersistenceMsg;
use crate::session::storage::SessionUpdate;

/// Transport layer for delivering session notifications to the client
/// and persistence layer.
#[derive(Clone)]
pub struct NotificationSender {
    /// Gateway handle for forwarding notifications to the client.
    pub gateway: GatewaySender,
    /// When false, notifications are persisted but NOT forwarded to the
    /// client. Opened by `MvpAgent::load_session` when the client
    /// explicitly loads the session.
    pub gateway_enabled: Arc<AtomicBool>,
    /// Persistence channel for writing updates to disk.
    pub persistence_tx: mpsc::UnboundedSender<PersistenceMsg>,
    /// Credits for live preview notifications awaiting gateway completion.
    pub preview_gateway_budget: sampler::PreviewEventBudget,
}

/// Already-budgeted transport copy. Only the persistence owner publishes it,
/// after the unique presentation fact crossed its durable barrier.
pub struct CommittedGrowDelivery {
    pub(crate) gateway: GatewaySender,
    pub(crate) method: &'static str,
    pub(crate) raw: Box<serde_json::value::RawValue>,
    pub(crate) permit: tokio::sync::OwnedSemaphorePermit,
}

impl std::fmt::Debug for CommittedGrowDelivery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommittedGrowDelivery")
            .field("method", &self.method)
            .finish_non_exhaustive()
    }
}

impl CommittedGrowDelivery {
    pub(crate) fn publish(self) {
        let outbound =
            agent_client_protocol::schema::v1::ExtNotification::new(self.method, self.raw.into());
        let completed = self.gateway.forward_with_completion(outbound);
        tokio::spawn(async move {
            let _ = completed.await;
            drop(self.permit);
        });
    }
}

struct PreviewByteCounter(usize);

impl Write for PreviewByteCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.checked_add(bytes.len()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "preview notification size overflow",
            )
        })?;
        if self.0 > sampler::PreviewEventBudget::MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "preview notification exceeds the byte budget",
            ));
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl NotificationSender {
    pub(crate) fn reserve_preview_delivery(
        &self,
        notification: &impl Serialize,
    ) -> io::Result<tokio::sync::OwnedSemaphorePermit> {
        let mut bytes = PreviewByteCounter(0);
        serde_json::to_writer(&mut bytes, notification).map_err(io::Error::other)?;
        self.preview_gateway_budget.try_acquire_bytes(bytes.0)
    }
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

    /// Append one immutable sideband fact and wait for its fsync boundary.
    pub async fn append_sideband_event_durably(
        &self,
        event: chat_state::SidebandEvent,
    ) -> Result<(), crate::session::persistence::DurableAppendError> {
        use crate::session::persistence::DurableAppendError;
        let (respond_to, response) = tokio::sync::oneshot::channel();
        self.persistence_tx
            .send(PersistenceMsg::SidebandDurablyAndAck { event, respond_to })
            .map_err(|_| {
                DurableAppendError::NotCommitted(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "session persistence actor stopped before sideband append",
                ))
            })?;
        response
            .await
            .map_err(|_| {
                DurableAppendError::AcknowledgementLost(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "session persistence actor dropped sideband acknowledgement",
                ))
            })?
            .map_err(DurableAppendError::NotCommitted)
    }
}

/// Bound both copies of a state snapshot before either unbounded transport
/// channel can retain it. Goal and Workflow state have separate durable
/// authorities; these Grow records are presentation projections.
pub(crate) fn dispatch_auxiliary_grow(
    gateway: &GatewaySender,
    persistence_tx: &mpsc::UnboundedSender<PersistenceMsg>,
    budget: &sampler::PreviewEventBudget,
    preview: &parking_lot::Mutex<Option<(String, u32, bool)>>,
    method: &'static str,
    notification: GrowSessionNotification,
    persist: bool,
) {
    let Some(prepared) =
        prepare_auxiliary_grow(persistence_tx, budget, preview, notification, persist)
    else {
        return;
    };
    publish_prepared_auxiliary_grow(gateway, persistence_tx, method, prepared);
}

pub(crate) struct PreparedAuxiliaryGrow {
    notification: GrowSessionNotification,
    raw: Box<serde_json::value::RawValue>,
    owner: Option<(String, u32)>,
    persistence_permit: Option<tokio::sync::OwnedSemaphorePermit>,
    gateway_permit: tokio::sync::OwnedSemaphorePermit,
}

/// Reserve serialization and both bounded downstream copies before a
/// projection expectation is committed. A failed reservation must not leave
/// behind an expectation for a row that could never be published.
pub(crate) fn prepare_auxiliary_grow(
    persistence_tx: &mpsc::UnboundedSender<PersistenceMsg>,
    budget: &sampler::PreviewEventBudget,
    preview: &parking_lot::Mutex<Option<(String, u32, bool)>>,
    notification: GrowSessionNotification,
    persist: bool,
) -> Option<PreparedAuxiliaryGrow> {
    let raw = match serde_json::value::to_raw_value(&notification) {
        Ok(raw) => raw,
        Err(error) => {
            let owner = preview
                .lock()
                .as_ref()
                .map(|(request_id, attempt, _)| (request_id.clone(), *attempt));
            report_auxiliary_failure(persistence_tx, owner.as_ref(), error.to_string());
            return None;
        }
    };
    let bytes = raw.get().len();
    let preview_guard = preview.lock();
    let owner = preview_guard
        .as_ref()
        .map(|(request_id, attempt, _)| (request_id.clone(), *attempt));
    let persistence_permit = if persist {
        match budget.try_acquire_bytes(bytes) {
            Ok(permit) => Some(permit),
            Err(error) => {
                report_auxiliary_failure(persistence_tx, owner.as_ref(), error.to_string());
                return None;
            }
        }
    } else {
        None
    };
    let gateway_permit = match budget.try_acquire_bytes(bytes) {
        Ok(permit) => permit,
        Err(error) => {
            report_auxiliary_failure(persistence_tx, owner.as_ref(), error.to_string());
            return None;
        }
    };
    drop(preview_guard);
    Some(PreparedAuxiliaryGrow {
        notification,
        raw,
        owner,
        persistence_permit,
        gateway_permit,
    })
}

pub(crate) fn publish_prepared_auxiliary_grow(
    gateway: &GatewaySender,
    persistence_tx: &mpsc::UnboundedSender<PersistenceMsg>,
    method: &'static str,
    prepared: PreparedAuxiliaryGrow,
) {
    let PreparedAuxiliaryGrow {
        notification,
        raw,
        owner,
        persistence_permit,
        gateway_permit,
    } = prepared;
    if let Some(permit) = persistence_permit {
        if persistence_tx
            .send(PersistenceMsg::AuxiliaryGrow {
                notification,
                permit,
                preview: owner,
                respond_to: None,
                delivery: Some(CommittedGrowDelivery {
                    gateway: gateway.clone(),
                    method,
                    raw,
                    permit: gateway_permit,
                }),
            })
            .is_err()
        {
            tracing::warn!("session persistence actor stopped before auxiliary Grow append");
        }
        return;
    }
    let outbound = agent_client_protocol::schema::v1::ExtNotification::new(method, raw.into());
    let completed = gateway.forward_with_completion(outbound);
    tokio::spawn(async move {
        let _ = completed.await;
        drop(gateway_permit);
    });
}

fn report_auxiliary_failure(
    persistence_tx: &mpsc::UnboundedSender<PersistenceMsg>,
    preview: Option<&(String, u32)>,
    reason: String,
) {
    tracing::warn!(%reason, "auxiliary Grow notification was not delivered");
    if let Some((request_id, attempt)) = preview {
        let _ = persistence_tx.send(PersistenceMsg::AuxiliaryPreviewFailure {
            request_id: request_id.clone(),
            attempt: *attempt,
            reason,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_preview_notification_is_rejected_without_buffering() {
        let mut counter = PreviewByteCounter(sampler::PreviewEventBudget::MAX_BYTES);
        let error = counter.write_all(b"x").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn auxiliary_grow_holds_both_queue_credits_until_consumed() {
        use std::time::Duration;

        const WAIT: Duration = Duration::from_secs(1);

        let (gateway_tx, mut gateway_rx) = mpsc::unbounded_channel();
        let (persistence_tx, mut persistence_rx) = mpsc::unbounded_channel();
        let gateway = GatewaySender::new(gateway_tx);
        let budget = sampler::PreviewEventBudget::default();
        let preview = parking_lot::Mutex::new(Some(("request".into(), 1, false)));
        let held = tokio::time::timeout(WAIT, budget.acquire(4094))
            .await
            .expect("budget reservation should complete")
            .unwrap();
        let notification = || GrowSessionNotification {
            session_id: agent_client_protocol::schema::v1::SessionId::new("budget-test"),
            update: crate::extensions::notification::SessionUpdate::MemoryFlushStarted,
            meta: None,
        };
        dispatch_auxiliary_grow(
            &gateway,
            &persistence_tx,
            &budget,
            &preview,
            "grow/session_notification",
            notification(),
            true,
        );
        dispatch_auxiliary_grow(
            &gateway,
            &persistence_tx,
            &budget,
            &preview,
            "grow/session_notification",
            notification(),
            true,
        );

        let persisted = tokio::time::timeout(WAIT, persistence_rx.recv())
            .await
            .expect("writer should receive the first AuxiliaryGrow")
            .unwrap();
        let (persistence_permit, delivery) = match persisted {
            PersistenceMsg::AuxiliaryGrow {
                notification,
                permit,
                delivery: Some(delivery),
                respond_to: None,
                ..
            } => {
                assert!(matches!(
                    notification.update,
                    crate::extensions::notification::SessionUpdate::MemoryFlushStarted
                ));
                (permit, delivery)
            }
            other => {
                panic!("expected budgeted AuxiliaryGrow with deferred delivery, got {other:?}")
            }
        };
        assert!(matches!(
            tokio::time::timeout(WAIT, persistence_rx.recv())
                .await
                .expect("failed reservation should be reported to the writer")
                .unwrap(),
            PersistenceMsg::AuxiliaryPreviewFailure { .. }
        ));
        assert!(budget.try_acquire_bytes(1).is_err());

        // The persistence owner publishes only after the typed presentation
        // fact has been consumed and its durable append succeeds.
        assert!(
            gateway_rx.try_recv().is_err(),
            "uncommitted snapshots stay private"
        );
        drop(persistence_permit);
        delivery.publish();
        let delivered = tokio::time::timeout(WAIT, gateway_rx.recv())
            .await
            .expect("committed snapshot should reach the gateway")
            .unwrap();
        assert!(gateway_rx.try_recv().is_err());
        assert!(
            budget.try_acquire_bytes(16 * 1024 + 1).is_err(),
            "the freed persistence credit alone cannot cover both queue copies"
        );
        drop(delivered);
        let released = tokio::time::timeout(WAIT, budget.acquire(2))
            .await
            .expect("gateway completion should release its credit")
            .unwrap();
        drop(released);
        assert!(budget.try_acquire_bytes(32 * 1024).is_ok());
        drop(held);
        assert!(budget.try_acquire_bytes(16 * 1024).is_ok());
    }
}
