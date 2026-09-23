//! Shared terminal-probe primitives: startup queries and the teardown DA1
//! fence's bounded write/read.
//! XTVERSION uses only `write_query`;
//! its reply is handled by the event loop's response filter.
//!
//! Safety invariants (timed-read path):
//! - Startup reads run before crossterm's reader exists; teardown reads run
//!   only after that reader has exited. Both compete for stdin otherwise.
//! - Keystrokes typed inside the read window are consumed and dropped — no
//!   portable re-injection exists (TIOCSTI is blocked); accepted loss.

use std::io::Write;
#[cfg(unix)]
use std::time::Duration;

/// Bounds the reply buffer against terminals that stream without a terminator.
#[cfg(unix)]
pub(crate) const MAX_PROBE_RESPONSE: usize = 256;

/// Hard cap on post-deadline consumption of an in-flight reply.
#[cfg(unix)]
const LATE_REPLY_GRACE: Duration = Duration::from_millis(100);

/// Per-byte quiet window during the grace period.
#[cfg(unix)]
const LATE_REPLY_QUIET_MS: i32 = 25;

/// Write a probe query via the shared stderr lock; `false` if the TUI fd is
/// not a TTY or the write fails.
pub(crate) fn write_query(query: &[u8]) -> bool {
    use std::io::IsTerminal;

    let write_result: std::io::Result<()> = client_support::stderr::with_locked_stderr(|stderr| {
        // fd 2 is /dev/null-redirected; the TTY check must run on the
        // dup'd render fd inside the lock, not on std::io::stderr().
        if !stderr.is_terminal() {
            return Err(std::io::Error::other("TUI output is not a TTY"));
        }
        stderr.write_all(query)?;
        stderr.flush()
    });
    write_result.is_ok()
}

/// Write a teardown query through the render fd with one deadline covering
/// stderr lock acquisition and the tty write. A failed write sends no reply
/// reader into stdin.
#[cfg(unix)]
pub(crate) fn write_query_until(query: &[u8], deadline: std::time::Instant) -> bool {
    use std::io::IsTerminal;
    use std::os::unix::io::AsRawFd;

    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
    client_support::stderr::try_with_locked_stderr_for(remaining, |stderr| {
        stderr.is_terminal() && write_all_until(stderr.as_raw_fd(), query, deadline)
    })
    .unwrap_or(false)
}

#[cfg(unix)]
fn write_all_until(fd: i32, mut bytes: &[u8], deadline: std::time::Instant) -> bool {
    let original_flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if original_flags < 0 {
        return false;
    }
    if original_flags & libc::O_NONBLOCK == 0
        && unsafe { libc::fcntl(fd, libc::F_SETFL, original_flags | libc::O_NONBLOCK) } < 0
    {
        return false;
    }
    let written = (|| {
        while !bytes.is_empty() {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let mut pfd = libc::pollfd {
                fd,
                events: libc::POLLOUT,
                revents: 0,
            };
            let timeout = remaining
                .as_millis()
                .saturating_add(1)
                .min(i32::MAX as u128) as i32;
            let ready = unsafe { libc::poll(&mut pfd, 1, timeout) };
            if ready < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return false;
            }
            if ready == 0 {
                continue;
            }
            let count = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
            if count < 0 {
                if matches!(
                    std::io::Error::last_os_error().kind(),
                    std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                ) {
                    continue;
                }
                return false;
            }
            bytes = &bytes[count as usize..];
        }
        true
    })();
    // The duplicated fd normally shares the same open file description as
    // the render fd. Do not leave its nonblocking flag set for the shell.
    let restored = loop {
        if unsafe { libc::fcntl(fd, libc::F_SETFL, original_flags) } >= 0 {
            break true;
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted || std::time::Instant::now() >= deadline
        {
            tracing::warn!(%error, "could not restore tty file-status flags");
            break false;
        }
    };
    written && restored
}

/// Crossterm reads stdin when it is a TTY, otherwise the controlling TTY.
#[cfg(unix)]
pub(crate) enum TtyInput {
    Stdin,
    Controlling(std::fs::File),
}

#[cfg(unix)]
impl TtyInput {
    pub(crate) fn fd(&self) -> i32 {
        use std::os::unix::io::AsRawFd;

        match self {
            Self::Stdin => libc::STDIN_FILENO,
            Self::Controlling(file) => file.as_raw_fd(),
        }
    }
}

