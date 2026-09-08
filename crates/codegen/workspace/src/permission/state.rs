#![allow(dead_code)] // Phase 1 internal helpers

use crate::permission::types::EditPolicy;
use paths::AbsPathBuf;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tools::util::grow_home::grow_home;

const PERMISSION_STATE_SCHEMA_VERSION: u32 = 1;
const MAX_PERMISSION_STATE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionState {
    pub(crate) schema_version: u32,
    pub edit_policy: EditPolicy,
    pub allow_bash_execute: bool,
    pub allowed_bash_commands: HashSet<String>,
    pub disallowed_bash_commands: HashSet<String>,
    /// Domains the user has approved for `web_fetch`
    /// during this session.
    pub allowed_web_fetch_domains: HashSet<String>,
    /// Exact MCP tool names (e.g. `"notion__notion-fetch"`)
    /// the user has granted "always allow" for. Lookup is exact.
    pub allowed_mcp_tools: HashSet<String>,
    /// Server components of valid qualified MCP IDs (e.g. `"notion"`)
    /// for which the user has granted "always allow" to every tool. Lookup
    /// validates and parses the complete qualified ID before matching.
    pub allowed_mcp_servers: HashSet<String>,
}

impl Default for PermissionState {
    fn default() -> Self {
        Self {
            schema_version: PERMISSION_STATE_SCHEMA_VERSION,
            edit_policy: EditPolicy::default(),
            allow_bash_execute: false,
            allowed_bash_commands: HashSet::new(),
            disallowed_bash_commands: HashSet::new(),
            allowed_web_fetch_domains: HashSet::new(),
            allowed_mcp_tools: HashSet::new(),
            allowed_mcp_servers: HashSet::new(),
        }
    }
}

fn state_dir_for_cwd(cwd: &AbsPathBuf) -> std::path::PathBuf {
    config::sessions_cwd_dir(cwd.as_str())
}

fn state_file_path(dir: &std::path::Path, client_identifier: Option<&str>) -> std::path::PathBuf {
    match client_identifier {
        Some(id) => {
            use sha2::{Digest, Sha256};
            let key = Sha256::digest(id.as_bytes());
            dir.join(format!("permission_{key:x}.toml"))
        }
        None => dir.join("permission.toml"),
    }
}

fn permission_state_admission_error() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "permission state must be an ordinary file within 1 MiB",
    )
}

fn read_permission_state_at(path: &std::path::Path) -> std::io::Result<String> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_PERMISSION_STATE_BYTES {
        return Err(permission_state_admission_error());
    }
    read_permission_state_from(file)
}

