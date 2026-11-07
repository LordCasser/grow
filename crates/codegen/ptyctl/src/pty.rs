//! PTY wrapper using `portable-pty` for cross-platform pseudoterminal support.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

/// Configuration for spawning a PTY session.
#[derive(Debug, Clone)]
pub struct PtyConfig {
    /// Command and arguments to run.
    pub command: Vec<String>,
    /// Terminal width in columns.
    pub cols: u16,
    /// Terminal height in rows.
    pub rows: u16,
    /// Working directory.
    pub cwd: Option<PathBuf>,
    /// Additional environment variables.
    pub env: HashMap<String, String>,
}

/// Handle to a running PTY session.
pub struct PtyHandle {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
}

/// Resize and (on Windows) retirement share the one actual master handle.
#[derive(Clone)]
pub struct PtyMaster {
    master: std::sync::Arc<std::sync::Mutex<Option<Box<dyn MasterPty + Send>>>>,
}

impl PtyMaster {
    /// Resize the PTY (TIOCSWINSZ; the kernel delivers SIGWINCH to the child).
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let guard = self.master.lock().unwrap();
        guard
            .as_ref()
            .context("PTY master has retired")?
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("failed to resize PTY")
    }

    #[cfg(windows)]
    pub(crate) fn close(&self) {
        // ClosePseudoConsole can wait for output to drain on older Windows.
        // The owner calls this on its owned close worker while the reader and
        // FIFO consumer are still running; never hold the resize mutex in Drop.
        let master = self
            .master
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        drop(master);
    }
}

/// Child-process half of a dismantled [`PtyHandle`].
pub struct PtyChild {
    child: Box<dyn portable_pty::Child + Send>,
}

impl PtyChild {
    /// Observe actual child exit without discarding an observation failure.
    pub fn try_wait(&mut self) -> Result<Option<u32>> {
        #[cfg(windows)]
        {
            // portable-pty 0.9.0 reports every 259 (STILL_ACTIVE) status from
            // GetExitCodeProcess as "still running" and turns observation
            // failures into None; liveness must come from the scheduler.
            self.try_wait_windows()
        }

        #[cfg(unix)]
        {
            self.child
                .try_wait()
                .map(|status| status.map(|status| status.exit_code()))
                .context("failed to observe child exit")
        }
    }

    /// Liveness comes from WaitForSingleObject on this child's own process
    /// handle, not from GetExitCodeProcess: a genuinely exited child whose
    /// code is 259 would otherwise be reported as still running forever.
    #[cfg(windows)]
    fn try_wait_windows(&mut self) -> Result<Option<u32>> {
        use portable_pty::Child as _;
        use windows_sys::Win32::Foundation::{WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows_sys::Win32::System::Threading::WaitForSingleObject;

        let handle = self
            .child
            .as_raw_handle()
            .context("PTY child has no process handle to observe")?;
        // SAFETY: the child object owns this process handle for as long as
        // this PtyChild exists; a zero timeout only polls the signaled state.
        match unsafe { WaitForSingleObject(handle as _, 0) } {
            WAIT_TIMEOUT => Ok(None),
            // The same child reaps the real status; a genuine 259 is preserved.
            WAIT_OBJECT_0 => self.wait().map(Some),
            WAIT_FAILED => Err(anyhow::Error::new(std::io::Error::last_os_error())
                .context("failed to observe child exit")),
            status => Err(anyhow::Error::msg(format!(
                "unexpected WaitForSingleObject status {status}"
            ))),
        }
    }

    /// Get the child process ID.
    pub fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }

    /// Wait for the child to exit and return the exit code.
    pub fn wait(&mut self) -> Result<u32> {
        let status = self.child.wait().context("failed to wait for child")?;
        Ok(status.exit_code())
    }

    /// Kill the child process.
    pub fn kill(&mut self) -> Result<()> {
        self.child.kill().context("failed to kill child process")
    }
}

impl PtyHandle {
    /// Spawn a new process in a PTY.
    pub fn spawn(config: &PtyConfig) -> Result<Self> {
        let pty_system = native_pty_system();

        let pty_size = PtySize {
            rows: config.rows,
            cols: config.cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = pty_system.openpty(pty_size).context("failed to open PTY")?;

        let mut cmd = CommandBuilder::new(&config.command[0]);
        if config.command.len() > 1 {
            cmd.args(&config.command[1..]);
        }
        if let Some(ref cwd) = config.cwd {
            cmd.cwd(cwd);
        }
        for (key, value) in &config.env {
            cmd.env(key, value);
        }
        // Set TERM for proper terminal detection.
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        // dup readers and writers share the master's open-file description.
        // Set nonblocking before child spawn, and handle retries on both streams.
        #[cfg(unix)]
        {
            let fd = pair
                .master
                .as_raw_fd()
                .context("PTY master has no raw fd")?;
            // SAFETY: the actual master remains alive throughout both fcntl calls.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags == -1
                || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
            {
                return Err(std::io::Error::last_os_error())
                    .context("failed to make PTY I/O nonblocking");
            }
        }

        let reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone PTY reader")?;

        let writer = pair
            .master
            .take_writer()
            .context("failed to take PTY writer")?;

        // Acquire every fallible I/O primitive before creating the actual child.
        // Only the Session owner receives the resulting child handle.
        #[allow(clippy::disallowed_methods)]
        let child = pair
            .slave
            .spawn_command(cmd)
            .context("failed to spawn command in PTY")?;

        Ok(Self {
            master: pair.master,
            child,
            reader,
            writer,
        })
    }

    /// Dismantle into the master half (kept for resize), the child half
    /// (moved into a waiter task), and the reader/writer streams.
    pub fn into_parts(
        self,
    ) -> (
        PtyMaster,
        PtyChild,
        Box<dyn Read + Send>,
        Box<dyn Write + Send>,
    ) {
        (
            PtyMaster {
                master: std::sync::Arc::new(std::sync::Mutex::new(Some(self.master))),
            },
            PtyChild { child: self.child },
            self.reader,
            self.writer,
        )
    }
}
