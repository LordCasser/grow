//! Owns the crossterm input reader until terminal teardown can stop it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedSender;

use super::root::event_loop::TimedInputEvent;

const POLL_TIMEOUT: Duration = Duration::from_millis(100);
pub(crate) const READER_JOIN_GRACE: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReaderJoin {
    Joined,
    TimedOut,
    Absent,
}

pub(crate) struct ReaderThread {
    handle: Option<JoinHandle<()>>,
}

impl ReaderThread {
    pub(crate) fn absent() -> Self {
        Self { handle: None }
    }

    pub(crate) fn spawn(
        tx: UnboundedSender<TimedInputEvent>,
        paused: Arc<AtomicBool>,
        parked: Arc<AtomicBool>,
    ) -> Self {
        let handle = std::thread::spawn(move || {
            let mut consecutive_event_errors: u32 = 0;
            loop {
                if tx.is_closed() {
                    break;
                }
                if paused.load(Ordering::Acquire) {
                    parked.store(true, Ordering::Release);
                    std::thread::sleep(POLL_TIMEOUT);
                    continue;
                }
                parked.store(false, Ordering::Release);
                let event = match crossterm::event::poll(POLL_TIMEOUT) {
                    Ok(true) => crossterm::event::read(),
                    Ok(false) => continue,
                    Err(error) => Err(error),
                };
                match event {
                    Ok(event) => {
                        consecutive_event_errors = 0;
                        if tx.send(TimedInputEvent::now(event)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        consecutive_event_errors += 1;
                        if consecutive_event_errors >= 50 {
                            tracing::error!(
                                "crossterm read returned {consecutive_event_errors} \
                                 consecutive errors, exiting reader: {error}"
                            );
                            break;
                        }
                        tracing::warn!("crossterm read error (skipping): {error}");
                    }
                }
            }
        });
        Self {
            handle: Some(handle),
        }
    }

    /// A timed-out reader is detached. Its possible continued stdin ownership
    /// forbids both a DA1 read and a competing crossterm drain.
    pub(crate) fn join_within(mut self, grace: Duration) -> ReaderJoin {
        let Some(handle) = self.handle.take() else {
            return ReaderJoin::Absent;
        };
        let deadline = Instant::now() + grace;
        while !handle.is_finished() {
            if Instant::now() >= deadline {
                drop(handle);
                return ReaderJoin::TimedOut;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        if handle.join().is_err() {
            tracing::warn!("terminal input reader panicked during teardown");
        }
        ReaderJoin::Joined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_reader_is_confirmed() {
        assert_eq!(
            ReaderThread::absent().join_within(Duration::ZERO),
            ReaderJoin::Absent
        );
    }

    #[test]
    fn blocked_reader_join_returns_at_deadline() {
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let thread = ReaderThread {
            handle: Some(std::thread::spawn(move || {
                let _ = release_rx.recv();
            })),
        };
        let start = Instant::now();
        assert_eq!(
            thread.join_within(Duration::from_millis(10)),
            ReaderJoin::TimedOut
        );
        assert!(start.elapsed() < Duration::from_secs(1));
        let _ = release_tx.send(());
    }
}
