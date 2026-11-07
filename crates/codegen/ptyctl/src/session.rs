//! Core PTY session — ties PTY + Terminal + I/O channels together.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::{Mutex, broadcast, mpsc, watch};

use crate::keys;
use crate::pty::{PtyConfig, PtyHandle, PtyMaster};
use crate::styled::StyledLine;
use crate::term::{
    CursorPosition, ScreenOpts, ScreenOutput, ScrollbackLine, SessionListener, Terminal,
    TerminalModes,
};
// Re-exported so existing `session::Wait*` paths keep working after the wait-module extraction.
use crate::wait::{RAW_TAIL_CAP, push_raw_tail};
pub use crate::wait::{WaitCondition, WaitDiagnostics, WaitHandle, WaitOutcome};

/// Configuration for starting a session.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub pty: PtyConfig,
    /// Auto-shutdown timeout in seconds (None = no timeout).
    pub timeout: Option<u64>,
    /// Keep server running after child exits.
    pub linger: bool,
}

/// Status of a PTY session.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionStatus {
    pub alive: bool,
    pub pid: Option<u32>,
    pub exit_code: Option<u32>,
    pub size: (u16, u16),
    pub modes: TerminalModes,
    pub scrollback_lines: usize,
}

/// A running PTY session.
pub struct PtySession {
    terminal: Arc<Mutex<Terminal>>,
    /// Master half of the PTY, kept here so resize reaches the real PTY.
    master: PtyMaster,
    pty_write_tx: mpsc::UnboundedSender<Vec<u8>>,
    alive: Arc<AtomicBool>,
    exit_code: Arc<std::sync::Mutex<Option<u32>>>,
    pid: Option<u32>,
    /// Grid generation counter, bumped by the feeder after each `term.feed()`.
    generation_rx: watch::Receiver<u64>,
    /// Weak so the feeder's exit still drops the sender, signalling "ended" to waiters;
    /// lets `resize` bump the generation too (a resize changes the grid without output).
    generation_tx: Weak<watch::Sender<u64>>,
    /// Last [`RAW_TAIL_CAP`] bytes of raw PTY output for wait-timeout diagnostics.
    raw_tail: Arc<std::sync::Mutex<VecDeque<u8>>>,
    control: SessionControl,
    /// Broadcast channel for real-time PTY output streaming (WebSocket).
    output_tx: broadcast::Sender<Vec<u8>>,
}

/// A cloneable control for the one owner of the actual spawned child and its I/O.
#[derive(Clone)]
pub struct SessionControl {
    stop: Arc<AtomicBool>,
    state: watch::Receiver<OwnerState>,
}

#[derive(Clone, Default)]
struct OwnerState {
    terminal: Option<std::result::Result<u32, String>>,
    shutdown: bool,
}

impl SessionControl {
    /// Request settlement. Once admitted, cancellation of this wait does not undo stop.
    pub async fn stop(&self) -> Result<()> {
        self.stop.store(true, Ordering::Release);
        self.wait_for_exit().await.map(|_| ())
    }

    /// Observe the actual child status after all owned I/O and final screen updates.
    pub async fn wait_for_exit(&self) -> Result<u32> {
        let mut state = self.state.clone();
        loop {
            if let Some(result) = state.borrow_and_update().terminal.clone() {
                return result.map_err(anyhow::Error::msg);
            }
            state
                .changed()
                .await
                .context("PTY lifecycle owner closed without settlement")?;
        }
    }

    /// Observe the service policy: natural completion closes non-linger sessions;
    /// explicit stop and the original startup timeout also close linger sessions.
    pub async fn wait_for_shutdown(&self) -> Result<()> {
        let mut state = self.state.clone();
        loop {
            let current = state.borrow_and_update().clone();
            if let Some(Err(error)) = current.terminal {
                anyhow::bail!(error);
            }
            if current.shutdown {
                return Ok(());
            }
            state
                .changed()
                .await
                .context("PTY lifecycle owner closed without shutdown")?;
        }
    }
}

