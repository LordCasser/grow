//! Folder-trust store ("do you trust this folder?").
//!
//! Persists per-folder trust decisions to `~/.grow/trusted_folders.toml`.
//! This is the durable backing store for the VS-Code-style folder-trust gate
//! that decides whether repo-local MCP / LSP servers (which run arbitrary
//! commands from repo-controlled config files) are allowed to spawn.
//!
//! TOML shape (schema 2):
//! ```toml
//! schema_version = 2
//!
//! [folders."/abs/repo/root"]
//! trusted = true
//! decided_at = 1780000000
//!
//! [folders."/abs/repo/root".identity.current.root]
//! platform = "unix"
//! device = 16777234
//! inode = 123456
//! ```
//!
//! Decisions match one canonical [`workspace_key`] exactly. Callers collapse a
//! cwd to its repository/source root before consulting the store, so prefix
//! inheritance is both unnecessary and unsafe. Every decision is additionally
//! bound to the filesystem entity at that key (and to the repository common
//! gitdir when present). A grow-managed checkout also binds its source repository
//! as provenance, but the source never replaces the current checkout identity or
//! key. Replacing a directory, reinitializing its repository, or creating a new
//! worktree/clone therefore cannot inherit a prior grant. The persisted file is
//! written atomically with owner-only (`0600`) permissions.
//!
//! The store is rooted at [`config::user_grow_home`] — the **Option**
//! home that resolves to `None` (rather than a cwd-relative `./.grow`) when
//! neither `$GROW_HOME` nor a home directory is set (e.g. a minimal container /
//! CI). In that no-home environment [`TrustStore::load`] yields an **empty,
//! trust-nothing** store that persists nothing, so a cloned repo can never ship
//! a `./.grow/trusted_folders.toml` that self-trusts its own checkout (fail
//! closed).

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Filename of the folder-trust store under `~/.grow/`.
pub const TRUST_FILE_NAME: &str = "trusted_folders.toml";

/// Current, intentionally incompatible trust-store schema.
const TRUST_SCHEMA_VERSION: u32 = 2;

/// Stable identity of one filesystem entity.
///
/// Pathnames are deliberately absent: the containing map already records the
/// canonical workspace key, while this value proves that the object currently
/// found at that path is the same object the user decided about.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "platform", rename_all = "snake_case")]
enum FilesystemEntityIdentity {
    #[cfg(unix)]
    Unix { device: u64, inode: u64 },
    #[cfg(windows)]
    Windows {
        volume_serial_number: u32,
        file_index: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct WorkspaceEntityIdentity {
    root: FilesystemEntityIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    git_marker: Option<FilesystemEntityIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    git_common_dir: Option<FilesystemEntityIdentity>,
}

/// Complete identity used by the durable store and the shell's decision cache.
///
/// `current` always identifies the workspace key the user was shown. Its
/// `git_marker` binds that checkout's `.git` directory/file and its
/// `git_common_dir` binds repository-shared metadata. For a grow-managed
/// checkout, `managed_source` is an additional provenance conjunct: both the
/// current checkout and source must still match, but source identity never
/// substitutes for current identity or makes two checkout paths share trust.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkspaceIdentity {
    current: WorkspaceEntityIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    managed_source: Option<WorkspaceEntityIdentity>,
}

/// A single folder's trust record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderTrust {
    /// Whether this exact workspace entity is trusted.
    pub trusted: bool,
    /// Unix timestamp (seconds) of when the decision was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<i64>,
    /// Filesystem entity that existed when the user made the decision.
    identity: WorkspaceIdentity,
}

/// On-disk document shape for `trusted_folders.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrustDocument {
    schema_version: u32,
    #[serde(default)]
    folders: BTreeMap<String, FolderTrust>,
}

impl Default for TrustDocument {
    fn default() -> Self {
        Self {
            schema_version: TRUST_SCHEMA_VERSION,
            folders: BTreeMap::new(),
        }
    }
}

/// Persisted set of trusted folders.
///
/// Construct with [`TrustStore::load`] (production) or [`TrustStore::load_from`]
/// (tests). Mutating with [`TrustStore::set_trusted`] persists to disk.
///
/// `path` is `None` only in a no-home environment (see [`TrustStore::load`]):
/// such a store holds no folders, trusts nothing, and persists nothing.
#[derive(Debug, Clone)]
pub struct TrustStore {
    doc: TrustDocument,
    /// Backing file, or `None` when no user home resolves — a trust-nothing,
    /// persist-nothing store. Never a cwd-relative path.
    path: Option<PathBuf>,
}

impl TrustStore {
    /// Load the trust store from `<user_grow_home>/trusted_folders.toml`.
    ///
    /// When no user home resolves (see the module-level fail-closed note) the
    /// path is `None` and this returns an [`Self::empty`] store. Otherwise an
    /// empty store is returned if the file is missing, unparseable, or has an
    /// incompatible schema (logged). No migration is attempted.
    pub fn load() -> Self {
        match Self::default_path() {
            Some(path) => Self::load_from(path),
            None => Self::empty(),
        }
    }

    /// Load from a custom path (for tests).
    pub fn load_from(path: PathBuf) -> Self {
        let doc = Self::read_doc(&path);
        Self {
            doc,
            path: Some(path),
        }
    }

    /// An empty store with no backing path: trusts nothing and persists
    /// nothing. Used for the no-home environment where [`Self::default_path`]
    /// resolves to `None`.
    fn empty() -> Self {
        Self {
            doc: TrustDocument::default(),
            path: None,
        }
    }

    /// Default on-disk path: `<user_grow_home>/trusted_folders.toml`, or `None`
    /// when no user home resolves.
    ///
    /// Resolves via [`config::user_grow_home`], never
    /// [`config::grow_home`], so it never falls back to a cwd-relative
    /// `./.grow` — that fallback would let an untrusted cloned repo's `.grow`
    /// masquerade as the user-global store and self-trust the checkout.
    pub fn default_path() -> Option<PathBuf> {
        Self::default_path_in(config::user_grow_home())
    }

    /// Map a resolved user-grow-home to the store path, preserving "no home" as
    /// "no path" (never synthesizing a fallback). Split from [`Self::default_path`]
    /// as a pure seam so the no-home branch is unit-testable without the
    /// process-global home cache.
    fn default_path_in(user_grow_home: Option<PathBuf>) -> Option<PathBuf> {
        Some(user_grow_home?.join(TRUST_FILE_NAME))
    }

