//! Stable agent identifier.
//!
//! Extracted from `grow-shell::agent::unique_identifier` so the
//! diagnostics engine can stamp events without depending on shell internals.
//! `$GROW_HOME` is resolved through `grow-config::grow_home`.

use std::sync::OnceLock;

/// Cached agent ID - stored in memory after first load.
static AGENT_ID: OnceLock<String> = OnceLock::new();

/// Returns the agent ID, using a file-based cache to avoid expensive system calls.
///
/// On macOS, `mid::get()` calls `system_profiler` which takes ~1-3 seconds.
/// This function caches the result in `$GROW_HOME/agent_id` so subsequent calls
/// (even across process restarts) are instant file reads.
///
/// The in-memory `OnceLock` ensures we only read the file once per process.
pub fn agent_id() -> String {
    AGENT_ID.get_or_init(load_or_compute_agent_id).clone()
}

fn load_or_compute_agent_id() -> String {
    let cache_path = grow_config::grow_home().join("agent_id");

    // Try to read from cache file first (fast path)
    if let Ok(cached) = std::fs::read_to_string(&cache_path) {
        let cached = cached.trim();
        if !cached.is_empty() {
            tighten_agent_id_cache_perms(&cache_path);
            return cached.to_string();
        }
    }

    // Compute a unique machine hash:
    // - macOS: mid uses unique hardware IDs (serial, UUID, SEID).
    // - Linux: /etc/machine-id is shared across containers from the same base
    //   image, so include $HOSTNAME (container/host name) for uniqueness.
    // - Fallback: random UUIDv4 if mid or hostname are unavailable.
    let machine_hash = if cfg!(target_os = "linux") {
        match std::env::var("HOSTNAME") {
            Ok(hostname) if !hostname.is_empty() => {
                let key = format!("agent_id:{hostname}");
                mid::get(&key).unwrap_or_else(|_| uuid::Uuid::new_v4().to_string())
            }
            _ => uuid::Uuid::new_v4().to_string(),
        }
    } else {
        mid::get("agent_id").unwrap_or_else(|_| uuid::Uuid::new_v4().to_string())
    };
    let id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, machine_hash.as_bytes()).to_string();

    // Save to cache file with owner-only perms (best effort).
    let _ = write_agent_id_cache(&cache_path, &id);

    id
}

/// Write `$GROW_HOME/agent_id` as owner-read/write only (Unix 0o600) — it is a
/// stable device identifier and must not be world-readable. Atomic temp+rename,
/// so overwriting a loose-perms cache from an older build never leaves the id
/// in a world-readable file.
fn write_agent_id_cache(path: &std::path::Path, id: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    grow_config::fs_atomic::write_atomically(path, id, Some(0o600))
}

/// Best-effort 0o600 on an existing cache: tightens caches written world-readable
/// by older builds. No-op off Unix or on error (the id itself still loads).
fn tighten_agent_id_cache_perms(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &std::path::Path) -> u32 {
        std::fs::metadata(path).expect("meta").permissions().mode() & 0o777
    }

    #[test]
    fn agent_id_cache_written_owner_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("agent_id");
        write_agent_id_cache(&path, "test-agent-id-value").expect("write");
        assert_eq!(mode(&path), 0o600, "agent_id cache must be 0o600");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read").trim(),
            "test-agent-id-value"
        );
    }

    /// Overwriting an existing loose-perms cache (e.g. an old build's empty or
    /// torn write) must still land 0600 — mode-at-create alone would keep 0644.
    #[test]
    fn rewrite_over_loose_perms_cache_lands_owner_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("agent_id");
        std::fs::write(&path, "").expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        write_agent_id_cache(&path, "fresh-id").expect("rewrite");
        assert_eq!(mode(&path), 0o600, "rewrite must not inherit loose perms");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "fresh-id");
    }

    #[test]
    fn older_world_readable_cache_is_tightened() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("agent_id");
        std::fs::write(&path, "legacy-id").expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        tighten_agent_id_cache_perms(&path);
        assert_eq!(mode(&path), 0o600, "legacy cache must be tightened on read");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "legacy-id");
    }
}
