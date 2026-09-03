use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::agent::roster::RosterActivity;

pub(crate) const SCHEMA_VERSION: u32 = 2;
#[cfg(unix)]
pub(crate) const TRANSPORT_KIND: &str = "unix";
#[cfg(windows)]
pub(crate) const TRANSPORT_KIND: &str = "windows";
pub(crate) const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
pub(crate) const LEASE_DURATION: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentStats {
    pub active: usize,
}

#[derive(Debug, Clone)]
pub struct LocalSessionSnapshot {
    pub session_id: String,
    pub canonical_cwd: String,
    pub main_agent: String,
    pub activity: RosterActivity,
    pub subagents: SubagentStats,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PeerSession {
    pub session_id: String,
    pub canonical_cwd: String,
    pub main_agent: String,
    pub activity: RosterActivity,
    pub subagents: SubagentStats,
    pub started_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PeerManifest {
    pub schema_version: u32,
    pub peer_id: String,
    pub pid: u32,
    pub incarnation: String,
    pub endpoint: PathBuf,
    pub transport: String,
    pub token: String,
    pub started_at: i64,
    pub heartbeat_at: i64,
    pub expires_at: i64,
    pub capabilities: Vec<String>,
    pub sessions: Vec<PeerSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PeerDescription {
    pub peer_id: String,
    pub incarnation: String,
    pub started_at: i64,
    pub heartbeat_at: i64,
    pub capabilities: Vec<String>,
    pub sessions: Vec<PeerSession>,
}

impl From<&PeerManifest> for PeerDescription {
    fn from(manifest: &PeerManifest) -> Self {
        Self {
            peer_id: manifest.peer_id.clone(),
            incarnation: manifest.incarnation.clone(),
            started_at: manifest.started_at,
            heartbeat_at: manifest.heartbeat_at,
            capabilities: manifest.capabilities.clone(),
            sessions: manifest.sessions.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredSession {
    pub session_id: String,
    pub canonical_cwd: String,
    pub main_agent: String,
    pub activity: RosterActivity,
    pub subagents: SubagentStats,
    pub started_at: i64,
    pub process_started_at: i64,
    pub last_heartbeat: i64,
}

pub(crate) fn coordination_dir(grow_home: &Path) -> PathBuf {
    grow_home.join("run").join("coordination")
}

pub(crate) fn peers_dir(grow_home: &Path) -> PathBuf {
    coordination_dir(grow_home).join("peers")
}

pub(crate) fn ensure_private_runtime_dirs(grow_home: &Path) -> io::Result<()> {
    let coordination = coordination_dir(grow_home);
    let peers = peers_dir(grow_home);
    std::fs::create_dir_all(grow_home.join("run"))?;
    secure_directory(&coordination)?;
    secure_directory(&peers)
}

fn secure_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
        match std::fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "coordination directory is not private",
            ));
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        crate::local_ipc::security::create_private_directory(path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Ok(())
    }
}

pub(crate) fn write_manifest(path: &Path, manifest: &PeerManifest) -> io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static WRITE_NONCE: AtomicU64 = AtomicU64::new(0);

    let bytes = serde_json::to_vec(manifest).map_err(io::Error::other)?;
    let nonce = WRITE_NONCE.fetch_add(1, Ordering::Relaxed);
    let temp = path.with_extension(format!("{}.{}.tmp", manifest.incarnation, nonce));
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temp)?
    };
    #[cfg(windows)]
    let mut file = crate::local_ipc::security::create_private_file(&temp)?;
    if let Err(error) = file.write_all(&bytes).and_then(|_| file.flush()) {
        drop(file);
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    drop(file);
    if let Err(error) = replace_file(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

/// Held for the peer's entire lifetime. Reapers must acquire this lock before
/// touching a stale manifest, so an expired heartbeat can never evict a live
/// process or race its next atomic publication. PID reuse is irrelevant.
pub(crate) fn lock_peer(manifest_path: &Path) -> io::Result<std::fs::File> {
    #[cfg(unix)]
    let lock = {
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        options.open(manifest_path.with_extension("lock"))?
    };
    #[cfg(windows)]
    let lock = {
        use std::os::windows::fs::OpenOptionsExt;
        let path = manifest_path.with_extension("lock");
        match crate::local_ipc::security::create_private_file(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .custom_flags(
                        windows::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT.0,
                    )
                    .open(path)?;
                crate::local_ipc::security::verify_private_file(&file)?;
                file
            }
            Err(error) => return Err(error),
        }
    };
    fs2::FileExt::try_lock_exclusive(&lock)?;
    Ok(lock)
}

#[cfg(not(windows))]
fn replace_file(source: &Path, target: &Path) -> io::Result<()> {
    std::fs::rename(source, target)
}

#[cfg(windows)]
fn replace_file(source: &Path, target: &Path) -> io::Result<()> {
    use windows::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    use windows::core::PCWSTR;

    let source = crate::local_ipc::security::wide_path(source)?;
    let target = crate::local_ipc::security::wide_path(target)?;
    for attempt in 0..=5 {
        let result = unsafe {
            MoveFileExW(
                PCWSTR(source.as_ptr()),
                PCWSTR(target.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        match result {
            Ok(()) => return Ok(()),
            Err(error) if matches!(error.code().0 & 0xffff, 32 | 33) && attempt < 5 => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(io::Error::from_raw_os_error(error.code().0 & 0xffff)),
        }
    }
    unreachable!("last replace attempt returns")
}

pub(crate) fn read_manifest(path: &Path) -> io::Result<PeerManifest> {
    let file = open_private_manifest(path)?;
    let mut bytes = Vec::new();
    file.take(crate::local_ipc::frame::MAX_FRAME_SIZE as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > crate::local_ipc::frame::MAX_FRAME_SIZE as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "coordination manifest exceeds size limit",
        ));
    }
    let manifest: PeerManifest = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let expected_id = path.file_stem().and_then(|name| name.to_str());
    if manifest.schema_version != SCHEMA_VERSION || expected_id != Some(&manifest.peer_id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "coordination manifest identity or schema mismatch",
        ));
    }
    Ok(manifest)
}

fn open_private_manifest(path: &Path) -> io::Result<std::fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "coordination manifest is not owner-only",
            ));
        }
        Ok(file)
    }
    #[cfg(windows)]
    {
        crate::local_ipc::security::open_private_file(path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        std::fs::File::open(path)
    }
}