    /// Whether this exact `workspace_key` filesystem entity is trusted.
    ///
    /// This is the SHARED folder-trust gate for repo-local MCP/LSP servers and
    /// project hooks. The query key is canonicalized here, then matched exactly;
    /// cwd-to-repository collapsing belongs solely to [`workspace_key`].
    ///
    /// Over-broad keys are ignored on read (fail closed): an empty/relative
    /// key, the filesystem root, or the user's home directory are never honored
    /// even if such a record reaches the file via hand-edit.
    /// See [`is_unsafe_trust_root`].
    pub fn is_trusted(&self, workspace_key: &Path) -> bool {
        self.is_trusted_for_cwd(workspace_key, workspace_key)
    }

    /// Whether `workspace_key` contains a trusted record for exactly
    /// `expected_identity`.
    ///
    /// The identity is supplied by the caller so one filesystem observation can
    /// be carried through decision, prompt, cache, and persistence without a
    /// path-only re-resolution changing which entity the decision applies to.
    pub fn is_trusted_identity(
        &self,
        workspace_key: &Path,
        expected_identity: &WorkspaceIdentity,
    ) -> bool {
        let workspace_key = canonicalize_or_owned(workspace_key);
        if is_unsafe_trust_root(&workspace_key) {
            return false;
        }
        let Some(key) = workspace_key.to_str() else {
            return false;
        };
        self.doc
            .folders
            .get(key)
            .is_some_and(|record| record.trusted && record.identity == *expected_identity)
    }

    /// Resolve and verify the complete identity for `cwd`, including managed
    /// source provenance when present. Identity resolution failure is untrusted.
    pub fn is_trusted_for_cwd(&self, cwd: &Path, workspace_key: &Path) -> bool {
        workspace_identity_for_cwd(cwd, workspace_key)
            .is_ok_and(|identity| self.is_trusted_identity(workspace_key, &identity))
    }

    /// Record `workspace_key` as **trusted** and persist to disk.
    ///
    /// The key is canonicalized before storage so alias spellings (symlinks,
    /// `/tmp` vs `/private/tmp`, …) still match later lookups, which canonicalize
    /// too. Keys are stored as UTF-8 strings; a non-UTF-8 path is rejected rather
    /// than lossily serialized, because two distinct byte paths could otherwise
    /// collide in the persisted map.
    ///
    /// **Over-broad roots are refused:** if the canonical key is non-absolute,
    /// the filesystem root, or the user's home directory it is rejected —
    /// nothing is recorded (neither in memory nor on disk) and `Ok(())` is
    /// returned, so `is_trusted` stays `false` for it on both read and write.
    /// A later [`Self::set_untrusted`] on the same folder flips the stored
    /// decision (the insert overwrites). See `record_decision` for the locked
    /// read-modify-write contract and the no-home `Ok(())` no-op.
    pub fn set_trusted(&mut self, workspace_key: &Path) -> io::Result<()> {
        self.record_decision(workspace_key, true)
    }

    /// Persist a grant for an identity captured at the user-decision boundary.
    ///
    /// The caller must obtain `identity` from [`workspace_identity_for_cwd`]
    /// before prompting/committing. Recording that frozen value, rather than
    /// re-reading the path here, prevents a same-path replacement between the
    /// consent check and the store write from receiving the old decision.
    pub(crate) fn set_trusted_identity(
        &mut self,
        workspace_key: &Path,
        identity: WorkspaceIdentity,
    ) -> io::Result<()> {
        self.record_decision_with_identity(workspace_key, true, identity)
    }

    pub(crate) fn set_untrusted_identity(
        &mut self,
        workspace_key: &Path,
        identity: WorkspaceIdentity,
    ) -> io::Result<()> {
        self.record_decision_with_identity(workspace_key, false, identity)
    }

    /// Record `workspace_key` as **untrusted** ("Never" / explicitly declined)
    /// and persist to disk.
    ///
    /// Mirrors [`Self::set_trusted`] exactly (canonicalization + over-broad-root
    /// refusal) but stores `trusted = false`. [`Self::is_trusted`] already
    /// returns `false` for such a record; recording it lets a consumer tell
    /// "explicitly declined" apart from "undecided" (e.g. to avoid
    /// re-prompting). A later [`Self::set_trusted`] flips it back.
    pub fn set_untrusted(&mut self, workspace_key: &Path) -> io::Result<()> {
        self.record_decision(workspace_key, false)
    }

    /// Number of recorded folders (for diagnostics / tests).
    pub fn len(&self) -> usize {
        self.doc.folders.len()
    }

    /// Whether the store has no recorded folders.
    pub fn is_empty(&self) -> bool {
        self.doc.folders.is_empty()
    }

    // ── Internal ──────────────────────────────────────────────────────

    /// Shared write path for [`Self::set_trusted`] / [`Self::set_untrusted`].
    ///
    /// Canonicalizes the key, refuses over-broad roots (non-absolute /
    /// filesystem root / home dir → `warn!` + `Ok(())` recording nothing), and
    /// is a `warn!` + `Ok(())` no-op when there is no backing path (no-home
    /// environment), so it never writes a cwd-relative file. Otherwise it
    /// performs a locked read-modify-write-commit:
    /// 1. take an exclusive advisory lock on a sidecar `*.toml.lock` file, held
    ///    for the whole critical section (released on drop), so concurrent
    ///    writers serialize;
    /// 2. re-read the current on-disk document so a peer's decisions are merged
    ///    rather than clobbered (lost-update fix);
    /// 3. insert the record and persist atomically;
    /// 4. only on success commit the new document to memory — on any
    ///    lock/persist error `self.doc` is left unchanged.
    fn record_decision(&mut self, workspace_key: &Path, trusted: bool) -> io::Result<()> {
        let canonical = canonicalize_or_owned(workspace_key);
        if is_unsafe_trust_root(&canonical) {
            tracing::warn!(
                path = %canonical.display(),
                trusted,
                "folder trust: refusing to record an over-broad root (home, filesystem root, or non-absolute path); nothing recorded"
            );
            return Ok(());
        }
        if self.path.is_none() {
            tracing::warn!(
                path = %canonical.display(),
                trusted,
                "folder trust: no user grow home resolved; trust decision not recorded"
            );
            return Ok(());
        }
        let identity = workspace_identity(&canonical)?;
        self.record_decision_with_identity(&canonical, trusted, identity)
    }

