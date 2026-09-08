//! Unified log forwarding for the pager.
//!
//! Buffers log entries in memory and flushes them to the shell via
//! `grow/log` ACP notifications. Call [`init`] once at startup with
//! the ACP sender, then use [`info`], [`warn`], [`error`], [`debug`]
//! from anywhere.

use std::sync::{Mutex, OnceLock};

use acp_transport::AcpAgentTx;
use acp_transport::protocol as acp;
use diagnostics::unified_log::{
    ClientLogEntry, LOG_METHOD, LogLevel, LogNotificationParams, LogSource,
};
use tokio::runtime::Handle;

struct DispatchOwner {
    tx: AcpAgentTx,
    runtime: Handle,
}

struct Forwarder {
    owner: OnceLock<DispatchOwner>,
    buffer: Mutex<Vec<ClientLogEntry>>,
}

static FORWARDER: Forwarder = Forwarder::new();
const FLUSH_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Initialize the unified log forwarder with the ACP sender.
///
/// Must be called once after the ACP connection is established.
/// Spawns a background task that flushes buffered entries every few
/// seconds so events are delivered promptly without manual flush calls.
/// Entries buffered before this call will be picked up on the first tick.
pub fn init(tx: AcpAgentTx) {
    if !FORWARDER.initialize(tx) {
        return;
    }
    tokio::spawn(async {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            flush();
        }
    });
}

fn now_ts() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn push_entry(lvl: LogLevel, msg: &str, sid: Option<&str>, ctx: Option<serde_json::Value>) {
    let entry = ClientLogEntry {
        ts: now_ts(),
        pid: std::process::id(),
        ver: version::VERSION.to_owned(),
        lvl,
        sid: sid.map(Into::into),
        msg: msg.into(),
        ctx,
    };
    FORWARDER.push(entry);
}

fn build_notification(entries: Vec<ClientLogEntry>) -> Option<acp::ExtNotification> {
    if entries.is_empty() {
        return None;
    }
    let params = LogNotificationParams {
        src: LogSource::GrowPager,
        entries,
    };
    let raw = serde_json::value::to_raw_value(&params).ok()?;
    Some(acp::ExtNotification::new(LOG_METHOD, raw.into()))
}

impl Forwarder {
    const fn new() -> Self {
        Self { owner: OnceLock::new(), buffer: Mutex::new(Vec::new()) }
    }

    /// Only the successful installer may start the periodic consumer.
    fn initialize(&self, tx: AcpAgentTx) -> bool {
        if self.owner.get().is_some() {
            return false;
        }
        self.owner.set(DispatchOwner { tx, runtime: Handle::current() }).is_ok()
    }

    fn push(&self, entry: ClientLogEntry) {
        let Ok(mut buffer) = self.buffer.lock() else { return; };
        buffer.push(entry);
        if buffer.len() >= 16 && let Some(owner) = self.owner.get() {
            let entries = buffer.drain(..).collect();
            drop(buffer);
            Self::send_entries(owner, entries);
        }
    }

    fn take_entries(&self) -> Option<(&DispatchOwner, Vec<ClientLogEntry>)> {
        // No initialized owner means startup entries must stay buffered.
        let owner = self.owner.get()?;
        let mut buffer = self.buffer.lock().ok()?;
        if buffer.is_empty() { return None; }
        Some((owner, buffer.drain(..).collect()))
    }

    fn send_entries(owner: &DispatchOwner, entries: Vec<ClientLogEntry>) {
        let Some(notification) = build_notification(entries) else { return; };
        let tx = owner.tx.clone();
        owner.runtime.spawn(async move {
            let _ = acp_transport::acp_send(notification, &tx).await;
        });
    }

    fn flush(&self) {
        if let Some((owner, entries)) = self.take_entries() {
            Self::send_entries(owner, entries);
        }
    }

    async fn flush_blocking(&self) {
        let Some((owner, entries)) = self.take_entries() else { return; };
        let Some(notification) = build_notification(entries) else { return; };
        // Bound shutdown latency. An enqueued notification may still be
        // processed after this local wait expires; never retry it here.
        let _ = tokio::time::timeout(
            FLUSH_WAIT,
            acp_transport::acp_send(notification, &owner.tx),
        ).await;
    }
}

/// Flush any buffered entries to the shell (fire-and-forget).
pub fn flush() {
    FORWARDER.flush();
}

/// Flush the currently buffered batch, waiting at most two seconds for delivery.
/// Earlier fire-and-forget batches are not joined by this operation.
pub async fn flush_blocking() {
    FORWARDER.flush_blocking().await;
}

