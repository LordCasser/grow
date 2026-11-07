//! Named session registry stored at ~/.local/state/ptyctl/sessions/.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Information about a registered session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub port: u16,
    pub pid: Option<u32>,
    pub command: Vec<String>,
    pub cwd: String,
    pub started_at: String,
}

/// Get the session registry directory.
fn registry_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("PTYCTL_SESSION_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let state_dir = dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .context("cannot determine state directory")?;
    Ok(state_dir.join("ptyctl").join("sessions"))
}

/// Register a named session.
pub fn register_session(name: &str, info: &SessionInfo) -> Result<()> {
    let dir = registry_dir()?;
    fs::create_dir_all(&dir).context("failed to create session registry directory")?;

    let _lock = registry_lock(&dir)?;
    register_in(&dir, name, info)
}

// Keep this inode in place: removing the lock file would split concurrent users
// between different locks. Each transaction opens an independent owned handle.
fn registry_lock(dir: &Path) -> Result<fs::File> {
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join(".registry.lock"))
        .context("failed to open session registry lock")?;
    lock.lock().context("failed to lock session registry")?;
    Ok(lock)
}

fn register_in(dir: &Path, name: &str, info: &SessionInfo) -> Result<()> {
    let path = dir.join(format!("{name}.json"));
    let json = serde_json::to_string_pretty(info)?;

    // Atomic write: write to temp file, then rename.
    let tmp = dir.join(format!(".{name}.json.tmp"));
    fs::write(&tmp, &json).context("failed to write session file")?;
    fs::rename(&tmp, &path).context("failed to rename session file")?;

    Ok(())
}

/// Look up a named session.
pub fn lookup_session(name: &str) -> Result<SessionInfo> {
    let dir = registry_dir()?;
    let path = dir.join(format!("{name}.json"));

    if !path.exists() {
        bail!("session '{name}' not found");
    }

    let json = fs::read_to_string(&path).context("failed to read session file")?;
    let info: SessionInfo = serde_json::from_str(&json).context("failed to parse session file")?;

    Ok(info)
}

/// Remove only this registration generation, atomically with respect to writers.
pub fn unregister_session_if_current(name: &str, expected: &SessionInfo) -> Result<()> {
    let dir = registry_dir()?;
    let _lock = registry_lock(&dir)?;
    unregister_in(&dir, name, expected)
}

fn unregister_in(dir: &Path, name: &str, expected: &SessionInfo) -> Result<()> {
    let path = dir.join(format!("{name}.json"));
    let json = match fs::read_to_string(&path) {
        Ok(json) => json,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("failed to read session file during cleanup"),
    };
    let current: SessionInfo =
        serde_json::from_str(&json).context("failed to parse session file during cleanup")?;
    if current.port == expected.port
        && current.started_at == expected.started_at
        && current.pid == expected.pid
    {
        fs::remove_file(&path).context("failed to remove session file")?;
    }
    Ok(())
}

/// Check whether a registered session's ptyctl server is reachable.
///
/// Probes `GET /query/status` and requires a 200 with the ptyctl status body
/// shape — a bare TCP connect (or bare 200) would misread an unrelated
/// process on a recycled port as live. Not PID-based because the recorded
/// PID is the child, which may exit while a `--linger` server is still up.
pub async fn server_alive(port: u16) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
    else {
        return false;
    };
    match client
        .get(format!("http://127.0.0.1:{port}/query/status"))
        .send()
        .await
    {
        // Require the status body shape, not just a 200, so wildcard-200 servers read dead.
        Ok(resp) if resp.status() == reqwest::StatusCode::OK => resp
            .json::<serde_json::Value>()
            .await
            .is_ok_and(|v| v.get("size").is_some()),
        _ => false,
    }
}

/// List all registered sessions.
pub fn list_sessions() -> Result<Vec<(String, SessionInfo)>> {
    let dir = registry_dir()?;
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    for entry in fs::read_dir(&dir).context("failed to read session directory")? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if name.starts_with('.') {
                continue; // skip temp files
            }
            if let Ok(json) = fs::read_to_string(&path)
                && let Ok(info) = serde_json::from_str::<SessionInfo>(&json)
            {
                sessions.push((name, info));
            }
        }
    }

    Ok(sessions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    struct Registry(PathBuf);

    impl Registry {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "ptyctl-registry-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn read(&self) -> SessionInfo {
            serde_json::from_slice(&fs::read(self.0.join("named.json")).unwrap()).unwrap()
        }
    }

    impl Drop for Registry {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn info(generation: &str) -> SessionInfo {
        SessionInfo {
            port: 12345,
            pid: Some(123),
            command: vec!["fixture".into()],
            cwd: ".".into(),
            started_at: generation.into(),
        }
    }

    #[test]
    fn cleanup_preserves_a_replacement_even_if_port_and_pid_are_reused() {
        let registry = Registry::new();
        let _lock = registry_lock(&registry.0).unwrap();
        register_in(&registry.0, "named", &info("new")).unwrap();
        unregister_in(&registry.0, "named", &info("old")).unwrap();
        assert_eq!(registry.read().started_at, "new");
        unregister_in(&registry.0, "named", &info("new")).unwrap();
        assert!(!registry.0.join("named.json").exists());
    }

    #[test]
    fn writer_waits_for_the_entire_compare_delete_transaction() {
        let registry = Registry::new();
        let lock = registry_lock(&registry.0).unwrap();
        register_in(&registry.0, "named", &info("old")).unwrap();
        let path = registry.0.clone();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (written_tx, written_rx) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let _lock = registry_lock(&path).unwrap();
            register_in(&path, "named", &info("new")).unwrap();
            written_tx.send(()).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            written_rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "writer bypassed the transaction lock"
        );
        unregister_in(&registry.0, "named", &info("old")).unwrap();
        drop(lock);
        written_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        writer.join().unwrap();
        assert_eq!(registry.read().started_at, "new");
        assert!(registry.0.join(".registry.lock").exists());
    }
}
