//! Unified log forwarding for the pager.
//!
//! Buffers log entries in memory and flushes them to the shell via
//! `grow/log` ACP notifications. Call [`init`] once at startup with
//! the ACP sender, then use [`info`], [`warn`], [`error`], [`debug`]
//! from anywhere.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use acp_transport::AcpAgentTx;
use acp_transport::protocol as acp;
use diagnostics::unified_log::{
    ClientLogEntry, LOG_METHOD, LogLevel, LogNotificationParams, LogSource,
};
const MAX_ENTRY_BYTES: usize = 64 * 1024;
const MAX_PENDING_BYTES: usize = 1024 * 1024;
const MAX_PENDING_ENTRIES: usize = 256;
const MAX_BATCH_BYTES: usize = 256 * 1024;
const MAX_BATCH_ENTRIES: usize = 16;

struct QueuedEntry {
    seq: u64,
    bytes: usize,
    entry: ClientLogEntry,
}

#[derive(Default)]
struct QueueState {
    entries: VecDeque<QueuedEntry>,
    bytes: usize,
    accepted_seq: u64,
}

struct Forwarder {
    started: OnceLock<()>,
    queue: Mutex<QueueState>,
    wake: tokio::sync::Notify,
    settled: tokio::sync::watch::Sender<u64>,
    warned_about_drops: AtomicBool,
}

static FORWARDER: LazyLock<Arc<Forwarder>> = LazyLock::new(|| Arc::new(Forwarder::new()));
const FLUSH_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// Initialize the unified log forwarder with the ACP sender.
///
/// Must be called once after the ACP connection is established.
/// Spawns the one background sender. Entries buffered before this call
/// are picked up by that sender without creating detached batch tasks.
pub fn init(tx: AcpAgentTx) {
    FORWARDER.initialize(tx);
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
    fn new() -> Self {
        let (settled, _) = tokio::sync::watch::channel(0);
        Self {
            started: OnceLock::new(),
            queue: Mutex::new(QueueState::default()),
            wake: tokio::sync::Notify::new(),
            settled,
            warned_about_drops: AtomicBool::new(false),
        }
    }

    /// Only the successful installer may start the sender.
    fn initialize(self: &Arc<Self>, tx: AcpAgentTx) -> bool {
        if self.started.set(()).is_err() {
            return false;
        }
        let worker = Arc::clone(self);
        tokio::runtime::Handle::current().spawn(async move { worker.run(tx).await });
        self.wake.notify_one();
        true
    }

    fn push(&self, entry: ClientLogEntry) {
        let Some(bytes) = bounded_entry_size(&entry) else {
            self.warn_drop();
            return;
        };
        let Ok(mut queue) = self.queue.lock() else {
            return;
        };
        if queue.entries.len() >= MAX_PENDING_ENTRIES
            || queue.bytes.saturating_add(bytes) > MAX_PENDING_BYTES
        {
            drop(queue);
            self.warn_drop();
            return;
        }
        queue.accepted_seq = queue.accepted_seq.saturating_add(1);
        let seq = queue.accepted_seq;
        queue.bytes += bytes;
        queue.entries.push_back(QueuedEntry { seq, bytes, entry });
        if queue.entries.len() >= MAX_BATCH_ENTRIES && self.started.get().is_some() {
            self.wake.notify_one();
        }
    }

    fn warn_drop(&self) {
        if !self.warned_about_drops.swap(true, Ordering::Relaxed) {
            tracing::warn!("pager unified log entry dropped at forwarding budget");
        }
    }

    fn take_batch(&self) -> Option<(Vec<ClientLogEntry>, u64)> {
        let mut queue = self.queue.lock().ok()?;
        let mut entries = Vec::new();
        let mut batch_bytes = 0;
        let mut last_seq = 0;
        while let Some(front) = queue.entries.front() {
            if entries.len() >= MAX_BATCH_ENTRIES
                || (!entries.is_empty() && batch_bytes + front.bytes > MAX_BATCH_BYTES)
            {
                break;
            }
            let queued = queue.entries.pop_front().expect("front exists");
            queue.bytes -= queued.bytes;
            batch_bytes += queued.bytes;
            last_seq = queued.seq;
            entries.push(queued.entry);
        }
        (!entries.is_empty()).then_some((entries, last_seq))
    }

    async fn run(self: Arc<Self>, tx: AcpAgentTx) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                () = self.wake.notified() => {},
                _ = interval.tick() => {},
            }
            while let Some((entries, last_seq)) = self.take_batch() {
                if let Some(notification) = build_notification(entries) {
                    let _ = tokio::time::timeout(
                        FLUSH_WAIT,
                        acp_transport::acp_send(notification, &tx),
                    )
                    .await;
                }
                self.settled.send_replace(last_seq);
            }
        }
    }

    fn flush(&self) {
        if self.started.get().is_some() {
            self.wake.notify_one();
        }
    }

    async fn flush_blocking(&self) {
        if self.started.get().is_none() {
            return;
        }
        let target = match self.queue.lock() {
            Ok(queue) => queue.accepted_seq,
            Err(_) => return,
        };
        if target == 0 {
            return;
        }
        let mut settled = self.settled.subscribe();
        let deadline = tokio::time::Instant::now() + FLUSH_WAIT;
        self.flush();
        while *settled.borrow_and_update() < target {
            if tokio::time::timeout_at(deadline, settled.changed())
                .await
                .is_err()
            {
                break;
            }
        }
    }
}

