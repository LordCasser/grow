//! Real CLI lifecycle checks; every process and registry belongs to this fixture.
#![cfg(unix)]

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        diagnostics::tls::install_ring_provider_once();
        let root = std::env::temp_dir().join(format!(
            "ptyctl-service-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ptyctl"));
        command
            .env("PTYCTL_SESSION_DIR", self.root.join("sessions"))
            .env("HOME", &self.root)
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command
    }

    async fn start(&self, options: &[&str], script: &str) -> (Child, u16) {
        let mut child = self
            .command()
            .args(["run", "--quiet", "--port", "0"])
            .args(options)
            .args(["--", "/bin/sh", "-c", script])
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let line = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
            .await
            .expect("CLI handshake timed out")
            .unwrap()
            .expect("CLI exited before port handshake");
        let port = line.parse().expect("quiet handshake must be a port");
        (child, port)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn natural_exit_closes_cli_service() {
    let fixture = Fixture::new();
    // The child is self-limited even when the old service hangs after exit.
    let (mut cli, port) = fixture
        .start(&[], "printf 'FINAL_SCREEN\\n'; exit 23")
        .await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(result) = cli.try_wait().unwrap() {
            assert!(
                result.success(),
                "normal service retirement must succeed: {result}"
            );
            return;
        }
        let response = client
            .get(format!("http://127.0.0.1:{port}/query/status"))
            .send()
            .await;
        if let Ok(response) = response {
            let status: serde_json::Value = response.json().await.unwrap();
            if status["alive"] == false {
                assert_eq!(status["exit_code"], 23);
                eprintln!(
                    "old-red prerequisite: quiet port={port}, directly owned child exited with code 23"
                );
                break;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "child failed to exit"
        );
        tokio::task::yield_now().await;
    }
    let result = tokio::time::timeout(Duration::from_secs(3), cli.wait()).await;
    if result.is_err() {
        cli.kill().await.unwrap();
    }
    assert!(
        result.is_ok(),
        "natural Session terminal state must close the CLI service"
    );
    assert!(
        result.unwrap().unwrap().success(),
        "child exit status is delivered through HTTP/WS, while normal server retirement succeeds"
    );
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
        .unwrap()
}

async fn status(port: u16) -> serde_json::Value {
    client()
        .get(format!("http://127.0.0.1:{port}/query/status"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn stop(port: u16) {
    let response = client()
        .post(format!("http://127.0.0.1:{port}/control/stop"))
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "stop failed: {}",
        response.text().await.unwrap()
    );
}

async fn retired(cli: &mut Child) {
    let result = tokio::time::timeout(Duration::from_secs(5), cli.wait()).await;
    if result.is_err() {
        cli.kill().await.unwrap();
    }
    let result = result.expect("CLI exceeded bounded shutdown").unwrap();
    let mut stderr = String::new();
    cli.stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .await
        .unwrap();
    assert!(result.success(), "CLI returned {result}: {stderr}");
}

async fn assert_child_exited(pid: u32) {
    let status = Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .unwrap();
    assert!(
        !status.success(),
        "this fixture's directly spawned child {pid} is still alive"
    );
}

#[tokio::test]
async fn explicit_stop_retires_actual_child_and_cli() {
    let fixture = Fixture::new();
    let (mut cli, port) = fixture
        .start(&["--linger", "--name", "owned"], "exec /bin/sleep 15")
        .await;
    let pid = status(port).await["pid"].as_u64().unwrap() as u32;
    stop(port).await;
    assert_child_exited(pid).await;
    retired(&mut cli).await;
    assert!(!fixture.root.join("sessions/owned.json").exists());
}

#[tokio::test]
async fn timeout_retires_actual_child_even_with_linger() {
    let fixture = Fixture::new();
    let (mut cli, port) = fixture
        .start(&["--linger", "--timeout", "1"], "exec /bin/sleep 15")
        .await;
    let pid = status(port).await["pid"].as_u64().unwrap() as u32;
    retired(&mut cli).await;
    assert_child_exited(pid).await;
}

// A tiny wire client avoids a new test dependency. Read the handshake bytewise
// so a coalesced first frame stays in the socket. Server frames are unmasked.
struct Ws(tokio::net::TcpStream);

impl Ws {
    async fn connect(port: u16) -> Self {
        tokio::time::timeout(Duration::from_secs(3), Self::handshake(port))
            .await
            .expect("WS handshake timed out")
    }

    async fn handshake(port: u16) -> Self {
        let socket = tokio::net::TcpSocket::new_v4().unwrap();
        socket.set_recv_buffer_size(1024).unwrap();
        let mut stream = socket.connect(([127, 0, 0, 1], port).into()).await.unwrap();
        // Base64 of sixteen zero bytes, generated to keep a visibly nonsecret key.
        let key = format!("{}==", "A".repeat(22));
        stream.write_all(format!("GET /ws HTTP/1.1\r\nHost: localhost:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n").as_bytes()).await.unwrap();
        let mut response = Vec::new();
        while !response.ends_with(b"\r\n\r\n") {
            response.push(stream.read_u8().await.unwrap());
            assert!(response.len() < 8192);
        }
        assert!(
            response.starts_with(b"HTTP/1.1 101"),
            "{}",
            String::from_utf8_lossy(&response)
        );
        Self(stream)
    }

    async fn frame(&mut self) -> (u8, Vec<u8>) {
        let opcode = self.0.read_u8().await.unwrap() & 0x0f;
        let length = self.0.read_u8().await.unwrap();
        assert_eq!(length & 0x80, 0);
        let length = match length & 0x7f {
            126 => self.0.read_u16().await.unwrap() as usize,
            127 => self.0.read_u64().await.unwrap() as usize,
            n => n as usize,
        };
        assert!(length <= 16 * 1024 * 1024);
        let mut payload = vec![0; length];
        self.0.read_exact(&mut payload).await.unwrap();
        (opcode, payload)
    }

    async fn terminal(&mut self) -> (Vec<u8>, u32) {
        tokio::time::timeout(Duration::from_secs(4), async {
            let mut output = Vec::new();
            loop {
                let (opcode, payload) = self.frame().await;
                match opcode {
                    2 => output.extend(payload),
                    1 => {
                        let event: serde_json::Value = serde_json::from_slice(&payload).unwrap();
                        assert_ne!(event["type"], "error", "{event}");
                        if event["type"] == "closed" {
                            return (output, event["exit_code"].as_u64().unwrap() as u32);
                        }
                    }
                    8 => panic!("transport closed before terminal event"),
                    _ => {}
                }
            }
        })
        .await
        .expect("WS did not publish terminal event")
    }
}

async fn release_child(port: u16) {
    let response = client()
        .post(format!("http://127.0.0.1:{port}/control/send"))
        .json(&serde_json::json!({"keys": "go<CR>"}))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
}

#[tokio::test]
async fn natural_exit_flushes_ws_output_before_actual_code() {
    let fixture = Fixture::new();
    let (mut cli, port) = fixture
        .start(&[], "read line; printf 'FINAL_WS_23\\n'; exit 23")
        .await;
    let mut ws = Ws::connect(port).await;
    release_child(port).await;
    let (output, code) = ws.terminal().await;
    assert_eq!(code, 23);
    assert!(String::from_utf8_lossy(&output).contains("FINAL_WS_23"));
    retired(&mut cli).await;
}

#[tokio::test]
async fn natural_linger_keeps_final_screen_status_then_stop_closes() {
    let fixture = Fixture::new();
    let (mut cli, port) = fixture
        .start(
            &["--linger"],
            "read line; printf 'LINGER_FINAL_31\\n'; exit 31",
        )
        .await;
    let mut ws = Ws::connect(port).await;
    release_child(port).await;
    let (output, code) = ws.terminal().await;
    assert_eq!(code, 31);
    assert!(String::from_utf8_lossy(&output).contains("LINGER_FINAL_31"));
    let status = status(port).await;
    assert_eq!(status["alive"], false);
    assert_eq!(status["exit_code"], 31);
    let screen = client()
        .get(format!("http://127.0.0.1:{port}/query/screen"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(screen.contains("LINGER_FINAL_31"), "{screen}");
    assert!(cli.try_wait().unwrap().is_none());
    stop(port).await;
    retired(&mut cli).await;
}

#[tokio::test]
async fn stop_with_open_ws_reports_the_same_real_exit() {
    let fixture = Fixture::new();
    let (mut cli, port) = fixture.start(&["--linger"], "exec /bin/sleep 15").await;
    let pid = status(port).await["pid"].as_u64().unwrap() as u32;
    let mut ws = Ws::connect(port).await;
    stop(port).await;
    let (_, code) = ws.terminal().await;
    assert_ne!(code, 0);
    assert_child_exited(pid).await;
    retired(&mut cli).await;
}

#[tokio::test]
async fn unread_ws_and_partial_http_cannot_hold_shutdown() {
    let fixture = Fixture::new();
    let (mut cli, port) = fixture
        .start(&[], "read line; head -c 262144 /dev/zero; exit 0")
        .await;
    let _unread_ws = Ws::connect(port).await;
    let mut partial = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    partial
        .write_all(b"GET /query/status HTTP/1.1\r\nHost: localhost\r\n")
        .await
        .unwrap();
    let pid = status(port).await["pid"].as_u64().unwrap() as u32;
    release_child(port).await;
    // Observe either the real child exit or a service that has already retired;
    // a successful shutdown can close the listener before this HTTP poll.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if cli.try_wait().unwrap().is_some() {
            break;
        }
        if let Ok(response) = client()
            .get(format!("http://127.0.0.1:{port}/query/status"))
            .send()
            .await
        {
            let state: serde_json::Value = response.json().await.unwrap();
            if state["alive"] == false {
                break;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "output producer did not exit"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let result = tokio::time::timeout(Duration::from_secs(3), cli.wait()).await;
    if result.is_err() {
        cli.kill().await.unwrap();
    }
    let result = result
        .expect("slow connections exceeded bounded service shutdown")
        .unwrap();
    let mut stderr = String::new();
    cli.stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .await
        .unwrap();
    assert!(
        !result.success(),
        "incomplete HTTP request must not masquerade as a drained service"
    );
    assert_child_exited(pid).await;
    assert!(
        stderr.contains("HTTP/WS service did not drain within 2 seconds"),
        "{stderr}"
    );
}

#[tokio::test]
async fn force_replaces_only_registration_and_old_retirement_keeps_new() {
    let fixture = Fixture::new();
    let (mut old, old_port) = fixture
        .start(&["--name", "shared", "--linger"], "exec /bin/sleep 15")
        .await;
    let old_pid = status(old_port).await["pid"].as_u64().unwrap() as u32;
    let (mut new, new_port) = fixture
        .start(
            &["--name", "shared", "--force", "--linger"],
            "exec /bin/sleep 15",
        )
        .await;
    assert_ne!(old_port, new_port);
    assert_eq!(status(old_port).await["alive"], true);
    assert!(old.try_wait().unwrap().is_none());
    stop(old_port).await;
    retired(&mut old).await;
    assert_child_exited(old_pid).await;
    let info: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.root.join("sessions/shared.json")).unwrap())
            .unwrap();
    assert_eq!(info["port"], new_port);
    assert_eq!(status(new_port).await["alive"], true);
    stop(new_port).await;
    retired(&mut new).await;
    assert!(!fixture.root.join("sessions/shared.json").exists());
}

async fn failed_start(fixture: &Fixture, options: &[&str], expected: &str) {
    // A leaked child would write after a delay, then exit by itself.
    // No failure requires killing an unknown process.
    let mut cli = fixture
        .command()
        .args(["run", "--quiet"])
        .args(options)
        .args([
            "--",
            "/bin/sh",
            "-c",
            "sleep 0.5; printf leaked > survived; sleep 2",
        ])
        .spawn()
        .unwrap();
    let status = tokio::time::timeout(Duration::from_secs(7), cli.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(!status.success());
    let mut stderr = String::new();
    cli.stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .await
        .unwrap();
    assert!(stderr.contains(expected), "{stderr}");
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(
        !fixture.root.join("survived").exists(),
        "startup failure left its child running"
    );
}

#[tokio::test]
async fn bind_conflict_cleans_its_owned_child() {
    let fixture = Fixture::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port().to_string();
    failed_start(&fixture, &["--port", &port], "failed to bind TCP listener").await;
}

#[tokio::test]
async fn registry_file_failure_cleans_its_owned_child() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.root.join("sessions"),
        "a file cannot be a registry directory",
    )
    .unwrap();
    failed_start(
        &fixture,
        &["--name", "broken"],
        "session registry directory",
    )
    .await;
}