#[cfg(unix)]
pub(crate) fn tty_input() -> Option<TtyInput> {
    use std::io::IsTerminal;

    if std::io::stdin().is_terminal() {
        return Some(TtyInput::Stdin);
    }
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()
        .map(TtyInput::Controlling)
}

/// Read stdin until `is_terminated`, the size cap, or the deadline.
///
/// Returns `Some(buf)` whenever bytes were consumed (even partial, so a
/// half-read reply is never left for the EventStream); `None` when nothing
/// arrived or stdin errored before any byte.
#[cfg(unix)]
pub(crate) fn read_tty_reply(
    timeout: Duration,
    mut is_terminated: impl FnMut(&[u8], u8) -> bool,
) -> Option<Vec<u8>> {
    use std::os::unix::io::AsRawFd;

    let fd = std::io::stdin().as_raw_fd();
    let start = std::time::Instant::now();
    let mut buf: Vec<u8> = Vec::with_capacity(64);

    loop {
        let Some(remaining) = timeout.checked_sub(start.elapsed()) else {
            return finish_after_deadline(fd, buf, is_terminated);
        };
        let remaining_ms = remaining.as_millis().min(i32::MAX as u128) as i32;

        match poll_read_byte(fd, remaining_ms) {
            PollRead::Byte(byte) => {
                buf.push(byte);
                if buf.len() >= MAX_PROBE_RESPONSE || is_terminated(&buf, byte) {
                    return Some(buf);
                }
            }
            // Re-entry recomputes the deadline, so EINTR cannot extend it.
            PollRead::Interrupted => continue,
            PollRead::Timeout => return finish_after_deadline(fd, buf, is_terminated),
            PollRead::Error => return if buf.is_empty() { None } else { Some(buf) },
        }
    }
}

/// Deadline expiry: an in-flight reply (ESC byte seen — replies are
/// DCS/CSI/OSC, plain keystrokes aren't) is consumed until quiet so its
/// tail can't reach the EventStream as typed garbage; otherwise return
/// immediately to avoid eating keystrokes at a silent terminal.
#[cfg(unix)]
fn finish_after_deadline(
    fd: i32,
    mut buf: Vec<u8>,
    mut is_terminated: impl FnMut(&[u8], u8) -> bool,
) -> Option<Vec<u8>> {
    if buf.is_empty() {
        return None;
    }
    if !buf.contains(&0x1b) {
        return Some(buf);
    }
    let grace_start = std::time::Instant::now();
    while grace_start.elapsed() < LATE_REPLY_GRACE {
        match poll_read_byte(fd, LATE_REPLY_QUIET_MS) {
            PollRead::Byte(byte) => {
                buf.push(byte);
                if buf.len() >= MAX_PROBE_RESPONSE || is_terminated(&buf, byte) {
                    break;
                }
            }
            PollRead::Interrupted => continue,
            PollRead::Timeout | PollRead::Error => break,
        }
    }
    Some(buf)
}

#[cfg(unix)]
pub(crate) enum PollRead {
    Byte(u8),
    Interrupted,
    Timeout,
    Error,
}

/// One poll-then-read step for a single byte. Callers recompute their deadline
/// after an interrupted read rather than retrying here without a bound.
#[cfg(unix)]
pub(crate) fn poll_read_byte(fd: i32, timeout_ms: i32) -> PollRead {
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: pfd is a valid pollfd struct with a valid fd.
    let ret = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
    if ret == 0 {
        return PollRead::Timeout;
    }
    if ret < 0 {
        return if last_errno_is_eintr() {
            PollRead::Interrupted
        } else {
            PollRead::Error
        };
    }

    let mut byte = [0u8; 1];
    // SAFETY: byte is a valid buffer of length 1.
    let n = unsafe { libc::read(fd, byte.as_mut_ptr().cast(), 1) };
    if n == 1 {
        PollRead::Byte(byte[0])
    } else if n < 0 && last_errno_is_eintr() {
        PollRead::Interrupted
    } else {
        PollRead::Error
    }
}

#[cfg(unix)]
fn last_errno_is_eintr() -> bool {
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR)
}