    fn record_decision_with_identity(
        &mut self,
        workspace_key: &Path,
        trusted: bool,
        identity: WorkspaceIdentity,
    ) -> io::Result<()> {
        let canonical = canonicalize_or_owned(workspace_key);
        if is_unsafe_trust_root(&canonical) {
            tracing::warn!(
                path = %canonical.display(),
                trusted,
                "folder trust: refusing to record an over-broad root (home, filesystem root, or non-absolute path); nothing recorded"
            );
            return Ok(());
        }
        // No backing file (no-home env) → record nothing, return `Ok` so
        // callers treat "no home" like "nothing to persist" (see fn doc).
        let Some(path) = self.path.as_deref() else {
            tracing::warn!(
                path = %canonical.display(),
                trusted,
                "folder trust: no user grow home resolved; trust decision not recorded"
            );
            return Ok(());
        };
        let key = canonical.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "folder trust key is not valid UTF-8",
            )
        })?;
        // The lock file lives beside the store, so ensure the dir exists first.
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "trust store path has no parent",
            )
        })?;
        std::fs::create_dir_all(parent)?;

        // Serialize cross-process writers for the whole read-modify-write so a
        // concurrent peer's records are preserved, not clobbered.
        let _lock = ExclusiveLock::acquire(&path.with_extension("toml.lock"))?;

        // Re-read the latest on-disk state (merges a peer's concurrent writes).
        let mut doc = Self::read_doc(path);
        doc.folders.insert(
            key.to_owned(),
            FolderTrust {
                trusted,
                decided_at: now_unix(),
                identity,
            },
        );

        // Commit to memory only after a successful durable write, so a failure
        // leaves the in-memory store unchanged.
        Self::persist_doc(path, &doc)?;
        self.doc = doc;
        Ok(())
    }

    fn read_doc(path: &Path) -> TrustDocument {
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return TrustDocument::default(),
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "folder trust: failed to read trust store; treating as empty"
                );
                return TrustDocument::default();
            }
        };
        let doc: TrustDocument = match toml::from_str(&contents) {
            Ok(doc) => doc,
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "folder trust: failed to parse trust store; treating as empty"
                );
                return TrustDocument::default();
            }
        };
        if doc.schema_version != TRUST_SCHEMA_VERSION {
            tracing::warn!(
                path = %path.display(),
                found = doc.schema_version,
                expected = TRUST_SCHEMA_VERSION,
                "folder trust: incompatible trust-store schema; treating as empty"
            );
            return TrustDocument::default();
        }
        doc
    }

    /// Write `doc` to `path` atomically (unique temp + fsync + rename) with
    /// owner-only (`0600`) permissions.
    ///
    /// Uses a unique temp file in the destination directory so concurrent
    /// writers never share a temp path, fsyncs it for crash durability, then
    /// renames it over the destination. `tempfile::NamedTempFile` creates the
    /// temp with `O_EXCL` and `0600` permissions on Unix, and `persist`
    /// performs an atomic replace (including over an existing destination on
    /// Windows).
    fn persist_doc(path: &Path, doc: &TrustDocument) -> io::Result<()> {
        use std::io::Write;

        let parent = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "trust store path has no parent",
            )
        })?;
        std::fs::create_dir_all(parent)?;

        let body = toml::to_string_pretty(doc)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        // Unique temp in the same directory (atomic rename requires same FS).
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        tmp.write_all(body.as_bytes())?;
        // Durably flush to disk before publishing so a crash can't leave a
        // zero-length or stale store behind. (`File::flush` is a no-op for
        // durability; `sync_all` is what guarantees the bytes hit disk.)
        tmp.as_file().sync_all()?;
        // Atomic publish.
        tmp.persist(path).map_err(|e| e.error)?;
        Ok(())
    }
}

/// Resolve the stable filesystem identity for an already-derived workspace key.
///
/// Callers must derive the key with [`workspace_key`] first. A plain directory
/// binds only its root. A repository additionally binds its `.git` marker and
/// common gitdir. Any metadata or repository-identity read error fails closed:
/// grants are not recorded and existing grants are not honored.
pub fn workspace_identity(workspace_key: &Path) -> io::Result<WorkspaceIdentity> {
    Ok(WorkspaceIdentity {
        current: workspace_entity_identity(workspace_key)?,
        managed_source: None,
    })
}

fn workspace_entity_identity(workspace_root: &Path) -> io::Result<WorkspaceEntityIdentity> {
    let key = canonicalize_or_owned(workspace_root);
    let root = filesystem_entity_identity(&key)?;

    let dot_git = key.join(".git");
    let git_marker = match dot_git.try_exists() {
        Ok(true) => Some(filesystem_entity_identity(&dot_git)?),
        Ok(false) => None,
        Err(error) => return Err(error),
    };
    let git_common_dir = match git2::Repository::open(&key) {
        Ok(repo) => Some(filesystem_entity_identity(repo.commondir())?),
        Err(error) if git_marker.is_some() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("workspace has git metadata that cannot be resolved: {error}"),
            ));
        }
        Err(_) => None,
    };

    Ok(WorkspaceEntityIdentity {
        root,
        git_marker,
        git_common_dir,
    })
}

/// Resolve the current workspace identity and optional grow-managed source
/// provenance as independent conjuncts.
///
/// The current workspace key is always identified first. A managed source is
/// then discovered and identified separately; it never replaces the current
/// identity. Failure of either required identity fails closed.
pub fn workspace_identity_for_cwd(
    cwd: &Path,
    workspace_key: &Path,
) -> io::Result<WorkspaceIdentity> {
    let current = workspace_entity_identity(workspace_key)?;
    let managed_source =
        if let Some(source_repo) = crate::worktree::source_repo_for_cwd(&cwd.to_string_lossy()) {
            let repo = git2::Repository::discover(&source_repo).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("managed worktree source repository is unavailable: {error}"),
                )
            })?;
            let source_root = repo.workdir().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "managed worktree source repository has no workdir",
                )
            })?;
            Some(workspace_entity_identity(source_root)?)
        } else {
            None
        };
    Ok(WorkspaceIdentity {
        current,
        managed_source,
    })
}

#[cfg(unix)]
fn filesystem_entity_identity(path: &Path) -> io::Result<FilesystemEntityIdentity> {
    use std::os::unix::fs::MetadataExt as _;

    let metadata = std::fs::metadata(path)?;
    Ok(FilesystemEntityIdentity::Unix {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(windows)]
fn filesystem_entity_identity(path: &Path) -> io::Result<FilesystemEntityIdentity> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::os::windows::io::AsRawHandle as _;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, GetFileInformationByHandle,
    };

    // FILE_FLAG_BACKUP_SEMANTICS is required to open directories. Sharing read,
    // write, and delete avoids turning an identity probe into a rename/delete
    // lock; the handle remains open through GetFileInformationByHandle.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0)
        .open(path)?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    unsafe {
        GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut information)
            .map_err(io::Error::other)?;
    }
    let file_index =
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow);
    Ok(FilesystemEntityIdentity::Windows {
        volume_serial_number: information.dwVolumeSerialNumber,
        file_index,
    })
}