/// Log an info-level entry.
pub fn info(msg: &str, sid: Option<&str>, ctx: Option<serde_json::Value>) {
    push_entry(LogLevel::Info, msg, sid, ctx);
}

/// Log a warn-level entry.
pub fn warn(msg: &str, sid: Option<&str>, ctx: Option<serde_json::Value>) {
    push_entry(LogLevel::Warn, msg, sid, ctx);
}

/// Log an error-level entry.
pub fn error(msg: &str, sid: Option<&str>, ctx: Option<serde_json::Value>) {
    push_entry(LogLevel::Error, msg, sid, ctx);
}

/// Log a debug-level entry.
pub fn debug(msg: &str, sid: Option<&str>, ctx: Option<serde_json::Value>) {
    push_entry(LogLevel::Debug, msg, sid, ctx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn entry(message: &str) -> ClientLogEntry {
        ClientLogEntry { ts: now_ts(), pid: 1, ver: "test".into(),
            lvl: LogLevel::Info, sid: None, msg: message.into(), ctx: None }
    }

    async fn receive(rx: &mut acp_transport::AcpAgentRx) -> LogNotificationParams {
        let message = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await.expect("forwarding timeout").expect("channel closed");
        let acp_transport::AcpAgentMessage::ExtNotification(args) = message else {
            panic!("expected log notification");
        };
        assert_eq!(args.request.method.as_ref(), LOG_METHOD);
        let params = serde_json::from_str(args.request.params.get()).unwrap();
        let _ = args.response_tx.send(Ok(()));
        params
    }

    #[tokio::test]
    async fn unified_log_plain_thread_batch_uses_initialization_runtime() {
        let forwarder = Arc::new(Forwarder::new());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        assert!(forwarder.initialize(tx));
        let producer = Arc::clone(&forwarder);
        std::thread::spawn(move || {
            assert!(Handle::try_current().is_err());
            for n in 0..16 { producer.push(entry(&n.to_string())); }
        }).join().unwrap();
        let params = receive(&mut rx).await;
        assert_eq!(params.entries.len(), 16);
        assert_eq!(params.entries[0].msg, "0");
        assert_eq!(params.entries[15].msg, "15");
        assert!(forwarder.buffer.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unified_log_preinit_flushes_preserve_entries_and_duplicate_init_keeps_owner() {
        let forwarder = Forwarder::new();
        forwarder.push(entry("startup"));
        forwarder.flush();
        forwarder.flush_blocking().await;
        assert_eq!(forwarder.buffer.lock().unwrap().len(), 1);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        assert!(forwarder.initialize(tx));
        let (other_tx, mut other_rx) = tokio::sync::mpsc::unbounded_channel();
        assert!(!forwarder.initialize(other_tx));
        let ((), params) = tokio::join!(forwarder.flush_blocking(), receive(&mut rx));
        assert_eq!(params.entries[0].msg, "startup");
        assert!(other_rx.try_recv().is_err());
        assert!(forwarder.buffer.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unified_log_flush_releases_unacknowledged_batch_at_deadline() {
        let forwarder = Arc::new(Forwarder::new());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        assert!(forwarder.initialize(tx));
        forwarder.push(entry("held by peer"));
        let task_forwarder = Arc::clone(&forwarder);
        let started = tokio::time::Instant::now();
        let flushing = tokio::spawn(async move { task_forwarder.flush_blocking().await });
        let message = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await.unwrap().unwrap();
        let acp_transport::AcpAgentMessage::ExtNotification(args) = message else {
            panic!("expected log notification");
        };
        // Keep the response sender alive: only the production deadline can
        // release this wait, not channel closure or a fake acknowledgement.
        tokio::time::timeout(FLUSH_WAIT + std::time::Duration::from_secs(2), flushing)
            .await.expect("flush exceeded watchdog").unwrap();
        assert!(started.elapsed() >= FLUSH_WAIT);
        assert!(args.response_tx.is_closed());
        assert!(args.response_tx.send(Ok(())).is_err());
        assert!(forwarder.buffer.lock().unwrap().is_empty());
        assert!(rx.try_recv().is_err(), "timeout must not retry");
    }

    #[tokio::test]
    async fn unified_log_flush_closed_peer_finishes_without_deadline() {
        let forwarder = Forwarder::new();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        assert!(forwarder.initialize(tx));
        drop(rx);
        forwarder.push(entry("disconnected"));
        tokio::time::timeout(std::time::Duration::from_secs(1), forwarder.flush_blocking())
            .await.expect("closed channel should finish promptly");
    }

}