pub(crate) fn read_all_manifests(grow_home: &Path) -> io::Result<Vec<PeerManifest>> {
    let mut manifests = Vec::new();
    for entry in std::fs::read_dir(peers_dir(grow_home))? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        match read_manifest(&path) {
            Ok(manifest) if manifest.transport == TRANSPORT_KIND => manifests.push(manifest),
            Ok(_) => {}
            // Disappearing peers and malformed/versioned records are not online.
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::InvalidData
                ) => {}
            // An unreadable directory or ACL failure is not an empty online set.
            Err(error) => return Err(error),
        }
    }
    Ok(manifests)
}

pub(crate) fn merge_local_sessions(
    prior: &HashMap<String, PeerSession>,
    snapshots: Vec<LocalSessionSnapshot>,
    now: i64,
) -> HashMap<String, PeerSession> {
    snapshots
        .into_iter()
        .map(|snapshot| {
            let started_at = prior
                .get(&snapshot.session_id)
                .map(|session| session.started_at)
                .unwrap_or(now);
            let session = PeerSession {
                session_id: snapshot.session_id.clone(),
                canonical_cwd: snapshot.canonical_cwd,
                main_agent: snapshot.main_agent,
                activity: snapshot.activity,
                subagents: snapshot.subagents,
                started_at,
            };
            (snapshot.session_id, session)
        })
        .collect()
}

pub(crate) fn conflicted_session_ids(manifests: &[PeerManifest]) -> HashSet<String> {
    let mut identities: HashMap<&str, (&str, &str)> = HashMap::new();
    let mut conflicts = HashSet::new();
    for manifest in manifests {
        for session in &manifest.sessions {
            let identity = (
                manifest.incarnation.as_str(),
                session.canonical_cwd.as_str(),
            );
            match identities.get(session.session_id.as_str()) {
                Some(previous) if *previous != identity => {
                    conflicts.insert(session.session_id.clone());
                }
                None => {
                    identities.insert(session.session_id.as_str(), identity);
                }
                _ => {}
            }
        }
    }
    conflicts
}

