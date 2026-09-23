//! Drain Kitty key reports until a DA1 reply proves the terminal processed the pop.

use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PopFenceOutcome {
    Replied,
    TimedOut,
    ReadFailed,
    QueryFailed,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PopFence {
    pub outcome: PopFenceOutcome,
    pub residue_bytes: usize,
    pub elapsed: Duration,
}

/// The caller must have joined the sole input reader, completed teardown/pop,
/// and kept raw mode enabled. The query and read share one deadline.
#[cfg(unix)]
pub fn run(timeout: Duration) -> PopFence {
    use super::probe::{poll_read_byte, tty_input, write_query_until};

    let started = std::time::Instant::now();
    let Some(input) = tty_input() else {
        return PopFence {
            outcome: PopFenceOutcome::QueryFailed,
            residue_bytes: 0,
            elapsed: started.elapsed(),
        };
    };
    run_with_io(
        started,
        timeout,
        |deadline| write_query_until(b"\x1b[c", deadline),
        |wait_ms| poll_read_byte(input.fd(), wait_ms),
    )
}

#[cfg(unix)]
fn run_with_io(
    started: std::time::Instant,
    timeout: Duration,
    write_query: impl FnOnce(std::time::Instant) -> bool,
    mut read_byte: impl FnMut(i32) -> super::probe::PollRead,
) -> PopFence {
    use super::probe::PollRead;

    let deadline = started + timeout;
    if !write_query(deadline) {
        return PopFence {
            outcome: PopFenceOutcome::QueryFailed,
            residue_bytes: 0,
            elapsed: started.elapsed(),
        };
    }

    let mut tail = [0u8; 64];
    let mut tail_len = 0usize;
    let mut consumed = 0usize;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return PopFence {
                outcome: PopFenceOutcome::TimedOut,
                residue_bytes: consumed,
                elapsed: started.elapsed(),
            };
        }
        let wait_ms = remaining
            .as_millis()
            .saturating_add(1)
            .min(i32::MAX as u128) as i32;
        match read_byte(wait_ms) {
            PollRead::Byte(byte) => {
                consumed += 1;
                if tail_len == tail.len() {
                    tail.copy_within(1.., 0);
                    tail_len -= 1;
                }
                tail[tail_len] = byte;
                tail_len += 1;
                if let Some(reply_len) = da1_reply_len(&tail[..tail_len]) {
                    return PopFence {
                        outcome: PopFenceOutcome::Replied,
                        residue_bytes: consumed - reply_len,
                        elapsed: started.elapsed(),
                    };
                }
            }
            PollRead::Interrupted | PollRead::Timeout => continue,
            PollRead::Error => {
                return PopFence {
                    outcome: PopFenceOutcome::ReadFailed,
                    residue_bytes: consumed,
                    elapsed: started.elapsed(),
                };
            }
        }
    }
}

#[cfg(not(unix))]
pub fn run(_timeout: Duration) -> PopFence {
    PopFence {
        outcome: PopFenceOutcome::Unsupported,
        residue_bytes: 0,
        elapsed: Duration::ZERO,
    }
}

/// Complete DA1 at the end of a rolling tail. Supports 7-bit and 8-bit CSI.
#[cfg(any(unix, test))]
fn da1_reply_len(tail: &[u8]) -> Option<usize> {
    if *tail.last()? != b'c' {
        return None;
    }
    let mut start = tail.len() - 1;
    while start > 0 && (tail[start - 1].is_ascii_digit() || tail[start - 1] == b';') {
        start -= 1;
    }
    let prefix = &tail[..start];
    if prefix.ends_with(b"\x1b[?") {
        Some(tail.len() - start + 3)
    } else if prefix.ends_with(b"\x9b?") {
        Some(tail.len() - start + 2)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::Duration;
    use super::da1_reply_len;

    #[test]
    fn accepts_complete_da1_forms_after_key_residue() {
        for reply in [
            b"\x1b[?62;c".as_slice(),
            b"\x1b[?1;2c",
            b"\x1b[?64;1;2;6;9;15;16;17;18;21;22;28c",
            b"\x9b?62;c",
        ] {
            let mut bytes = b"text\x1b[99;5:3u".to_vec();
            bytes.extend_from_slice(reply);
            assert_eq!(da1_reply_len(&bytes), Some(reply.len()));
        }
    }

    #[test]
    fn rejects_other_and_partial_replies() {
        for bytes in [
            b"\x1b[100;5:3u".as_slice(),
            b"\x1b[>0;2500;1c",
            b"\x1b[?0u",
            b"\x1b[c",
            b"\x1b[?62;2",
            b"\x9b?62;2",
            b"\x1b[?6x",
        ] {
            assert_eq!(da1_reply_len(bytes), None);
        }
    }

    #[cfg(unix)]
    #[test]
    fn query_write_failure_never_reads_input() {
        use super::{PopFenceOutcome, run_with_io};

        let report = run_with_io(
            std::time::Instant::now(),
            Duration::from_secs(1),
            |_| false,
            |_| panic!("failed query must not read the TTY"),
        );
        assert_eq!(report.outcome, PopFenceOutcome::QueryFailed);
        assert_eq!(report.residue_bytes, 0);
    }

    #[cfg(unix)]
    #[test]
    fn eof_is_a_read_failure() {
        use super::super::probe::poll_read_byte;
        use super::{PopFenceOutcome, run_with_io};
        use std::os::fd::AsRawFd;

        let (input, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        drop(peer);

        let report = run_with_io(
            std::time::Instant::now(),
            Duration::from_secs(1),
            |_| true,
            |wait_ms| poll_read_byte(input.as_raw_fd(), wait_ms),
        );
        assert_eq!(report.outcome, PopFenceOutcome::ReadFailed);
        assert_eq!(report.residue_bytes, 0);
    }

    #[cfg(unix)]
    #[test]
    fn partial_da1_expires_at_the_deadline() {
        use super::super::probe::PollRead;
        use super::{PopFenceOutcome, run_with_io};

        let partial = b"\x1b[?62;2";
        let mut bytes = partial.iter().copied();
        let report = run_with_io(
            std::time::Instant::now(),
            Duration::from_millis(40),
            |_| true,
            |_| match bytes.next() {
                Some(byte) => PollRead::Byte(byte),
                None => {
                    std::thread::sleep(Duration::from_millis(50));
                    PollRead::Timeout
                }
            },
        );
        assert_eq!(report.outcome, PopFenceOutcome::TimedOut);
        assert_eq!(report.residue_bytes, partial.len());
    }
}
