use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::time::{Duration, Instant};

pub(super) const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
pub(super) const MAX_RECORD_BYTES: usize = 64 * 1024;
const QUEUE_RECORDS: usize = 64;
const FLUSH_DEADLINE: Duration = Duration::from_secs(2);

enum Command {
    Record(Vec<u8>),
    Flush(SyncSender<()>),
}

pub(super) struct Firehose {
    sender: SyncSender<Command>,
    dropped: Arc<AtomicU64>,
}

impl Firehose {
    pub(super) fn open(path: PathBuf) -> io::Result<Self> {
        let file = LogFile::open(path)?;
        let (sender, receiver) = mpsc::sync_channel(QUEUE_RECORDS);
        let dropped = Arc::new(AtomicU64::new(0));
        let count = dropped.clone();
        std::thread::Builder::new()
            .name("grow-debug-firehose".into())
            .spawn(move || worker(receiver, file, &count))?;
        Ok(Self { sender, dropped })
    }

    pub(super) fn submit(&self, line: Vec<u8>) {
        debug_assert!(line.len() <= MAX_RECORD_BYTES && line.last() == Some(&b'\n'));
        if self.sender.try_send(Command::Record(line)).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(super) fn flush(&self) -> bool {
        let deadline = Instant::now() + FLUSH_DEADLINE;
        let (reply, done) = mpsc::sync_channel(0);
        let mut command = Command::Flush(reply);
        loop {
            match self.sender.try_send(command) {
                Ok(()) => break,
                Err(TrySendError::Full(pending)) => {
                    command = pending;
                    if Instant::now() >= deadline {
                        return false;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(TrySendError::Disconnected(_)) => return false,
            }
        }
        done.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .is_ok()
    }
}

fn loss_line(count: u64) -> Vec<u8> {
    format!(
        "{} WARN diagnostics::debug_log: firehose dropped {count} records\n",
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
    )
    .into_bytes()
}

fn worker(receiver: mpsc::Receiver<Command>, mut file: LogFile, dropped: &AtomicU64) {
    while let Ok(command) = receiver.recv() {
        match command {
            Command::Record(line) => {
                let count = dropped.swap(0, Ordering::AcqRel);
                if count != 0 && file.append(&loss_line(count)).is_err() {
                    dropped.fetch_add(count, Ordering::AcqRel);
                }
                if file.append(&line).is_err() {
                    dropped.fetch_add(1, Ordering::AcqRel);
                }
            }
            Command::Flush(reply) => {
                let count = dropped.swap(0, Ordering::AcqRel);
                if count != 0 && file.append(&loss_line(count)).is_err() {
                    dropped.fetch_add(count, Ordering::AcqRel);
                }
                let _ = file.file.flush();
                let _ = reply.send(());
            }
        }
    }
}

type FileIdentity = (u64, u64);

#[cfg(unix)]
fn file_identity(file: &File) -> io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(unix)]
fn path_identity(path: &std::path::Path) -> Option<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(path).ok()?;
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn file_identity(file: &File) -> io::Result<FileIdentity> {
    file.metadata().map(|_| (0, 0))
}

#[cfg(not(unix))]
fn path_identity(path: &std::path::Path) -> Option<FileIdentity> {
    fs::metadata(path).ok().map(|_| (0, 0))
}

struct LogFile {
    path: PathBuf,
    file: File,
    identity: FileIdentity,
}

impl LogFile {
    fn open(path: PathBuf) -> io::Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "debug log is not a regular file",
            ));
        }
        let identity = file_identity(&file)?;
        Ok(Self {
            path,
            file,
            identity,
        })
    }

    fn append(&mut self, line: &[u8]) -> io::Result<()> {
        if line.len() > MAX_RECORD_BYTES || line.last() != Some(&b'\n') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid debug log record",
            ));
        }
        for _ in 0..2 {
            self.file.lock()?;
            if path_identity(&self.path) != Some(self.identity) {
                self.file.unlock()?;
                *self = Self::open(self.path.clone())?;
                continue;
            }
            let result = self.append_locked(line);
            let unlocked = self.file.unlock();
            return result.and(unlocked);
        }
        Err(io::Error::other("debug log path repeatedly replaced"))
    }

    fn append_locked(&mut self, line: &[u8]) -> io::Result<()> {
        if self
            .file
            .metadata()?
            .len()
            .saturating_add(line.len() as u64)
            > MAX_FILE_BYTES
        {
            self.retain_tail()?;
        }
        if self
            .file
            .metadata()?
            .len()
            .saturating_add(line.len() as u64)
            > MAX_FILE_BYTES
        {
            return Err(io::Error::other(
                "debug log cannot fit record after retention",
            ));
        }
        self.file.seek(io::SeekFrom::End(0))?;
        self.file.write_all(line)
    }

    fn retain_tail(&mut self) -> io::Result<()> {
        let len = self.file.metadata()?.len();
        let start = len.saturating_sub(MAX_FILE_BYTES / 2);
        self.file.seek(io::SeekFrom::Start(start))?;
        let mut tail = Vec::new();
        Read::by_ref(&mut self.file)
            .take(MAX_FILE_BYTES / 2)
            .read_to_end(&mut tail)?;
        let complete = if start == 0 {
            tail.as_slice()
        } else {
            tail.iter()
                .position(|byte| *byte == b'\n')
                .map_or(&[][..], |index| &tail[index + 1..])
        };
        self.file.rewind()?;
        self.file.write_all(complete)?;
        self.file.set_len(complete.len() as u64)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_file_keeps_complete_recent_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("firehose.txt");
        let mut file = LogFile::open(path.clone()).unwrap();
        let line = vec![b'x'; MAX_RECORD_BYTES - 1];
        let mut line = line;
        line.push(b'\n');
        for _ in 0..(MAX_FILE_BYTES as usize / MAX_RECORD_BYTES + 3) {
            file.append(&line).unwrap();
        }
        let bytes = fs::read(path).unwrap();
        assert!(bytes.len() as u64 <= MAX_FILE_BYTES);
        assert_eq!(bytes.len() % MAX_RECORD_BYTES, 0);
        assert_eq!(bytes.last(), Some(&b'\n'));
    }

    #[test]
    fn replaced_path_does_not_receive_writes_on_detached_inode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("firehose.txt");
        let displaced = dir.path().join("old.txt");
        let mut file = LogFile::open(path.clone()).unwrap();
        file.append(b"before\n").unwrap();
        fs::rename(&path, &displaced).unwrap();
        fs::write(&path, b"replacement\n").unwrap();
        file.append(b"after\n").unwrap();
        assert_eq!(fs::read(displaced).unwrap(), b"before\n");
        assert_eq!(fs::read(path).unwrap(), b"replacement\nafter\n");
    }

    #[test]
    fn queued_records_flush_to_one_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("firehose.txt");
        let firehose = Firehose::open(path.clone()).unwrap();
        firehose.submit(b"one\n".to_vec());
        firehose.submit(b"two\n".to_vec());
        assert!(firehose.flush());
        assert_eq!(fs::read(path).unwrap(), b"one\ntwo\n");
    }

    #[test]
    fn blocked_disk_overflows_queue_without_blocking_producers_and_reports_loss() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("firehose.txt");
        let firehose = Firehose::open(path.clone()).unwrap();
        let blocker = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        blocker.lock().unwrap();
        let started = Instant::now();
        for _ in 0..(QUEUE_RECORDS * 3) {
            firehose.submit(b"event\n".to_vec());
        }
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(firehose.dropped.load(Ordering::Relaxed) > 0);
        assert!(
            !firehose.flush(),
            "a blocked disk write cannot pass the flush barrier"
        );
        blocker.unlock().unwrap();
        assert!(firehose.flush());
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("firehose dropped"), "{text}");
    }

    #[cfg(unix)]
    #[test]
    fn concurrent_processes_keep_shared_file_bounded() {
        const CHILD_PATH: &str = "GROW_DEBUG_SHARED_FILE_TEST_PATH";
        let (path, _dir) = if let Some(path) = std::env::var_os(CHILD_PATH) {
            (PathBuf::from(path), None)
        } else {
            let dir = tempfile::tempdir().unwrap();
            (dir.path().join("firehose.txt"), Some(dir))
        };
        let child = _dir.as_ref().map(|_| {
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "debug_log::stream_firehose::tests::concurrent_processes_keep_shared_file_bounded",
                ])
                .env(CHILD_PATH, &path)
                .spawn()
                .unwrap()
        });
        let mut file = LogFile::open(path.clone()).unwrap();
        let mut line = vec![b'a'; MAX_RECORD_BYTES - 1];
        line.push(b'\n');
        for _ in 0..(MAX_FILE_BYTES as usize / MAX_RECORD_BYTES + 8) {
            file.append(&line).unwrap();
        }
        if let Some(mut child) = child {
            assert!(child.wait().unwrap().success());
            let bytes = fs::read(path).unwrap();
            assert!(bytes.len() as u64 <= MAX_FILE_BYTES);
            assert_eq!(bytes.len() % MAX_RECORD_BYTES, 0);
            assert_eq!(bytes.last(), Some(&b'\n'));
        }
    }
}
