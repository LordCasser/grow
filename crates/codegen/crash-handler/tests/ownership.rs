//! PB-05: real process owners and competing recoverers in isolated directories.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

fn raw_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            name == "last-crash.bin" || name.starts_with("capture-") && name.ends_with(".bin")
        })
        .collect();
    files.sort();
    files
}

fn blob(version: &str, timestamp: u64) -> Vec<u8> {
    use crash_handler::format::{self, writer};
    let mut data = vec![0; format::MAX_FILE_SIZE];
    let mut field = [0; format::VERSION_STRING_LEN];
    field[..version.len()].copy_from_slice(version.as_bytes());
    // SAFETY: MAX_FILE_SIZE covers the header and the version field is fixed-size.
    let size = unsafe { writer::write_header(&mut data, 6, 1, 0, 4242, timestamp, 0, &field) };
    data.truncate(size);
    data
}

// A guard owns precisely the process it spawned, including assertion failures.
struct Fixture {
    child: Child,
    lines: Receiver<String>,
}

impl Fixture {
    fn spawn(dir: &Path, mode: &str, version: &str) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "--nocapture", "ownership_fixture"])
            .env("PB05_DIR", dir)
            .env("PB05_MODE", mode)
            .env("PB05_VERSION", version)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn owned fixture");
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let line = line.expect("fixture stdout");
                if let Some(start) = line.find("PB05 ")
                    && tx.send(line[start..].to_owned()).is_err()
                {
                    break;
                }
            }
        });
        let fixture = Self { child, lines };
        fixture.until("PB05 READY");
        fixture
    }

    fn until(&self, prefix: &str) -> String {
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(15))
                .expect("fixture response within 15s");
            if line.starts_with(prefix) {
                return line;
            }
        }
    }

    fn send(&mut self, command: &str) {
        writeln!(self.child.stdin.as_mut().unwrap(), "{command}").unwrap();
    }

    fn finish(&mut self) {
        self.until("PB05 DONE");
        assert!(self.child.wait().unwrap().success());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

#[test]
#[ignore]
fn ownership_fixture() {
    let Ok(dir) = std::env::var("PB05_DIR") else {
        return;
    };
    let dir = Path::new(&dir);
    // The owned fixture may intentionally crash; keep disposable evidence in
    // its temporary directory rather than producing a system core file.
    let core_limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    assert_eq!(
        unsafe { libc::setrlimit(libc::RLIMIT_CORE, &core_limit) },
        0
    );
    let mode = std::env::var("PB05_MODE").unwrap();
    if mode == "owner" || mode == "exec-owner" {
        assert!(crash_handler::install(crash_handler::CrashHandlerConfig {
            app_version: std::env::var("PB05_VERSION").unwrap(),
            crash_dir: dir.to_owned(),
        }));
    }
    println!("PB05 READY");
    std::io::stdout().flush().unwrap();
    for command in std::io::stdin().lock().lines() {
        match command.unwrap().as_str() {
            "exit" => break,
            "crash" => unsafe {
                libc::raise(libc::SIGABRT);
            },
            "recover" => {
                while let Some(report) = crash_handler::check_previous_crash(dir).expect("recover")
                {
                    println!("PB05 REPORT {}", report.app_version);
                }
                break;
            }
            "exec" => {
                use std::os::unix::process::CommandExt;
                let error = Command::new(std::env::current_exe().unwrap())
                    .args(["--ignored", "--exact", "--nocapture", "ownership_fixture"])
                    .env("PB05_MODE", "idle")
                    .exec();
                panic!("exec failed: {error}");
            }
            other => panic!("unknown command {other}"),
        }
    }
    println!("PB05 DONE");
    std::io::stdout().flush().unwrap();
}

#[test]
fn live_owners_survive_startup_and_only_retired_capture_is_recovered() {
    use std::os::unix::fs::MetadataExt;
    let dir = tempfile::tempdir().unwrap();
    let mut a = Fixture::spawn(dir.path(), "owner", "owner-A");
    let a_path = raw_files(dir.path()).pop().expect("A raw");
    let inode = std::fs::metadata(&a_path).unwrap().ino();
    // A recognizable pre-crash marker exposes truncation even while A is live.
    let marker = blob("live-marker-A", 1_700_000_000);
    std::fs::write(&a_path, &marker).unwrap();
    let mut b = Fixture::spawn(dir.path(), "owner", "owner-B");
    assert_eq!(
        std::fs::read(&a_path).unwrap(),
        marker,
        "B startup truncated live A"
    );
    assert_eq!(std::fs::metadata(&a_path).unwrap().ino(), inode);
    assert_eq!(
        raw_files(dir.path()).len(),
        2,
        "live owners need distinct captures"
    );
    assert!(
        crash_handler::check_previous_crash(dir.path())
            .unwrap()
            .is_none()
    );
    assert!(!dir.path().join("last-crash-report.txt").exists());
    assert!(!dir.path().join("history").exists());
    assert_eq!(
        std::fs::read(&a_path).unwrap(),
        marker,
        "recovery read/consumed live A"
    );
    assert_eq!(
        raw_files(dir.path()).len(),
        2,
        "live empty B must survive recovery"
    );
    a.send("crash");
    assert!(!a.child.wait().unwrap().success());
    let raw = std::fs::read(&a_path).unwrap();
    assert_eq!(
        crash_handler::format::CrashBlob::parse(&raw)
            .unwrap()
            .app_version,
        "owner-A"
    );
    let mut c = Fixture::spawn(dir.path(), "recover", "unused");
    c.send("recover");
    assert_eq!(c.until("PB05 REPORT"), "PB05 REPORT owner-A");
    c.finish();
    assert!(!a_path.exists());
    assert_eq!(
        raw_files(dir.path()).len(),
        1,
        "C may consume only retired A"
    );
    b.send("exit");
    b.finish();
    assert!(
        crash_handler::check_previous_crash(dir.path())
            .unwrap()
            .is_none()
    );
    assert!(
        raw_files(dir.path()).is_empty(),
        "inactive empty B must be cleaned"
    );
}

#[test]
fn simultaneous_recoverers_publish_distinct_same_second_histories_once() {
    let dir = tempfile::tempdir().unwrap();
    for (name, version) in [
        ("capture-AAAAAAAAAAAAAAAA.bin", "crash-A"),
        ("capture-BBBBBBBBBBBBBBBB.bin", "crash-B"),
    ] {
        std::fs::write(dir.path().join(name), blob(version, 1_700_000_000)).unwrap();
    }
    let mut a = Fixture::spawn(dir.path(), "recover", "unused");
    let mut b = Fixture::spawn(dir.path(), "recover", "unused");
    a.send("recover");
    b.send("recover");
    // Each recoverer loops until None; inspect every report before DONE.
    let mut recovered = Vec::new();
    for fixture in [&mut a, &mut b] {
        loop {
            let line = fixture.lines.recv_timeout(Duration::from_secs(15)).unwrap();
            if let Some(version) = line.strip_prefix("PB05 REPORT ") {
                recovered.push(version.to_owned());
            }
            if line.starts_with("PB05 DONE") {
                break;
            }
        }
        assert!(fixture.child.wait().unwrap().success());
    }
    recovered.sort();
    assert_eq!(
        recovered,
        ["crash-A", "crash-B"],
        "each raw may be claimed exactly once"
    );
    assert!(raw_files(dir.path()).is_empty());
    let history: Vec<_> = std::fs::read_dir(dir.path().join("history"))
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect();
    assert_eq!(history.len(), 2, "same-second histories must not overwrite");
    for version in ["crash-A", "crash-B"] {
        assert_eq!(
            history.iter().filter(|text| text.contains(version)).count(),
            1
        );
    }
}

#[test]
fn capture_lock_is_not_inherited_across_exec() {
    let dir = tempfile::tempdir().unwrap();
    let mut owner = Fixture::spawn(dir.path(), "exec-owner", "exec-owner");
    let raw = raw_files(dir.path()).pop().unwrap();
    owner.send("exec");
    owner.until("PB05 READY");
    assert!(
        crash_handler::check_previous_crash(dir.path())
            .unwrap()
            .is_none()
    );
    assert!(
        !raw.exists(),
        "exec must close capture handle and release its claim"
    );
    owner.send("exit");
    owner.finish();
}
