//! Shared test utilities for the pager crate.
//!
//! Compiled only in `#[cfg(test)]` builds. Import via `crate::test_util`.
/// Minimal `AgentView` for unit tests outside the dispatch/handler modules
/// (which keep their own richer factories).
pub fn make_agent_view(session_id: Option<&str>, cwd: &str) -> crate::app::agent_view::AgentView {
    use crate::app::session::{AgentId, AgentSession};
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let session = {
        let session = AgentSession::new(
            AgentId(0),
            tx,
            session_id.map(agent_client_protocol::schema::v1::SessionId::new),
            crate::acp::model_state::ModelState::default(),
            std::path::PathBuf::from(cwd),
            shell::util::config::PermissionMode::Ask,
        );
        session
    };
    crate::app::agent_view::AgentView::new(
        session,
        crate::scrollback::state::ScrollbackState::new(),
    )
}
/// RAII guard for temporarily overriding an environment variable.
///
/// Captures the original value on construction and restores it on drop.
/// Used by theme and persist tests to redirect `HOME`/`USERPROFILE` to
/// temp directories without affecting the real user config.
pub struct EnvVarGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}
impl EnvVarGuard {
    /// Override `key` to `value` (paths, URLs, flags — anything OsStr-able),
    /// returning a guard that restores the original on drop.
    pub fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let original = std::env::var_os(key);
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, original }
    }
}
impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(value) = &self.original {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}
/// Shared GROW_HOME boundary fixture for the resume-by-title startup and
/// pre-sandbox tests.
///
/// `grow_home()` is OnceLock-cached process-wide, so summaries land under the
/// *resolved* home (possibly the real `~/.grow` when another test pinned the
/// cache first); cwd-encoded dirnames are tempdir-unique, and cleanup runs on
/// drop so it survives assertion panics. Callers must hold
/// `#[serial_test::serial(GROW_HOME)]`.
pub struct GrowHomeFixture {
    _home: tempfile::TempDir,
    cwd: tempfile::TempDir,
    cleanup: Vec<std::path::PathBuf>,
}
impl Drop for GrowHomeFixture {
    fn drop(&mut self) {
        for dir in &self.cleanup {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
impl Default for GrowHomeFixture {
    fn default() -> Self {
        Self::new()
    }
}
impl GrowHomeFixture {
    pub fn new() -> Self {
        let home = tempfile::tempdir().expect("home tempdir");
        unsafe { std::env::set_var("GROW_HOME", home.path()) };
        let cwd = tempfile::tempdir().expect("cwd tempdir");
        Self {
            _home: home,
            cwd,
            cleanup: Vec::new(),
        }
    }
    /// Canonicalized so the summary cwd encoding matches what production
    /// path resolution sees (macOS tempdirs are symlinked). Tests pass this
    /// through the explicit `*_for_cwd` seams; the process cwd is never
    /// mutated.
    pub fn cwd_str(&self) -> String {
        self.cwd
            .path()
            .canonicalize()
            .expect("canonicalize cwd")
            .to_string_lossy()
            .to_string()
    }
    /// Write a minimal valid summary.json (every non-defaulted `Summary`
    /// field) for `id` under `cwd`, merging `extra` fields on top.
    pub fn write_summary(&mut self, cwd: &str, id: &str, extra: serde_json::Value) {
        let sessions_cwd_dir = Self::sessions_cwd_dir(cwd);
        if !self.cleanup.contains(&sessions_cwd_dir) {
            self.cleanup.push(sessions_cwd_dir.clone());
        }
        let dir = sessions_cwd_dir.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let mut v = serde_json::json!({
            "info": { "id": id, "cwd": cwd },
            "created_at": "2026-07-01T00:00:00Z",
            "updated_at": "2026-07-01T00:00:00Z",
            "num_messages": 1,
            "session_format_version": shell::session::persistence::SESSION_FORMAT_VERSION,
            "current_model_id": "grow-build",
        });
        if let Some(map) = extra.as_object() {
            for (k, val) in map {
                v[k.as_str()] = val.clone();
            }
        }
        std::fs::write(dir.join("summary.json"), serde_json::to_vec(&v).unwrap()).unwrap();
    }
    /// Delete a previously written session dir (concurrent-delete simulation).
    pub fn remove_session(&self, cwd: &str, id: &str) {
        let _ = std::fs::remove_dir_all(Self::sessions_cwd_dir(cwd).join(id));
    }
    fn sessions_cwd_dir(cwd: &str) -> std::path::PathBuf {
        let encoded = shell::util::grow_home::encode_cwd_dirname(cwd);
        shell::util::grow_home::grow_home()
            .join("sessions")
            .join(&encoded)
    }
}