impl PtySession {
    /// Start a new PTY session. The independent owner creates the actual child,
    /// so cancelling this async startup cannot abandon a partly started process.
    pub async fn start(config: SessionConfig) -> Result<Self> {
        let (pty_response_tx, responses) = mpsc::unbounded_channel();
        let (pty_write_tx, writes) = mpsc::unbounded_channel();
        let (output_tx, _) = broadcast::channel(256);
        let (generation_tx, generation_rx) = watch::channel(0);
        let generation_tx = Arc::new(generation_tx);
        let generation_tx_weak = Arc::downgrade(&generation_tx);
        let raw_tail = Arc::new(std::sync::Mutex::new(VecDeque::with_capacity(RAW_TAIL_CAP)));
        let terminal = Arc::new(Mutex::new(Terminal::new(
            config.pty.cols,
            config.pty.rows,
            SessionListener::new(pty_response_tx),
        )));
        let alive = Arc::new(AtomicBool::new(true));
        let exit_code = Arc::new(std::sync::Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let (state_tx, state) = watch::channel(OwnerState::default());
        let control = SessionControl {
            stop: stop.clone(),
            state,
        };
        let owner = SessionOwner {
            terminal: terminal.clone(),
            alive: alive.clone(),
            exit_code: exit_code.clone(),
            generation: generation_tx,
            raw_tail: raw_tail.clone(),
            output: output_tx.clone(),
            writes,
            responses,
            stop: Arc::downgrade(&stop),
            state: state_tx,
        };
        // Only external control/session handles retain strong stop references.
        drop(stop);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        // Dropping this launcher's handle does not retire any resource. The
        // autonomous owner holds the actual child and all worker join handles
        // until settlement, even after every runtime/external handle has gone.
        std::thread::Builder::new()
            .name("pty-session-owner".into())
            .spawn(move || owner.run(config, started_tx))
            .context("failed to spawn PTY lifecycle owner")?;
        let (master, pid) = started_rx
            .await
            .context("PTY lifecycle owner failed during start")??;
        Ok(Self {
            terminal,
            master,
            pty_write_tx,
            alive,
            exit_code,
            pid,
            generation_rx,
            generation_tx: generation_tx_weak,
            raw_tail,
            control,
            output_tx,
        })
    }

    /// Send keystrokes using vim notation (e.g. `"<C-c>"`, `"hello<CR>"`).
    pub async fn send_keys(&self, notation: &str) -> Result<()> {
        let bytes = keys::parse_keys(notation)?;
        self.send_bytes(&bytes).await
    }

    /// Send raw bytes to the PTY.
    pub async fn send_bytes(&self, bytes: &[u8]) -> Result<()> {
        self.pty_write_tx
            .send(bytes.to_vec())
            .map_err(|_| anyhow::anyhow!("PTY write channel closed"))
    }

    /// Read screen content as plain text.
    pub async fn screen(&self, opts: &ScreenOpts) -> ScreenOutput {
        let term = self.terminal.lock().await;
        term.screen_content(opts)
    }

    /// Read screen content with style information.
    pub async fn screen_styled(&self, opts: &ScreenOpts) -> Vec<StyledLine> {
        let term = self.terminal.lock().await;
        term.screen_styled(opts)
    }

    /// Read screen content as HTML.
    pub async fn screen_html(&self, opts: &ScreenOpts) -> String {
        let term = self.terminal.lock().await;
        term.screen_html(opts)
    }

    /// Get cursor position (1-indexed).
    pub async fn cursor(&self) -> CursorPosition {
        let term = self.terminal.lock().await;
        term.cursor_position()
    }

    /// Get session status (basic info, no terminal lock needed).
    pub fn status_basic(&self) -> (bool, Option<u32>, Option<u32>) {
        (
            self.alive.load(Ordering::SeqCst),
            self.pid,
            *self.exit_code.lock().unwrap(),
        )
    }

    /// Get full session status including terminal modes.
    pub async fn status(&self) -> SessionStatus {
        let (alive, pid, exit_code) = self.status_basic();
        let term = self.terminal.lock().await;
        // Size comes from the grid under the same lock resize holds, so it can never be torn.
        let size = term.size();
        SessionStatus {
            alive,
            pid,
            exit_code,
            size: (size.cols as u16, size.rows as u16),
            modes: term.terminal_modes(),
            scrollback_lines: term.scrollback_count(),
        }
    }

    /// Resize the real PTY and the terminal grid.
    pub async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let mut term = self.terminal.lock().await;
        // PTY first (fail-fast); the held terminal lock keeps the SIGWINCH redraw out of a stale grid.
        self.master.resize(cols, rows)?;
        term.resize(cols, rows);
        // Bump after the grid resize (matching the feeder's post-feed ordering): reflow/clipping
        // changes screen text, so in-flight waits must re-check — and StableMs must restart.
        if let Some(generation_tx) = self.generation_tx.upgrade() {
            generation_tx.send_modify(|g| *g += 1);
        }
        Ok(())
    }

    /// Check if the child process is still alive.
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Subscribe to real-time PTY output for WebSocket streaming.
    pub fn subscribe(&self) -> broadcast::Receiver<Vec<u8>> {
        self.output_tx.subscribe()
    }

    /// Read scrollback history lines.
    pub async fn scrollback(&self, count: usize) -> Vec<ScrollbackLine> {
        let term = self.terminal.lock().await;
        term.scrollback_lines(count)
    }

    /// Clone the handles needed to wait without holding any outer session lock.
    pub fn wait_handle(&self) -> WaitHandle {
        WaitHandle {
            terminal: self.terminal.clone(),
            generation_rx: self.generation_rx.clone(),
            raw_tail: self.raw_tail.clone(),
        }
    }

    /// Wait until `condition` is met or `timeout` elapses.
    pub async fn wait_for(
        &self,
        condition: WaitCondition,
        timeout: Duration,
    ) -> Result<WaitOutcome> {
        self.wait_handle().wait_for(condition, timeout).await
    }

    /// Clone lifecycle control without retaining an outer Session lock.
    pub fn control_handle(&self) -> SessionControl {
        self.control.clone()
    }

    /// Stop this owner's actual child and retire its I/O.
    pub async fn stop(&self) -> Result<()> {
        self.control.stop().await
    }
}

const DRAIN_TIMEOUT: Duration = Duration::from_secs(1);
const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const IO_RETRY: Duration = Duration::from_millis(10);

struct SessionOwner {
    terminal: Arc<Mutex<Terminal>>,
    alive: Arc<AtomicBool>,
    exit_code: Arc<std::sync::Mutex<Option<u32>>>,
    generation: Arc<watch::Sender<u64>>,
    raw_tail: Arc<std::sync::Mutex<VecDeque<u8>>>,
    output: broadcast::Sender<Vec<u8>>,
    writes: mpsc::UnboundedReceiver<Vec<u8>>,
    responses: mpsc::UnboundedReceiver<Vec<u8>>,
    stop: Weak<AtomicBool>,
    state: watch::Sender<OwnerState>,
}