struct BoundedCounter(usize);

impl Write for BoundedCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_ENTRY_BYTES.saturating_sub(self.0) {
            return Err(io::Error::other("pager log entry exceeds byte budget"));
        }
        self.0 += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn bounded_entry_size(entry: &ClientLogEntry) -> Option<usize> {
    let mut counter = BoundedCounter(0);
    serde_json::to_writer(&mut counter, entry).ok()?;
    Some(counter.0)
}

/// Wake the single sender to flush buffered entries to the shell.
pub fn flush() {
    FORWARDER.flush();
}

/// Wait at most two seconds for all entries accepted before this call to settle.
/// Settlement is not a guarantee of remote persistence.
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
    use tokio::runtime::Handle;

    fn entry(message: &str) -> ClientLogEntry {
        ClientLogEntry {
            ts: now_ts(),
            pid: 1,
            ver: "test".into(),
            lvl: LogLevel::Info,
            sid: None,
            msg: message.into(),
            ctx: None,
        }
    }

    async fn receive(rx: &mut acp_transport::AcpAgentRx) -> LogNotificationParams {
        let message = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("forwarding timeout")
            .expect("channel closed");
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
            for n in 0..16 {
                producer.push(entry(&n.to_string()));
            }
        })
        .join()
        .unwrap();
        let params = receive(&mut rx).await;
        assert_eq!(params.entries.len(), 16);
        assert_eq!(params.entries[0].msg, "0");
        assert_eq!(params.entries[15].msg, "15");
        assert!(forwarder.queue.lock().unwrap().entries.is_empty());
    }

    #[tokio::test]
    async fn unified_log_preinit_flushes_preserve_entries_and_duplicate_init_keeps_owner() {
        let forwarder = Arc::new(Forwarder::new());
        forwarder.push(entry("startup"));
        forwarder.flush();
        forwarder.flush_blocking().await;
        assert_eq!(forwarder.queue.lock().unwrap().entries.len(), 1);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        assert!(forwarder.initialize(tx));
        let (other_tx, mut other_rx) = tokio::sync::mpsc::unbounded_channel();
        assert!(!forwarder.initialize(other_tx));
        let ((), params) = tokio::join!(forwarder.flush_blocking(), receive(&mut rx));
        assert_eq!(params.entries[0].msg, "startup");
        assert!(other_rx.try_recv().is_err());
        assert!(forwarder.queue.lock().unwrap().entries.is_empty());
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
            .await
            .unwrap()
            .unwrap();
        let acp_transport::AcpAgentMessage::ExtNotification(args) = message else {
            panic!("expected log notification");
        };
        // Keep the response sender alive: only the production deadline can
        // release this wait, not channel closure or a fake acknowledgement.
        tokio::time::timeout(FLUSH_WAIT + std::time::Duration::from_secs(2), flushing)
            .await
            .expect("flush exceeded watchdog")
            .unwrap();
        assert!(started.elapsed() >= FLUSH_WAIT);
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while !args.response_tx.is_closed() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("worker did not release timed-out acknowledgement");
        assert!(args.response_tx.send(Ok(())).is_err());
        assert!(forwarder.queue.lock().unwrap().entries.is_empty());
        assert!(rx.try_recv().is_err(), "timeout must not retry");
    }

    #[tokio::test]
    async fn unified_log_flush_closed_peer_finishes_without_deadline() {
        let forwarder = Arc::new(Forwarder::new());
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        assert!(forwarder.initialize(tx));
        drop(rx);
        forwarder.push(entry("disconnected"));
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            forwarder.flush_blocking(),
        )
        .await
        .expect("closed channel should finish promptly");
    }

    #[tokio::test]
    async fn preinit_queue_rejects_oversize_and_new_entries_at_capacity() {
        let forwarder = Forwarder::new();
        let oversized = entry(&"x".repeat(MAX_ENTRY_BYTES));
        assert!(bounded_entry_size(&oversized).is_none());
        forwarder.push(oversized);
        for n in 0..MAX_PENDING_ENTRIES {
            forwarder.push(entry(&n.to_string()));
        }
        forwarder.push(entry("rejected"));
        let queue = forwarder.queue.lock().unwrap();
        assert_eq!(queue.entries.len(), MAX_PENDING_ENTRIES);
        assert_eq!(queue.accepted_seq, MAX_PENDING_ENTRIES as u64);
        assert_eq!(queue.entries.front().unwrap().entry.msg, "0");
        assert_eq!(
            queue.entries.back().unwrap().entry.msg,
            (MAX_PENDING_ENTRIES - 1).to_string()
        );
        assert!(queue.bytes <= MAX_PENDING_BYTES);
    }

    #[tokio::test]
    async fn preinit_queue_obeys_aggregate_byte_budget() {
        let forwarder = Forwarder::new();
        let large = entry(&"x".repeat(MAX_ENTRY_BYTES / 2));
        let bytes = bounded_entry_size(&large).unwrap();
        let accepted = MAX_PENDING_BYTES / bytes;
        assert!(accepted < MAX_PENDING_ENTRIES);
        for _ in 0..accepted {
            forwarder.push(large.clone());
        }
        forwarder.push(large);
        let queue = forwarder.queue.lock().unwrap();
        assert_eq!(queue.entries.len(), accepted);
        assert_eq!(queue.bytes, accepted * bytes);
        assert!(queue.bytes <= MAX_PENDING_BYTES);
    }

    #[tokio::test]
    async fn earlier_batch_is_included_in_later_exit_flush() {
        let forwarder = Arc::new(Forwarder::new());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        assert!(forwarder.initialize(tx));
        for n in 0..MAX_BATCH_ENTRIES {
            forwarder.push(entry(&n.to_string()));
        }
        let first = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        let acp_transport::AcpAgentMessage::ExtNotification(first) = first else {
            panic!("expected first batch");
        };
        forwarder.push(entry("after first batch"));
        assert!(
            rx.try_recv().is_err(),
            "second batch bypassed the first ACK"
        );
        let waiting = Arc::clone(&forwarder);
        let mut flush = tokio::spawn(async move { waiting.flush_blocking().await });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut flush)
                .await
                .is_err(),
            "exit flush returned before an earlier batch settled"
        );
        let _ = first.response_tx.send(Ok(()));
        let second = receive(&mut rx).await;
        assert_eq!(second.entries.len(), 1);
        assert_eq!(second.entries[0].msg, "after first batch");
        tokio::time::timeout(std::time::Duration::from_secs(1), flush)
            .await
            .expect("exit flush did not reach prior accepted frontier")
            .unwrap();
    }

    #[tokio::test]
    async fn concurrent_flushes_wait_for_the_same_accepted_frontier() {
        let forwarder = Arc::new(Forwarder::new());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        assert!(forwarder.initialize(tx));
        forwarder.push(entry("shared"));
        let a = Arc::clone(&forwarder);
        let b = Arc::clone(&forwarder);
        let mut first = tokio::spawn(async move { a.flush_blocking().await });
        let mut second = tokio::spawn(async move { b.flush_blocking().await });
        let message = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        let acp_transport::AcpAgentMessage::ExtNotification(args) = message else {
            panic!("expected log batch");
        };
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut first)
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut second)
                .await
                .is_err()
        );
        let _ = args.response_tx.send(Ok(()));
        tokio::time::timeout(std::time::Duration::from_secs(1), first)
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), second)
            .await
            .unwrap()
            .unwrap();
        assert!(
            rx.try_recv().is_err(),
            "concurrent flush duplicated the batch"
        );
    }
}
