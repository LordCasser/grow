//! Wake-up edge for UI-owned snapshots produced by background workers.
//!
//! Workers publish data into their existing synchronized snapshots/channels,
//! then call [`wake`]. The pager event loop consumes the edge and applies every
//! pending snapshot on the UI thread. `Notify` coalesces bursts, so completion
//! traffic cannot become a frame clock.

use std::sync::{Arc, OnceLock};

static UPDATE: OnceLock<Arc<tokio::sync::Notify>> = OnceLock::new();

pub(crate) fn notifier() -> &'static Arc<tokio::sync::Notify> {
    UPDATE.get_or_init(|| Arc::new(tokio::sync::Notify::new()))
}

pub(crate) fn wake() {
    notifier().notify_one();
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn wake_releases_event_waiter() {
        let waiting = super::notifier().notified();
        super::wake();
        tokio::time::timeout(std::time::Duration::from_secs(1), waiting)
            .await
            .expect("wake edge");
    }
}