pub(crate) fn canonical_cwd(path: &Path) -> String {
    dunce::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn now_unix_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(id: &str, cwd: &str) -> LocalSessionSnapshot {
        LocalSessionSnapshot {
            session_id: id.to_owned(),
            canonical_cwd: cwd.to_owned(),
            main_agent: "Grow".to_owned(),
            activity: RosterActivity::Idle,
            subagents: SubagentStats::default(),
        }
    }

    #[test]
    fn session_start_time_survives_heartbeat_refresh() {
        let first = merge_local_sessions(&HashMap::new(), vec![snapshot("s", "/a")], 10);
        let refreshed = merge_local_sessions(&first, vec![snapshot("s", "/a")], 20);
        assert_eq!(refreshed["s"].started_at, 10);
    }

    #[test]
    fn cwd_or_incarnation_mismatch_hides_conflicted_session() {
        let make = |peer: &str, incarnation: &str, cwd: &str| PeerManifest {
            schema_version: SCHEMA_VERSION,
            peer_id: peer.to_owned(),
            pid: 1,
            incarnation: incarnation.to_owned(),
            endpoint: PathBuf::from("endpoint"),
            transport: TRANSPORT_KIND.to_owned(),
            token: "token".to_owned(),
            started_at: 1,
            heartbeat_at: 1,
            expires_at: 2,
            capabilities: Vec::new(),
            sessions: merge_local_sessions(&HashMap::new(), vec![snapshot("same", cwd)], 1)
                .into_values()
                .collect(),
        };
        let conflicts =
            conflicted_session_ids(&[make("a", "one", "/repo"), make("b", "two", "/repo")]);
        assert!(conflicts.contains("same"));
    }

    #[test]
    fn manifest_is_written_owner_only_and_atomically_replaceable() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        ensure_private_runtime_dirs(root.path()).unwrap();
        let path = peers_dir(root.path()).join("peer.json");
        let mut manifest = PeerManifest {
            schema_version: SCHEMA_VERSION,
            peer_id: "peer".to_owned(),
            pid: 1,
            incarnation: "incarnation".to_owned(),
            endpoint: PathBuf::from("endpoint"),
            transport: TRANSPORT_KIND.to_owned(),
            token: "secret".to_owned(),
            started_at: 1,
            heartbeat_at: 2,
            expires_at: 3,
            capabilities: Vec::new(),
            sessions: Vec::new(),
        };

        write_manifest(&path, &manifest).unwrap();
        manifest.heartbeat_at = 4;
        write_manifest(&path, &manifest).unwrap();

        assert_eq!(read_manifest(&path).unwrap().heartbeat_at, 4);
        #[cfg(windows)]
        crate::local_ipc::security::open_private_file(&path).unwrap();
        #[cfg(unix)]
        {
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::metadata(peers_dir(root.path()))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_long_home_supports_private_manifests_and_replacement() {
        let root = tempfile::tempdir().unwrap();
        let home = root
            .path()
            .join("long-grow-home".repeat(12))
            .join("nested".repeat(20));
        std::fs::create_dir_all(&home).unwrap();
        ensure_private_runtime_dirs(&home).unwrap();
        let path = peers_dir(&home).join("peer.json");
        let manifest = PeerManifest {
            schema_version: SCHEMA_VERSION,
            peer_id: "peer".into(),
            pid: 1,
            incarnation: "incarnation".into(),
            endpoint: PathBuf::from("pipe"),
            transport: TRANSPORT_KIND.into(),
            token: "test-token".into(),
            started_at: 0,
            heartbeat_at: 1,
            expires_at: 2,
            capabilities: vec![],
            sessions: vec![],
        };
        write_manifest(&path, &manifest).unwrap();
        let reader = crate::local_ipc::security::open_private_file(&path).unwrap();
        write_manifest(&path, &manifest).unwrap();
        assert_eq!(read_manifest(&path).unwrap().token, "test-token");
        drop(reader);
    }

    #[cfg(unix)]
    #[test]
    fn discovery_reports_unreadable_manifests_instead_of_empty_success() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        ensure_private_runtime_dirs(root.path()).unwrap();
        let path = peers_dir(root.path()).join("insecure.json");
        std::fs::write(&path, "{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            read_all_manifests(root.path()).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}