fn read_permission_state_from(reader: impl std::io::Read) -> std::io::Result<String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    reader
        .take(MAX_PERMISSION_STATE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PERMISSION_STATE_BYTES {
        return Err(permission_state_admission_error());
    }
    String::from_utf8(bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

async fn try_load_state(path: &std::path::Path) -> Option<PermissionState> {
    let source = path.to_path_buf();
    let result = tokio::task::spawn_blocking(move || read_permission_state_at(&source))
        .await
        .map_err(std::io::Error::other)
        .and_then(|result| result);
    match result {
        Ok(s) => {
            if let Ok(state) = toml::from_str::<PermissionState>(&s)
                && state.schema_version == PERMISSION_STATE_SCHEMA_VERSION
            {
                return Some(state);
            }

            tracing::warn!(
                path = %path.display(),
                "discarding permission state with an invalid or unsupported schema"
            );
            let state = PermissionState::default();
            if let Err(e) = persist_state_to_path(path, &state).await {
                tracing::warn!(?e, path = %path.display(), "failed resetting permission state");
            }
            Some(state)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            tracing::warn!(?e, path = %path.display(), "failed reading permission state");
            // Only absence permits shared-state fallback; unreadable client state
            // must not import grants from another scope.
            Some(PermissionState::default())
        }
    }
}

async fn load_state_from_dir(
    dir: &std::path::Path,
    client_identifier: Option<&str>,
) -> PermissionState {
    if let Some(id) = client_identifier {
        let per_client = state_file_path(dir, Some(id));
        if let Some(state) = try_load_state(&per_client).await {
            return state;
        }
    }
    try_load_state(&state_file_path(dir, None))
        .await
        .unwrap_or_default()
}

pub(crate) async fn load_state_from_disk(
    cwd: &AbsPathBuf,
    client_identifier: Option<&str>,
) -> PermissionState {
    load_state_from_dir(&state_dir_for_cwd(cwd), client_identifier).await
}

async fn persist_state_to_path(
    path: &std::path::Path,
    state: &PermissionState,
) -> std::io::Result<()> {
    let contents = toml::to_string_pretty(state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if contents.len() as u64 > MAX_PERMISSION_STATE_BYTES {
        return Err(permission_state_admission_error());
    }
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || config::fs_atomic::write_atomically(&path, &contents, None))
        .await
        .map_err(std::io::Error::other)?
}

async fn persist_state_to_dir(
    dir: &std::path::Path,
    state: &PermissionState,
    client_identifier: Option<&str>,
) {
    if let Err(e) = tokio::fs::create_dir_all(dir).await {
        tracing::warn!(?e, "failed creating permission state directory");
        return;
    }
    let path = state_file_path(dir, client_identifier);
    if let Err(e) = persist_state_to_path(&path, state).await {
        tracing::warn!(?e, path = %path.display(), "failed writing permission state");
    }
}

pub(crate) async fn persist_state(
    cwd: &AbsPathBuf,
    state: &PermissionState,
    client_identifier: Option<&str>,
) {
    persist_state_to_dir(&state_dir_for_cwd(cwd), state, client_identifier).await
}

pub async fn cleanup_stale_permission_state(max_age: std::time::Duration) {
    let sessions_dir = grow_home().join("sessions");
    let Ok(mut entries) = tokio::fs::read_dir(&sessions_dir).await else {
        return;
    };
    while let Ok(Some(session_entry)) = entries.next_entry().await {
        let Ok(ft) = session_entry.file_type().await else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let session_dir = session_entry.path();
        let Ok(mut files) = tokio::fs::read_dir(&session_dir).await else {
            continue;
        };
        while let Ok(Some(file_entry)) = files.next_entry().await {
            let path = file_entry.path();
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !file_name.starts_with("permission") || !file_name.ends_with(".toml") {
                continue;
            }
            if let Ok(metadata) = tokio::fs::metadata(&path).await
                && let Ok(modified) = metadata.modified()
                && let Ok(age) = modified.elapsed()
                && age > max_age
            {
                tracing::debug!(path = %path.display(), "removing stale permission state");
                let _ = tokio::fs::remove_file(&path).await;
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn permission_cache_byte_boundaries_preserve_sources() {
        let tmp = tempfile::tempdir().unwrap();
        let mut granted = PermissionState::default();
        granted.allow_bash_execute = true;
        persist_state_to_dir(tmp.path(), &granted, None).await;
        let path = state_file_path(tmp.path(), Some("bounded"));
        let mut exact = toml::to_string(&granted).unwrap();
        exact.extend(std::iter::repeat_n(
            ' ',
            MAX_PERMISSION_STATE_BYTES as usize - exact.len(),
        ));
        tokio::fs::write(&path, &exact).await.unwrap();
        assert!(
            load_state_from_dir(tmp.path(), Some("bounded"))
                .await
                .allow_bash_execute
        );
        exact.push(' ');
        tokio::fs::write(&path, &exact).await.unwrap();
        assert!(
            !load_state_from_dir(tmp.path(), Some("bounded"))
                .await
                .allow_bash_execute
        );
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), exact);
        let previous = b"previous destination";
        tokio::fs::write(&path, previous).await.unwrap();
        granted
            .allowed_bash_commands
            .insert("x".repeat(MAX_PERMISSION_STATE_BYTES as usize));
        assert!(persist_state_to_path(&path, &granted).await.is_err());
        assert_eq!(tokio::fs::read(&path).await.unwrap(), previous);
    }

    #[test]
    fn permission_stream_read_stops_at_budget() {
        let mut cursor = std::io::Cursor::new(vec![b' '; MAX_PERMISSION_STATE_BYTES as usize + 20]);
        assert!(read_permission_state_from(&mut cursor).is_err());
        assert_eq!(cursor.position(), MAX_PERMISSION_STATE_BYTES + 1);
    }

    #[cfg(unix)]
    #[test]
    fn permission_fifo_is_rejected_without_waiting_for_writer() {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::{FileTypeExt, symlink};
        let tmp = tempfile::tempdir().unwrap();
        let fifo = tmp.path().join("fifo");
        let c_path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        let source = fifo.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(read_permission_state_at(&source).is_err());
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(2))
                .expect("FIFO read blocked")
        );
        assert!(
            std::fs::symlink_metadata(fifo)
                .unwrap()
                .file_type()
                .is_fifo()
        );
        let regular = tmp.path().join("regular");
        std::fs::write(&regular, "state").unwrap();
        let link = tmp.path().join("link");
        symlink(regular, &link).unwrap();
        assert_eq!(read_permission_state_at(&link).unwrap(), "state");
    }

    // ── PermissionState serialization roundtrip tests ─────────────

    #[test]
    fn default_state_serialization() {
        let state = PermissionState::default();
        let toml_str = toml::to_string_pretty(&state).unwrap();
        let restored: PermissionState = toml::from_str(&toml_str).unwrap();
        assert!(!restored.allow_bash_execute);
        assert!(restored.allowed_bash_commands.is_empty());
        assert!(restored.disallowed_bash_commands.is_empty());
        assert_eq!(restored.schema_version, PERMISSION_STATE_SCHEMA_VERSION);
    }

    #[test]
    fn roundtrip_with_allowed_commands() {
        let mut state = PermissionState::default();
        state.allow_bash_execute = true;
        state.allowed_bash_commands.insert("cargo test".to_string());
        state
            .allowed_bash_commands
            .insert("npm run build".to_string());

        let toml_str = toml::to_string_pretty(&state).unwrap();
        let restored: PermissionState = toml::from_str(&toml_str).unwrap();

        assert!(restored.allow_bash_execute);
        assert!(restored.allowed_bash_commands.contains("cargo test"));
        assert!(restored.allowed_bash_commands.contains("npm run build"));
        assert_eq!(restored.allowed_bash_commands.len(), 2);
    }

    #[test]
    fn roundtrip_with_disallowed_commands() {
        let mut state = PermissionState::default();
        state.disallowed_bash_commands.insert("rm -rf".to_string());
        state
            .disallowed_bash_commands
            .insert("git push --force".to_string());

        let toml_str = toml::to_string_pretty(&state).unwrap();
        let restored: PermissionState = toml::from_str(&toml_str).unwrap();

        let denied = &restored.disallowed_bash_commands;
        assert!(denied.contains("rm -rf"));
        assert!(denied.contains("git push --force"));
        assert_eq!(denied.len(), 2);
    }

    #[test]
    fn roundtrip_with_both_allowed_and_disallowed() {
        // Simulate a real scenario: some commands explicitly allowed,
        // others explicitly denied.
        let mut state = PermissionState::default();
        state.allow_bash_execute = false;
        state.allowed_bash_commands.insert("cargo test".to_string());
        state.allowed_bash_commands.insert("git status".to_string());
        state
            .disallowed_bash_commands
            .insert("rm -rf /".to_string());
        state.disallowed_bash_commands.insert("curl".to_string());

        let toml_str = toml::to_string_pretty(&state).unwrap();
        let restored: PermissionState = toml::from_str(&toml_str).unwrap();

        assert!(!restored.allow_bash_execute);
        assert_eq!(restored.allowed_bash_commands.len(), 2);
        assert_eq!(restored.disallowed_bash_commands.len(), 2);
        assert!(restored.allowed_bash_commands.contains("cargo test"));
        assert!(restored.disallowed_bash_commands.contains("curl"));
    }

    #[test]
    fn edit_policy_reject_roundtrip() {
        let mut state = PermissionState::default();
        state.edit_policy = EditPolicy::Reject;

        let toml_str = toml::to_string_pretty(&state).unwrap();
        let restored: PermissionState = toml::from_str(&toml_str).unwrap();
        assert_eq!(restored.edit_policy, EditPolicy::Reject);
    }

    #[test]
    fn partial_state_is_rejected() {
        let toml_str = r#"allow_bash_execute = false"#;
        assert!(toml::from_str::<PermissionState>(toml_str).is_err());
    }

    #[test]
    fn empty_state_is_rejected() {
        assert!(toml::from_str::<PermissionState>("").is_err());
    }

    #[test]
    fn roundtrip_with_allowed_web_fetch_domains() {
        let mut state = PermissionState::default();
        state
            .allowed_web_fetch_domains
            .insert("stackoverflow.com".to_string());
        state
            .allowed_web_fetch_domains
            .insert("custom.example.com".to_string());

        let toml_str = toml::to_string_pretty(&state).unwrap();
        let restored: PermissionState = toml::from_str(&toml_str).unwrap();

        assert_eq!(restored.allowed_web_fetch_domains.len(), 2);
        assert!(
            restored
                .allowed_web_fetch_domains
                .contains("stackoverflow.com")
        );
        assert!(
            restored
                .allowed_web_fetch_domains
                .contains("custom.example.com")
        );
    }

    #[test]
    fn roundtrip_with_allowed_mcp_tools() {
        let mut state = PermissionState::default();
        state
            .allowed_mcp_tools
            .insert("notion__notion-fetch".to_string());
        state
            .allowed_mcp_tools
            .insert("linear__list_issues".to_string());

        let toml_str = toml::to_string_pretty(&state).unwrap();
        let restored: PermissionState = toml::from_str(&toml_str).unwrap();

        assert_eq!(restored.allowed_mcp_tools.len(), 2);
        assert!(restored.allowed_mcp_tools.contains("notion__notion-fetch"));
        assert!(restored.allowed_mcp_tools.contains("linear__list_issues"));
        assert!(restored.allowed_mcp_servers.is_empty());
    }

    #[test]
    fn roundtrip_with_allowed_mcp_servers() {
        let mut state = PermissionState::default();
        state.allowed_mcp_servers.insert("slack".to_string());
        state.allowed_mcp_servers.insert("linear".to_string());

        let toml_str = toml::to_string_pretty(&state).unwrap();
        let restored: PermissionState = toml::from_str(&toml_str).unwrap();

        assert_eq!(restored.allowed_mcp_servers.len(), 2);
        assert!(restored.allowed_mcp_servers.contains("slack"));
        assert!(restored.allowed_mcp_servers.contains("linear"));
        assert!(restored.allowed_mcp_tools.is_empty());
    }

    #[test]
    fn roundtrip_with_both_mcp_sets() {
        let mut state = PermissionState::default();
        state.allowed_mcp_tools.insert("notion__fetch".to_string());
        state.allowed_mcp_servers.insert("linear".to_string());

        let toml_str = toml::to_string_pretty(&state).unwrap();
        let restored: PermissionState = toml::from_str(&toml_str).unwrap();

        assert_eq!(restored.allowed_mcp_tools.len(), 1);
        assert_eq!(restored.allowed_mcp_servers.len(), 1);
        assert!(restored.allowed_mcp_tools.contains("notion__fetch"));
        assert!(restored.allowed_mcp_servers.contains("linear"));
    }

    #[test]
    fn unknown_state_fields_are_rejected() {
        let mut value = toml::Value::try_from(PermissionState::default()).unwrap();
        value
            .as_table_mut()
            .unwrap()
            .insert("unknown_field".into(), toml::Value::Boolean(true));
        assert!(toml::from_str::<PermissionState>(&value.to_string()).is_err());
    }

    // ── Disk persistence roundtrip tests ─────────────────────────

    #[tokio::test]
    async fn incompatible_state_is_reset_to_current_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let path = state_file_path(tmp.path(), None);
        tokio::fs::write(&path, "allow_bash_execute = true\n")
            .await
            .unwrap();

        let loaded = load_state_from_dir(tmp.path(), None).await;
        assert_eq!(loaded.schema_version, PERMISSION_STATE_SCHEMA_VERSION);
        assert!(!loaded.allow_bash_execute);
        let rewritten: PermissionState =
            toml::from_str(&tokio::fs::read_to_string(path).await.unwrap()).unwrap();
        assert_eq!(rewritten.schema_version, PERMISSION_STATE_SCHEMA_VERSION);
        assert!(!rewritten.allow_bash_execute);
    }

    #[tokio::test]
    async fn persist_and_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = PermissionState::default();
        state.allow_bash_execute = true;
        state
            .allowed_bash_commands
            .insert("cargo build".to_string());
        state.disallowed_bash_commands.insert("rm -rf".to_string());

        persist_state_to_dir(tmp.path(), &state, None).await;
        let restored = load_state_from_dir(tmp.path(), None).await;
        assert!(restored.allow_bash_execute);
        assert!(restored.allowed_bash_commands.contains("cargo build"));
        assert!(restored.disallowed_bash_commands.contains("rm -rf"));
    }

    #[tokio::test]
    async fn load_missing_file_returns_default() {
        // Simulates load_state_from_disk behavior for a missing file.
        let path = std::path::Path::new("/nonexistent/permission.toml");
        let result = tokio::fs::read_to_string(path).await;
        match result {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let state = PermissionState::default();
                assert!(!state.allow_bash_execute);
            }
            _ => panic!("expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn load_corrupt_file_returns_default() {
        // Simulates load_state_from_disk behavior for corrupt TOML.
        let corrupt = "this is not valid toml {{{{";
        let state: PermissionState = toml::from_str(corrupt).unwrap_or_default();
        assert!(!state.allow_bash_execute);
        assert!(state.allowed_bash_commands.is_empty());
    }

    // ── Per-client state file path tests ──────────────────────────

    #[test]
    fn state_file_path_without_client_id() {
        let tmp = tempfile::tempdir().unwrap();
        let path = state_file_path(tmp.path(), None);
        assert_eq!(path.file_name().unwrap(), "permission.toml");
    }

    #[test]
    fn client_cache_keys_are_fixed_length_and_distinct() {
        let tmp = tempfile::tempdir().unwrap();
        let long = "客户".repeat(1000);
        let ids = [
            "foo/bar",
            "foo\\bar",
            "foo_bar",
            "Foo_bar",
            "",
            "../",
            "has\0null",
            &long,
        ];
        let mut paths = HashSet::new();
        for id in ids {
            let path = state_file_path(tmp.path(), Some(id));
            assert_eq!(path.parent(), Some(tmp.path()));
            assert_eq!(path.file_name().unwrap().len(), 80);
            assert_ne!(path, state_file_path(tmp.path(), None));
            assert!(paths.insert(path));
        }
        assert_eq!(
            state_file_path(tmp.path(), Some("")).file_name().unwrap(),
            "permission_e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855.toml"
        );
    }

    #[tokio::test]
    async fn previously_colliding_clients_keep_distinct_grants() {
        let tmp = tempfile::tempdir().unwrap();
        let ids = ["foo/bar", "foo\\bar", "foo_bar"];
        for id in ids {
            let mut state = PermissionState::default();
            state.allowed_bash_commands.insert(id.to_string());
            persist_state_to_dir(tmp.path(), &state, Some(id)).await;
        }
        for id in ids {
            let state = load_state_from_dir(tmp.path(), Some(id)).await;
            assert_eq!(state.allowed_bash_commands, HashSet::from([id.to_string()]));
        }
        let mut legacy = PermissionState::default();
        legacy.allow_bash_execute = true;
        persist_state_to_path(&tmp.path().join("permission_old_client.toml"), &legacy)
            .await
            .unwrap();
        assert!(
            !load_state_from_dir(tmp.path(), Some("old/client"))
                .await
                .allow_bash_execute
        );
    }

    #[tokio::test]
    async fn try_load_state_missing_returns_none() {
        let result = try_load_state(std::path::Path::new("/nonexistent/permission.toml")).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn try_load_state_valid_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("permission.toml");
        let mut expected = PermissionState::default();
        expected.allow_bash_execute = true;
        tokio::fs::write(&path, toml::to_string_pretty(&expected).unwrap())
            .await
            .unwrap();
        let state = try_load_state(&path).await.unwrap();
        assert!(state.allow_bash_execute);
        assert_eq!(state.schema_version, PERMISSION_STATE_SCHEMA_VERSION);
    }

    #[tokio::test]
    async fn per_client_persist_and_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        let mut state = PermissionState::default();
        state.allow_bash_execute = true;
        state.allowed_bash_commands.insert("cargo test".to_string());

        persist_state_to_dir(dir, &state, Some("client_a")).await;

        let loaded = load_state_from_dir(dir, Some("client_a")).await;
        assert!(loaded.allow_bash_execute);
        assert!(loaded.allowed_bash_commands.contains("cargo test"));
    }

    #[tokio::test]
    async fn client_read_errors_do_not_inherit_shared_grants() {
        let tmp = tempfile::tempdir().unwrap();
        let mut shared = PermissionState::default();
        shared.allow_bash_execute = true;
        shared.allowed_bash_commands.insert("shared-command".into());
        shared
            .allowed_web_fetch_domains
            .insert("example.com".into());
        shared.allowed_mcp_servers.insert("shared-server".into());
        persist_state_to_dir(tmp.path(), &shared, None).await;
        let invalid = state_file_path(tmp.path(), Some("invalid"));
        tokio::fs::write(&invalid, [0xff, 0xfe]).await.unwrap();
        let directory = state_file_path(tmp.path(), Some("directory"));
        tokio::fs::create_dir(&directory).await.unwrap();
        tokio::fs::write(directory.join("keep"), "untouched")
            .await
            .unwrap();
        for id in ["invalid", "directory"] {
            let loaded = load_state_from_dir(tmp.path(), Some(id)).await;
            assert_eq!(
                serde_json::to_value(loaded).unwrap(),
                serde_json::to_value(PermissionState::default()).unwrap(),
                "{id}"
            );
        }
        assert_eq!(tokio::fs::read(invalid).await.unwrap(), [0xff, 0xfe]);
        assert_eq!(
            tokio::fs::read(directory.join("keep")).await.unwrap(),
            b"untouched"
        );
        assert!(
            load_state_from_dir(tmp.path(), None)
                .await
                .allow_bash_execute
        );
    }

    #[tokio::test]
    async fn per_client_load_falls_back_to_shared() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        let mut shared_state = PermissionState::default();
        shared_state.allow_bash_execute = true;
        shared_state
            .allowed_bash_commands
            .insert("cargo test".to_string());
        persist_state_to_dir(dir, &shared_state, None).await;

        let loaded = load_state_from_dir(dir, Some("new_client")).await;
        assert!(loaded.allow_bash_execute);
        assert!(loaded.allowed_bash_commands.contains("cargo test"));
    }

    #[tokio::test]
    async fn per_client_file_takes_priority_over_shared() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        let mut shared_state = PermissionState::default();
        shared_state.allow_bash_execute = true;
        persist_state_to_dir(dir, &shared_state, None).await;

        let mut client_state = PermissionState::default();
        client_state.allow_bash_execute = false;
        client_state
            .allowed_bash_commands
            .insert("npm test".to_string());
        persist_state_to_dir(dir, &client_state, Some("my-client")).await;

        let loaded = load_state_from_dir(dir, Some("my-client")).await;
        assert!(!loaded.allow_bash_execute);
        assert!(loaded.allowed_bash_commands.contains("npm test"));

        let shared_loaded = load_state_from_dir(dir, None).await;
        assert!(shared_loaded.allow_bash_execute);
    }

    #[tokio::test]
    async fn load_none_client_returns_default_when_no_file() {
        let tmp = tempfile::tempdir().unwrap();
        let loaded = load_state_from_dir(tmp.path(), None).await;
        assert!(!loaded.allow_bash_execute);
        assert!(loaded.allowed_bash_commands.is_empty());
    }

    #[tokio::test]
    async fn per_client_isolation_between_clients() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();

        let mut state_a = PermissionState::default();
        state_a
            .allowed_bash_commands
            .insert("cargo test".to_string());
        persist_state_to_dir(dir, &state_a, Some("client_a")).await;

        let mut state_b = PermissionState::default();
        state_b.allowed_bash_commands.insert("npm test".to_string());
        persist_state_to_dir(dir, &state_b, Some("client_b")).await;

        let loaded_a = load_state_from_dir(dir, Some("client_a")).await;
        assert!(loaded_a.allowed_bash_commands.contains("cargo test"));
        assert!(!loaded_a.allowed_bash_commands.contains("npm test"));

        let loaded_b = load_state_from_dir(dir, Some("client_b")).await;
        assert!(loaded_b.allowed_bash_commands.contains("npm test"));
        assert!(!loaded_b.allowed_bash_commands.contains("cargo test"));
    }
}