impl SessionOwner {
    /// This thread is independent of the async runtime. It never gives away the
    /// actual child or an unjoined I/O worker, including after reporting failure.
    fn run(
        mut self,
        config: SessionConfig,
        started: tokio::sync::oneshot::Sender<Result<(PtyMaster, Option<u32>)>>,
    ) {
        let pty = match PtyHandle::spawn(&config.pty) {
            Ok(pty) => pty,
            Err(error) => {
                let message = format!("failed to spawn PTY: {error:#}");
                self.fail(message.clone());
                let _ = started.send(Err(anyhow::Error::msg(message)));
                return;
            }
        };
        let began = std::time::Instant::now();
        let (master, mut child, reader, writer) = pty.into_parts();
        let pid = child.pid();
        let (mut io, mut failure) = IoWorkers::start(reader, writer, &master);
        let mut stopping = failure.is_some();
        if started.send(Ok((master, pid))).is_err() {
            // Start was cancelled: this owner still owes settlement of its child.
            stopping = true;
        }
        if let Some(error) = &failure {
            self.fail(error.clone());
        }
        let mut killed = false;
        let mut stop_started = None;
        let mut child_code = None;
        let mut drain_started = None;
        let mut pending_output = VecDeque::new();
        let mut feed_failed = false;

        loop {
            stopping |= self
                .stop
                .upgrade()
                .is_none_or(|stop| stop.load(Ordering::Acquire));
            stopping |= config
                .timeout
                .is_some_and(|seconds| began.elapsed() >= Duration::from_secs(seconds));
            if stopping && stop_started.is_none() {
                stop_started = Some(std::time::Instant::now());
                self.writes.close();
                self.responses.close();
                io.stop_writing();
            }

            if child_code.is_none() {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        // try_wait established actual exit. wait reaps through the
                        // same handle; no reader or second waiter can race this owner.
                        match child.wait() {
                            Ok(waited) => {
                                child_code = Some(waited);
                                *self.exit_code.lock().unwrap() = Some(waited);
                                self.alive.store(false, Ordering::Release);
                                drain_started = Some(std::time::Instant::now());
                                io.stop_writing();
                                self.writes.close();
                                self.responses.close();
                            }
                            Err(error) => self.record_failure(&mut failure, format!("{error:#}")),
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        self.record_failure(&mut failure, format!("{error:#}"));
                        stopping = true;
                    }
                }
                if stopping && child_code.is_none() && !killed {
                    killed = true;
                    if let Err(error) = child.kill() {
                        // A concurrent natural exit may make kill fail. Confirm it
                        // through this same child before declaring an actual failure.
                        if !matches!(child.try_wait(), Ok(Some(_))) {
                            self.record_failure(&mut failure, format!("{error:#}"));
                        }
                    }
                }
                if stop_started.is_some_and(|start| start.elapsed() >= STOP_TIMEOUT)
                    && child_code.is_none()
                {
                    self.record_failure(
                        &mut failure,
                        "PTY child settlement exceeded five seconds; owner retained".into(),
                    );
                }
            }
            if child_code.is_some() {
                if let Err(error) = io.release_master() {
                    self.record_failure(&mut failure, format!("{error:#}"));
                }
            }

            // Limit each batch so continuous output cannot starve stop/timeout or
            // child observation. FIFO order is retained across lock contention.
            for _ in 0..64 {
                match io.output.try_recv() {
                    Ok(bytes) => pending_output.push_back(bytes),
                    Err(_) => break,
                }
            }
            if let Ok(mut terminal) = self.terminal.try_lock() {
                while let Some(bytes) = pending_output.pop_front() {
                    let _ = self.output.send(bytes.clone());
                    push_raw_tail(&self.raw_tail, &bytes);
                    if !feed_failed {
                        let fed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            terminal.feed(&bytes)
                        }));
                        if fed.is_err() {
                            feed_failed = true;
                            stopping = true;
                            self.record_failure(
                                &mut failure,
                                "PTY terminal feeder panicked; owner retained".into(),
                            );
                        }
                    }
                    self.generation.send_modify(|generation| *generation += 1);
                }
            }
            if !stopping && child_code.is_none() {
                for receiver in [&mut self.writes, &mut self.responses] {
                    for _ in 0..64 {
                        match receiver.try_recv() {
                            Ok(bytes) => {
                                let _ = io.input.send(bytes);
                            }
                            Err(_) => break,
                        }
                    }
                }
            }
            if let Some(error) = io.retire_finished() {
                self.record_failure(&mut failure, error);
                stopping = true;
            }

            if child_code.is_some() {
                // Once the reader is joined its sender is gone, so draining the
                // remaining FIFO is finite; do not publish before its last chunk.
                if io.retired()
                    && pending_output.is_empty()
                    && io
                        .output
                        .try_recv()
                        .map(|bytes| pending_output.push_back(bytes))
                        .is_err()
                {
                    break;
                }
                if drain_started.is_some_and(|start| start.elapsed() >= DRAIN_TIMEOUT) {
                    self.record_failure(
                        &mut failure,
                        "PTY I/O drain exceeded one second; owner retained until join".into(),
                    );
                    io.stop_reading();
                }
            }
            std::thread::sleep(IO_RETRY);
        }
        // All threads were joined and their final FIFO has been fed. Close the
        // screen generation before announcing terminal success to observers.
        drop(io);
        drop(self.generation);
        if failure.is_some() {
            return;
        }
        self.state.send_modify(|state| {
            state.terminal = Some(Ok(child_code.expect("settled child")));
            state.shutdown = stopping || !config.linger;
        });
        if stopping || !config.linger {
            return;
        }
        // Natural linger keeps only service policy observation alive. No I/O or
        // child responsibility remains, but the original timeout still applies.
        loop {
            let requested = self
                .stop
                .upgrade()
                .is_none_or(|stop| stop.load(Ordering::Acquire));
            let timed_out = config
                .timeout
                .is_some_and(|seconds| began.elapsed() >= Duration::from_secs(seconds));
            if requested || timed_out {
                self.state.send_modify(|state| state.shutdown = true);
                return;
            }
            std::thread::sleep(IO_RETRY);
        }
    }

    fn fail(&self, error: String) {
        self.state
            .send_modify(|state| state.terminal = Some(Err(error)));
    }

    fn record_failure(&self, failure: &mut Option<String>, error: String) {
        if failure.is_none() {
            self.fail(error.clone());
            *failure = Some(error);
        }
    }
}