#[cfg(not(any(unix, windows)))]
fn filesystem_entity_identity(_path: &Path) -> io::Result<FilesystemEntityIdentity> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "folder trust has no filesystem identity implementation for this platform",
    ))
}

/// Compute the trust **workspace key** for a working directory.
///
/// The key is the canonicalized git repository root when `cwd` is inside a
/// repo (trust applies to the whole repo), otherwise the canonicalized `cwd`.
///
/// Linked worktrees and standalone clones key on their own checkout roots. They
/// may share git metadata or carry managed-source provenance in
/// [`workspace_identity_for_cwd`], but neither relationship makes a new checkout
/// inherit another checkout's trust decision.
///
/// Finally, an over-broad derived root is rejected in favor of the cwd: when
/// `$HOME` is itself a git repo (dotfiles-in-home) the up-walk would otherwise
/// land on the home dir, so [`is_unsafe_trust_root`] re-scopes the key to the
/// cwd (keeping trust bound to the working dir, not the whole home subtree). A
/// cwd that IS home is out of scope — no narrower safe fallback exists.
pub fn workspace_key(cwd: &Path) -> PathBuf {
    let key = git_derived_workspace_key(cwd);
    if is_unsafe_trust_root(&key) {
        return canonicalize_or_owned(cwd);
    }
    key
}

/// Git-topology-derived workspace key (pre-safety-guard); see [`workspace_key`],
/// which rejects an over-broad derived root in favor of the cwd.
fn git_derived_workspace_key(cwd: &Path) -> PathBuf {
    if let Ok(repo) = git2::Repository::discover(cwd)
        && let Some(workdir) = repo.workdir()
    {
        return canonicalize_or_owned(workdir);
    }
    canonicalize_or_owned(cwd)
}

/// Whether `path` resolves to the user's home directory.
pub fn is_home_dir(path: &Path) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    canonicalize_or_owned(path) == canonicalize_or_owned(&home)
}

/// Whether `key` is too broad to ever be a safe trust root — refused on write
/// and ignored on read (fail closed).
///
/// Empty/relative, filesystem-root, and home-directory keys are refused. This
/// narrow-key policy prevents a trust decision from being attached to an
/// ambiguous or non-project workspace and preserves the prompt/persistence
/// contract.
///
/// Also consumed by [`crate::folder_trust`] as the "key can never be recorded"
/// signal: such a key can't be durably gated, so it resolves Trusted instead of
/// prompting on a decision that could never persist. Public because the shell's
/// revoke path refuses the same roots symmetrically — an in-process cache deny
/// for a key the store can never grant would be unliftable.
pub fn is_unsafe_trust_root(key: &Path) -> bool {
    !key.is_absolute() || key.parent().is_none() || is_home_dir(key)
}

fn canonicalize_or_owned(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn now_unix() -> Option<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

/// RAII exclusive advisory lock on a sidecar lock file, released on drop.
///
/// Serializes concurrent `TrustStore` writers (multiple processes / instances
/// sharing `~/.grow/`) across the whole read-modify-write so updates merge
/// instead of clobbering each other. The lock is advisory; only writers that
/// take it (i.e. this code) coordinate, which is sufficient since this store is
/// the sole writer of its file.
struct ExclusiveLock {
    file: std::fs::File,
}

impl ExclusiveLock {
    fn acquire(lock_path: &Path) -> io::Result<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        fs2::FileExt::lock_exclusive(&file)?;
        Ok(Self { file })
    }
}

impl Drop for ExclusiveLock {
    fn drop(&mut self) {
        // Best-effort unlock; the OS also releases the flock when `file` closes.
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_store_trusts_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TrustStore::load_from(tmp.path().join(TRUST_FILE_NAME));
        assert!(store.is_empty());
        assert!(!store.is_trusted(tmp.path()));
    }

    #[test]
    fn default_path_in_maps_home_and_preserves_no_home() {
        // With a resolvable home the store sits at <home>/trusted_folders.toml.
        let home = PathBuf::from("/home/alice/.grow");
        assert_eq!(
            TrustStore::default_path_in(Some(home.clone())),
            Some(home.join(TRUST_FILE_NAME))
        );

        // With NO resolvable home the path is `None` — never a synthesized
        // fallback. This is the regression guard that keeps the store off the
        // cwd-relative `./.grow` that grow_home() would invent, which is exactly
        // how a cloned repo's own `<repo>/.grow/trusted_folders.toml` could
        // masquerade as the user-global store and self-trust the checkout.
        assert_eq!(TrustStore::default_path_in(None), None);
    }

    #[test]
    fn default_path_sources_from_user_grow_home() {
        // Thin source-pin: the production accessor reads user_grow_home()
        // (Option, no cwd fallback), not grow_home(). The real regression guard
        // is the seam test above (default_path_in(None) == None).
        assert_eq!(
            TrustStore::default_path(),
            config::user_grow_home().map(|h| h.join(TRUST_FILE_NAME))
        );
    }

    #[test]
    fn no_home_store_trusts_nothing_and_persists_nothing() {
        // Simulate the no-home environment where `default_path()` is `None`:
        // `load()` yields `empty()`, a store with no backing path. It must
        // trust nothing and silently no-op on writes — never touching a
        // cwd-relative `./.grow`.
        let mut store = TrustStore::empty();
        assert!(store.is_empty());

        let key = Path::new("/some/abs/repo");
        assert!(!store.is_trusted(key), "no-home store trusts nothing");

        // set_trusted is a no-op that returns Ok and records nothing.
        store
            .set_trusted(key)
            .expect("no-home set_trusted is a no-op Ok");
        assert!(
            store.is_empty(),
            "no-home set_trusted must record nothing (in memory)"
        );
        assert!(
            !store.is_trusted(key),
            "still trusts nothing after the no-op write"
        );

        // set_untrusted likewise no-ops without panicking or recording.
        store
            .set_untrusted(key)
            .expect("no-home set_untrusted is a no-op Ok");
        assert!(store.is_empty());
    }

    #[test]
    fn set_trusted_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let key = canonicalize_or_owned(&repo);

        let mut store = TrustStore::load_from(store_path.clone());
        assert!(!store.is_trusted(&key));
        store.set_trusted(&key).unwrap();
        assert!(store.is_trusted(&key));

        // Reload from disk and verify persistence.
        let reloaded = TrustStore::load_from(store_path);
        assert!(reloaded.is_trusted(&key));
    }

    #[test]
    fn same_path_directory_replacement_does_not_inherit_trust() {
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let mut store = TrustStore::load_from(store_path.clone());
        store.set_trusted(&repo).unwrap();
        assert!(store.is_trusted(&repo));

        // Keep the original entity alive under a different name so inode/file
        // index reuse cannot make the test flaky, then create a new entity at
        // the exact trusted pathname.
        std::fs::rename(&repo, tmp.path().join("old-repo")).unwrap();
        std::fs::create_dir_all(&repo).unwrap();

        assert!(!store.is_trusted(&repo));
        assert!(!TrustStore::load_from(store_path).is_trusted(&repo));
    }

    #[test]
    fn replacing_git_metadata_in_place_does_not_inherit_trust() {
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git2::Repository::init(&repo).unwrap();

        let mut store = TrustStore::load_from(store_path.clone());
        store.set_trusted(&repo).unwrap();
        assert!(store.is_trusted(&repo));

        // The worktree root entity stays unchanged. Only the repository
        // identity changes, which must independently invalidate the grant.
        std::fs::rename(repo.join(".git"), repo.join(".git-from-trusted-repo")).unwrap();
        git2::Repository::init(&repo).unwrap();

        assert!(!store.is_trusted(&repo));
        assert!(!TrustStore::load_from(store_path).is_trusted(&repo));
    }

    #[test]
    fn identity_read_failure_never_grants_trust() {
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let mut store = TrustStore::load_from(store_path);
        store.set_trusted(&repo).unwrap();
        std::fs::rename(&repo, tmp.path().join("moved-repo")).unwrap();

        assert!(!store.is_trusted(&repo));
    }

    #[test]
    fn persist_overwrites_existing_and_round_trips_both() {
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        let repo_a = tmp.path().join("repo-a");
        let repo_b = tmp.path().join("repo-b");
        std::fs::create_dir_all(&repo_a).unwrap();
        std::fs::create_dir_all(&repo_b).unwrap();
        let key_a = canonicalize_or_owned(&repo_a);
        let key_b = canonicalize_or_owned(&repo_b);

        let mut store = TrustStore::load_from(store_path.clone());
        store.set_trusted(&key_a).unwrap();
        // The second persist runs over an already-existing destination file.
        store.set_trusted(&key_b).unwrap();

        // Both decisions survive the overwrite, after reloading from disk.
        let reloaded = TrustStore::load_from(store_path.clone());
        assert!(reloaded.is_trusted(&key_a));
        assert!(reloaded.is_trusted(&key_b));

        // The owner-only guarantee still holds after the overwrite, independent
        // of umask (NamedTempFile creates 0600 on Unix regardless of umask).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&store_path).unwrap().permissions().mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "trust store must stay 0600 after overwrite"
            );
        }
    }

    #[test]
    fn trust_matches_only_the_exact_workspace_key() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        let child = repo.join("crates").join("inner");
        std::fs::create_dir_all(&child).unwrap();
        let repo_key = canonicalize_or_owned(&repo);
        let child_key = canonicalize_or_owned(&child);

        let mut store = TrustStore::load_from(tmp.path().join(TRUST_FILE_NAME));
        store.set_trusted(&repo_key).unwrap();

        assert!(
            !store.is_trusted(&child_key),
            "the store must not inherit a parent path's grant; callers collapse cwd via workspace_key"
        );
    }

    #[test]
    fn parent_deny_does_not_override_explicit_child_grant() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().join("parent");
        let child = parent.join("child");
        std::fs::create_dir_all(&child).unwrap();
        let parent_key = canonicalize_or_owned(&parent);
        let child_key = canonicalize_or_owned(&child);

        let mut store = TrustStore::load_from(tmp.path().join(TRUST_FILE_NAME));
        store.set_untrusted(&parent_key).unwrap();
        store.set_trusted(&child_key).unwrap();

        assert!(
            !store.is_trusted(&parent_key),
            "the ancestor stays untrusted"
        );
        assert!(
            store.is_trusted(&child_key),
            "the child's exact grant is independent from the parent record"
        );

        let reloaded = TrustStore::load_from(tmp.path().join(TRUST_FILE_NAME));
        assert!(!reloaded.is_trusted(&parent_key));
        assert!(reloaded.is_trusted(&child_key));
    }

    #[cfg(unix)]
    #[test]
    fn persisted_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let mut store = TrustStore::load_from(store_path.clone());
        store.set_trusted(&canonicalize_or_owned(&repo)).unwrap();

        let mode = std::fs::metadata(&store_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "trust store must be 0600");
    }

    #[test]
    fn home_dir_is_not_persisted() {
        // Serialize with the in-file $HOME mutator: its temp $HOME window could
        // otherwise flip is_home_dir mid-test. This test mutates no env itself.
        let _lock = crate::ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        let Some(home) = dirs::home_dir() else {
            return; // no home dir in this environment; nothing to assert
        };

        let mut store = TrustStore::load_from(store_path.clone());
        store.set_trusted(&home).unwrap();

        // Nothing was persisted, and the store still holds no folders.
        assert!(store.is_empty(), "home dir must not be recorded");
        assert!(
            !store_path.exists(),
            "no trust file should be written for the home dir"
        );
    }

    #[test]
    fn workspace_key_falls_back_to_cwd_outside_repo() {
        // A freshly created temp dir is not inside a git repo in CI sandboxes;
        // the key should be the canonicalized dir itself.
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("plain");
        std::fs::create_dir_all(&sub).unwrap();
        let key = workspace_key(&sub);
        assert!(key.is_absolute());
        // Only pin the fallback when the temp dir is genuinely outside any git
        // repo (a dev/CI checkout may place $TMPDIR inside the source repository).
        if git2::Repository::discover(&sub).is_err() {
            assert_eq!(key, canonicalize_or_owned(&sub));
        }
    }

    #[test]
    fn workspace_key_ignores_home_git_repo_for_subdir() {
        // Home-is-a-git-repo (dotfiles in $HOME): a subdir launched from under
        // home must key trust on the SUBDIR, not on $HOME — even though the git
        // up-walk discovers home as the repo root. Serialize + guard $HOME
        // (dirs::home_dir reads it) via the crate-shared env lock.
        let _lock = crate::ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let _home_guard = crate::TestEnvGuard::set("HOME", home.path());
        git2::Repository::init(home.path()).unwrap();
        let civ = home.path().join("Documents").join("civ");
        std::fs::create_dir_all(&civ).unwrap();

        let key = workspace_key(&civ);
        assert_eq!(
            key,
            canonicalize_or_owned(&civ),
            "a subdir under a home git repo must key on the subdir, not $HOME"
        );
        assert!(
            !is_home_dir(&key),
            "the workspace key must never resolve to the home dir"
        );
    }

    #[test]
    fn old_schema_fails_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(
            &store_path,
            format!(
                "[folders.'{}']\ntrusted = true\n",
                canonicalize_or_owned(&repo).to_string_lossy()
            ),
        )
        .unwrap();

        let store = TrustStore::load_from(store_path);
        assert!(store.is_empty(), "schema 1 records must not be migrated");
        assert!(
            !store.is_trusted(&repo),
            "an incompatible document must never grant trust"
        );
    }

    #[test]
    fn mismatched_schema_version_fails_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let mut writer = TrustStore::load_from(store_path.clone());
        writer.set_trusted(&repo).unwrap();
        let body = std::fs::read_to_string(&store_path).unwrap().replacen(
            "schema_version = 2",
            "schema_version = 1",
            1,
        );
        std::fs::write(&store_path, body).unwrap();

        let store = TrustStore::load_from(store_path);
        assert!(store.is_empty());
        assert!(!store.is_trusted(&repo));
    }

    #[test]
    fn current_schema_missing_identity_fails_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(
            &store_path,
            format!(
                "schema_version = {TRUST_SCHEMA_VERSION}\n\n[folders.'{}']\ntrusted = true\n",
                canonicalize_or_owned(&repo).to_string_lossy()
            ),
        )
        .unwrap();

        let store = TrustStore::load_from(store_path);
        assert!(store.is_empty(), "identity-less records must be rejected");
        assert!(!store.is_trusted(&repo));
    }

    #[test]
    fn malformed_store_fails_soft_to_empty() {
        // Corrupt TOML must fail closed: empty store, trust nothing, no panic.
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        std::fs::write(&store_path, "this is not = valid toml [[[").unwrap();

        let store = TrustStore::load_from(store_path);
        assert!(store.is_empty(), "malformed store must load as empty");
        assert!(!store.is_trusted(Path::new("/any/path")));
    }

    #[test]
    fn unsafe_keys_are_not_persisted() {
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        let mut store = TrustStore::load_from(store_path.clone());
        store.set_trusted(Path::new("")).unwrap();
        assert!(store.is_empty(), "relative/empty keys must be refused");

        let root = std::path::Path::new(std::path::MAIN_SEPARATOR_STR);
        store.set_trusted(root).unwrap();
        assert!(store.is_empty(), "filesystem root must be refused");
        assert!(!store_path.exists());
    }

    #[test]
    fn home_key_on_disk_is_not_honored() {
        // Serialize with the in-file $HOME mutator: its temp $HOME window could
        // otherwise flip is_home_dir mid-test. This test mutates no env itself.
        let _lock = crate::ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // A hand-edited current-schema `[folders."<home>"]` record must not be
        // honored. This exercises the read guard rather than the write refusal.
        let Some(home) = dirs::home_dir() else {
            return; // no home dir in this environment; nothing to assert
        };
        let canonical_home = canonicalize_or_owned(&home);
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        let mut doc = TrustDocument::default();
        doc.folders.insert(
            canonical_home.to_string_lossy().into_owned(),
            FolderTrust {
                trusted: true,
                decided_at: None,
                // The read-side unsafe-key guard runs before identity matching;
                // a valid but unrelated identity keeps this test independent
                // from the real home directory's repository state.
                identity: workspace_identity(tmp.path()).unwrap(),
            },
        );
        TrustStore::persist_doc(&store_path, &doc).unwrap();

        let store = TrustStore::load_from(store_path);
        assert!(
            !store.is_trusted(&canonical_home),
            "a home-dir key on disk must not be honored"
        );
    }

    #[cfg(unix)]
    #[test]
    fn set_trusted_canonicalizes_key() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        let real = tmp.path().join("real-repo");
        std::fs::create_dir_all(&real).unwrap();
        let link = tmp.path().join("link-repo");
        symlink(&real, &link).unwrap();

        // Trust via the symlink alias.
        let mut store = TrustStore::load_from(store_path);
        store.set_trusted(&link).unwrap();

        // The stored key was canonicalized, so a canonical lookup matches.
        let canonical_real = canonicalize_or_owned(&real);
        assert!(
            store.is_trusted(&canonical_real),
            "set_trusted must store the canonical path so canonical lookups match"
        );
    }

    #[test]
    fn set_untrusted_records_explicit_deny() {
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let key = canonicalize_or_owned(&repo);

        let mut store = TrustStore::load_from(store_path.clone());
        store.set_untrusted(&key).unwrap();
        assert!(!store.is_trusted(&key), "an explicit deny is not trusted");
        assert!(!store.is_empty(), "the deny decision is recorded");

        // Reload from disk: the deny record persisted.
        let reloaded = TrustStore::load_from(store_path);
        assert!(!reloaded.is_trusted(&key));
        assert!(!reloaded.is_empty(), "deny record survives reload");
    }

    #[test]
    fn trust_decision_flips() {
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let key = canonicalize_or_owned(&repo);

        let mut store = TrustStore::load_from(store_path.clone());
        store.set_trusted(&key).unwrap();
        assert!(store.is_trusted(&key));
        store.set_untrusted(&key).unwrap();
        assert!(!store.is_trusted(&key), "untrust flips the stored bool");
        store.set_trusted(&key).unwrap();
        assert!(store.is_trusted(&key), "re-trust flips it back");
        // The insert overwrites — one record per folder, no duplicates.
        assert_eq!(store.len(), 1);

        let reloaded = TrustStore::load_from(store_path);
        assert!(reloaded.is_trusted(&key));
        assert_eq!(reloaded.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn is_trusted_canonicalizes_query() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        let real = tmp.path().join("real-repo");
        std::fs::create_dir_all(&real).unwrap();
        let link = tmp.path().join("link-repo");
        symlink(&real, &link).unwrap();

        let mut store = TrustStore::load_from(store_path);
        store.set_trusted(&canonicalize_or_owned(&real)).unwrap();

        // A query via the symlink alias resolves to the trusted real dir.
        assert!(
            store.is_trusted(&link),
            "is_trusted must canonicalize the query so a symlink alias matches"
        );
    }

    #[test]
    fn concurrent_writers_do_not_clobber() {
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        let repo_a = tmp.path().join("repo-a");
        let repo_b = tmp.path().join("repo-b");
        std::fs::create_dir_all(&repo_a).unwrap();
        std::fs::create_dir_all(&repo_b).unwrap();
        let key_a = canonicalize_or_owned(&repo_a);
        let key_b = canonicalize_or_owned(&repo_b);

        // Two instances loaded while the file is empty: both start with an empty
        // in-memory doc, mimicking two processes that raced the initial load.
        let mut s1 = TrustStore::load_from(store_path.clone());
        let mut s2 = TrustStore::load_from(store_path.clone());
        s1.set_trusted(&key_a).unwrap();
        s2.set_trusted(&key_b).unwrap();

        // The locked re-read-merge means s2's write did not clobber s1's.
        let reloaded = TrustStore::load_from(store_path);
        assert!(
            reloaded.is_trusted(&key_a),
            "A must survive a concurrent write"
        );
        assert!(
            reloaded.is_trusted(&key_b),
            "B must survive a concurrent write"
        );
    }

    #[cfg(unix)]
    #[test]
    fn persist_failure_leaves_memory_unchanged() {
        // Make the destination path itself a DIRECTORY so the final atomic
        // rename in persist fails (renaming a file over a directory). This is
        // robust even when tests run as root (a chmod 0o500 dir would be
        // bypassed by root), and it exercises the invariant: on a write error
        // the in-memory doc is left unchanged (memory-before-persist fix).
        let tmp = tempfile::tempdir().unwrap();
        let store_path = tmp.path().join(TRUST_FILE_NAME);
        std::fs::create_dir_all(&store_path).unwrap(); // store path is a dir, not a file
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let key = canonicalize_or_owned(&repo);

        let mut store = TrustStore::load_from(store_path);
        let result = store.set_trusted(&key);
        assert!(
            result.is_err(),
            "persist over a directory destination must fail"
        );
        assert!(
            !store.is_trusted(&key),
            "memory must be unchanged on persist failure"
        );
    }

    #[test]
    fn linked_worktrees_have_distinct_trust_keys_and_do_not_inherit_main_trust() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        std::fs::create_dir_all(&main).unwrap();
        let repo = git2::Repository::init(&main).unwrap();

        // Worktree creation requires a valid HEAD, so make an initial commit.
        let sig = git2::Signature::now("t", "t@t").unwrap();
        let tree = {
            let mut idx = repo.index().unwrap();
            let oid = idx.write_tree().unwrap();
            repo.find_tree(oid).unwrap()
        };
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        // Linked worktrees OUTSIDE the main dir (git2 creates the paths).
        let wt1 = dir.path().join("wt1");
        let wt2 = dir.path().join("wt2");
        repo.worktree("wt1", &wt1, None).unwrap();
        repo.worktree("wt2", &wt2, None).unwrap();

        let main_key = workspace_key(&main);
        assert_eq!(main_key, canonicalize_or_owned(&main));
        assert_eq!(
            workspace_key(&wt1),
            canonicalize_or_owned(&wt1),
            "a linked worktree keys on its own checkout"
        );
        assert_eq!(
            workspace_key(&wt2),
            canonicalize_or_owned(&wt2),
            "a second linked worktree has its own key"
        );
        let nested = wt1.join("crates").join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(workspace_key(&nested), canonicalize_or_owned(&wt1));

        let mut store = TrustStore::load_from(dir.path().join(TRUST_FILE_NAME));
        store.set_trusted(&main_key).unwrap();
        assert!(!store.is_trusted_for_cwd(&wt1, &workspace_key(&wt1)));
        assert!(!store.is_trusted_for_cwd(&wt2, &workspace_key(&wt2)));
    }

    #[test]
    fn workspace_key_bare_repo_worktree_does_not_widen_to_parent() {
        // A bare repo's `commondir()` is the bare dir itself, so a naive
        // `commondir().parent()` would key off the dir CONTAINING the repo and
        // make unrelated sibling worktrees share one trust identity. The key
        // must instead fall back to the worktree's OWN dir (narrow, never widened).
        let dir = tempfile::tempdir().unwrap();
        let bare = dir.path().join("repo.git");
        let repo = git2::Repository::init_bare(&bare).unwrap();

        // Worktree creation needs a valid HEAD; build an empty commit (bare repo
        // has no index, so use a treebuilder for the empty tree).
        let sig = git2::Signature::now("t", "t@t").unwrap();
        let tree_oid = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_oid).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        let wt = dir.path().join("wt");
        repo.worktree("wt", &wt, None).unwrap();

        let key = workspace_key(&wt);
        assert_ne!(
            key,
            canonicalize_or_owned(dir.path()),
            "bare-repo worktree key must not widen to the parent dir"
        );
        assert_eq!(
            key,
            canonicalize_or_owned(&wt),
            "bare-repo worktree falls back to its own dir (narrow, safe)"
        );
    }

    #[test]
    fn workspace_key_separate_gitdir_worktree_does_not_widen() {
        // `git init --separate-git-dir` leaves `core.worktree` unset, so the
        // common gitdir's INFERRED workdir is the PARENT of the relocated gitdir,
        // not the checkout. The layout guard (`<workdir>/.git` must equal the
        // common gitdir) rejects that, so the key falls back to the worktree's
        // own dir — never widening to the gitdir's parent.
        let dir = tempfile::tempdir().unwrap();
        let checkout = dir.path().join("checkout");
        let gitdir = dir.path().join("gitstore");
        std::fs::create_dir_all(&checkout).unwrap();
        let run = |args: &[&str], cwd: &std::path::Path| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(cwd)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        };
        // If git isn't usable for this layout, skip rather than false-fail.
        if !run(
            &[
                "init",
                "--separate-git-dir",
                gitdir.to_str().unwrap(),
                checkout.to_str().unwrap(),
            ],
            dir.path(),
        ) || !run(&["commit", "--allow-empty", "-m", "init"], &checkout)
        {
            return;
        }
        let wt = dir.path().join("wt");
        if !run(&["worktree", "add", wt.to_str().unwrap()], &checkout) {
            return;
        }

        // Only assert once the worktree is a real linked worktree whose common
        // gitdir is the relocated separate gitdir (the layout this test targets).
        let Ok(repo) = git2::Repository::discover(&wt) else {
            return;
        };
        if !repo.is_worktree() {
            return;
        }

        let key = workspace_key(&wt);
        // The invariant: the key is NOT a broad ancestor of the checkout.
        assert_ne!(
            key,
            canonicalize_or_owned(dir.path()),
            "separate-gitdir worktree key must not widen to the gitdir's parent"
        );
        // It falls back to the worktree's own dir (narrow, safe).
        assert_eq!(
            key,
            canonicalize_or_owned(&wt),
            "separate-gitdir worktree falls back to its own dir (narrow, safe)"
        );
    }

    // ── grow-managed worktree provenance ─────────────────────────────────

    // Crate-shared env lock + env guards bundled as ONE value so the env restores
    // before the lock releases by struct field order (see lib.rs), regardless of
    // how the caller binds the fixture's return.
    use crate::LockedTestEnv;

    /// Point `GROW_HOME` at an isolated tempdir and register one grow-managed
    /// worktree at `<home>/worktrees/repo/<name>` recording `source_repo` and
    /// `creation_mode`. Returns `(env, worktree dir)`; the [`LockedTestEnv`]
    /// holds the lock and restores `GROW_HOME` on
    /// drop (before releasing the lock), so the caller may bind it any way.
    fn register_grow_worktree(
        temp: &tempfile::TempDir,
        name: &str,
        source_repo: &Path,
        creation_mode: &str,
    ) -> (LockedTestEnv, PathBuf) {
        use fast_worktree::{WorktreeDb, WorktreeKind, WorktreeRecord, WorktreeStatus};

        // Canonicalize so macOS /var -> /private/var agrees between the stored
        // record path and the canonicalized lookup query.
        let root = dunce::canonicalize(temp.path()).unwrap();
        let home = root.join("grow-home");
        let wt = home.join("worktrees").join("repo").join(name);
        std::fs::create_dir_all(&wt).unwrap();

        // Acquire the lock, then set the env under it (LockedTestEnv restores the
        // env before releasing the lock on drop).
        let env = LockedTestEnv::lock().set("GROW_HOME", &home);

        let db = WorktreeDb::open(&home).unwrap();
        let record = WorktreeRecord {
            id: name.to_string(),
            path: wt.clone(),
            source_repo: source_repo.to_path_buf(),
            repo_name: "repo".to_string(),
            kind: WorktreeKind::Session,
            creation_mode: creation_mode.to_string(),
            git_ref: None,
            head_commit: None,
            session_id: None,
            creator_pid: None,
            created_at: 100,
            last_accessed_at: None,
            status: WorktreeStatus::Alive,
            metadata: None,
        };
        db.register(&record).unwrap();
        (env, wt)
    }

    #[test]
    fn standalone_managed_clone_keeps_its_own_key_and_does_not_inherit_source_trust() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let source_repo = root.join("source-repo");
        std::fs::create_dir_all(&source_repo).unwrap();
        git2::Repository::init(&source_repo).unwrap();

        let (_env, wt) = register_grow_worktree(&temp, "wt", &source_repo, "standalone");
        git2::Repository::init(&wt).unwrap();

        let expected = canonicalize_or_owned(&wt);
        assert_eq!(
            workspace_key(&wt),
            expected,
            "a standalone clone keys on its own checkout"
        );
        let nested = wt.join("crates").join("inner");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(
            workspace_key(&nested),
            expected,
            "a nested cwd collapses only to its current repository root"
        );

        let mut store = TrustStore::load_from(root.join(TRUST_FILE_NAME));
        store.set_trusted(&source_repo).unwrap();
        assert!(store.is_trusted(&source_repo));
        assert!(
            !store.is_trusted_for_cwd(&wt, &expected),
            "a newly created clone must not inherit source trust"
        );
    }

    #[test]
    fn managed_worktree_grant_requires_current_and_source_identity() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let source_repo = root.join("source-repo");
        std::fs::create_dir_all(&source_repo).unwrap();
        git2::Repository::init(&source_repo).unwrap();

        let (_env, wt) = register_grow_worktree(&temp, "wt", &source_repo, "standalone");
        git2::Repository::init(&wt).unwrap();
        let key = workspace_key(&wt);
        let expected_identity = workspace_identity_for_cwd(&wt, &key).unwrap();
        let store_path = root.join(TRUST_FILE_NAME);
        let mut store = TrustStore::load_from(store_path.clone());
        store
            .set_trusted_identity(&key, expected_identity.clone())
            .unwrap();
        assert!(store.is_trusted_for_cwd(&wt, &key));

        std::fs::rename(
            source_repo.join(".git"),
            source_repo.join(".git-from-source-repo"),
        )
        .unwrap();
        let unavailable_source_key = workspace_key(&wt);
        assert_eq!(unavailable_source_key, wt);
        assert!(
            workspace_identity(&unavailable_source_key).is_ok(),
            "the current checkout remains identifiable by itself"
        );
        assert!(
            workspace_identity_for_cwd(&wt, &unavailable_source_key).is_err(),
            "managed provenance must additionally require a resolvable source repository"
        );
        assert!(!store.is_trusted_for_cwd(&wt, &unavailable_source_key));
        assert!(
            !TrustStore::load_from(store_path).is_trusted_for_cwd(&wt, &unavailable_source_key)
        );

        let mut fresh = TrustStore::load_from(root.join("unavailable-source-trust.toml"));
        assert!(
            !crate::folder_trust::persist_trust(
                &mut fresh,
                &wt,
                &unavailable_source_key,
                &expected_identity,
            ),
            "an explicit grant must also fail when managed source identity is unavailable"
        );
        assert!(fresh.is_empty());
    }

    #[test]
    fn managed_source_subdir_is_provenance_not_workspace_key() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let repo = root.join("realrepo");
        std::fs::create_dir_all(&repo).unwrap();
        git2::Repository::init(&repo).unwrap();
        let subdir = repo.join("crates").join("sub");
        std::fs::create_dir_all(&subdir).unwrap();

        let (_env, wt) = register_grow_worktree(&temp, "wt", &subdir, "standalone");
        git2::Repository::init(&wt).unwrap();

        assert_eq!(
            workspace_key(&wt),
            canonicalize_or_owned(&wt),
            "a source subdir must not replace the current checkout key"
        );
        assert!(workspace_identity_for_cwd(&wt, &workspace_key(&wt)).is_ok());
    }

    #[test]
    fn workspace_key_ignores_registry_for_cwd_outside_worktrees_dir() {
        // A populated registry must NOT collapse a cwd OUTSIDE
        // `<grow_home>/worktrees`: `worktree_record_for_cwd` skips the registry
        // there, so the key falls back to git/cwd. Non-vacuous: the registry IS
        // populated with a real git source repo that WOULD be returned for a
        // worktree cwd, and `outside` is its OWN git repo (under grow HOME but not
        // under its `worktrees/`) so the fallback is deterministic (no conditional
        // skip) — we assert the key is `outside`'s own root, never the source repo.
        let temp = tempfile::TempDir::new().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        let source_repo = root.join("source-repo");
        std::fs::create_dir_all(&source_repo).unwrap();
        git2::Repository::init(&source_repo).unwrap();

        let (_env, _wt) = register_grow_worktree(&temp, "wt", &source_repo, "standalone");

        // Under grow HOME but NOT under `<home>/worktrees`, and its own git repo.
        let outside = root.join("grow-home").join("not-worktrees").join("proj");
        std::fs::create_dir_all(&outside).unwrap();
        git2::Repository::init(&outside).unwrap();

        let key = workspace_key(&outside);
        assert_eq!(
            key,
            canonicalize_or_owned(&outside),
            "a cwd outside <grow_home>/worktrees keys on its own repo root"
        );
        assert_ne!(
            key,
            canonicalize_or_owned(&source_repo),
            "it must not collapse onto the populated registry's source repo"
        );
    }
}