/// Streams and join handles remain owned here until the actual threads finish.
/// Requesting cancellation never stands in for joining a worker.
struct IoWorkers {
    reader: Option<std::thread::JoinHandle<Result<()>>>,
    writer: Option<std::thread::JoinHandle<Result<()>>>,
    output: std::sync::mpsc::Receiver<Vec<u8>>,
    input: std::sync::mpsc::Sender<Vec<u8>>,
    stop_reader: Arc<AtomicBool>,
    stop_writer: Arc<AtomicBool>,
    #[cfg(windows)]
    master: PtyMaster,
    #[cfg(windows)]
    master_close: Option<std::thread::JoinHandle<()>>,
    #[cfg(windows)]
    master_released: bool,
}

impl IoWorkers {
    fn start(
        mut reader: Box<dyn std::io::Read + Send>,
        mut writer: Box<dyn std::io::Write + Send>,
        _master: &PtyMaster,
    ) -> (Self, Option<String>) {
        let (output_tx, output) = std::sync::mpsc::channel();
        let (input, input_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let stop_reader = Arc::new(AtomicBool::new(false));
        let stop_writer = Arc::new(AtomicBool::new(false));
        let reader_stop = stop_reader.clone();
        let reader = std::thread::Builder::new()
            .name("pty-reader".into())
            .spawn(move || {
                let mut buf = [0u8; 65536];
                while !reader_stop.load(Ordering::Acquire) {
                    match reader.read(&mut buf) {
                        Ok(0) => return Ok(()),
                        Ok(n) => {
                            if output_tx.send(buf[..n].to_vec()).is_err() {
                                return Ok(());
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(IO_RETRY)
                        }
                        Err(_) if reader_stop.load(Ordering::Acquire) => return Ok(()),
                        Err(e) => return Err(e).context("PTY reader failed"),
                    }
                }
                Ok(())
            });
        let writer_stop = stop_writer.clone();
        let writer = std::thread::Builder::new()
            .name("pty-writer".into())
            .spawn(move || {
                while !writer_stop.load(Ordering::Acquire) {
                    let bytes = match input_rx.recv_timeout(IO_RETRY) {
                        Ok(bytes) => bytes,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let mut remaining = bytes.as_slice();
                    while !remaining.is_empty() && !writer_stop.load(Ordering::Acquire) {
                        match writer.write(remaining) {
                            Ok(0) => anyhow::bail!("PTY writer returned zero bytes"),
                            Ok(n) => remaining = &remaining[n..],
                            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(IO_RETRY)
                            }
                            Err(_) if writer_stop.load(Ordering::Acquire) => return Ok(()),
                            Err(e) => return Err(e).context("PTY writer failed"),
                        }
                    }
                    while !writer_stop.load(Ordering::Acquire) {
                        match writer.flush() {
                            Ok(()) => break,
                            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(IO_RETRY)
                            }
                            Err(_) if writer_stop.load(Ordering::Acquire) => return Ok(()),
                            Err(e) => return Err(e).context("PTY writer flush failed"),
                        }
                    }
                }
                Ok(())
            });
        let mut failure = None;
        let reader = match reader {
            Ok(thread) => Some(thread),
            Err(error) => {
                failure = Some(format!("failed to spawn PTY reader: {error}"));
                None
            }
        };
        let writer = match writer {
            Ok(thread) => Some(thread),
            Err(error) => {
                failure.get_or_insert_with(|| format!("failed to spawn PTY writer: {error}"));
                None
            }
        };
        (
            Self {
                reader,
                writer,
                output,
                input,
                stop_reader,
                stop_writer,
                #[cfg(windows)]
                master: _master.clone(),
                #[cfg(windows)]
                master_close: None,
                #[cfg(windows)]
                master_released: false,
            },
            failure,
        )
    }

    fn release_master(&mut self) -> Result<()> {
        #[cfg(windows)]
        if !self.master_released {
            let master = self.master.clone();
            // Before Windows 11 24H2 ClosePseudoConsole can wait for output.
            // Keep its reader and the owner FIFO consumer running concurrently.
            self.master_close = Some(
                std::thread::Builder::new()
                    .name("pty-master-close".into())
                    .spawn(move || master.close())
                    .context("failed to spawn PTY master retirement worker")?,
            );
            self.master_released = true;
        }
        Ok(())
    }

    fn stop_writing(&self) {
        self.stop_writer.store(true, Ordering::Release);
    }

    fn stop_reading(&self) {
        #[cfg(windows)]
        if self.master_close.is_some() || !self.master_released {
            // Canceling the drain while ClosePseudoConsole waits could itself
            // cause an old-Windows deadlock. Retain both workers after failure.
            return;
        }
        self.stop_reader.store(true, Ordering::Release);
    }

    fn retire_finished(&mut self) -> Option<String> {
        let mut failure = None;
        for (thread, stop) in [
            (&mut self.reader, &self.stop_reader),
            (&mut self.writer, &self.stop_writer),
        ] {
            if let Some(handle) = thread.as_ref() {
                if handle.is_finished() {
                    let result = thread.take().expect("owned thread").join();
                    match result {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => {
                            failure.get_or_insert_with(|| format!("{error:#}"));
                        }
                        Err(_) => {
                            failure.get_or_insert_with(|| "PTY I/O worker panicked".to_owned());
                        }
                    }
                } else if stop.load(Ordering::Acquire) {
                    // Repeat cancellation: the previous request may have raced the
                    // worker between its stop check and the next synchronous call.
                    if let Err(error) = cancel_io(handle) {
                        failure
                            .get_or_insert_with(|| format!("PTY I/O cancellation failed: {error}"));
                    }
                }
            }
        }
        #[cfg(windows)]
        if self
            .master_close
            .as_ref()
            .is_some_and(|thread| thread.is_finished())
        {
            if self
                .master_close
                .take()
                .expect("owned master worker")
                .join()
                .is_err()
            {
                failure.get_or_insert_with(|| "PTY master retirement worker panicked".to_owned());
            }
        }
        failure
    }

    fn retired(&self) -> bool {
        let retired = self.reader.is_none() && self.writer.is_none();
        #[cfg(windows)]
        let retired = retired && self.master_released && self.master_close.is_none();
        retired
    }
}

#[cfg(unix)]
fn cancel_io<T>(_: &std::thread::JoinHandle<T>) -> std::io::Result<()> {
    // Both dup streams are nonblocking; they check their stop flag on retries.
    Ok(())
}

#[cfg(windows)]
fn cancel_io<T>(thread: &std::thread::JoinHandle<T>) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::ERROR_NOT_FOUND;
    use windows_sys::Win32::System::IO::CancelSynchronousIo;
    // SAFETY: JoinHandle keeps this exact owned thread's handle alive for the call.
    // Cancellation only requests completion; the owner still joins the thread.
    if unsafe { CancelSynchronousIo(thread.as_raw_handle() as _) } == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_NOT_FOUND as i32) {
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
pub(crate) mod tests {
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    use super::*;

    /// Closing every slave descriptor is an output event, not child settlement.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lifecycle_eof_does_not_retire_child() {
        let session = start_session(vec![
            "/usr/bin/python3".into(),
            "-c".into(),
            "import os,time; os.closerange(0,1024); time.sleep(1); os._exit(23)".into(),
        ])
        .await;
        // The direct shell remains alive after its PTY stream has ended.
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(session.is_alive(), "reader EOF fabricated child retirement");
        assert_eq!(session.status_basic().2, None);
        let deadline = Instant::now() + Duration::from_secs(3);
        while session.is_alive() && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(session.status_basic().2, Some(23));
    }

    /// An ended grid must include the last output and the real nonzero status.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lifecycle_final_output_has_real_exit_code() {
        let session = start_session(vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf 'FINAL-LINE-PTY\\n'; exit 37".into(),
        ])
        .await;
        let outcome = session
            .wait_for(
                WaitCondition::Text("absent-marker".into()),
                Duration::from_secs(3),
            )
            .await
            .unwrap();
        assert!(outcome.diagnostics.unwrap().ended);
        let screen = session.screen(&ScreenOpts::default()).await;
        assert!(screen.lines.join("\n").contains("FINAL-LINE-PTY"));
        assert_eq!(
            session.status_basic(),
            (false, session.status_basic().1, Some(37))
        );
    }

    /// Stop acknowledges the actual child, rather than a discarded signal.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lifecycle_explicit_stop_reaps_actual_child() {
        // Even the old-red fixture is bounded if the original stop does nothing.
        let session = start_session(vec!["/bin/sleep".into(), "3".into()]).await;
        session.stop().await.unwrap();
        assert!(
            !session.is_alive(),
            "stop acknowledged a still-running child"
        );
        assert!(
            session.status_basic().2.is_some(),
            "stop did not collect real status"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lifecycle_cancel_start_keeps_actual_child_owner() {
        let marker = std::env::temp_dir().join(format!(
            "ptyctl-start-cancel-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut config = SessionConfig {
            pty: PtyConfig {
                command: vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    "printf '%s' \"$$\" > \"$PTY_CANCEL_MARKER\"; exec sleep 30".into(),
                ],
                cols: 80,
                rows: 24,
                cwd: None,
                env: HashMap::new(),
            },
            timeout: None,
            linger: false,
        };
        config.pty.env.insert(
            "PTY_CANCEL_MARKER".into(),
            marker.to_string_lossy().into_owned(),
        );
        let mut starting = Box::pin(PtySession::start(config));
        assert!(futures_util::poll!(starting.as_mut()).is_pending());
        let deadline = Instant::now() + Duration::from_secs(3);
        let pid = loop {
            if let Ok(text) = std::fs::read_to_string(&marker) {
                if let Ok(pid) = text.parse::<i32>() {
                    break pid;
                }
            }
            assert!(Instant::now() < deadline, "child fixture did not start");
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        drop(starting);
        std::fs::remove_file(marker).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            // SAFETY: signal zero only observes this fixture's actual spawned PID.
            if unsafe { libc::kill(pid, 0) } != 0 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "cancelled start abandoned its child"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[test]
    fn lifecycle_runtime_and_last_handle_drop_preserve_owner() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let session = runtime.block_on(start_session(vec!["/bin/sleep".into(), "30".into()]));
        let mut generation = session.generation_rx.clone();
        let control = session.control_handle();
        drop(runtime);
        let next_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        next_runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(3), control.stop())
                .await
                .unwrap()
                .unwrap();
        });
        assert!(
            generation.has_changed().is_err(),
            "I/O workers were not joined"
        );
        drop(control);
        drop(session);
        let session = next_runtime.block_on(start_session(vec!["/bin/sleep".into(), "30".into()]));
        generation = session.generation_rx.clone();
        drop(session);
        drop(next_runtime);
        let deadline = Instant::now() + Duration::from_secs(3);
        while generation.has_changed().is_ok() {
            assert!(
                Instant::now() < deadline,
                "last handle drop abandoned I/O responsibility"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    async fn bounded_exit(control: &SessionControl) -> Result<u32> {
        tokio::time::timeout(Duration::from_secs(4), control.wait_for_exit())
            .await
            .expect("owner did not provide a bounded terminal result")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lifecycle_stop_repeated_and_cancelled_observer() {
        let session = start_session(vec![
            "/bin/sh".into(),
            "-c".into(),
            "trap '' HUP; printf 'STOP-READY\\n'; exec sleep 30".into(),
        ])
        .await;
        wait_for_screen(&session, |text| text.contains("STOP-READY")).await;
        let control = session.control_handle();
        let stopping = tokio::spawn({
            let control = control.clone();
            async move { control.stop().await }
        });
        while !control.stop.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
        stopping.abort();
        let observer = tokio::spawn({
            let control = control.clone();
            async move { control.wait_for_exit().await }
        });
        observer.abort();
        let (first, second) = tokio::time::timeout(Duration::from_secs(4), async {
            tokio::join!(control.stop(), control.stop())
        })
        .await
        .unwrap();
        first.unwrap();
        second.unwrap();
        let code = bounded_exit(&control).await.unwrap();
        assert_ne!(code, 0);
        assert_eq!(
            session.status_basic(),
            (false, session.status_basic().1, Some(code))
        );
        assert!(
            session.generation_tx.upgrade().is_none(),
            "I/O retirement was not complete"
        );
        control.wait_for_shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lifecycle_linger_natural_exit_then_stop() {
        let session = PtySession::start(SessionConfig {
            pty: PtyConfig {
                command: vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    "printf 'LINGER-FINAL\\n'; exit 17".into(),
                ],
                cols: 80,
                rows: 24,
                cwd: None,
                env: HashMap::new(),
            },
            timeout: None,
            linger: true,
        })
        .await
        .unwrap();
        let control = session.control_handle();
        assert_eq!(bounded_exit(&control).await.unwrap(), 17);
        assert!(
            session
                .screen(&ScreenOpts::default())
                .await
                .lines
                .join("\n")
                .contains("LINGER-FINAL")
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(100), control.wait_for_shutdown())
                .await
                .is_err()
        );
        control.stop().await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), control.wait_for_shutdown())
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lifecycle_natural_linger_keeps_original_timeout() {
        let session = PtySession::start(SessionConfig {
            pty: PtyConfig {
                command: vec!["/bin/sh".into(), "-c".into(), "exit 29".into()],
                cols: 80,
                rows: 24,
                cwd: None,
                env: HashMap::new(),
            },
            timeout: Some(1),
            linger: true,
        })
        .await
        .unwrap();
        let control = session.control_handle();
        assert_eq!(bounded_exit(&control).await.unwrap(), 29);
        tokio::time::timeout(Duration::from_secs(2), control.wait_for_shutdown())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.status_basic().2, Some(29));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lifecycle_timeout_zero_overrides_linger() {
        let session = PtySession::start(SessionConfig {
            pty: PtyConfig {
                command: vec!["/bin/sleep".into(), "30".into()],
                cols: 80,
                rows: 24,
                cwd: None,
                env: HashMap::new(),
            },
            timeout: Some(0),
            linger: true,
        })
        .await
        .unwrap();
        let control = session.control_handle();
        assert_ne!(bounded_exit(&control).await.unwrap(), 0);
        control.wait_for_shutdown().await.unwrap();
        assert!(!session.is_alive());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lifecycle_timeout_keeps_start_deadline() {
        let began = Instant::now();
        let session = PtySession::start(SessionConfig {
            pty: PtyConfig {
                command: vec!["/bin/sleep".into(), "30".into()],
                cols: 80,
                rows: 24,
                cwd: None,
                env: HashMap::new(),
            },
            timeout: Some(1),
            linger: true,
        })
        .await
        .unwrap();
        for _ in 0..6 {
            let control = session.control_handle();
            assert!(
                tokio::time::timeout(Duration::from_millis(100), control.wait_for_exit())
                    .await
                    .is_err()
            );
            session.status().await;
        }
        let control = session.control_handle();
        assert_ne!(bounded_exit(&control).await.unwrap(), 0);
        control.wait_for_shutdown().await.unwrap();
        assert!(began.elapsed() < Duration::from_secs(3));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lifecycle_descendant_holds_slave_has_bounded_io_retirement() {
        // A real fork descendant inherits the slave and ignores HUP. It is an
        // owned, bounded fixture and exits on its own after three seconds.
        let session = start_session(vec![
            "/usr/bin/python3".into(),
            "-c".into(),
            "import os,signal,time\nr,w=os.pipe()\npid=os.fork()\nif pid == 0:\n os.close(r); signal.signal(signal.SIGHUP,signal.SIG_IGN); os.fstat(0); print('SLAVE-HOLDER',flush=True); os.write(w,b'1'); os.close(w); time.sleep(3); os._exit(0)\nos.close(w); os.read(r,1); os.close(r); os._exit(19)".into(),
        ]).await;
        let control = session.control_handle();
        let began = Instant::now();
        match bounded_exit(&control).await {
            Ok(code) => {
                assert_eq!(code, 19);
                control.stop().await.unwrap();
                control.wait_for_shutdown().await.unwrap();
            }
            Err(error) => {
                assert!(format!("{error:#}").contains("drain exceeded"), "{error:#}");
                assert!(control.stop().await.is_err());
                assert!(control.wait_for_shutdown().await.is_err());
            }
        }
        assert!(began.elapsed() < Duration::from_secs(3));
        assert_eq!(session.status_basic().2, Some(19));
        // Independently of bounded failure acknowledgement, the original owner
        // must join its actual reader/writer, rather than drop their handles.
        let mut generation = session.generation_rx.clone();
        tokio::time::timeout(Duration::from_secs(1), async {
            while generation.changed().await.is_ok() {}
        })
        .await
        .expect("reader/writer were not joined after cancellation");
        assert!(
            session
                .screen(&ScreenOpts::default())
                .await
                .lines
                .join("\n")
                .contains("SLAVE-HOLDER")
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lifecycle_drain_failure_retains_fifo_owner_until_delivery() {
        let session = start_session(vec![
            "/bin/sh".into(),
            "-c".into(),
            "sleep 0.2; printf 'RETAINED-FINAL\\n'; exit 11".into(),
        ])
        .await;
        let terminal_guard = session.terminal.lock().await;
        let control = session.control_handle();
        let error = bounded_exit(&control).await.unwrap_err();
        assert!(format!("{error:#}").contains("drain exceeded"), "{error:#}");
        assert_eq!(session.status_basic().2, Some(11));
        assert!(
            session.generation_tx.upgrade().is_some(),
            "owner abandoned pending final output"
        );
        assert!(control.stop().await.is_err());
        assert!(control.wait_for_shutdown().await.is_err());
        drop(terminal_guard);
        let mut generation = session.generation_rx.clone();
        tokio::time::timeout(Duration::from_secs(1), async {
            while generation.changed().await.is_ok() {}
        })
        .await
        .expect("owner did not deliver retained FIFO and join I/O");
        assert!(
            session
                .screen(&ScreenOpts::default())
                .await
                .lines
                .join("\n")
                .contains("RETAINED-FINAL")
        );
        assert!(
            control.wait_for_exit().await.is_err(),
            "incomplete drain was changed into success"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn lifecycle_blocked_writer_stops_on_same_owner() {
        let session = start_session(vec![
            "/bin/sh".into(),
            "-c".into(),
            "stty -echo -icanon; printf 'NO-READ\\n'; exec sleep 30".into(),
        ])
        .await;
        wait_for_screen(&session, |text| text.contains("NO-READ")).await;
        session
            .send_bytes(&vec![b'x'; 4 * 1024 * 1024])
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let control = session.control_handle();
        tokio::time::timeout(Duration::from_secs(3), control.stop())
            .await
            .unwrap()
            .unwrap();
        assert!(session.generation_tx.upgrade().is_none());
    }

    /// Start a session running `command` at 80x24.
    pub(crate) async fn start_session(command: Vec<String>) -> PtySession {
        PtySession::start(SessionConfig {
            pty: PtyConfig {
                command,
                cols: 80,
                rows: 24,
                cwd: None,
                env: HashMap::new(),
            },
            timeout: None,
            linger: false,
        })
        .await
        .expect("failed to start session")
    }

    /// Send `keys` then wait (bounded) for the child to exit, so the shell and
    /// reader thread don't outlive the test.
    pub(crate) async fn shutdown(session: &PtySession, keys: &str) {
        session.send_keys(keys).await.unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while session.is_alive() {
            assert!(
                Instant::now() < deadline,
                "child did not exit after {keys:?}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Poll the screen until `pred` matches, panicking with the last screen on timeout.
    async fn wait_for_screen(session: &PtySession, pred: impl Fn(&str) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let screen = session.screen(&ScreenOpts::default()).await;
            let text = screen.lines.join("\n");
            if pred(&text) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for screen; last screen:\n{text}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// The child must observe a resize on its own TTY (TIOCSWINSZ), not just the emulator grid.
    #[tokio::test(flavor = "multi_thread")]
    async fn resize_reaches_child_process() {
        let session = start_session(vec!["/bin/sh".into()]).await;

        // `stty size` prints "rows cols" as reported by the child's TTY.
        session.send_keys("stty size<CR>").await.unwrap();
        wait_for_screen(&session, |s| s.contains("24 80")).await;

        session.resize(120, 40).await.unwrap();
        let screen = session.screen(&ScreenOpts::default()).await;
        assert_eq!((screen.size.cols, screen.size.rows), (120, 40));

        session.send_keys("stty size<CR>").await.unwrap();
        wait_for_screen(&session, |s| s.contains("40 120")).await;

        shutdown(&session, "exit<CR>").await;
    }

    /// resize() must bump the wait generation: a resize reflows/clips the grid without
    /// any PTY output, so in-flight waits would otherwise sleep through the change.
    #[tokio::test(flavor = "multi_thread")]
    async fn resize_bumps_wait_generation() {
        // A child that never writes: the resize is the only possible generation bump.
        let session = start_session(vec!["/bin/sleep".into(), "30".into()]).await;

        let mut generation_rx = session.wait_handle().generation_rx;
        let before = *generation_rx.borrow_and_update();

        session.resize(120, 40).await.unwrap();

        // The bump happens inside resize(), so it is visible as soon as resize returns.
        assert!(
            *generation_rx.borrow() > before,
            "resize did not bump the wait generation (still {before})"
        );

        shutdown(&session, "<C-c>").await;
    }

    /// wait_for(Text) returns as soon as delayed output lands, not at the timeout.
    #[tokio::test(flavor = "multi_thread")]
    async fn wait_text_matches_delayed_output() {
        let session = start_session(vec![
            "/bin/sh".into(),
            "-c".into(),
            "sleep 0.3; echo READY; sleep 30".into(),
        ])
        .await;

        let outcome = session
            .wait_for(WaitCondition::Text("READY".into()), Duration::from_secs(10))
            .await
            .unwrap();
        assert!(outcome.matched, "expected match: {outcome:?}");
        assert!(outcome.diagnostics.is_none());
        // Event-driven completion: far below the 10s timeout despite the delayed echo.
        assert!(
            outcome.elapsed_ms < 5000,
            "elapsed_ms = {}",
            outcome.elapsed_ms
        );

        // Interrupt the trailing `sleep 30` so the child exits.
        shutdown(&session, "<C-c>").await;
    }

    /// A wait for absent text times out at the deadline and carries the diagnostic snapshot.
    #[tokio::test(flavor = "multi_thread")]
    async fn wait_timeout_carries_diagnostics() {
        let session = start_session(vec!["/bin/sh".into()]).await;
        session
            .send_keys("echo hello-from-the-shell<CR>")
            .await
            .unwrap();
        wait_for_screen(&session, |s| s.contains("hello-from-the-shell")).await;

        let outcome = session
            .wait_for(
                WaitCondition::Text("NEVER_APPEARS_123".into()),
                Duration::from_millis(1000),
            )
            .await
            .unwrap();
        assert!(!outcome.matched);
        // Timed out at the deadline (with scheduler tolerance), neither early nor far late.
        assert!(
            (950..4000).contains(&outcome.elapsed_ms),
            "elapsed_ms = {}",
            outcome.elapsed_ms
        );
        let diag = outcome.diagnostics.expect("timeout must carry diagnostics");
        assert!(diag.screen.contains("hello-from-the-shell"));
        assert!(diag.raw_tail.contains("hello-from-the-shell"));
        assert!(diag.generation > 0);
        assert!(diag.cursor.row >= 1);
        assert!(!diag.ended, "deadline timeout must not be flagged as ended");

        // Once the child exits the grid is final: the wait fails fast, flagged `ended`.
        shutdown(&session, "exit<CR>").await;
        let outcome = session
            .wait_for(
                WaitCondition::Text("NEVER_APPEARS_123".into()),
                Duration::from_secs(10),
            )
            .await
            .unwrap();
        assert!(!outcome.matched);
        assert!(
            outcome.elapsed_ms < 5000,
            "fail-fast took {}ms",
            outcome.elapsed_ms
        );
        assert!(outcome.diagnostics.expect("diagnostics on ended").ended);
    }

    /// wait_for(Gone) matches once the text is cleared from the screen.
    #[tokio::test(flavor = "multi_thread")]
    async fn wait_gone_matches_after_clear() {
        let session = start_session(vec!["/bin/sh".into()]).await;
        session.send_keys("echo MARKER_GONE_42<CR>").await.unwrap();
        let outcome = session
            .wait_for(
                WaitCondition::Text("MARKER_GONE_42".into()),
                Duration::from_secs(10),
            )
            .await
            .unwrap();
        assert!(outcome.matched);

        // Clear the screen and home the cursor; the marker must vanish from the grid.
        session
            .send_keys(r"printf '\033[2J\033[H'<CR>")
            .await
            .unwrap();
        let outcome = session
            .wait_for(
                WaitCondition::Gone("MARKER_GONE_42".into()),
                Duration::from_secs(10),
            )
            .await
            .unwrap();
        assert!(outcome.matched, "marker still on screen: {outcome:?}");

        shutdown(&session, "exit<CR>").await;
    }

    /// wait_for(StableMs) matches once output quiesces, never before the window elapses.
    #[tokio::test(flavor = "multi_thread")]
    async fn wait_stable_matches_after_quiesce() {
        let session = start_session(vec!["/bin/sh".into()]).await;
        session.send_keys("echo quiesce-now<CR>").await.unwrap();

        let outcome = session
            .wait_for(WaitCondition::StableMs(400), Duration::from_secs(10))
            .await
            .unwrap();
        assert!(outcome.matched, "screen never stabilized: {outcome:?}");
        // A full uninterrupted window is a lower bound on the elapsed time.
        assert!(
            outcome.elapsed_ms >= 400,
            "elapsed_ms = {}",
            outcome.elapsed_ms
        );

        shutdown(&session, "exit<CR>").await;
    }

    /// wait_for(StableMs) treats a resize as grid activity: the stability window restarts.
    #[tokio::test(flavor = "multi_thread")]
    async fn wait_stable_restarts_window_on_resize() {
        // A child that never writes: the resize is the only grid activity.
        let session = start_session(vec!["/bin/sleep".into(), "30".into()]).await;

        let wait = tokio::spawn(
            session
                .wait_handle()
                .wait_for(WaitCondition::StableMs(800), Duration::from_secs(10)),
        );
        // Land the resize inside the first stability window.
        tokio::time::sleep(Duration::from_millis(200)).await;
        session.resize(100, 30).await.unwrap();
        let resized_at = Instant::now();

        let outcome = wait.await.unwrap().unwrap();
        assert!(outcome.matched, "screen never stabilized: {outcome:?}");
        // A full window must elapse after the resize; without the restart the wait
        // matches off the original window, well under 800ms after the resize.
        assert!(
            resized_at.elapsed() >= Duration::from_millis(800),
            "stability window did not restart on resize (completed {:?} after it)",
            resized_at.elapsed()
        );

        shutdown(&session, "<C-c>").await;
    }

    /// wait_for(Regex) matches the screen text; invalid patterns error instead of waiting.
    #[tokio::test(flavor = "multi_thread")]
    async fn wait_regex_matches() {
        let session = start_session(vec!["/bin/sh".into()]).await;
        session.send_keys("echo exit code 42<CR>").await.unwrap();
        let outcome = session
            .wait_for(
                WaitCondition::Regex(r"exit code \d+".into()),
                Duration::from_secs(10),
            )
            .await
            .unwrap();
        assert!(outcome.matched);

        assert!(
            session
                .wait_for(
                    WaitCondition::Regex("(unclosed".into()),
                    Duration::from_secs(1)
                )
                .await
                .is_err()
        );

        shutdown(&session, "exit<CR>").await;
    }
}

#[cfg(all(test, windows))]
#[path = "session_windows_tests.rs"]
mod windows_tests;
