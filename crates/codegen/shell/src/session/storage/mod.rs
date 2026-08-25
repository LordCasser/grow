use async_trait::async_trait;
use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
use std::ffi::CString;
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

use crate::extensions::notification::SessionNotification;
use crate::sampling::ConversationItem;
use crate::session::info::Info;
use crate::session::persistence::Summary;
use crate::session::wire_tags::{
    AVAILABLE_COMMANDS_UPDATE_PREFIX, REWIND_MARKER, USER_MESSAGE_CHUNK,
};
use agent_client_protocol as acp;
use sampling_types::ReasoningEffort;
use workspace::session::file_state::RewindPoint;

pub mod jsonl;
pub mod search;
pub mod search_fts;
mod search_recovery;
pub(crate) mod summary_write;

/// On-disk file names, relative to a session directory. Single source of truth for
/// the storage adapter and the session/state and session/import extensions.
pub(crate) const SUMMARY_FILE: &str = "summary.json";
pub(crate) const UPDATES_FILE: &str = "updates.jsonl";
pub(crate) const TIMELINE_FILE: &str = "timeline.jsonl";
pub(crate) const SIDEBANDS_DIR: &str = "sidebands";
pub(crate) const MAX_JSONL_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const MAX_SESSION_SUMMARY_BYTES: u64 = 1024 * 1024;

pub(crate) type SidebandLedgers = BTreeMap<String, Vec<chat_state::SidebandEvent>>;

#[cfg(unix)]
pub(crate) fn sync_directory(path: &Path) -> io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

#[cfg(not(unix))]
pub(crate) fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

pub(crate) fn require_regular_directory(path: &Path, description: &str) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{description} is not a regular directory: {}",
                path.display()
            ),
        ));
    }
    Ok(())
}

/// Create a relative directory tree below an existing authority root.
///
/// Every component is created and verified independently. This prevents an
/// entity-owned directory such as `sidebands/` or `workflows/` from redirecting
/// canonical writes through a symlink outside the session directory.
pub(crate) fn create_contained_dir_all(
    root: &Path,
    relative: &Path,
    description: &str,
) -> io::Result<PathBuf> {
    contained_directory(root, relative, description, true)
}

fn contained_directory(
    root: &Path,
    relative: &Path,
    description: &str,
    create_missing: bool,
) -> io::Result<PathBuf> {
    #[cfg(any(unix, windows))]
    {
        return ContainedDirectory::open(root, relative, description, create_missing)
            .map(|directory| directory.path);
    }
    #[cfg(not(any(unix, windows)))]
    {
        require_regular_directory(root, description)?;
        let mut current = root.to_path_buf();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{description} path must be relative and contained"),
                ));
            };
            current.push(name);
            match std::fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "{description} is not a regular directory: {}",
                            current.display()
                        ),
                    ));
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound && create_missing => {
                    match std::fs::create_dir(&current) {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                        Err(error) => return Err(error),
                    }
                    require_regular_directory(&current, description)?;
                    sync_directory(&current)?;
                    if let Some(parent) = current.parent() {
                        sync_directory(parent)?;
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Ok(current)
    }
}

/// A directory capability pinned to one inode. All descendants and final
/// filesystem operations are resolved relative to its fd, so renaming a
/// previously validated path and replacing it with a symlink cannot redirect
/// a canonical write outside the authority root.
#[cfg(unix)]
#[derive(Debug)]
pub(crate) struct ContainedDirectory {
    path: PathBuf,
    handle: std::fs::File,
}

#[cfg(unix)]
impl ContainedDirectory {
    pub(crate) fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            path: self.path.clone(),
            handle: self.handle.try_clone()?,
        })
    }

    pub(crate) fn is_same_entity(&self, other: &Self) -> io::Result<bool> {
        use std::os::unix::fs::MetadataExt as _;

        let left = self.handle.metadata()?;
        let right = other.handle.metadata()?;
        Ok(left.dev() == right.dev() && left.ino() == right.ino())
    }

    /// Re-label an already pinned directory after its parent has atomically
    /// renamed the directory entry. The capability itself is unchanged; the
    /// path is display-only and must never be reopened as authority.
    pub(crate) fn rebind_child_display_path(
        mut self,
        parent: &Self,
        child_name: &std::ffi::OsStr,
    ) -> Self {
        debug_assert_eq!(Path::new(child_name).file_name(), Some(child_name));
        self.path = parent.path.join(child_name);
        self
    }

    pub(crate) fn open(
        root: &Path,
        relative: &Path,
        description: &str,
        create_missing: bool,
    ) -> io::Result<Self> {
        let root_c = CString::new(root.as_os_str().as_bytes()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "authority root contains NUL")
        })?;
        let root_fd = unsafe {
            libc::open(
                root_c.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            )
        };
        if root_fd == -1 {
            let error = io::Error::last_os_error();
            return Err(
                if error
                    .raw_os_error()
                    .is_some_and(|code| code == libc::ELOOP || code == libc::ENOTDIR)
                {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{description} authority is not a regular directory"),
                    )
                } else {
                    error
                },
            );
        }
        let mut handle = unsafe { std::fs::File::from_raw_fd(root_fd) };
        if !handle.metadata()?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{description} authority is not a regular directory"),
            ));
        }
        let mut path = root.to_path_buf();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{description} path must be relative and contained"),
                ));
            };
            handle = Self::open_child_directory(&handle, name, description, create_missing)?;
            path.push(name);
        }
        Ok(Self { path, handle })
    }

    pub(crate) fn open_relative(
        &self,
        relative: &Path,
        description: &str,
        create_missing: bool,
    ) -> io::Result<Self> {
        let mut handle = self.handle.try_clone()?;
        let mut path = self.path.clone();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{description} path must be relative and contained"),
                ));
            };
            handle = Self::open_child_directory(&handle, name, description, create_missing)?;
            path.push(name);
        }
        Ok(Self { path, handle })
    }

    pub(crate) fn display_path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn list_names(&self) -> io::Result<Vec<std::ffi::OsString>> {
        use std::os::unix::ffi::OsStringExt as _;

        let owned: std::os::fd::OwnedFd = self.handle.try_clone()?.into();
        let mut directory = nix::dir::Dir::from_fd(owned).map_err(io::Error::from)?;
        let mut names = Vec::new();
        for entry in directory.iter() {
            let entry = entry.map_err(io::Error::from)?;
            let name = entry.file_name().to_bytes();
            if name != b"." && name != b".." {
                names.push(std::ffi::OsString::from_vec(name.to_vec()));
            }
        }
        Ok(names)
    }

    /// Visit child names without materializing the directory. Maintenance
    /// callers can apply their own bounded batching while retaining this
    /// directory's pinned filesystem authority.
    pub(crate) fn visit_names(
        &self,
        mut visit: impl FnMut(&std::ffi::OsStr) -> io::Result<()>,
    ) -> io::Result<()> {
        use std::os::unix::ffi::OsStrExt as _;

        let owned: std::os::fd::OwnedFd = self.handle.try_clone()?.into();
        let mut directory = nix::dir::Dir::from_fd(owned).map_err(io::Error::from)?;
        for entry in directory.iter() {
            let entry = entry.map_err(io::Error::from)?;
            let name = entry.file_name().to_bytes();
            if name != b"." && name != b".." {
                visit(std::ffi::OsStr::from_bytes(name))?;
            }
        }
        Ok(())
    }

    pub(crate) fn create_child(
        &self,
        name: &std::ffi::OsStr,
        description: &str,
    ) -> io::Result<Self> {
        use std::os::unix::ffi::OsStrExt as _;

        let name_c = std::ffi::CString::new(name.as_bytes()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "contained path contains NUL")
        })?;
        if unsafe { libc::mkdirat(self.handle.as_raw_fd(), name_c.as_ptr(), 0o700) } == -1 {
            return Err(io::Error::last_os_error());
        }
        self.sync()?;
        let handle = Self::open_child_directory(&self.handle, name, description, false)?;
        Ok(Self {
            path: self.path.join(name),
            handle,
        })
    }

    pub(crate) fn sync_tree(&self) -> io::Result<()> {
        for name in self.list_names()? {
            match self.open_relative(Path::new(&name), "contained child directory", false) {
                Ok(directory) => directory.sync_tree()?,
                Err(directory_error) => match self.open_regular(&name, "contained child file") {
                    Ok(file) => sync_file_durable(&file)?,
                    Err(_) => return Err(directory_error),
                },
            }
        }
        self.sync()
    }

    pub(crate) fn remove_tree_child(&self, name: &std::ffi::OsStr) -> io::Result<()> {
        match self.open_relative(Path::new(name), "contained child directory", false) {
            Ok(directory) => {
                directory.remove_all_contents()?;
                self.remove_empty_child(name, true)
            }
            Err(_) => self.remove_file(name, true),
        }
    }

    pub(crate) fn remove_all_contents(&self) -> io::Result<()> {
        for child in self.list_names()? {
            match self.open_relative(Path::new(&child), "contained child directory", false) {
                Ok(_) => self.remove_tree_child(&child)?,
                Err(_) => self.remove_file(&child, false)?,
            }
        }
        self.sync()
    }

    pub(crate) fn remove_empty_child(
        &self,
        name: &std::ffi::OsStr,
        durable: bool,
    ) -> io::Result<()> {
        let name = Self::component(name)?;
        if unsafe { libc::unlinkat(self.handle.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) }
            == -1
        {
            return Err(io::Error::last_os_error());
        }
        if durable && let Err(error) = self.sync() {
            tracing::warn!(%error, "directory removal committed but parent sync failed");
        }
        Ok(())
    }

    pub(crate) fn rename_child_no_replace(
        &self,
        source: &std::ffi::OsStr,
        target: &std::ffi::OsStr,
    ) -> io::Result<()> {
        Self::component(source)?;
        Self::component(target)?;
        #[cfg(target_os = "linux")]
        {
            use nix::fcntl::{RenameFlags, renameat2};
            renameat2(
                &self.handle,
                Path::new(source),
                &self.handle,
                Path::new(target),
                RenameFlags::RENAME_NOREPLACE,
            )
            .map_err(|error| match error {
                nix::errno::Errno::EEXIST => io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    self.path.join(target).display().to_string(),
                ),
                other => io::Error::from(other),
            })?;
        }
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::ffi::OsStrExt as _;
            let source = std::ffi::CString::new(source.as_bytes()).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "source path contains NUL")
            })?;
            let target = std::ffi::CString::new(target.as_bytes()).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "target path contains NUL")
            })?;
            if unsafe {
                libc::renameatx_np(
                    self.handle.as_raw_fd(),
                    source.as_ptr(),
                    self.handle.as_raw_fd(),
                    target.as_ptr(),
                    libc::RENAME_EXCL,
                )
            } == -1
            {
                let error = io::Error::last_os_error();
                return Err(if error.raw_os_error() == Some(libc::EEXIST) {
                    io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        self.path
                            .join(target.to_string_lossy().as_ref())
                            .display()
                            .to_string(),
                    )
                } else {
                    error
                });
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (source, target);
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "handle-relative no-replace rename is unsupported",
            ));
        }
        Ok(())
    }

    fn open_child_directory(
        parent: &std::fs::File,
        name: &std::ffi::OsStr,
        description: &str,
        create_missing: bool,
    ) -> io::Result<std::fs::File> {
        let name = Self::component(name)?;
        let flags = libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW;
        let mut fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
        if fd == -1
            && create_missing
            && io::Error::last_os_error().kind() == io::ErrorKind::NotFound
        {
            let created = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
            if created == -1 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(error);
                }
            } else {
                parent.sync_all()?;
            }
            fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
        }
        if fd == -1 {
            let error = io::Error::last_os_error();
            return Err(
                if error
                    .raw_os_error()
                    .is_some_and(|code| code == libc::ELOOP || code == libc::ENOTDIR)
                {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{description} contains a symlink"),
                    )
                } else {
                    error
                },
            );
        }
        let directory = unsafe { std::fs::File::from_raw_fd(fd) };
        if !directory.metadata()?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{description} is not a regular directory"),
            ));
        }
        if create_missing {
            directory.sync_all()?;
        }
        Ok(directory)
    }

    fn component(name: &std::ffi::OsStr) -> io::Result<CString> {
        if Path::new(name).components().count() != 1
            || !matches!(
                Path::new(name).components().next(),
                Some(Component::Normal(_))
            )
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "contained file name must be one normal component",
            ));
        }
        CString::new(name.as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "file name contains NUL"))
    }

    pub(crate) fn open_read_write_create(
        &self,
        name: &std::ffi::OsStr,
    ) -> io::Result<std::fs::File> {
        let name = Self::component(name)?;
        let mut fd = -1;
        let mut last_error = None;
        // Darwin can transiently return ENOENT when two threads race to
        // O_CREAT the same dirfd-relative name. The winner has already made
        // the lock/ledger visible, so a bounded reopen is the correct result.
        for attempt in 0..8 {
            fd = unsafe {
                libc::openat(
                    self.handle.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDWR | libc::O_CREAT | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                    0o600,
                )
            };
            if fd != -1 {
                break;
            }
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::NotFound && attempt < 7 {
                last_error = Some(error);
                std::thread::yield_now();
                continue;
            }
            last_error = Some(error);
            break;
        }
        if fd == -1 {
            let error = last_error.expect("failed openat records its OS error");
            return Err(if error.raw_os_error() == Some(libc::ELOOP) {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "contained write target is a symlink",
                )
            } else {
                io::Error::new(
                    error.kind(),
                    format!(
                        "cannot open {} relative to pinned directory {}: {error}",
                        name.to_string_lossy(),
                        self.path.display()
                    ),
                )
            });
        }
        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        if !file.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "contained write target is not a regular file",
            ));
        }
        Ok(file)
    }

    pub(crate) fn read_bounded(
        &self,
        name: &std::ffi::OsStr,
        description: &str,
        max_bytes: u64,
    ) -> io::Result<Vec<u8>> {
        let mut file = self.open_regular(name, description)?;
        let metadata = file.metadata()?;
        if metadata.len() > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{description} exceeds the byte limit"),
            ));
        }
        let mut bytes = Vec::new();
        file.take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{description} grew while reading"),
            ));
        }
        Ok(bytes)
    }

    pub(crate) fn open_regular(
        &self,
        name: &std::ffi::OsStr,
        description: &str,
    ) -> io::Result<std::fs::File> {
        let name = Self::component(name)?;
        let fd = unsafe {
            libc::openat(
                self.handle.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd == -1 {
            let error = io::Error::last_os_error();
            return Err(if error.raw_os_error() == Some(libc::ELOOP) {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{description} is a symlink"),
                )
            } else {
                error
            });
        }
        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{description} is not a regular file"),
            ));
        }
        Ok(file)
    }

    pub(crate) fn sync(&self) -> io::Result<()> {
        self.handle.sync_all()
    }

    pub(crate) fn remove_file(&self, name: &std::ffi::OsStr, durable: bool) -> io::Result<()> {
        let name = Self::component(name)?;
        if unsafe { libc::unlinkat(self.handle.as_raw_fd(), name.as_ptr(), 0) } == -1 {
            return Err(io::Error::last_os_error());
        }
        if durable {
            self.sync()?;
        }
        Ok(())
    }

    pub(crate) fn write_atomic(
        &self,
        name: &std::ffi::OsStr,
        bytes: &[u8],
        durable: bool,
        replace: bool,
    ) -> io::Result<()> {
        let target = Self::component(name)?;
        let existing_fd = unsafe {
            libc::openat(
                self.handle.as_raw_fd(),
                target.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if existing_fd != -1 {
            let existing = unsafe { std::fs::File::from_raw_fd(existing_fd) };
            if !existing.metadata()?.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "contained write target is not a regular file",
                ));
            }
        } else {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::NotFound {
                return Err(if error.raw_os_error() == Some(libc::ELOOP) {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "contained write target is a symlink",
                    )
                } else {
                    error
                });
            }
        }
        let tmp_name = format!(
            ".{}.{}.tmp",
            std::process::id(),
            uuid::Uuid::now_v7().simple()
        );
        let tmp = CString::new(tmp_name).expect("generated temp name has no NUL");
        let fd = unsafe {
            libc::openat(
                self.handle.as_raw_fd(),
                tmp.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o600,
            )
        };
        if fd == -1 {
            return Err(io::Error::last_os_error());
        }
        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        let result = (|| {
            file.write_all(bytes)?;
            if durable {
                sync_file_durable(&file)?;
            }
            drop(file);
            let published = if replace {
                unsafe {
                    libc::renameat(
                        self.handle.as_raw_fd(),
                        tmp.as_ptr(),
                        self.handle.as_raw_fd(),
                        target.as_ptr(),
                    )
                }
            } else {
                let linked = unsafe {
                    libc::linkat(
                        self.handle.as_raw_fd(),
                        tmp.as_ptr(),
                        self.handle.as_raw_fd(),
                        target.as_ptr(),
                        0,
                    )
                };
                if linked == 0 {
                    unsafe {
                        libc::unlinkat(self.handle.as_raw_fd(), tmp.as_ptr(), 0);
                    }
                    0
                } else {
                    linked
                }
            };
            if published == -1 {
                return Err(io::Error::last_os_error());
            }
            if durable {
                // The target name is already committed. Returning an ordinary
                // error here would invite callers to retry a write that did in
                // fact publish. Keep the committed entity authoritative and
                // report the directory durability degradation separately.
                if let Err(error) = self.sync() {
                    tracing::warn!(
                        path = %self.path.join(name).display(),
                        %error,
                        "atomic write committed but directory sync failed"
                    );
                }
            }
            Ok(())
        })();
        if result.is_err() {
            unsafe {
                libc::unlinkat(self.handle.as_raw_fd(), tmp.as_ptr(), 0);
            }
        }
        result
    }
}

#[cfg(windows)]
#[derive(Debug)]
pub(crate) struct ContainedDirectory {
    path: PathBuf,
    handle: cap_std::fs::Dir,
}

#[cfg(windows)]
impl ContainedDirectory {
    pub(crate) fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            path: self.path.clone(),
            handle: self.handle.try_clone()?,
        })
    }

    pub(crate) fn is_same_entity(&self, other: &Self) -> io::Result<bool> {
        use std::os::windows::fs::MetadataExt as _;

        let left = self.handle.try_clone()?.into_std_file().metadata()?;
        let right = other.handle.try_clone()?.into_std_file().metadata()?;
        let Some(left_identity) = left.volume_serial_number().zip(left.file_index()) else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Windows filesystem did not expose a stable directory identity",
            ));
        };
        let Some(right_identity) = right.volume_serial_number().zip(right.file_index()) else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Windows filesystem did not expose a stable directory identity",
            ));
        };
        Ok(left_identity == right_identity)
    }

    /// Re-label an already pinned directory after its parent has atomically
    /// renamed the directory entry. The capability itself is unchanged; the
    /// path is display-only and must never be reopened as authority.
    pub(crate) fn rebind_child_display_path(
        mut self,
        parent: &Self,
        child_name: &std::ffi::OsStr,
    ) -> Self {
        debug_assert_eq!(Path::new(child_name).file_name(), Some(child_name));
        self.path = parent.path.join(child_name);
        self
    }

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

    pub(crate) fn open(
        root: &Path,
        relative: &Path,
        description: &str,
        create_missing: bool,
    ) -> io::Result<Self> {
        use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};

        // Open the authority itself without traversing a reparse point. The
        // capability therefore names the directory we validated, even if the
        // ambient path is concurrently renamed or replaced afterwards.
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        let root_handle = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(root)?;
        let metadata = root_handle.metadata()?;
        if metadata.file_attributes() & Self::FILE_ATTRIBUTE_REPARSE_POINT != 0
            || !metadata.is_dir()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{description} authority is not a regular directory"),
            ));
        }
        let handle = cap_std::fs::Dir::from_std_file(root_handle);
        let root = Self {
            path: root.to_path_buf(),
            handle,
        };
        root.open_relative(relative, description, create_missing)
    }

    pub(crate) fn open_relative(
        &self,
        relative: &Path,
        description: &str,
        create_missing: bool,
    ) -> io::Result<Self> {
        let mut handle = self.handle.try_clone()?;
        let mut path = self.path.clone();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{description} path must be relative and contained"),
                ));
            };
            handle = Self::open_child_directory(&handle, name, description, create_missing)?;
            path.push(name);
        }
        Ok(Self { path, handle })
    }

    pub(crate) fn display_path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn list_names(&self) -> io::Result<Vec<std::ffi::OsString>> {
        let mut names = Vec::new();
        for entry in self.handle.entries()? {
            names.push(entry?.file_name());
        }
        Ok(names)
    }

    /// Visit child names without materializing the directory.
    pub(crate) fn visit_names(
        &self,
        mut visit: impl FnMut(&std::ffi::OsStr) -> io::Result<()>,
    ) -> io::Result<()> {
        for entry in self.handle.entries()? {
            let name = entry?.file_name();
            visit(&name)?;
        }
        Ok(())
    }

    pub(crate) fn create_child(
        &self,
        name: &std::ffi::OsStr,
        description: &str,
    ) -> io::Result<Self> {
        Self::component(name)?;
        self.handle.create_dir(name)?;
        self.sync()?;
        let handle = Self::open_regular_child_directory(&self.handle, name, description)?;
        Ok(Self {
            path: self.path.join(name),
            handle,
        })
    }

    pub(crate) fn sync_tree(&self) -> io::Result<()> {
        for name in self.list_names()? {
            match self.open_relative(Path::new(&name), "contained child directory", false) {
                Ok(directory) => directory.sync_tree()?,
                Err(directory_error) => match self.open_regular(&name, "contained child file") {
                    Ok(file) => sync_file_durable(&file)?,
                    Err(_) => return Err(directory_error),
                },
            }
        }
        self.sync()
    }

    pub(crate) fn remove_tree_child(&self, name: &std::ffi::OsStr) -> io::Result<()> {
        match self.open_relative(Path::new(name), "contained child directory", false) {
            Ok(directory) => {
                directory.remove_all_contents()?;
                self.remove_empty_child(name, true)
            }
            Err(_) => {
                use cap_fs_ext::DirExt as _;
                self.handle.remove_file_or_symlink(name)?;
                self.sync()
            }
        }
    }

    pub(crate) fn remove_all_contents(&self) -> io::Result<()> {
        for child in self.list_names()? {
            match self.open_relative(Path::new(&child), "contained child directory", false) {
                Ok(_) => self.remove_tree_child(&child)?,
                Err(_) => self.remove_file(&child, false)?,
            }
        }
        self.sync()
    }

    pub(crate) fn remove_empty_child(
        &self,
        name: &std::ffi::OsStr,
        durable: bool,
    ) -> io::Result<()> {
        Self::component(name)?;
        self.handle.remove_dir(name)?;
        if durable && let Err(error) = self.sync() {
            tracing::warn!(%error, "directory removal committed but parent sync failed");
        }
        Ok(())
    }

    pub(crate) fn rename_child_no_replace(
        &self,
        source: &std::ffi::OsStr,
        target: &std::ffi::OsStr,
    ) -> io::Result<()> {
        use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt as _};
        use cap_std::fs::OpenOptionsExt as _;
        use std::os::windows::ffi::OsStrExt as _;
        use std::os::windows::io::AsRawHandle as _;
        use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, HANDLE};
        use windows::Win32::Storage::FileSystem::{
            DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_RENAME_INFO,
            FILE_RENAME_INFO_0, FileRenameInfo, SetFileInformationByHandle,
        };

        Self::component(source)?;
        Self::component(target)?;
        let mut options = cap_std::fs::OpenOptions::new();
        options.access_mode(DELETE.0);
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | FILE_FLAG_BACKUP_SEMANTICS.0);
        options.follow(FollowSymlinks::No);
        let source_file = self.handle.open_with(source, &options)?.into_std();
        let parent_file = self.handle.try_clone()?.into_std_file();
        let target_display = self.path.join(target);
        let target = target.encode_wide().collect::<Vec<_>>();
        let header = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
        let byte_len = header
            .checked_add(target.len().saturating_mul(std::mem::size_of::<u16>()))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "rename target is too long")
            })?;
        let words = byte_len.div_ceil(std::mem::size_of::<usize>());
        let mut buffer = vec![0usize; words];
        let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        unsafe {
            (*info).Anonymous = FILE_RENAME_INFO_0 {
                ReplaceIfExists: false,
            };
            (*info).RootDirectory = HANDLE(parent_file.as_raw_handle());
            (*info).FileNameLength =
                u32::try_from(target.len().saturating_mul(2)).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "rename target is too long")
                })?;
            std::ptr::copy_nonoverlapping(
                target.as_ptr(),
                (*info).FileName.as_mut_ptr(),
                target.len(),
            );
            SetFileInformationByHandle(
                HANDLE(source_file.as_raw_handle()),
                FileRenameInfo,
                info.cast(),
                u32::try_from(byte_len).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "rename buffer is too large")
                })?,
            )
            .map_err(|error| {
                if error.code() == ERROR_ALREADY_EXISTS.to_hresult()
                    || error.code() == ERROR_FILE_EXISTS.to_hresult()
                {
                    io::Error::new(io::ErrorKind::AlreadyExists, target_display)
                } else {
                    io::Error::other(error)
                }
            })?;
        }
        Ok(())
    }

    fn open_child_directory(
        parent: &cap_std::fs::Dir,
        name: &std::ffi::OsStr,
        description: &str,
        create_missing: bool,
    ) -> io::Result<cap_std::fs::Dir> {
        Self::component(name)?;
        match Self::open_regular_child_directory(parent, name, description) {
            Ok(directory) => Ok(directory),
            Err(error) if error.kind() == io::ErrorKind::NotFound && create_missing => {
                match parent.create_dir(name) {
                    Ok(()) => parent.try_clone()?.into_std_file().sync_all()?,
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error),
                }
                Self::open_regular_child_directory(parent, name, description)
            }
            Err(error) => Err(io::Error::new(
                error.kind(),
                format!("{description} is not a contained regular directory: {error}"),
            )),
        }
    }

    fn open_regular_child_directory(
        parent: &cap_std::fs::Dir,
        name: &std::ffi::OsStr,
        description: &str,
    ) -> io::Result<cap_std::fs::Dir> {
        use cap_fs_ext::DirExt as _;
        use std::os::windows::fs::MetadataExt as _;

        let directory = parent.open_dir_nofollow(name).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("{description} is not a contained regular directory: {error}"),
            )
        })?;
        let metadata = directory.try_clone()?.into_std_file().metadata()?;
        if metadata.file_attributes() & Self::FILE_ATTRIBUTE_REPARSE_POINT != 0
            || !metadata.is_dir()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{description} is not a contained regular directory"),
            ));
        }
        Ok(directory)
    }

    fn component(name: &std::ffi::OsStr) -> io::Result<()> {
        if Path::new(name).components().count() != 1
            || !matches!(
                Path::new(name).components().next(),
                Some(Component::Normal(_))
            )
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "contained file name must be one normal component",
            ));
        }
        Ok(())
    }

    pub(crate) fn open_read_write_create(
        &self,
        name: &std::ffi::OsStr,
    ) -> io::Result<std::fs::File> {
        use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt as _};

        Self::component(name)?;
        let mut options = cap_std::fs::OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create(true)
            .follow(FollowSymlinks::No);
        let file = self.handle.open_with(name, &options)?;
        Self::into_regular_file(file, "contained write target")
    }

    pub(crate) fn read_bounded(
        &self,
        name: &std::ffi::OsStr,
        description: &str,
        max_bytes: u64,
    ) -> io::Result<Vec<u8>> {
        let mut file = self.open_regular(name, description)?;
        let metadata = file.metadata()?;
        if metadata.len() > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{description} exceeds the byte limit"),
            ));
        }
        let mut bytes = Vec::new();
        file.take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{description} grew while reading"),
            ));
        }
        Ok(bytes)
    }

    pub(crate) fn open_regular(
        &self,
        name: &std::ffi::OsStr,
        description: &str,
    ) -> io::Result<std::fs::File> {
        use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt as _};

        Self::component(name)?;
        let mut options = cap_std::fs::OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let file = self.handle.open_with(name, &options)?;
        Self::into_regular_file(file, description)
    }

    fn into_regular_file(file: cap_std::fs::File, description: &str) -> io::Result<std::fs::File> {
        use std::os::windows::fs::MetadataExt as _;

        let file = file.into_std();
        let metadata = file.metadata()?;
        if metadata.file_attributes() & Self::FILE_ATTRIBUTE_REPARSE_POINT != 0
            || !metadata.is_file()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{description} is not a regular file"),
            ));
        }
        Ok(file)
    }

    pub(crate) fn sync(&self) -> io::Result<()> {
        self.handle.try_clone()?.into_std_file().sync_all()
    }

    pub(crate) fn remove_file(&self, name: &std::ffi::OsStr, durable: bool) -> io::Result<()> {
        Self::component(name)?;
        self.handle.remove_file(name)?;
        if durable {
            self.sync()?;
        }
        Ok(())
    }

    pub(crate) fn write_atomic(
        &self,
        name: &std::ffi::OsStr,
        bytes: &[u8],
        durable: bool,
        replace: bool,
    ) -> io::Result<()> {
        use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt as _};

        Self::component(name)?;
        let tmp_name = format!(
            ".{}.{}.tmp",
            std::process::id(),
            uuid::Uuid::now_v7().simple()
        );
        let mut options = cap_std::fs::OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        let mut file = Self::into_regular_file(
            self.handle.open_with(&tmp_name, &options)?,
            "contained temporary file",
        )?;
        let result = (|| {
            file.write_all(bytes)?;
            if durable {
                file.sync_all()?;
            }
            drop(file);
            if replace {
                self.handle.rename(&tmp_name, &self.handle, name)?;
            } else {
                self.handle.hard_link(&tmp_name, &self.handle, name)?;
                if let Err(error) = self.handle.remove_file(&tmp_name) {
                    tracing::warn!(
                        path = %self.path.join(&tmp_name).display(),
                        %error,
                        "atomic create committed but temporary link cleanup failed"
                    );
                }
            }
            if durable {
                // The target name is already committed. Returning an ordinary
                // error here would invite callers to retry a write that did in
                // fact publish. Keep the committed entity authoritative and
                // report the directory durability degradation separately.
                if let Err(error) = self.sync() {
                    tracing::warn!(
                        path = %self.path.join(name).display(),
                        %error,
                        "atomic write committed but directory sync failed"
                    );
                }
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = self.handle.remove_file(&tmp_name);
        }
        result
    }
}

#[cfg(not(any(unix, windows)))]
#[derive(Debug)]
pub(crate) struct ContainedDirectory;

#[cfg(not(any(unix, windows)))]
impl ContainedDirectory {
    pub(crate) fn open(
        _root: &Path,
        _relative: &Path,
        _description: &str,
        _create_missing: bool,
    ) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "handle-relative contained storage is unsupported on this platform",
        ))
    }
}

pub(crate) fn write_contained_atomic_durable(
    root: &Path,
    relative: &Path,
    bytes: &[u8],
) -> io::Result<()> {
    write_contained_atomic_inner(root, relative, bytes, true, true)
}

pub(crate) fn write_contained_new_durable(
    root: &Path,
    relative: &Path,
    bytes: &[u8],
) -> io::Result<()> {
    write_contained_atomic_inner(root, relative, bytes, true, false)
}

fn write_contained_atomic_inner(
    root: &Path,
    relative: &Path,
    bytes: &[u8],
    durable: bool,
    replace: bool,
) -> io::Result<()> {
    let parent = relative.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "contained target has no parent",
        )
    })?;
    let name = relative.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "contained target has no file name",
        )
    })?;
    let directory = ContainedDirectory::open(root, parent, "contained write directory", true)?;
    #[cfg(any(unix, windows))]
    {
        directory.write_atomic(name, bytes, durable, replace)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (directory, name, bytes, durable, replace);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "handle-relative contained storage is unsupported on this platform",
        ))
    }
}

fn completed_sideband_result<'a>(
    ledgers: &'a SidebandLedgers,
    sideband_id: &str,
    result_seq: u64,
    owner: &str,
) -> io::Result<&'a chat_state::SidebandResult> {
    let result_index = usize::try_from(result_seq).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{owner} result seq exceeds platform capacity"),
        )
    })?;
    let events = ledgers.get(sideband_id).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{owner} references missing sideband {sideband_id}"),
        )
    })?;
    let result = events
        .get(result_index)
        .and_then(|event| match &event.kind {
            chat_state::SidebandEventKind::Result(result) => Some(result),
            _ => None,
        });
    let terminal_index = result_index.checked_add(1).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{owner} result seq exceeds platform capacity"),
        )
    })?;
    let completed = matches!(
        events.get(terminal_index).map(|event| &event.kind),
        Some(chat_state::SidebandEventKind::End(
            chat_state::SidebandEnd {
                outcome: chat_state::SidebandOutcome::Completed,
                error: None,
            }
        ))
    ) && events.len() == terminal_index.saturating_add(1);
    match (result, completed) {
        (Some(result), true) => Ok(result),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{owner} references unproven result {sideband_id}/{result_seq}"),
        )),
    }
}

/// Read every committed record from the canonical Timeline ledger.
///
/// A non-newline-terminated tail was never committed and is ignored. Every
/// complete record is parsed strictly; missing ledgers and interior corruption
/// fail closed for every consumer, including resume, history, and search.
pub(crate) fn read_timeline_file(path: &Path) -> io::Result<Vec<chat_state::TimelineEvent>> {
    read_committed_jsonl_file(path, "mandatory Timeline ledger")
}

/// Resolve a session by its globally unique id and fold its canonical
/// Timeline through the same pinned capability that validated Summary.
pub fn load_timeline_by_id_at(
    session_id: &str,
    grow_home: &Path,
) -> io::Result<Option<chat_state::Timeline>> {
    let storage = JsonlStorageAdapter::with_root(grow_home.to_path_buf());
    let Some(opened) = storage.open_session_by_id(session_id)? else {
        return Ok(None);
    };
    let timeline = chat_state::Timeline::from_events(opened.timeline_events()?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(Some(timeline))
}

pub fn load_timeline_by_id(session_id: &str) -> io::Result<Option<chat_state::Timeline>> {
    load_timeline_by_id_at(session_id, &crate::util::grow_home::grow_home())
}

const MAX_SESSION_TRACE_FILES: usize = 4096;
const MAX_SESSION_TRACE_DEPTH: usize = 32;
const MAX_SESSION_TRACE_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SESSION_TRACE_TOTAL_BYTES: u64 = 128 * 1024 * 1024;

const RESOURCES_STATE_FILE: &str = "resources_state.json";
const MAX_RESOURCES_STATE_BYTES: u64 = 16 * 1024 * 1024;

struct SessionResourcesStateStore {
    directory: std::sync::Arc<ContainedDirectory>,
    display_path: PathBuf,
}

impl tools::persistence::ResourcesStateStore for SessionResourcesStateStore {
    fn display_path(&self) -> &Path {
        &self.display_path
    }

    fn read(&self) -> io::Result<Option<Vec<u8>>> {
        match self.directory.read_bounded(
            std::ffi::OsStr::new(RESOURCES_STATE_FILE),
            "resources state",
            MAX_RESOURCES_STATE_BYTES,
        ) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn write_atomic(&self, bytes: &[u8], durable: bool) -> io::Result<()> {
        if bytes.len() as u64 > MAX_RESOURCES_STATE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "resources state exceeds the byte limit",
            ));
        }
        self.directory.write_atomic(
            std::ffi::OsStr::new(RESOURCES_STATE_FILE),
            bytes,
            durable,
            true,
        )
    }
}

pub(crate) fn resources_persistence(
    directory: std::sync::Arc<ContainedDirectory>,
) -> std::sync::Arc<tools::persistence::ResourcesPersistence> {
    let display_path = directory.display_path().join(RESOURCES_STATE_FILE);
    std::sync::Arc::new(tools::persistence::ResourcesPersistence::new(
        std::sync::Arc::new(SessionResourcesStateStore {
            directory,
            display_path,
        }),
    ))
}

/// One regular file captured through a pinned session-directory capability.
#[derive(Debug)]
pub struct SessionTraceFile {
    pub relative_path: PathBuf,
    pub bytes: Vec<u8>,
}

/// Identity-checked, bounded input for the pager's diagnostic archive writer.
#[derive(Debug)]
pub struct SessionTraceSnapshot {
    pub session_id: String,
    pub files: Vec<SessionTraceFile>,
}

/// Resolve one canonical session and capture its regular files without ever
/// reopening its ambient path. Symlinks, special files, excessive depth,
/// excessive file count, and excessive byte volume fail the whole export.
pub fn load_session_trace(session_id: &str) -> io::Result<Option<SessionTraceSnapshot>> {
    load_session_trace_at(session_id, &crate::util::grow_home::grow_home())
}

pub fn load_session_trace_at(
    session_id: &str,
    grow_home: &Path,
) -> io::Result<Option<SessionTraceSnapshot>> {
    let storage = JsonlStorageAdapter::with_root(grow_home.to_path_buf());
    let Some(opened) = storage.open_session_by_id(session_id)? else {
        return Ok(None);
    };
    let mut files = Vec::new();
    let mut total_bytes = 0u64;
    collect_session_trace_files(
        opened.directory(),
        Path::new(""),
        0,
        &mut files,
        &mut total_bytes,
    )?;
    Ok(Some(SessionTraceSnapshot {
        session_id: opened.summary().info.id.0.to_string(),
        files,
    }))
}

fn collect_session_trace_files(
    directory: &ContainedDirectory,
    relative: &Path,
    depth: usize,
    files: &mut Vec<SessionTraceFile>,
    total_bytes: &mut u64,
) -> io::Result<()> {
    if depth > MAX_SESSION_TRACE_DEPTH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session trace directory depth exceeds the limit",
        ));
    }
    for name in directory.list_names()? {
        let child_relative = relative.join(&name);
        match directory.open_relative(&PathBuf::from(&name), "session trace directory", false) {
            Ok(child) => collect_session_trace_files(
                &child,
                &child_relative,
                depth.saturating_add(1),
                files,
                total_bytes,
            )?,
            Err(directory_error) => {
                let bytes = directory
                    .read_bounded(
                        &name,
                        "session trace regular file",
                        MAX_SESSION_TRACE_FILE_BYTES,
                    )
                    .map_err(|file_error| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!(
                                "session trace entry '{}' is neither a contained directory nor a regular file: directory={directory_error}; file={file_error}",
                                child_relative.display()
                            ),
                        )
                    })?;
                *total_bytes = total_bytes.checked_add(bytes.len() as u64).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "session trace byte count overflow",
                    )
                })?;
                if *total_bytes > MAX_SESSION_TRACE_TOTAL_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "session trace exceeds the total byte limit",
                    ));
                }
                if files.len() >= MAX_SESSION_TRACE_FILES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "session trace exceeds the file-count limit",
                    ));
                }
                files.push(SessionTraceFile {
                    relative_path: child_relative,
                    bytes,
                });
            }
        }
    }
    Ok(())
}

pub(crate) fn read_committed_jsonl_from_directory<T: serde::de::DeserializeOwned>(
    directory: &ContainedDirectory,
    name: &std::ffi::OsStr,
    description: &str,
    max_entry_bytes: u64,
) -> io::Result<Vec<T>> {
    let file = directory.open_regular(name, description)?;
    let path = directory.display_path().join(name);
    read_committed_jsonl_from_file(file, path, description, max_entry_bytes)
}

/// Read complete records from an append-only JSONL ledger. A torn final
/// record was never acknowledged and is ignored; all complete records remain
/// strict. Callers choose the ledger-specific missing-file diagnostic.
pub(crate) fn read_committed_jsonl_file<T: serde::de::DeserializeOwned>(
    path: &Path,
    description: &str,
) -> io::Result<Vec<T>> {
    read_committed_jsonl_file_with_limit(path, description, MAX_JSONL_ENTRY_BYTES)
}

fn read_committed_jsonl_file_with_limit<T: serde::de::DeserializeOwned>(
    path: &Path,
    description: &str,
    max_entry_bytes: u64,
) -> io::Result<Vec<T>> {
    let file = open_regular_nofollow(path, description).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{description} is missing: {}", path.display()),
            )
        } else {
            error
        }
    })?;
    read_committed_jsonl_from_file(file, path.to_path_buf(), description, max_entry_bytes)
}

fn read_committed_jsonl_from_file<T: serde::de::DeserializeOwned>(
    file: std::fs::File,
    path: PathBuf,
    description: &str,
    max_entry_bytes: u64,
) -> io::Result<Vec<T>> {
    let mut lines =
        CommittedJsonlLines::from_file(file, path.clone(), description.to_owned(), max_entry_bytes);
    let mut items = Vec::new();
    while let Some(line) = lines.next() {
        let line = line?;
        let line_number = lines.line_number();
        let item = serde_json::from_slice(&line).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}:{line_number}: {error}", path.display()),
            )
        })?;
        items.push(item);
    }
    Ok(items)
}

/// Streaming reader for newline-committed JSONL records.
///
/// The final non-newline fragment is never visible. Each committed record is
/// bounded independently, and an oversized torn tail is scanned with the
/// fixed `BufReader` buffer instead of being allocated.
pub(crate) struct CommittedJsonlLines {
    reader: BufReader<std::fs::File>,
    line_buffer: Vec<u8>,
    path: PathBuf,
    description: String,
    max_entry_bytes: u64,
    line_number: u64,
    committed_position: u64,
}

impl CommittedJsonlLines {
    pub(crate) fn open(path: &Path, description: &str) -> io::Result<Option<Self>> {
        let Some(file) = open_optional_regular_nofollow(path, description)? else {
            return Ok(None);
        };
        Ok(Some(Self::from_file(
            file,
            path.to_path_buf(),
            description.to_owned(),
            MAX_JSONL_ENTRY_BYTES,
        )))
    }

    pub(crate) fn open_at(path: &Path, description: &str, offset: u64) -> io::Result<Option<Self>> {
        let Some(mut lines) = Self::open(path, description)? else {
            return Ok(None);
        };
        lines.reader.seek(SeekFrom::Start(offset))?;
        lines.committed_position = offset;
        Ok(Some(lines))
    }

    /// Start a committed-record stream from an already-opened regular file.
    /// `label` is diagnostic only; authority remains the pinned handle.
    pub(crate) fn from_open_file_at(
        file: std::fs::File,
        label: PathBuf,
        description: &str,
        offset: u64,
    ) -> io::Result<Self> {
        let mut lines = Self::from_file(file, label, description.to_owned(), MAX_JSONL_ENTRY_BYTES);
        lines.reader.seek(SeekFrom::Start(offset))?;
        lines.committed_position = offset;
        Ok(lines)
    }

    fn from_file(
        file: std::fs::File,
        path: PathBuf,
        description: String,
        max_entry_bytes: u64,
    ) -> Self {
        Self {
            reader: BufReader::new(file),
            line_buffer: Vec::new(),
            path,
            description,
            max_entry_bytes,
            line_number: 0,
            committed_position: 0,
        }
    }

    pub(crate) fn stream_position(&mut self) -> io::Result<u64> {
        Ok(self.committed_position)
    }

    pub(crate) fn line_number(&self) -> u64 {
        self.line_number
    }
}

impl Iterator for CommittedJsonlLines {
    type Item = io::Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.line_buffer.clear();
            let read = match (&mut self.reader)
                .take(self.max_entry_bytes.saturating_add(1))
                .read_until(b'\n', &mut self.line_buffer)
            {
                Ok(read) => read,
                Err(error) => return Some(Err(error)),
            };
            if read == 0 {
                return None;
            }
            self.line_number = self.line_number.saturating_add(1);
            if self.line_buffer.len() as u64 > self.max_entry_bytes {
                let mut committed = self.line_buffer.ends_with(b"\n");
                while !committed {
                    let buffered = match self.reader.fill_buf() {
                        Ok(buffered) => buffered,
                        Err(error) => return Some(Err(error)),
                    };
                    if buffered.is_empty() {
                        return None;
                    }
                    if let Some(index) = buffered.iter().position(|byte| *byte == b'\n') {
                        self.reader.consume(index.saturating_add(1));
                        committed = true;
                    } else {
                        let len = buffered.len();
                        self.reader.consume(len);
                    }
                }
                self.committed_position = match self.reader.stream_position() {
                    Ok(position) => position,
                    Err(error) => return Some(Err(error)),
                };
                return Some(Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "{} entry exceeds {} bytes at {}:{}",
                        self.description,
                        self.max_entry_bytes,
                        self.path.display(),
                        self.line_number
                    ),
                )));
            }
            if !self.line_buffer.ends_with(b"\n") {
                return None;
            }
            self.committed_position = match self.reader.stream_position() {
                Ok(position) => position,
                Err(error) => return Some(Err(error)),
            };
            self.line_buffer.pop();
            if self.line_buffer.is_empty() {
                continue;
            }
            return Some(Ok(std::mem::take(&mut self.line_buffer)));
        }
    }
}

pub(crate) fn open_regular_nofollow(path: &Path, description: &str) -> io::Result<std::fs::File> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "regular file path has no parent",
        )
    })?;
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "regular file path has no file name",
        )
    })?;
    let directory = ContainedDirectory::open(parent, Path::new(""), description, false)?;
    directory.open_regular(name, description)
}

pub(crate) fn open_optional_regular_nofollow(
    path: &Path,
    description: &str,
) -> io::Result<Option<std::fs::File>> {
    match open_regular_nofollow(path, description) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn open_read_write_create_nofollow(path: &Path) -> io::Result<std::fs::File> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "write target has no parent"))?;
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "write target has no file name")
    })?;
    let directory =
        ContainedDirectory::open(parent, Path::new(""), "write target directory", false)?;
    directory.open_read_write_create(name)
}

pub(crate) fn read_bounded_regular_file(
    path: &Path,
    description: &str,
    max_bytes: u64,
) -> io::Result<Vec<u8>> {
    let mut file = open_regular_nofollow(path, description)?;
    if file.metadata()?.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{description} exceeds {max_bytes} bytes: {}",
                path.display()
            ),
        ));
    }
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{description} changed during read: {}", path.display()),
        ));
    }
    Ok(bytes)
}

pub(crate) fn serialize_summary(summary: &Summary) -> io::Result<Vec<u8>> {
    summary.validate_current_format()?;
    let bytes = serde_json::to_vec_pretty(summary)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if bytes.len() as u64 > MAX_SESSION_SUMMARY_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("session summary exceeds {MAX_SESSION_SUMMARY_BYTES} bytes"),
        ));
    }
    Ok(bytes)
}

/// Validate every persisted independent ledger against its parent Timeline,
/// including title events whose provenance crosses the ledger boundary.
pub(crate) fn validate_sideband_ledgers(
    parent_timeline_id: &str,
    parent: &chat_state::Timeline,
    ledgers: &SidebandLedgers,
) -> io::Result<()> {
    let spawns = parent
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            chat_state::TimelineEventKind::Sideband(spawn) => {
                Some((spawn.sideband_id.as_str(), (event.seq.get(), spawn)))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();

    for (sideband_id, events) in ledgers {
        let (spawn_seq, spawn) = spawns.get(sideband_id.as_str()).copied().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("sideband {sideband_id} has no parent spawn fact"),
            )
        })?;
        if events.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("sideband {sideband_id} ledger is empty"),
            ));
        }
        let timeline = chat_state::SidebandTimeline::from_events(events.clone())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        timeline
            .validate_parent(parent_timeline_id, parent, spawn_seq, spawn)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    }

    for (_, spawn) in spawns.values() {
        if let Some(source_ref) = spawn
            .source_refs
            .iter()
            .find(|source_ref| source_ref.timeline_id != parent_timeline_id)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "sideband {} references foreign Timeline {}",
                    spawn.sideband_id, source_ref.timeline_id
                ),
            ));
        }
    }

    for event in parent.events() {
        let chat_state::TimelineEventKind::Compaction(chat_state::CompactionEvent::Summary {
            result_ref,
            summary_chars,
            ..
        }) = &event.kind
        else {
            continue;
        };
        let result = completed_sideband_result(
            ledgers,
            &result_ref.timeline_id,
            result_ref.first_seq,
            "compaction/summary",
        )?;
        if result.raw_output.chars().count() != *summary_chars {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "compaction/summary character count does not match sideband {}/{}",
                    result_ref.timeline_id, result_ref.first_seq
                ),
            ));
        }
    }

    for event in parent.events() {
        let chat_state::TimelineEventKind::SessionTitle(title) = &event.kind else {
            continue;
        };
        match &title.source {
            chat_state::SessionTitleSource::User => {}
            chat_state::SessionTitleSource::Generated {
                sideband_id,
                result_seq,
            } => {
                completed_sideband_result(
                    ledgers,
                    sideband_id,
                    *result_seq,
                    "generated session/title",
                )?;
            }
            chat_state::SessionTitleSource::Fallback {
                sideband_id,
                terminal_seq,
            } => {
                let terminal_index = usize::try_from(*terminal_seq).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "fallback session/title terminal seq exceeds platform capacity",
                    )
                })?;
                let terminal = ledgers
                    .get(sideband_id)
                    .and_then(|events| events.get(terminal_index));
                if !matches!(
                    terminal.map(|event| &event.kind),
                    Some(chat_state::SidebandEventKind::End(
                        chat_state::SidebandEnd {
                            outcome: chat_state::SidebandOutcome::Failed
                                | chat_state::SidebandOutcome::Cancelled,
                            error: Some(_),
                        }
                    ))
                ) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "fallback session/title references unproven terminal event {sideband_id}/{terminal_seq}"
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Write `bytes` to `path` by writing a uniquely named sibling temp file and
/// renaming it over the target, so a crash or a concurrent writer never leaves a
/// torn file. The temp is removed on failure.
pub(crate) fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_bytes_atomic_inner(path, bytes, false)
}

pub(crate) fn write_bytes_atomic_durable(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_bytes_atomic_inner(path, bytes, true)
}

fn write_bytes_atomic_inner(path: &Path, bytes: &[u8], durable: bool) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "atomic write path has no parent",
        )
    })?;
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "atomic write path has no file name",
        )
    })?;
    let directory =
        ContainedDirectory::open(parent, Path::new(""), "atomic write directory", false)?;
    #[cfg(any(unix, windows))]
    {
        directory.write_atomic(name, bytes, durable, true)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (directory, name, bytes, durable);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "handle-relative atomic writes are unsupported on this platform",
        ))
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn sync_file_durable(file: &std::fs::File) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    file.sync_all()?;
    fullfsync_raw(file.as_raw_fd())
}

#[cfg(target_os = "macos")]
pub(crate) fn fullfsync_raw(fd: std::os::fd::RawFd) -> io::Result<()> {
    // macOS fsync may stop at volatile drive caches; F_FULLFSYNC requests stable media.
    if unsafe { libc::fcntl(fd, libc::F_FULLFSYNC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(any(all(unix, not(target_os = "macos")), windows))]
pub(crate) fn sync_file_durable(file: &std::fs::File) -> io::Result<()> {
    file.sync_all()
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn sync_file_durable(_file: &std::fs::File) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "durable file sync is unsupported on this platform",
    ))
}

/// Async sibling of [`write_bytes_atomic`].
pub(crate) async fn write_bytes_atomic_async(path: &Path, bytes: Vec<u8>) -> io::Result<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || write_bytes_atomic(&path, &bytes))
        .await
        .map_err(io::Error::other)?
}

/// Atomically replace a control-plane file and do not acknowledge until both
/// the new file contents and its directory entry have crossed a durability
/// barrier. This is intentionally reserved for state whose caller changes
/// live ownership only after the write returns.
pub(crate) async fn write_bytes_atomic_durable_async(
    path: &Path,
    bytes: Vec<u8>,
) -> io::Result<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || write_bytes_atomic_inner(&path, &bytes, true))
        .await
        .map_err(io::Error::other)?
}

/// Serialize `items` to newline-delimited JSON bytes.
pub(crate) fn to_jsonl_bytes<T: serde::Serialize>(items: &[T]) -> io::Result<Vec<u8>> {
    let mut content = Vec::new();
    for item in items {
        let start = content.len();
        serde_json::to_writer(&mut content, item)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        content.push(b'\n');
        if content.len().saturating_sub(start) as u64 > MAX_JSONL_ENTRY_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("JSONL entry exceeds {MAX_JSONL_ENTRY_BYTES} bytes"),
            ));
        }
    }
    Ok(content)
}

/// Write `items` as newline-delimited JSON to `path`, atomically (see
/// [`write_bytes_atomic`]).
pub(crate) fn write_jsonl_atomic<T: serde::Serialize>(path: &Path, items: &[T]) -> io::Result<()> {
    write_bytes_atomic(path, &to_jsonl_bytes(items)?)
}

/// Async sibling of [`write_jsonl_atomic`].
pub(crate) async fn write_jsonl_atomic_async<T: serde::Serialize>(
    path: &Path,
    items: &[T],
) -> io::Result<()> {
    write_bytes_atomic_async(path, to_jsonl_bytes(items)?).await
}

/// Iterator that streams session updates from a JSONL file without loading all into memory.
/// Each call to `next()` reads and parses one line.
pub struct UpdatesIterator {
    lines: CommittedJsonlLines,
}

impl UpdatesIterator {
    pub(crate) fn from_file(file: std::fs::File, path: PathBuf) -> Self {
        Self {
            lines: CommittedJsonlLines::from_file(
                file,
                path,
                "session updates ledger".to_owned(),
                MAX_JSONL_ENTRY_BYTES,
            ),
        }
    }

    /// Create a new iterator over updates in the given file.
    /// Returns None if the file doesn't exist.
    pub fn open(path: &Path) -> io::Result<Option<Self>> {
        let Some(lines) = CommittedJsonlLines::open(path, "session updates ledger")? else {
            return Ok(None);
        };
        Ok(Some(Self { lines }))
    }

    /// Create a new iterator starting at the given byte offset.
    /// Returns None if the file doesn't exist.
    /// Used for delta replay: read only updates appended after a known offset.
    pub fn open_at(path: &Path, offset: u64) -> io::Result<Option<Self>> {
        let Some(lines) = CommittedJsonlLines::open_at(path, "session updates ledger", offset)?
        else {
            return Ok(None);
        };
        Ok(Some(Self { lines }))
    }

    /// Returns the current byte position in the underlying file.
    /// After iterating, this is the offset of the next unread byte (i.e., EOF
    /// if all updates were consumed). Used to record the replay end offset for
    /// subsequent delta replay.
    pub fn stream_position(&mut self) -> io::Result<u64> {
        self.lines.stream_position()
    }
}

impl Iterator for UpdatesIterator {
    type Item = io::Result<SessionUpdate>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let line = match self.lines.next()? {
                Ok(line) => line,
                Err(error) => return Some(Err(error)),
            };
            let line = match std::str::from_utf8(line.trim_ascii()) {
                Ok("") => continue,
                Ok(line) => line,
                Err(error) => {
                    return Some(Err(io::Error::new(io::ErrorKind::InvalidData, error)));
                }
            };
            return Some(
                SessionUpdateEnvelope::from_str(line)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
            );
        }
    }
}

/// Method name for standard ACP session/update notifications.
const ACP_SESSION_UPDATE_METHOD: &str = "session/update";

/// Method name for Grow extension session/update notifications.
pub(crate) const GROW_SESSION_UPDATE_METHOD: &str = "_grow/session/update";

/// A unified session update that can be either an ACP notification or a Grow extension notification.
/// This allows storing all session updates in chronological order.
///
/// Note: The `Serialize` implementation produces a format without timestamp.
/// For local JSONL storage with timestamps, use `SessionUpdateEnvelope`.
#[derive(Debug, Clone)]
pub enum SessionUpdate {
    /// Standard ACP session/update notification (boxed due to large size)
    Acp(Box<acp::SessionNotification>),
    /// Grow extension session notification (e.g., diff_review)
    Grow(Box<SessionNotification>),
}

impl serde::Serialize for SessionUpdate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(Some(2))?;
        match self {
            SessionUpdate::Acp(notification) => {
                map.serialize_entry("method", ACP_SESSION_UPDATE_METHOD)?;
                map.serialize_entry("params", notification)?;
            }
            SessionUpdate::Grow(notification) => {
                map.serialize_entry("method", GROW_SESSION_UPDATE_METHOD)?;
                map.serialize_entry("params", notification)?;
            }
        }
        map.end()
    }
}

impl<'de> serde::Deserialize<'de> for SessionUpdate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // `updates.jsonl` and wire consumers share one method/params envelope.
        let value = serde_json::Value::deserialize(deserializer)?;
        SessionUpdateEnvelope::from_value(value).map_err(serde::de::Error::custom)
    }
}

/// The serialized envelope for a session update, including metadata for debugging.
/// This is the typed structure that gets written to updates.jsonl (disk storage only).
///
/// Note: This is separate from `SessionUpdate`'s own serialization to avoid affecting
/// other consumers (e.g., network listeners) who don't need the timestamp metadata.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SessionUpdateEnvelope {
    /// Unix timestamp (seconds since epoch) when this update was written.
    /// Useful for debugging timing issues in the updates.jsonl file.
    #[serde(default)]
    pub timestamp: u64,
    /// The method name identifying the update type.
    /// Either "session/update" for ACP or "_grow/session/update" for Grow extensions.
    pub method: String,
    /// The actual notification payload.
    pub params: serde_json::Value,
}

impl SessionUpdateEnvelope {
    /// Create a new envelope with the current timestamp for disk storage.
    pub(crate) fn from_update(update: &SessionUpdate) -> Result<Self, serde_json::Error> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        match update {
            SessionUpdate::Acp(notification) => Ok(Self {
                timestamp,
                method: ACP_SESSION_UPDATE_METHOD.to_string(),
                params: serde_json::to_value(notification)?,
            }),
            SessionUpdate::Grow(notification) => Ok(Self {
                timestamp,
                method: GROW_SESSION_UPDATE_METHOD.to_string(),
                params: serde_json::to_value(notification)?,
            }),
        }
    }

    /// Convert this envelope back into a SessionUpdate.
    pub(crate) fn into_update(self) -> Result<SessionUpdate, serde_json::Error> {
        match self.method.as_str() {
            GROW_SESSION_UPDATE_METHOD => {
                let notification: SessionNotification = serde_json::from_value(self.params)?;
                Ok(SessionUpdate::Grow(Box::new(notification)))
            }
            ACP_SESSION_UPDATE_METHOD => {
                let notification: acp::SessionNotification = serde_json::from_value(self.params)?;
                Ok(SessionUpdate::Acp(Box::new(notification)))
            }
            method => Err(invalid_update_envelope(format!(
                "unsupported session update method {method:?}"
            ))),
        }
    }

    /// Parse the canonical method/params envelope from a JSON value.
    pub(crate) fn from_value(value: serde_json::Value) -> Result<SessionUpdate, serde_json::Error> {
        let envelope: SessionUpdateEnvelope = serde_json::from_value(value)?;
        envelope.into_update()
    }

    /// Parse a session update directly from a JSON string, avoiding intermediate `Value` allocation.
    ///
    /// Uses a borrowing envelope with `&RawValue` for the params field so the JSON bytes
    /// for the notification payload are only parsed once (directly to the typed struct)
    /// instead of twice (str -> Value -> typed).
    pub(crate) fn from_str(line: &str) -> Result<SessionUpdate, serde_json::Error> {
        #[derive(serde::Deserialize)]
        struct BorrowedEnvelope<'a> {
            method: &'a str,
            #[serde(borrow)]
            params: &'a serde_json::value::RawValue,
        }

        let envelope = serde_json::from_str::<BorrowedEnvelope<'_>>(line)?;
        let raw_params = envelope.params.get();
        match envelope.method {
            GROW_SESSION_UPDATE_METHOD => {
                let notification: SessionNotification = serde_json::from_str(raw_params)?;
                Ok(SessionUpdate::Grow(Box::new(notification)))
            }
            ACP_SESSION_UPDATE_METHOD => {
                let notification: acp::SessionNotification = serde_json::from_str(raw_params)?;
                Ok(SessionUpdate::Acp(Box::new(notification)))
            }
            method => Err(invalid_update_envelope(format!(
                "unsupported session update method {method:?}"
            ))),
        }
    }
}

fn invalid_update_envelope(message: String) -> serde_json::Error {
    serde_json::Error::io(io::Error::new(io::ErrorKind::InvalidData, message))
}

/// All persisted data for a session
#[derive(Debug, Clone)]
pub struct PersistedData {
    pub summary: Summary,
    /// Immutable conversation facts. This is the restart source of truth.
    pub timeline_events: Vec<chat_state::TimelineEvent>,
    /// All session updates (ACP updates and Grow extension updates) in chronological order
    pub updates: Vec<SessionUpdate>,
    /// Latest Behavior/Goal projection folded from Timeline `Control` events.
    pub control_snapshot: Option<crate::session::control::SessionControlSnapshot>,
    /// Rewind points for session rewind functionality
    pub rewind_points: Vec<RewindPoint>,
    /// Latest session-signals projection folded from Timeline observations.
    pub signals: Option<crate::session::signals::SessionSignals>,
    /// Latest announcement projection folded from Timeline observations.
    pub announcement_state: Option<crate::session::announcement_state::AnnouncementState>,
    pub workflow_runs: Vec<crate::session::workflow::store::RestoredWorkflowRun>,
}

/// Persisted data WITHOUT updates - for memory-efficient session loading
#[derive(Debug, Clone)]
pub struct PersistedDataLight {
    pub summary: Summary,
    pub timeline_events: Vec<chat_state::TimelineEvent>,
    pub control_snapshot: Option<crate::session::control::SessionControlSnapshot>,
    // No `rewind_points` field: the resume path defers them (loaded lazily by
    // `FileStateTracker`). Use `load_session` for the eager set.
    /// Latest session-signals projection folded from Timeline observations.
    pub signals: Option<crate::session::signals::SessionSignals>,
    /// Latest announcement projection folded from Timeline observations.
    pub announcement_state: Option<crate::session::announcement_state::AnnouncementState>,
    pub workflow_runs: Vec<crate::session::workflow::store::RestoredWorkflowRun>,
}

/// Result of copying session data
#[derive(Debug, Clone)]
pub struct CopySessionResult {
    pub surface_items_copied: usize,
    pub updates_copied: usize,
    /// Whether a sanitized control event was seeded into the child Timeline.
    pub control_event_seeded: bool,
    /// Number of immutable large-prompt blobs referenced by the selected
    /// Surface and copied into the child lineage.
    pub prompt_blobs_copied: usize,
}

/// Options for copying session data during fork
#[derive(Debug, Clone)]
pub struct CopySessionOptions {
    /// Parent session ID to set in the forked session's summary.
    pub parent_session_id: Option<String>,
    /// Model ID override for the forked session (None = keep source model).
    pub new_model_id: Option<String>,
    /// Truncate copied history to this prompt index (0-based, inclusive).
    pub target_prompt_index: Option<usize>,
    /// When true, skip `transform_conversation_cwd` during copy.
    ///
    /// Set for forks where the child should see the original project path
    /// (e.g. worktree forks with a persisted `display_cwd`). Non-worktree
    /// forks should keep this false so conversation paths are rewritten to
    /// the new cwd.
    pub skip_cwd_transform: bool,
    /// Stable display path for fork sessions. Persisted in the forked
    /// summary so the prompt-facing cwd survives session restore/reload.
    pub prompt_display_cwd: Option<String>,

    // ── Generic fork extensions (used by subagent + worktree forks) ──
    /// Override `session_kind` in the forked summary. Defaults to `"fork"`.
    /// Subagent resume sets `"subagent_resume"`.
    pub session_kind: Option<String>,
    /// How the fork's initial context was bootstrapped: `"new"` or `"forked"`.
    pub fork_context_source: Option<String>,
    /// Parent prompt/turn ID that triggered this fork.
    pub fork_parent_prompt_id: Option<String>,
    /// Whether to seed sanitized parent control into the child Timeline.
    pub inherit_control: bool,
    /// When true, apply fork-safety filtering to the copied Surface:
    /// - Strip synthetic user messages (doom loop warnings, compaction metadata)
    /// - Truncate at the last complete turn boundary
    /// - Remove trailing incomplete assistant responses
    pub fork_filter: bool,
    /// When true, strip `reasoning` (thinking/reasoning_content) from all
    /// assistant messages in the copied Surface.
    ///
    /// Set for forks so that the new session does not inherit the prior
    /// model's chain-of-thought -- each fork starts with a clean slate
    /// for reasoning on the new prompt.
    pub strip_reasoning: bool,
    /// The original workspace directory this worktree session was spawned from.
    /// Propagated to the forked session's `Summary::source_workspace_dir`.
    pub source_workspace_dir: Option<String>,
}

impl Default for CopySessionOptions {
    fn default() -> Self {
        Self {
            parent_session_id: None,
            new_model_id: None,
            target_prompt_index: None,
            skip_cwd_transform: false,
            prompt_display_cwd: None,
            session_kind: None,
            fork_context_source: None,
            fork_parent_prompt_id: None,
            inherit_control: true,
            fork_filter: false,
            strip_reasoning: false,
            source_workspace_dir: None,
        }
    }
}

/// Chunk `_meta.promptIndex` on an ACP `UserMessageChunk`, if present.
fn acp_user_chunk_prompt_index(update: &SessionUpdate) -> Option<usize> {
    let SessionUpdate::Acp(n) = update else {
        return None;
    };
    let acp::SessionUpdate::UserMessageChunk(chunk) = &n.update else {
        return None;
    };
    chunk
        .meta
        .as_ref()
        .and_then(|m| m.get("promptIndex"))
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
}

pub(crate) const HOST_TURN_META_KEY: &str = "hostTurn";

pub(crate) fn is_host_turn_chunk(chunk: &acp::ContentChunk) -> bool {
    chunk
        .meta
        .as_ref()
        .and_then(|m| m.get(HOST_TURN_META_KEY))
        .and_then(|v| v.as_bool())
        == Some(true)
}

fn is_host_turn_update(update: &SessionUpdate) -> bool {
    let SessionUpdate::Acp(n) = update else {
        return false;
    };
    let acp::SessionUpdate::UserMessageChunk(chunk) = &n.update else {
        return false;
    };
    is_host_turn_chunk(chunk)
}

fn is_acp_user_message_chunk(update: &SessionUpdate) -> bool {
    matches!(
        update,
        SessionUpdate::Acp(n) if matches!(n.update, acp::SessionUpdate::UserMessageChunk(_))
    )
}

/// Tracks user-message runs for turn counting (updates truncate / filter_rewind).
///
/// Progressive: every user run counts until the first `promptIndex` appears;
/// after that only marked runs count (mid-turn phantoms omit the marker).
/// A change of `promptIndex` (including unmarked ↔ marked) opens a new run —
/// matching replay's split so back-to-back cancelled prompts stay distinct.
struct UserRunTurnTracker {
    seen_marker: bool,
    in_user: bool,
    /// `promptIndex` of the current user run (`None` = unmarked / phantom run).
    current_run_pi: Option<usize>,
}

impl UserRunTurnTracker {
    fn new() -> Self {
        Self {
            seen_marker: false,
            in_user: false,
            current_run_pi: None,
        }
    }

    /// Returns true if this user chunk opens a **counted** turn.
    fn on_user_chunk(&mut self, prompt_index: Option<usize>) -> bool {
        if prompt_index.is_some() {
            self.seen_marker = true;
        }
        let counts = if self.seen_marker {
            prompt_index.is_some()
        } else {
            true
        };
        let new_run = if !self.in_user {
            true
        } else if self.seen_marker || prompt_index.is_some() {
            prompt_index != self.current_run_pi
        } else {
            false
        };
        if new_run {
            self.current_run_pi = prompt_index;
            self.in_user = true;
            counts
        } else {
            self.in_user = true;
            false
        }
    }

    fn on_non_user(&mut self) {
        self.in_user = false;
        self.current_run_pi = None;
    }
}

/// Calculate how many updates to keep for a given target prompt index (0-based, inclusive).
///
/// Progressive: unmarked user runs before the first `_meta.promptIndex` count
/// as turns; after the first marker only marked runs count (phantoms omit it).
pub fn updates_truncate_for_prompt(updates: &[SessionUpdate], target_prompt_index: usize) -> usize {
    let mut user_turn_count = 0;
    let mut tracker = UserRunTurnTracker::new();

    for (i, update) in updates.iter().enumerate() {
        if is_acp_user_message_chunk(update) && !is_host_turn_update(update) {
            if tracker.on_user_chunk(acp_user_chunk_prompt_index(update)) {
                user_turn_count += 1;
                if user_turn_count > target_prompt_index + 1 {
                    return i;
                }
            }
        } else {
            tracker.on_non_user();
        }
    }

    updates.len()
}

#[derive(Debug)]
pub enum AppendUpdateError {
    NotCommitted(io::Error),
    Committed(io::Error),
}

impl AppendUpdateError {
    pub fn into_io_error(self) -> io::Error {
        match self {
            Self::NotCommitted(error) | Self::Committed(error) => error,
        }
    }
}

impl std::fmt::Display for AppendUpdateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotCommitted(error) | Self::Committed(error) => error.fmt(formatter),
        }
    }
}

/// Storage adapter trait for session persistence
/// Abstracts over different storage backends (JSONL, SQLite, etc.)
#[async_trait]
pub trait StorageAdapter: Send + Sync {
    /// Initialize a new session or load existing one
    /// Returns the Summary (creates if needed, loads if exists)
    async fn init_session(&self, info: &Info, model_id: acp::ModelId) -> io::Result<Summary>;

    /// Repair the denormalized title cache from an already-validated canonical
    /// Timeline fold. Ordinary writers never call this path. Returns false for
    /// a stale/idempotent sequence.
    async fn repair_session_title_projection(
        &self,
        info: &Info,
        event_seq: u64,
        title: String,
        source: chat_state::SessionTitleSource,
    ) -> io::Result<bool>;

    /// Append a session update (ACP update or Grow extension update) and increment counter
    async fn append_update(&self, info: &Info, update: &SessionUpdate) -> io::Result<()>;

    /// Append one update and report whether the replay record was committed before an error.
    async fn append_update_commit_aware(
        &self,
        info: &Info,
        update: &SessionUpdate,
    ) -> Result<(), AppendUpdateError> {
        self.append_update(info, update)
            .await
            .map_err(AppendUpdateError::NotCommitted)
    }

    /// Append one update durably, preserving whether the replay record committed before failure.
    async fn append_update_durable_commit_aware(
        &self,
        _info: &Info,
        _update: &SessionUpdate,
    ) -> Result<(), AppendUpdateError> {
        Err(AppendUpdateError::NotCommitted(io::Error::new(
            io::ErrorKind::Unsupported,
            "durable session update append is unsupported",
        )))
    }

    /// Append one immutable conversation event without rewriting prior facts.
    async fn append_timeline_event(
        &self,
        info: &Info,
        event: &chat_state::TimelineEvent,
    ) -> io::Result<()>;

    /// Append one timeline boundary and sync it before returning.
    async fn append_timeline_event_durable(
        &self,
        _info: &Info,
        _event: &chat_state::TimelineEvent,
    ) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "durable timeline append is unsupported",
        ))
    }

    /// Durably append one event to a short-lived sideband's independent
    /// Timeline. Implementations must preserve contiguous sequence identity.
    async fn append_sideband_event_durable(
        &self,
        _info: &Info,
        _event: &chat_state::SidebandEvent,
    ) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "durable sideband append is unsupported",
        ))
    }

    /// Update the current model and agent name in summary.
    /// `agent_name` is the resolved agent definition name
    /// persisted so session resume doesn't depend on the mutable model catalog.
    /// `None` preserves the corresponding persisted field.
    async fn update_current_model_and_agent(
        &self,
        info: &Info,
        model_id: &acp::ModelId,
        agent_name: Option<&str>,
        reasoning_effort: Option<Option<ReasoningEffort>>,
    ) -> io::Result<()>;

    /// Update the persisted HEAD commit and branch in summary
    async fn update_git_head(
        &self,
        info: &Info,
        commit: Option<String>,
        branch: Option<String>,
    ) -> io::Result<()>;

    async fn write_workflow_run_state(
        &self,
        info: &Info,
        manifest: &crate::session::workflow::store::WorkflowRunManifest,
    ) -> io::Result<()>;

    async fn delete_workflow_run_state(&self, info: &Info, run_id: &str) -> io::Result<()>;

    /// Load all persisted data for a session
    async fn load_session(&self, info: &Info) -> io::Result<PersistedData>;

    /// Load session data WITHOUT updates (for memory efficiency when updates
    /// will be streamed). Implementations also do NOT read rewind points here;
    /// those are deferred and lazily loaded on demand from the path returned by
    /// [`rewind_points_file_path`](StorageAdapter::rewind_points_file_path).
    async fn load_session_without_updates(&self, info: &Info) -> io::Result<PersistedDataLight>;

    /// Loads the summary of the session
    async fn load_summary(&self, info: &Info) -> io::Result<Summary>;

    /// List session summaries, optionally filtered by current working directory.
    /// When `cwd` is `None`, returns summaries for all sessions.
    async fn list_sessions(&self, cwd: Option<&str>) -> io::Result<Vec<Summary>>;

    /// Permanently delete a session's stored data (all files for the
    /// session). Implementations must treat a missing session as success
    /// (idempotent delete).
    async fn delete_session(&self, info: &Info) -> io::Result<()>;

    /// Append a rewind point for session rewind functionality
    async fn append_rewind_point(&self, info: &Info, point: &RewindPoint) -> io::Result<()>;

    /// Load all rewind points for a session
    async fn load_rewind_points(&self, info: &Info) -> io::Result<Vec<RewindPoint>>;

    /// Atomically replace the complete typed rewind projection.
    async fn replace_rewind_points(&self, info: &Info, points: &[RewindPoint]) -> io::Result<()>;

    async fn write_rewind_transaction(
        &self,
        info: &Info,
        transaction: &crate::session::persistence::RewindTransaction,
    ) -> io::Result<()>;

    async fn clear_rewind_transaction(&self, info: &Info) -> io::Result<()>;

    /// Copy session data from source to target, transforming session IDs
    /// The `options` parameter allows setting parent session tracking and model overrides.
    async fn copy_session_data(
        &self,
        source_info: &Info,
        target_info: &Info,
        options: CopySessionOptions,
    ) -> io::Result<CopySessionResult>;

    /// Load the current branch's typed user-authored inputs from Timeline.
    async fn load_prompt_records(&self, info: &Info) -> io::Result<Vec<chat_state::PromptRecord>>;

    /// Pin the canonical Timeline ledger for a bounded background projection.
    /// The returned reader owns the already-opened file handle, so callers
    /// cannot be redirected to a replacement session directory after identity
    /// validation.
    fn open_timeline_reader(&self, info: &Info) -> io::Result<TimelineLedgerReader>;
}

pub use jsonl::JsonlStorageAdapter;

/// An identity-bound read capability for one canonical Timeline ledger.
///
/// `label` is diagnostic only. Authority is the pinned file handle and reads
/// stop at the length observed when the capability was created, so a
/// concurrent append cannot turn a bounded projection into an unbounded one.
pub struct TimelineLedgerReader {
    file: std::fs::File,
    label: PathBuf,
    snapshot_len: u64,
}

impl TimelineLedgerReader {
    pub(crate) fn from_file(file: std::fs::File, label: PathBuf) -> io::Result<Self> {
        let snapshot_len = file.metadata()?.len();
        Ok(Self {
            file,
            label,
            snapshot_len,
        })
    }

    pub fn snapshot_len(&self) -> u64 {
        self.snapshot_len
    }

    pub(crate) fn read_events(self) -> io::Result<Vec<chat_state::TimelineEvent>> {
        let mut lines = CommittedJsonlLines::from_file(
            self.file,
            self.label.clone(),
            "mandatory Timeline ledger".to_owned(),
            MAX_JSONL_ENTRY_BYTES,
        );
        let mut events = Vec::new();
        while let Some(line) = lines.next() {
            let line = line?;
            if lines.stream_position()? > self.snapshot_len {
                break;
            }
            let line_number = lines.line_number();
            let event = serde_json::from_slice(&line).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}:{line_number}: {error}", self.label.display()),
                )
            })?;
            events.push(event);
        }
        Ok(events)
    }
}

/// Extracts `method` and raw `params` from an updates.jsonl envelope
/// without parsing the notification payload.
#[derive(serde::Deserialize)]
pub(crate) struct RawLinePeek<'a> {
    pub method: &'a str,
    #[serde(borrow)]
    pub params: &'a serde_json::value::RawValue,
}

/// Peeks at `update.sessionUpdate` tag and `_meta` without full deserialization.
#[derive(serde::Deserialize)]
pub(crate) struct RawParamsPeek<'a> {
    #[serde(borrow, default)]
    pub update: Option<RawUpdatePeek<'a>>,
    #[serde(borrow, default, rename = "_meta")]
    pub meta: Option<&'a serde_json::value::RawValue>,
}

#[derive(serde::Deserialize)]
pub(crate) struct RawUpdatePeek<'a> {
    #[serde(rename = "sessionUpdate")]
    pub session_update: &'a str,
    #[serde(default)]
    pub target_prompt_index: Option<usize>,
    /// Chunk `_meta.promptIndex` when present (owned; not borrowed).
    #[serde(default, rename = "_meta")]
    pub meta: Option<RawChunkMetaPeek>,
}

#[derive(serde::Deserialize)]
pub(crate) struct RawChunkMetaPeek {
    #[serde(default, rename = "promptIndex")]
    pub prompt_index: Option<u64>,
    #[serde(default, rename = "hostTurn")]
    pub host_turn: Option<bool>,
}

/// Role of one item in the rewind timeline, as seen by [`filter_rewind_by`].
enum RewindStep {
    /// Rewind marker: truncate survivors back to `target`'s prompt boundary.
    Rewind { target: usize },
    /// User-message chunk opening (or continuing) a prompt run.
    UserChunk { prompt_index: Option<usize> },
    /// Anything else: kept, but ends the current user run.
    Other,
}

/// Shared rewind dead-branch filter. `classify` maps each item to its
/// [`RewindStep`]; the driver tracks prompt boundaries and, on a marker,
/// truncates survivors back to the target prompt. [`filter_rewind_lines`] and
/// [`filter_rewind_updates`] wrap this over raw JSONL and typed updates so the
/// two paths share one algorithm.
fn filter_rewind_by<T>(items: Vec<T>, classify: impl Fn(&T) -> RewindStep) -> Vec<T> {
    let mut result: Vec<T> = Vec::with_capacity(items.len());
    let mut prompt_starts: Vec<usize> = Vec::new();
    let mut tracker = UserRunTurnTracker::new();

    for item in items {
        match classify(&item) {
            RewindStep::Rewind { target } => {
                // Out-of-range target keeps every survivor: fold to `result.len()`.
                let trunc = prompt_starts.get(target).copied().unwrap_or(result.len());
                result.truncate(trunc);
                prompt_starts.truncate(target);
                tracker.on_non_user();
                continue;
            }
            RewindStep::UserChunk { prompt_index } => {
                if tracker.on_user_chunk(prompt_index) {
                    prompt_starts.push(result.len());
                }
            }
            RewindStep::Other => tracker.on_non_user(),
        }
        result.push(item);
    }
    result
}

/// Classify a raw JSONL line by peeking at its tag and `_meta` without fully
/// deserializing the payload.
fn rewind_step_for_line(line: &str) -> RewindStep {
    let Ok(env) = serde_json::from_str::<RawLinePeek<'_>>(line) else {
        return RewindStep::Other;
    };
    let is_grow = match env.method {
        GROW_SESSION_UPDATE_METHOD => true,
        ACP_SESSION_UPDATE_METHOD => false,
        _ => return RewindStep::Other,
    };

    let Some(u) = serde_json::from_str::<RawParamsPeek<'_>>(env.params.get())
        .ok()
        .and_then(|p| p.update)
    else {
        return RewindStep::Other;
    };

    if is_grow
        && u.session_update == *REWIND_MARKER
        && let Some(target) = u.target_prompt_index
    {
        return RewindStep::Rewind { target };
    }

    let is_host_turn = u.meta.as_ref().and_then(|m| m.host_turn).unwrap_or(false);
    if !is_grow && !is_host_turn && u.session_update == *USER_MESSAGE_CHUNK {
        let prompt_index = u
            .meta
            .as_ref()
            .and_then(|m| m.prompt_index.map(|v| v as usize));
        return RewindStep::UserChunk { prompt_index };
    }

    RewindStep::Other
}

/// Classify a typed `SessionUpdate`.
fn rewind_step_for_update(update: &SessionUpdate) -> RewindStep {
    if let SessionUpdate::Grow(n) = update
        && let crate::extensions::notification::SessionUpdate::RewindMarker {
            target_prompt_index,
            ..
        } = &n.update
    {
        return RewindStep::Rewind {
            target: *target_prompt_index,
        };
    }
    if is_acp_user_message_chunk(update) && !is_host_turn_update(update) {
        return RewindStep::UserChunk {
            prompt_index: acp_user_chunk_prompt_index(update),
        };
    }
    RewindStep::Other
}

/// Filter rewind dead branches from raw JSONL lines.
///
/// Canonical raw-line rewind filter used by the initial and delta replay paths.
/// Skips parsing entirely when no rewind markers are present.
pub(crate) fn filter_rewind_lines(lines: Vec<&str>) -> Vec<&str> {
    if !lines.iter().any(|l| l.contains(&*REWIND_MARKER)) {
        return lines;
    }
    filter_rewind_by(lines, |line| rewind_step_for_line(line))
}

/// Filter rewind dead branches from typed `SessionUpdate` values.
///
/// Typed equivalent of [`filter_rewind_lines`] over the same
/// [`filter_rewind_by`] driver, operating on fully-deserialized updates.
pub fn filter_rewind_updates(updates: Vec<SessionUpdate>) -> Vec<SessionUpdate> {
    let has_rewinds = updates.iter().any(|u| {
        matches!(
            u,
            SessionUpdate::Grow(n) if matches!(
                n.update,
                crate::extensions::notification::SessionUpdate::RewindMarker { .. }
            )
        )
    });
    if !has_rewinds {
        return updates;
    }
    filter_rewind_by(updates, rewind_step_for_update)
}

/// Strip `<fork-context>` and `<resume-context>` XML wrappers from user
/// message chunks so replayed/exported prompts show clean text.
///
/// Only modifies `UserMessageChunk` text content; all other update types
/// pass through unchanged. The tags are injected by the subagent fork/resume
/// logic in `subagent.rs`.
pub fn strip_context_wrappers(update: acp::SessionUpdate) -> acp::SessionUpdate {
    let acp::SessionUpdate::UserMessageChunk(mut chunk) = update else {
        return update;
    };
    if let acp::ContentBlock::Text(ref mut t) = chunk.content {
        for tag in &["fork-context", "resume-context"] {
            let open = format!("<{tag}>");
            let close = format!("</{tag}>");
            if let Some(start) = t.text.find(&open)
                && let Some(rel_end) = t.text[start + open.len()..].find(&close)
            {
                let end = start + open.len() + rel_end;
                let remove_end = end + close.len();
                t.text = format!("{}{}", &t.text[..start], t.text[remove_end..].trim_start());
            }
        }
    }
    acp::SessionUpdate::UserMessageChunk(chunk)
}

// Replay-loader family, all resolving an identity-checked session and reading
// its pinned updates handle through `for_each_replay_update`. Pick by need:
//   - production, current grow home:   `load_updates_for_replay`
//   - production, streaming (bounded): `stream_replay_updates_at`
//   - tests, explicit grow home:       `load_updates_for_replay_at` (typed reference)

/// Load replay-ready typed ACP updates for a session, or `None` when the
/// session or its `updates.jsonl` is missing.
pub fn load_updates_for_replay(
    session_id: &str,
) -> std::io::Result<Option<Vec<acp::SessionUpdate>>> {
    let Some(reader) =
        open_replay_updates_reader(session_id, &crate::util::grow_home::grow_home())?
    else {
        return Ok(None);
    };
    Ok(Some(collect_replay_updates(reader)?))
}

/// Like [`load_updates_for_replay`], but resolves the session under a specific
/// grow home. Typed, materialize-all replay reader: collects every update into
/// owned `Vec`s. Production forwards replay through [`stream_replay_updates_at`]
/// to bound peak memory, so this has no production caller and is compiled only
/// for tests: the `testkit_synth_roundtrip` and `session_load_perf` parity
/// references and the in-crate relocation tests.
#[cfg(any(test, feature = "test-support"))]
pub fn load_updates_for_replay_at(
    session_id: &str,
    grow_home: &std::path::Path,
) -> std::io::Result<Option<Vec<acp::SessionUpdate>>> {
    let Some(reader) = open_replay_updates_reader(session_id, grow_home)? else {
        return Ok(None);
    };
    Ok(Some(collect_replay_updates(reader)?))
}

/// Collect every replay-ready ACP update from a pinned ledger into a `Vec`, the
/// materializing counterpart of the streaming [`for_each_replay_update`].
fn collect_replay_updates(reader: CommittedJsonlLines) -> std::io::Result<Vec<acp::SessionUpdate>> {
    let mut acp_updates: Vec<acp::SessionUpdate> = Vec::new();
    for_each_replay_update(reader, |u| acp_updates.push(u))?;
    Ok(acp_updates)
}

/// Resolve and pin `updates.jsonl` for `session_id` under `grow_home`.
/// Summary identity and the file handle are selected by the same storage
/// capability; no authoritative path escapes this boundary.
fn open_replay_updates_reader(
    session_id: &str,
    grow_home: &std::path::Path,
) -> std::io::Result<Option<CommittedJsonlLines>> {
    let storage = JsonlStorageAdapter::with_root(grow_home.to_path_buf());
    let Some(opened) = storage.open_session_by_id(session_id)? else {
        return Ok(None);
    };
    let file = match opened
        .directory()
        .open_regular(std::ffi::OsStr::new(UPDATES_FILE), "session updates ledger")
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(Some(CommittedJsonlLines::from_open_file_at(
        file,
        opened.directory().display_path().join(UPDATES_FILE),
        "session updates ledger",
        0,
    )?))
}

/// Whether a replay stream forwarded any update. Gates the caller's
/// post-replay memory purge: `Empty` means nothing was reclaimable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum ReplayEmission {
    Emitted,
    Empty,
}

/// Invoke `f` once per replay-ready ACP update for a session under `grow_home`,
/// never building the full typed `Vec`. Reads the session's JSONL transcript
/// directly; a non-JSONL backend would need its own bounded replay.
///
/// Forking or resuming replays the inherited transcript. The typed load parsed
/// the whole file and copied it several times, so a large session briefly held
/// several times its size in live heap and a per-user memory cgroup OOM-killed
/// it. Streaming holds one typed update at a time, so peak drops to about the
/// file size.
///
/// `Empty` folds the missing-session, missing-file, and no-ACP-updates cases;
/// the typed `load_updates_for_replay_at` keeps them distinct (`Ok(None)` vs
/// `Ok(Some(vec![]))`) since it returns the parsed contents rather than a purge
/// signal.
///
/// The sink is infallible by design: replay only rehydrates UI scrollback, a
/// best-effort step, so failing to apply one update must neither abort the
/// stream nor surface an error. I/O errors from reading the file still
/// propagate via the `Result`.
pub fn stream_replay_updates_at<F: FnMut(acp::SessionUpdate)>(
    session_id: &str,
    grow_home: &std::path::Path,
    f: F,
) -> std::io::Result<ReplayEmission> {
    let Some(reader) = open_replay_updates_reader(session_id, grow_home)? else {
        return Ok(ReplayEmission::Empty);
    };
    Ok(if for_each_replay_update(reader, f)? {
        ReplayEmission::Emitted
    } else {
        ReplayEmission::Empty
    })
}

/// Stream durable Grow extension notifications from one already-resolved
/// session directory. This is separate from conversation replay because the
/// pager normally applies only ACP chunks to a child view; reconnect
/// reconstruction additionally needs nested subagent lifecycle records to
/// rebuild descendant routing before pending interactions are replayed.
///
/// The full notification is retained deliberately: its `_meta.eventId` is the
/// source-session dedup identity shared by the persisted record and a live
/// event buffered during an ancestor `session/load`.
pub fn stream_replay_grow_notifications_at<
    F: FnMut(crate::extensions::notification::SessionNotification),
>(
    session_id: &str,
    grow_home: &std::path::Path,
    mut f: F,
) -> std::io::Result<ReplayEmission> {
    let Some(reader) = open_replay_updates_reader(session_id, grow_home)? else {
        return Ok(ReplayEmission::Empty);
    };
    let lines = read_committed_jsonl_text_lines_from_reader(reader)?;
    let live = filter_rewind_lines(lines.iter().map(String::as_str).collect());
    let mut emitted = false;
    for line in live {
        match SessionUpdateEnvelope::from_str(line) {
            Ok(SessionUpdate::Grow(notification)) => {
                emitted = true;
                f(*notification);
            }
            Ok(SessionUpdate::Acp(_)) => {}
            Err(error) => {
                tracing::debug!(?error, "skipping unparseable Grow replay line");
            }
        }
    }
    Ok(if emitted {
        ReplayEmission::Emitted
    } else {
        ReplayEmission::Empty
    })
}

// Rewind can drop earlier lines, so surviving lines are held until the end of
// the file. The reader allocates at most one bounded record at a time, while
// this projection retains only decoded lines needed for branch filtering.
// Output matches the typed load. Returns whether any ACP update was forwarded.
fn for_each_replay_update<F: FnMut(acp::SessionUpdate)>(
    reader: CommittedJsonlLines,
    mut f: F,
) -> std::io::Result<bool> {
    let lines = read_committed_jsonl_text_lines_from_reader(reader)?;
    let live = filter_rewind_lines(lines.iter().map(String::as_str).collect());
    let mut forwarded = false;
    for line in live {
        match SessionUpdateEnvelope::from_str(line) {
            // Only ACP updates replay.
            Ok(SessionUpdate::Acp(notif)) => {
                forwarded = true;
                f(strip_context_wrappers(notif.update));
            }
            // Grow extensions (rewind markers, compaction signals) are consumed
            // by the filter and intentionally dropped (matching the typed load).
            Ok(SessionUpdate::Grow(_)) => {}
            // Best-effort: an unparseable line (e.g. a partially written trailing
            // line) is skipped rather than aborting replay; the typed load drops
            // it too. Logged for diagnostics.
            Err(e) => tracing::debug!(error = %e, "skipping unparseable replay line"),
        }
    }
    Ok(forwarded)
}

pub(crate) fn read_committed_jsonl_text_lines(
    path: &Path,
    description: &str,
) -> io::Result<Vec<String>> {
    let Some(lines) = CommittedJsonlLines::open(path, description)? else {
        return Ok(Vec::new());
    };
    read_committed_jsonl_text_lines_from_reader(lines)
}

pub(crate) fn read_committed_jsonl_text_lines_from_reader(
    lines: CommittedJsonlLines,
) -> io::Result<Vec<String>> {
    let mut decoded = Vec::new();
    for line in lines {
        let line = line?;
        let line = String::from_utf8(line)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if !line.trim().is_empty() {
            decoded.push(line);
        }
    }
    Ok(decoded)
}

pub(crate) fn read_committed_jsonl_text_lines_from_file(
    file: std::fs::File,
    label: PathBuf,
    description: &str,
) -> io::Result<Vec<String>> {
    read_committed_jsonl_text_lines_from_reader(CommittedJsonlLines::from_open_file_at(
        file,
        label,
        description,
        0,
    )?)
}

#[doc(hidden)]
pub struct PreparedReplay<'a> {
    /// Rewind-filtered replay lines, each borrowed from the input transcript.
    pub lines: Vec<&'a str>,
    pub(crate) mark_replay: bool,
    /// Highest `eventId` counter across all live (rewind-filtered) lines, used
    /// to re-seed the process-global event counter on resume so post-load live
    /// events keep monotonically increasing ids (see
    /// [`crate::util::event_id::ensure_event_counter_at_least`]). `None` when no
    /// line carried a parseable `eventId`.
    pub(crate) max_event_seq: Option<u64>,
    pub(crate) total_live: usize,
    /// UI cache coverage for canonical subagent lifecycle facts.
    pub(crate) subagent_projections: SubagentProjectionState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SubagentProjectionState {
    pub spawned: std::collections::BTreeSet<String>,
    pub finished: std::collections::BTreeSet<String>,
}

/// Coverage of canonical lifecycle facts already present in the replay cache.
fn collect_subagent_projection_state(filtered: &[&str]) -> SubagentProjectionState {
    use crate::extensions::notification::SessionUpdate as Update;
    let mut state = SubagentProjectionState::default();
    for line in filtered {
        if !line.contains("subagent_spawned") && !line.contains("subagent_finished") {
            continue;
        }
        let Ok(envelope) = serde_json::from_str::<RawLinePeek<'_>>(line) else {
            continue;
        };
        if envelope.method != GROW_SESSION_UPDATE_METHOD {
            continue;
        }
        let Ok(notification) = serde_json::from_str::<SessionNotification>(envelope.params.get())
        else {
            continue;
        };
        match notification.update {
            Update::SubagentSpawned { subagent_id, .. } => {
                state.spawned.insert(subagent_id);
            }
            Update::SubagentFinished { subagent_id, .. } => {
                state.finished.insert(subagent_id);
            }
            _ => {}
        }
    }
    state
}

/// The raw `_meta` object of a canonical persisted envelope, if any, without
/// allocating a `serde_json::Value`.
fn line_meta(line: &str) -> Option<&serde_json::value::RawValue> {
    let env = serde_json::from_str::<RawLinePeek<'_>>(line).ok()?;
    if !matches!(
        env.method,
        ACP_SESSION_UPDATE_METHOD | GROW_SESSION_UPDATE_METHOD
    ) {
        return None;
    }
    serde_json::from_str::<RawParamsPeek<'_>>(env.params.get())
        .ok()?
        .meta
}

/// The `"update":` object key (a protocol key, not an enum discriminant). The
/// structural `params.update` is the FIRST occurrence in a persisted line: the
/// envelope prefix has no `"update":`, and any nested `"update"` (in `_meta` or a
/// tool's `rawInput`/`rawOutput`) is serialized after it, so the first match delimits it.
const UPDATE_KEY: &str = r#""update":"#;

/// Is this persisted line an `available_commands_update`?
///
/// The slash-command catalog is re-advertised in full after every `session/load`,
/// so the historical copies in `updates.jsonl` are redundant on replay and
/// dominate large sessions (~51% of bytes in pathological cases). The lines stay
/// on disk; this only skips forwarding them to the client.
///
/// A cheap [`AVAILABLE_COMMANDS_UPDATE_PREFIX`] substring pre-filter, then a
/// positional confirm that the value at the first [`UPDATE_KEY`] begins with the
/// ACU discriminant. Reads only the prefix (never the huge `availableCommands`
/// array), so it can't be fooled by the discriminant embedded in `_meta` or a
/// tool payload (never the first `"update":`).
pub(crate) fn line_is_available_commands_update(line: &str) -> bool {
    if !line.contains(&*AVAILABLE_COMMANDS_UPDATE_PREFIX) {
        return false;
    }
    line.find(UPDATE_KEY)
        .map(|pos| {
            line[pos + UPDATE_KEY.len()..]
                .trim_start()
                .starts_with(&*AVAILABLE_COMMANDS_UPDATE_PREFIX)
        })
        .unwrap_or(false)
}

// `_meta` protocol field names (not enum discriminants).
/// `_meta` key holding the per-event id used for cursor-based reconnect.
const EVENT_ID_KEY: &str = "eventId";

/// This line's `_meta.eventId`, if any. Cheap peek (no `Value`).
fn line_event_id(line: &str) -> Option<std::borrow::Cow<'_, str>> {
    if !line.contains(EVENT_ID_KEY) {
        return None;
    }
    #[derive(serde::Deserialize)]
    struct EventIdPeek<'a> {
        // `Cow` so an escaped eventId still parses and compares equal
        // (`Option<Cow>` always deserializes owned; `&str` would error).
        #[serde(rename = "eventId", borrow)]
        event_id: Option<std::borrow::Cow<'a, str>>,
    }
    serde_json::from_str::<EventIdPeek<'_>>(line_meta(line)?.get())
        .ok()
        .and_then(|e| e.event_id)
}

/// Does this line's `_meta.eventId` equal `cursor_id`?
fn line_has_event_id(line: &str, cursor_id: &str) -> bool {
    line_event_id(line).as_deref() == Some(cursor_id)
}

/// Rewind-filter, resolve the reconnect cursor, and drop redundant command
/// catalogs. Pure UI replay processing, no agent-state recovery.
///
/// The cursor is resolved before dropping ACUs, because an idle client often
/// reconnects with an ACU's `eventId` as its cursor; resolving against the
/// ACU-inclusive set keeps reconnect incremental instead of a full replay.
///
/// `#[doc(hidden)] pub` (not stable API): production replay uses it, and the
/// session-load memory test drives it to check the peek stays zero-copy.
#[doc(hidden)]
pub fn prepare_replay_lines<'a>(contents: &'a str, cursor: Option<&str>) -> PreparedReplay<'a> {
    let filtered = filter_rewind_lines(contents.lines().filter(|l| !l.trim().is_empty()).collect());

    // Highest `eventId` counter across all live (rewind-filtered) lines, used to
    // re-seed the process-global event counter on resume so post-load live events
    // keep monotonically increasing ids. eventId is "{sessionId}-{counter}" and
    // session ids contain dashes, so the counter is the suffix after the LAST '-'.
    let mut max_event_seq: Option<u64> = None;
    for line in &filtered {
        if line.contains("eventId")
            && let Ok(env) = serde_json::from_str::<RawLinePeek<'_>>(line)
            && matches!(
                env.method,
                ACP_SESSION_UPDATE_METHOD | GROW_SESSION_UPDATE_METHOD
            )
            && let Ok(pp) = serde_json::from_str::<RawParamsPeek<'_>>(env.params.get())
            && let Some(meta_raw) = pp.meta
            && let Ok(meta) = serde_json::from_str::<serde_json::Value>(meta_raw.get())
            && let Some(seq) = meta
                .get("eventId")
                .and_then(|v| v.as_str())
                .and_then(|s| s.rsplit('-').next())
                .and_then(|c| c.parse::<u64>().ok())
        {
            max_event_seq = Some(max_event_seq.map_or(seq, |m| m.max(seq)));
        }
    }

    // Resolve the reconnect cursor against the ACU-inclusive set. `mark_replay`
    // is true for a full historical replay (no cursor, or cursor not found).
    //
    // The cursor is refused when a FORWARDED tail line lacks an `eventId`:
    // such a line cannot be covered by a future cursor and has no client-side
    // dedup, so re-delivering it as live would re-apply it. Full replay is
    // the safe fallback — the client swaps it in wholesale. Id-less lines
    // come from older binaries or any emitter outside the stamping
    // chokepoints (see `ensure_event_id_meta`). ACU lines are exempt: they
    // are dropped below, never forwarded.
    let cursor_pos = cursor
        .and_then(|id| filtered.iter().rposition(|l| line_has_event_id(l, id)))
        .filter(|&pos| {
            let bounded = filtered[pos + 1..]
                .iter()
                .all(|l| line_is_available_commands_update(l) || line_event_id(l).is_some());
            if !bounded {
                tracing::warn!(
                    "replay: post-cursor tail contains eventId-less lines; full replay instead"
                );
            }
            bounded
        });
    let mark_replay = cursor_pos.is_none();
    let start = cursor_pos.map_or(0, |pos| pos + 1);

    // Single pass: drop ACUs (kept on disk), collect the post-cursor tail to
    // forward, and count the full ACU-free live set for the skip log.
    let mut lines: Vec<&str> = Vec::with_capacity(filtered.len().saturating_sub(start));
    let mut total_live = 0usize;
    for (i, &line) in filtered.iter().enumerate() {
        if line_is_available_commands_update(line) {
            continue;
        }
        total_live += 1;
        if i >= start {
            lines.push(line);
        }
    }

    PreparedReplay {
        lines,
        mark_replay,
        max_event_seq,
        total_live,
        subagent_projections: collect_subagent_projection_state(&filtered),
    }
}

/// Blank-strip, drop redundant command catalogs, and rewind-filter a raw
/// `updates.jsonl` segment. Shared by the delta-replay path (which has no
/// reconnect cursor); the initial replay path is [`prepare_replay_lines`], which
/// additionally resolves a cursor (and so must see ACUs) before dropping them.
pub(crate) fn filter_delta_replay_lines(lines: Vec<&str>) -> Vec<&str> {
    let live: Vec<&str> = lines
        .into_iter()
        .filter(|l| !l.trim().is_empty() && !line_is_available_commands_update(l))
        .collect();
    filter_rewind_lines(live)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(unix, windows))]
    #[test]
    fn contained_no_replace_rename_preserves_both_existing_entities() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("source")).unwrap();
        std::fs::create_dir(root.path().join("target")).unwrap();
        std::fs::write(root.path().join("source/source-marker"), b"source").unwrap();
        std::fs::write(root.path().join("target/target-marker"), b"target").unwrap();
        let directory =
            ContainedDirectory::open(root.path(), Path::new(""), "rename fixture", false).unwrap();

        let error = directory
            .rename_child_no_replace(
                std::ffi::OsStr::new("source"),
                std::ffi::OsStr::new("target"),
            )
            .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read(root.path().join("source/source-marker")).unwrap(),
            b"source"
        );
        assert_eq!(
            std::fs::read(root.path().join("target/target-marker")).unwrap(),
            b"target"
        );
    }

    #[cfg(unix)]
    #[test]
    fn pinned_contained_directory_resists_post_validation_symlink_swap() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("sidebands/id")).unwrap();
        std::fs::write(outside.path().join("timeline.jsonl"), b"outside").unwrap();
        std::fs::write(outside.path().join("state.json"), b"outside-state").unwrap();
        std::fs::write(root.path().join("sidebands/id/read.json"), b"inside-read").unwrap();
        std::fs::write(outside.path().join("read.json"), b"outside-read").unwrap();

        let pinned = ContainedDirectory::open(
            root.path(),
            Path::new("sidebands/id"),
            "race fixture",
            false,
        )
        .unwrap();
        let detached = root.path().join("sidebands/id-detached");
        std::fs::rename(root.path().join("sidebands/id"), &detached).unwrap();
        symlink(outside.path(), root.path().join("sidebands/id")).unwrap();

        assert_eq!(
            pinned
                .read_bounded(std::ffi::OsStr::new("read.json"), "race read", 64)
                .unwrap(),
            b"inside-read"
        );

        let mut append = pinned
            .open_read_write_create(std::ffi::OsStr::new("timeline.jsonl"))
            .unwrap();
        append.write_all(b"inside").unwrap();
        append.sync_all().unwrap();
        pinned
            .write_atomic(
                std::ffi::OsStr::new("state.json"),
                b"inside-state",
                true,
                true,
            )
            .unwrap();

        assert_eq!(
            std::fs::read(outside.path().join("timeline.jsonl")).unwrap(),
            b"outside"
        );
        assert_eq!(
            std::fs::read(outside.path().join("state.json")).unwrap(),
            b"outside-state"
        );
        assert_eq!(
            std::fs::read(outside.path().join("read.json")).unwrap(),
            b"outside-read"
        );
        assert_eq!(
            std::fs::read(detached.join("timeline.jsonl")).unwrap(),
            b"inside"
        );
        assert_eq!(
            std::fs::read(detached.join("state.json")).unwrap(),
            b"inside-state"
        );
    }

    #[test]
    fn committed_jsonl_reader_ignores_only_the_torn_tail_and_bounds_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.jsonl");
        std::fs::write(&path, b"1\n2\n333333333").unwrap();
        assert_eq!(
            read_committed_jsonl_file_with_limit::<u64>(&path, "test ledger", 8).unwrap(),
            vec![1, 2]
        );

        std::fs::write(&path, b"123456789\n").unwrap();
        let error = read_committed_jsonl_file_with_limit::<u64>(&path, "test ledger", 8)
            .expect_err("an oversized complete entry must fail closed");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("exceeds 8 bytes"));
    }

    #[test]
    fn committed_jsonl_reader_reports_only_the_durable_offset() {
        use std::io::Write;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.jsonl");
        std::fs::write(&path, b"one\ntw").unwrap();

        let mut lines = CommittedJsonlLines::open(&path, "test ledger")
            .unwrap()
            .unwrap();
        assert_eq!(lines.next().unwrap().unwrap(), b"one");
        assert_eq!(lines.stream_position().unwrap(), 4);
        assert!(lines.next().is_none());
        assert_eq!(
            lines.stream_position().unwrap(),
            4,
            "a torn tail must not advance the replay cursor"
        );

        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"o\n")
            .unwrap();
        let mut resumed = CommittedJsonlLines::open_at(&path, "test ledger", 4)
            .unwrap()
            .unwrap();
        assert_eq!(resumed.next().unwrap().unwrap(), b"two");
        assert_eq!(resumed.stream_position().unwrap(), 8);
    }

    #[cfg(unix)]
    #[test]
    fn committed_jsonl_reader_rejects_symlinked_ledgers() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.jsonl");
        let link = dir.path().join("ledger.jsonl");
        std::fs::write(&target, b"1\n").unwrap();
        symlink(&target, &link).unwrap();

        let error = read_committed_jsonl_file::<u64>(&link, "test ledger")
            .expect_err("canonical ledgers must not follow symlinks");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("symlink"), "{error}");
    }

    #[test]
    fn summary_serialization_enforces_the_same_bound_as_readers() {
        let info = Info {
            id: acp::SessionId::new("oversized-summary"),
            cwd: "/test".into(),
        };
        let mut summary =
            Summary::new(&info, crate::session::persistence::default_model_id()).unwrap();
        summary.info.cwd = "x".repeat(MAX_SESSION_SUMMARY_BYTES as usize);

        let error = serialize_summary(&summary)
            .expect_err("runtime summary writers must reject unreadable projections");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("session summary exceeds"));
    }

    // ── helpers ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn durable_atomic_write_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("atomic-state.json");
        write_bytes_atomic_durable_async(&path, b"old".to_vec())
            .await
            .unwrap();

        write_bytes_atomic_durable_async(&path, b"new".to_vec())
            .await
            .unwrap();

        assert_eq!(std::fs::read(path).unwrap(), b"new");
    }

    /// Wrap an ACP notification as the envelope stored in updates.jsonl.
    fn acp_envelope(session_update_json: &str) -> String {
        format!(
            r#"{{"timestamp":1,"method":"session/update","params":{{"sessionId":"s","update":{session_update_json}}}}}"#
        )
    }

    /// Wrap a Grow notification as the envelope stored in updates.jsonl.
    fn grow_envelope(session_update_json: &str) -> String {
        format!(
            r#"{{"timestamp":1,"method":"_grow/session/update","params":{{"sessionId":"s","update":{session_update_json}}}}}"#
        )
    }

    fn user_chunk(text: &str, prompt_index: Option<usize>) -> SessionUpdate {
        let mut chunk = acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(
            text.to_string(),
        )));
        if let Some(pi) = prompt_index {
            chunk = chunk.meta(
                serde_json::json!({ "promptIndex": pi })
                    .as_object()
                    .cloned(),
            );
        }
        SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
            acp::SessionId::new("s"),
            acp::SessionUpdate::UserMessageChunk(chunk),
        )))
    }

    fn agent_chunk(text: &str) -> SessionUpdate {
        SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
            acp::SessionId::new("s"),
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new(text.to_string()),
            ))),
        )))
    }

    #[test]
    fn updates_truncate_ignores_unmarked_phantoms_when_markers_present() {
        let updates = vec![
            user_chunk("P0", Some(0)),
            agent_chunk("A0"),
            user_chunk("!pwd", None),
            agent_chunk("out"),
            user_chunk("P1", Some(1)),
            agent_chunk("A1"),
            user_chunk("P2", Some(2)),
            agent_chunk("A2"),
        ];
        // Keep through P1 (indices 0,1); cut at start of P2 run.
        let cut = updates_truncate_for_prompt(&updates, 1);
        assert_eq!(cut, 6);
        assert!(matches!(
            &updates[cut],
            SessionUpdate::Acp(n) if matches!(
                &n.update,
                acp::SessionUpdate::UserMessageChunk(c)
                    if matches!(&c.content, acp::ContentBlock::Text(t) if t.text == "P2")
            )
        ));
    }

    #[test]
    fn updates_truncate_splits_consecutive_marked_prompts_without_agent() {
        let updates: Vec<_> = (0..6)
            .map(|i| user_chunk(&format!("P{i}"), Some(i)))
            .collect();
        // Target 2 keeps turns 0 and 1; cut at P2 (index 2).
        assert_eq!(updates_truncate_for_prompt(&updates, 1), 2);
        assert_eq!(updates_truncate_for_prompt(&updates, 2), 3);
        assert_eq!(updates_truncate_for_prompt(&updates, 5), 6);
    }

    /// Mixed stream: unmarked runs before the first promptIndex still count.
    #[test]
    fn updates_truncate_mixed_unmarked_prefix_then_markers() {
        let updates = vec![
            user_chunk("old0", None),
            agent_chunk("A0"),
            user_chunk("old1", None),
            agent_chunk("A1"),
            user_chunk("new2", Some(2)),
            agent_chunk("A2"),
            user_chunk("!pwd", None),
            agent_chunk("out"),
            user_chunk("new3", Some(3)),
            agent_chunk("A3"),
        ];
        // Target 1 keeps old0+old1; cut at new2.
        assert_eq!(updates_truncate_for_prompt(&updates, 1), 4);
        // Target 2 keeps through A2 (and phantom run does not add a turn); cut at new3.
        assert_eq!(updates_truncate_for_prompt(&updates, 2), 8);
        assert_eq!(updates_truncate_for_prompt(&updates, 0), 2);
    }

    #[test]
    fn filter_rewind_mixed_unmarked_prefix_then_markers() {
        let o0 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"old0"}}"#,
        );
        let a0 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"A0"}}"#,
        );
        let o1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"old1"}}"#,
        );
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"A1"}}"#,
        );
        let n2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"new2"},"_meta":{"promptIndex":2}}"#,
        );
        let a2 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"A2"}}"#,
        );
        let n3 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"new3"},"_meta":{"promptIndex":3}}"#,
        );
        // Rewind to target 2: keep turns 0,1 (old0, old1); drop new2+.
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":2,"created_at":"2024-01-01"}"#,
        );
        let after = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"after"},"_meta":{"promptIndex":2}}"#,
        );
        let lines = vec![
            o0.as_str(),
            a0.as_str(),
            o1.as_str(),
            a1.as_str(),
            n2.as_str(),
            a2.as_str(),
            n3.as_str(),
            rw.as_str(),
            after.as_str(),
        ];
        let kept = filter_rewind_lines(lines);
        let texts: Vec<&str> = kept
            .iter()
            .filter_map(|l| {
                if l.contains("\"text\":\"old0\"") {
                    Some("old0")
                } else if l.contains("\"text\":\"old1\"") {
                    Some("old1")
                } else if l.contains("\"text\":\"new2\"") {
                    Some("new2")
                } else if l.contains("\"text\":\"new3\"") {
                    Some("new3")
                } else if l.contains("\"text\":\"after\"") {
                    Some("after")
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(texts, vec!["old0", "old1", "after"]);
    }

    #[test]
    fn filter_rewind_ignores_unmarked_phantoms_when_markers_present() {
        let p0 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"P0"},"_meta":{"promptIndex":0}}"#,
        );
        let a0 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"A0"}}"#,
        );
        let phantom = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"!pwd"}}"#,
        );
        let p1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"P1"},"_meta":{"promptIndex":1}}"#,
        );
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"A1"}}"#,
        );
        let p2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"P2"},"_meta":{"promptIndex":2}}"#,
        );
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":2,"created_at":"2024-01-01"}"#,
        );
        let after = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"after"},"_meta":{"promptIndex":2}}"#,
        );
        let lines = vec![
            p0.as_str(),
            a0.as_str(),
            phantom.as_str(),
            p1.as_str(),
            a1.as_str(),
            p2.as_str(),
            rw.as_str(),
            after.as_str(),
        ];
        let kept = filter_rewind_lines(lines);
        let texts: Vec<&str> = kept
            .iter()
            .filter_map(|l| {
                if l.contains("\"text\":\"P0\"") {
                    Some("P0")
                } else if l.contains("!pwd") {
                    Some("phantom")
                } else if l.contains("\"text\":\"P1\"") {
                    Some("P1")
                } else if l.contains("\"text\":\"P2\"") {
                    Some("P2")
                } else if l.contains("\"text\":\"after\"") {
                    Some("after")
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(texts, vec!["P0", "phantom", "P1", "after"]);
    }

    // ── filter_rewind_lines tests ────────────────────────────────────────────

    #[test]
    fn filter_rewind_removes_dead_branch() {
        let u1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"first"}}"#,
        );
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"resp1"}}"#,
        );
        let u2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"second"}}"#,
        );
        let a2 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"resp2"}}"#,
        );
        // Rewind to prompt 1 — kills u2, a2
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":1,"created_at":"2024-01-01"}"#,
        );
        let u3 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"replacement"}}"#,
        );
        let a3 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"resp3"}}"#,
        );

        let lines = vec![
            u1.as_str(),
            a1.as_str(),
            u2.as_str(),
            a2.as_str(),
            rw.as_str(),
            u3.as_str(),
            a3.as_str(),
        ];
        let result = filter_rewind_lines(lines);

        // u1, a1 survive. u2, a2, rewind marker removed. u3, a3 added.
        assert_eq!(result.len(), 4);
        assert!(result[0].contains("first"));
        assert!(result[1].contains("resp1"));
        assert!(result[2].contains("replacement"));
        assert!(result[3].contains("resp3"));
    }

    #[test]
    fn filter_rewind_ignores_a_malformed_middle_line() {
        let user_message_1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"first"}}"#,
        );
        let agent_message_1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"resp1"}}"#,
        );
        let user_message_2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"second"}}"#,
        );
        let agent_message_2 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"resp2"}}"#,
        );
        let rewind_to_1 = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":1,"created_at":"2024-01-01"}"#,
        );
        let torn = "{ torn, unparseable jsonl line";

        // The malformed line is kept but not counted as a prompt boundary, so
        // the rewind still drops prompt 1.
        let survivors = filter_rewind_lines(vec![
            user_message_1.as_str(),
            agent_message_1.as_str(),
            torn,
            user_message_2.as_str(),
            agent_message_2.as_str(),
            rewind_to_1.as_str(),
        ]);

        pretty_assertions::assert_eq!(
            survivors,
            vec![user_message_1.as_str(), agent_message_1.as_str(), torn]
        );
    }

    #[test]
    fn filter_rewind_to_zero_clears_all() {
        let u1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"only"}}"#,
        );
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"resp"}}"#,
        );
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":0,"created_at":"2024-01-01"}"#,
        );
        let u2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"fresh start"}}"#,
        );

        let lines = vec![u1.as_str(), a1.as_str(), rw.as_str(), u2.as_str()];
        let result = filter_rewind_lines(lines);

        assert_eq!(result.len(), 1);
        assert!(result[0].contains("fresh start"));
    }

    #[test]
    fn filter_rewind_double_rewind() {
        let u1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p1"}}"#,
        );
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"r1"}}"#,
        );
        let u2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p2"}}"#,
        );
        let a2 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"r2"}}"#,
        );
        let u3 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p3"}}"#,
        );
        let a3 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"r3"}}"#,
        );
        // Rewind to prompt 2 — kills p3/r3
        let rw1 = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":2,"created_at":"2024-01-01"}"#,
        );
        let u4 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p4"}}"#,
        );
        let a4 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"r4"}}"#,
        );
        // Rewind to prompt 1 — kills p2/r2/p4/r4
        let rw2 = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":1,"created_at":"2024-01-01"}"#,
        );
        let u5 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"final"}}"#,
        );

        let lines = vec![
            u1.as_str(),
            a1.as_str(),
            u2.as_str(),
            a2.as_str(),
            u3.as_str(),
            a3.as_str(),
            rw1.as_str(),
            u4.as_str(),
            a4.as_str(),
            rw2.as_str(),
            u5.as_str(),
        ];
        let result = filter_rewind_lines(lines);

        // Only p1, r1, final survive
        assert_eq!(result.len(), 3);
        assert!(result[0].contains("p1"));
        assert!(result[1].contains("r1"));
        assert!(result[2].contains("final"));
    }

    /// The raw-line filter and the typed filter must truncate an identical
    /// rewind timeline to the same surviving updates, in the same order.
    #[test]
    fn filter_rewind_lines_and_updates_agree() {
        let u1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p1"}}"#,
        );
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"r1"}}"#,
        );
        let u2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p2"}}"#,
        );
        let a2 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"r2"}}"#,
        );
        let rw1 = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":2,"created_at":"2024-01-01"}"#,
        );
        let u3 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p3"}}"#,
        );
        let a3 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"r3"}}"#,
        );
        let rw2 = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":1,"created_at":"2024-01-01"}"#,
        );
        let u4 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"final"}}"#,
        );

        let lines = vec![
            u1.as_str(),
            a1.as_str(),
            u2.as_str(),
            a2.as_str(),
            rw1.as_str(),
            u3.as_str(),
            a3.as_str(),
            rw2.as_str(),
            u4.as_str(),
        ];

        let ser = |u: &SessionUpdate| serde_json::to_string(u).unwrap();
        let via_lines: Vec<String> = filter_rewind_lines(lines.clone())
            .iter()
            .map(|l| ser(&SessionUpdateEnvelope::from_str(l).unwrap()))
            .collect();
        let typed: Vec<SessionUpdate> = lines
            .iter()
            .map(|l| SessionUpdateEnvelope::from_str(l).unwrap())
            .collect();
        let via_updates: Vec<String> = filter_rewind_updates(typed).iter().map(ser).collect();

        assert_eq!(via_lines, via_updates);
    }

    /// An out-of-range rewind target folds to `result.len()` (the
    /// `unwrap_or(result.len())` branch in `filter_rewind_by`), so truncation is
    /// a no-op and every survivor is kept.
    #[test]
    fn filter_rewind_out_of_range_target_keeps_all() {
        let u1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p1"}}"#,
        );
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"r1"}}"#,
        );
        // Only prompt index 0 exists; target 5 is out of range.
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":5,"created_at":"2024-01-01"}"#,
        );
        let u2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p2"}}"#,
        );

        let lines = vec![u1.as_str(), a1.as_str(), rw.as_str(), u2.as_str()];
        let result = filter_rewind_lines(lines);

        // Marker is dropped; the three ACP survivors remain in order.
        assert_eq!(result.len(), 3);
        assert!(result[0].contains("p1"));
        assert!(result[1].contains("r1"));
        assert!(result[2].contains("p2"));
    }

    /// A session with no `updates.jsonl` streams nothing, so the emission gate
    /// reports `Empty` and forwards no updates.
    #[test]
    fn stream_replay_updates_at_missing_session_is_empty() {
        let grow_home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(grow_home.path().join("sessions")).unwrap();

        let mut count = 0usize;
        let emission =
            stream_replay_updates_at("does-not-exist", grow_home.path(), |_| count += 1).unwrap();

        assert_eq!(emission, ReplayEmission::Empty);
        assert_eq!(count, 0);
    }

    /// A resolvable session whose `updates.jsonl` cannot be read surfaces the
    /// error rather than folding to `Empty`, so the caller logs a real fault
    /// instead of mistaking it for an absent transcript. (The path is a
    /// directory, which `read_to_string` rejects.)
    #[test]
    fn stream_replay_updates_at_surfaces_read_errors() {
        let grow_home = tempfile::tempdir().unwrap();
        let session_dir = grow_home.path().join("sessions").join("cwd").join("sess");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join(SUMMARY_FILE), "{}").unwrap();
        std::fs::create_dir(session_dir.join(UPDATES_FILE)).unwrap();

        let result = stream_replay_updates_at("sess", grow_home.path(), |_| {});
        assert!(
            result.is_err(),
            "read fault must surface, not fold to Empty: {result:?}"
        );
    }

    /// End-to-end: the streaming core (`for_each_replay_update`, what
    /// `stream_replay_updates_at` wraps) applies rewind over a real file and
    /// yields the same survivors as the typed parse-all path.
    #[test]
    fn streaming_replay_applies_rewind_like_the_typed_path() {
        let u1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p1"}}"#,
        );
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"r1"}}"#,
        );
        let u2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p2"}}"#,
        );
        // Rewind to prompt 1 drops p2.
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":1,"created_at":"2024-01-01"}"#,
        );
        let u3 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"final"}}"#,
        );
        let raw = format!("{u1}\n{a1}\n{u2}\n{rw}\n{u3}\n");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(UPDATES_FILE);
        std::fs::write(&path, &raw).unwrap();

        let reader = CommittedJsonlLines::from_open_file_at(
            std::fs::File::open(&path).unwrap(),
            path,
            "session updates ledger",
            0,
        )
        .unwrap();
        let mut streamed = Vec::new();
        let forwarded = for_each_replay_update(reader, |u| streamed.push(u)).unwrap();
        assert!(forwarded);

        // Typed reference: parse all, rewind-filter, map ACP survivors.
        let typed: Vec<SessionUpdate> = raw
            .lines()
            .map(|l| SessionUpdateEnvelope::from_str(l).unwrap())
            .collect();
        let reference: Vec<acp::SessionUpdate> = filter_rewind_updates(typed)
            .into_iter()
            .filter_map(|u| match u {
                SessionUpdate::Acp(notif) => Some(strip_context_wrappers(notif.update)),
                SessionUpdate::Grow(_) => None,
            })
            .collect();

        let ser = |u: &acp::SessionUpdate| serde_json::to_string(u).unwrap();
        assert_eq!(
            streamed.iter().map(ser).collect::<Vec<_>>(),
            reference.iter().map(ser).collect::<Vec<_>>(),
        );
    }

    // ── prepare_replay_lines tests ───────────────────────────────────────────

    /// Envelope with _meta at the params level (where the real agent puts it).
    fn acp_envelope_with_meta(session_update_json: &str, meta_json: &str) -> String {
        format!(
            r#"{{"timestamp":1,"method":"session/update","params":{{"sessionId":"s","update":{session_update_json},"_meta":{meta_json}}}}}"#
        )
    }

    #[test]
    fn prepare_replay_cursor_skips_to_position() {
        let u1 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"old"}}"#,
            r#"{"eventId":"ev1"}"#,
        );
        let a1 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"old resp"}}"#,
            r#"{"eventId":"ev2"}"#,
        );
        let u2 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"new"}}"#,
            r#"{"eventId":"ev3"}"#,
        );
        let raw = format!("{u1}\n{a1}\n{u2}\n");

        let prepared = prepare_replay_lines(&raw, Some("ev2"));
        // Should skip ev1 and ev2, return only ev3
        assert_eq!(prepared.lines.len(), 1);
        assert!(!prepared.mark_replay);
        assert!(prepared.lines[0].contains("new"));
        assert_eq!(prepared.total_live, 3);
    }

    #[test]
    fn prepare_replay_cursor_not_found_returns_all() {
        let u1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hi"}}"#,
        );
        let raw = format!("{u1}\n");

        let prepared = prepare_replay_lines(&raw, Some("nonexistent"));
        assert_eq!(prepared.lines.len(), 1);
        assert!(prepared.mark_replay); // fallback to full replay
    }

    /// A resolved cursor is refused when the tail contains an eventId-less
    /// line (older-binary history): the line has no client-side dedup and no
    /// future cursor can cover it, so an incremental tail would re-apply it.
    /// Full replay is the safe fallback.
    #[test]
    fn prepare_replay_cursor_refused_when_tail_has_event_id_less_line() {
        let a1 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"seen"}}"#,
            r#"{"eventId":"ev1"}"#,
        );
        // Grow-style line persisted by an older binary: no _meta at all.
        let old_grow = r#"{"timestamp":2,"method":"_grow/session/update","params":{"sessionId":"s","update":{"sessionUpdate":"hook_annotation","message":"trailing"}}}"#;
        let raw = format!("{a1}\n{old_grow}\n");

        let prepared = prepare_replay_lines(&raw, Some("ev1"));
        assert!(
            prepared.mark_replay,
            "an unbounded tail must force a full replay"
        );
        assert_eq!(prepared.lines.len(), 2, "full history is replayed");

        // Same history with the trailing line stamped resolves incrementally.
        let new_grow = r#"{"timestamp":2,"method":"_grow/session/update","params":{"sessionId":"s","update":{"sessionUpdate":"hook_annotation","message":"trailing"},"_meta":{"eventId":"ev2"}}}"#;
        let raw = format!("{a1}\n{new_grow}\n");
        let prepared = prepare_replay_lines(&raw, Some("ev1"));
        assert!(!prepared.mark_replay);
        assert_eq!(prepared.lines.len(), 1);
        assert!(prepared.lines[0].contains("trailing"));

        // An id-less ACU in the tail is exempt from the refusal — ACUs are
        // dropped before forwarding, so they can never be re-applied.
        let acu =
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#);
        let raw = format!("{a1}\n{acu}\n");
        let prepared = prepare_replay_lines(&raw, Some("ev1"));
        assert!(
            !prepared.mark_replay,
            "a trailing id-less ACU must not force a full replay"
        );
        assert!(
            prepared.lines.is_empty(),
            "the ACU is dropped, never forwarded"
        );
    }

    #[test]
    fn prepare_replay_extracts_max_event_seq() {
        // eventId is "{sessionId}-{counter}" and session ids contain dashes, so
        // the counter is the suffix after the LAST '-'. max_event_seq is the
        // highest counter across all live lines — used to re-seed the global
        // event counter on resume so post-load live events stay monotonic and
        // don't get dropped by the client's eventId dedup.
        let a1 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"a"}}"#,
            r#"{"eventId":"019e-abcd-7","totalTokens":100}"#,
        );
        let a2 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"b"}}"#,
            r#"{"eventId":"019e-abcd-42","totalTokens":250}"#,
        );
        // Out-of-order counter (lower than the max) must not lower the result.
        let a3 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"c"}}"#,
            r#"{"eventId":"019e-abcd-13","totalTokens":250}"#,
        );
        let raw = format!("{a1}\n{a2}\n{a3}\n");

        let prepared = prepare_replay_lines(&raw, None);
        assert_eq!(
            prepared.max_event_seq,
            Some(42),
            "max counter across all lines (suffix after last '-')"
        );
    }

    #[test]
    fn prepare_replay_no_event_ids_yields_none_max_seq() {
        // Lines without a parseable numeric eventId suffix (older shell) yield
        // None, so the counter is left untouched on resume.
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"a"}}"#,
        );
        let raw = format!("{a1}\n");
        let prepared = prepare_replay_lines(&raw, None);
        assert_eq!(prepared.max_event_seq, None);
    }

    // ── available_commands_update skip (T1) + single-pass equivalence ─────────

    #[test]
    fn acu_line_detection_exact_and_no_false_positive() {
        let acu =
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#);
        assert!(line_is_available_commands_update(&acu));

        // A user message that merely mentions the phrase must NOT match.
        let user_mentions = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"what is available_commands_update?"}}"#,
        );
        assert!(!line_is_available_commands_update(&user_mentions));
    }

    /// The anchor must reject the discriminant when it sits inside `_meta` (not
    /// at the `params.update` position) — the real update here is a non-ACU.
    #[test]
    fn acu_anchor_ignores_discriminant_in_meta() {
        let line = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hi"}}"#,
            r#"{"sessionUpdate":"available_commands_update"}"#,
        );
        // The exact `"sessionUpdate":"available_commands_update"` substring IS
        // present (in _meta), but it's not anchored to `"update":{`.
        assert!(line.contains(r#""sessionUpdate":"available_commands_update""#));
        assert!(!line_is_available_commands_update(&line));
    }

    /// A NON-ACU line whose `_meta` embeds the FULL unescaped nested anchor
    /// (`{"update":{"sessionUpdate":"available_commands_update",...}}`) passes the
    /// cheap substring pre-filter but must be REJECTED by the positional confirm
    /// (its real `params.update` is a `tool_call`) — so it is never dropped.
    #[test]
    fn acu_confirm_rejects_nested_update_anchor_in_meta() {
        let line = acp_envelope_with_meta(
            r#"{"sessionUpdate":"tool_call","toolCallId":"t","title":"x"}"#,
            r#"{"echo":{"update":{"sessionUpdate":"available_commands_update","availableCommands":[]}}}"#,
        );
        // The discriminant prefix IS present (in _meta) — pre-filter would match...
        assert!(line.contains(&*AVAILABLE_COMMANDS_UPDATE_PREFIX));
        // ...but the structural params.update is a tool_call, so NOT an ACU.
        assert!(!line_is_available_commands_update(&line));

        // And the non-ACU line survives replay (is not dropped).
        let raw = format!("{line}\n");
        let prepared = prepare_replay_lines(&raw, None);
        assert_eq!(prepared.lines.len(), 1, "non-ACU line must not be dropped");
        assert!(prepared.lines[0].contains("tool_call"));
    }

    /// Pin the cross-crate assumption behind [`line_is_available_commands_update`]:
    /// the structural `params.update` serializes BEFORE the optional `_meta`. Run a
    /// genuine ACU through the real write path ([`SessionUpdateEnvelope::from_update`])
    /// and assert its first `"update":` precedes any `"_meta":`, and the detector accepts it.
    #[test]
    fn acu_real_write_path_serializes_update_before_meta() {
        let notif = acp::SessionNotification::new(
            acp::SessionId::new("s"),
            acp::SessionUpdate::AvailableCommandsUpdate(acp::AvailableCommandsUpdate::new(vec![])),
        )
        .meta(serde_json::json!({ "eventId": "ev1" }).as_object().cloned());
        let envelope =
            SessionUpdateEnvelope::from_update(&SessionUpdate::Acp(Box::new(notif))).unwrap();
        let line = serde_json::to_string(&envelope).unwrap();

        let update_idx = line
            .find(UPDATE_KEY)
            .expect("serialized ACU line must contain an \"update\" key");
        if let Some(meta_idx) = line.find(r#""_meta":"#) {
            assert!(
                update_idx < meta_idx,
                "params.update must serialize before _meta: {line}"
            );
        }
        assert!(line_is_available_commands_update(&line));
    }

    #[test]
    fn prepare_replay_drops_available_commands_update() {
        let u = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hi"}}"#,
        );
        let acu =
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#);
        let a = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"yo"}}"#,
        );
        let raw = format!("{u}\n{acu}\n{a}\n");

        let prepared = prepare_replay_lines(&raw, None);
        // ACU dropped; the two real updates kept in original order.
        assert_eq!(prepared.lines.len(), 2);
        assert_eq!(prepared.total_live, 2);
        assert!(
            prepared
                .lines
                .iter()
                .all(|l| !l.contains("available_commands_update"))
        );
        assert!(prepared.lines[0].contains("hi"));
        assert!(prepared.lines[1].contains("yo"));
        assert!(prepared.mark_replay);
    }

    #[test]
    fn prepare_replay_rewind_truncates_and_drops_acu() {
        let u0 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p0"}}"#,
            r#"{"totalTokens":5}"#,
        );
        let acu =
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#);
        let a0 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"a0"}}"#,
            r#"{"totalTokens":7}"#,
        );
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":0,"created_at":"2024-01-01"}"#,
        );
        let u1 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p1"}}"#,
            r#"{"totalTokens":9}"#,
        );
        let raw = format!("{u0}\n{acu}\n{a0}\n{rw}\n{u1}\n");

        let prepared = prepare_replay_lines(&raw, None);
        // Rewind to 0 kills u0/a0; ACU dropped; only the new p1 survives.
        assert_eq!(prepared.lines.len(), 1);
        assert!(prepared.lines[0].contains("p1"));
        assert_eq!(prepared.total_live, 1);
        assert!(prepared.mark_replay);
    }

    /// The single-pass implementation must match an independent reference that
    /// drops ACU then applies the (canonical) rewind filter — for a mixed input.
    #[test]
    fn prepare_replay_single_pass_matches_reference() {
        let lines_src = [
            acp_envelope_with_meta(
                r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p0"}}"#,
                r#"{"totalTokens":3}"#,
            ),
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#),
            acp_envelope_with_meta(
                r#"{"sessionUpdate":"tool_call_update","toolCallId":"t","status":"completed"}"#,
                r#"{"totalTokens":11}"#,
            ),
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#),
            acp_envelope(
                r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"a0"}}"#,
            ),
        ];
        let raw = format!("{}\n", lines_src.join("\n"));

        // Reference: filter blanks + ACU, then canonical rewind filter, count.
        let reference: Vec<&str> = filter_rewind_lines(
            raw.lines()
                .filter(|l| !l.trim().is_empty() && !line_is_available_commands_update(l))
                .collect(),
        );

        let prepared = prepare_replay_lines(&raw, None);
        assert_eq!(prepared.lines, reference);
        assert_eq!(prepared.total_live, reference.len());
    }

    /// A user prompt whose text contains the literal escaped-JSON ACU
    /// discriminant must NOT be dropped as an `available_commands_update` — the
    /// `"update":{` anchor only matches the real structural discriminant, not the
    /// escaped fragment in content.
    #[test]
    fn acu_drop_ignores_escaped_json_in_content() {
        let line = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"paste: {\"sessionUpdate\":\"available_commands_update\"}"}}"#,
        );
        // The bare phrase appears in the (escaped) content, but it's not at the
        // structural `"update":{"sessionUpdate":...` position, so it's kept.
        assert!(line.contains("available_commands_update"));
        assert!(!line_is_available_commands_update(&line));

        let raw = format!("{line}\n");
        let prepared = prepare_replay_lines(&raw, None);
        assert_eq!(prepared.lines.len(), 1, "user prompt must survive replay");
        assert!(prepared.lines[0].contains("available_commands_update"));
    }

    /// An idle client reconnecting with the cursor pointing at the LAST persisted
    /// event — an ACU (the post-load re-advertise) — must resolve the cursor on the
    /// ACU-inclusive set rather than fall back to full replay.
    #[test]
    fn prepare_replay_cursor_on_dropped_acu_resolves() {
        let u = acp_envelope_with_meta(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hi"}}"#,
            r#"{"eventId":"ev1"}"#,
        );
        let a = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"yo"}}"#,
            r#"{"eventId":"ev2"}"#,
        );
        let acu = acp_envelope_with_meta(
            r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#,
            r#"{"eventId":"ev3"}"#,
        );
        let raw = format!("{u}\n{a}\n{acu}\n");

        // Cursor == the ACU's eventId → resolved; nothing after → no replay,
        // and crucially NOT a full replay.
        let prepared = prepare_replay_lines(&raw, Some("ev3"));
        assert!(!prepared.mark_replay, "must not fall back to full replay");
        assert!(prepared.lines.is_empty(), "client is already caught up");

        // Cursor == ev1 → replay ev2, ev3; the ACU (ev3) is dropped from the tail.
        let prepared = prepare_replay_lines(&raw, Some("ev1"));
        assert!(!prepared.mark_replay);
        assert_eq!(prepared.lines.len(), 1);
        assert!(prepared.lines[0].contains("yo"));
    }

    /// A trailing `rewind_marker` empties the live replay set.
    #[test]
    fn prepare_replay_trailing_rewind_marker_empties() {
        let u0 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p0"}}"#,
            r#"{"totalTokens":5}"#,
        );
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":0,"created_at":"2024-01-01"}"#,
        );
        let raw = format!("{u0}\n{rw}\n");
        let prepared = prepare_replay_lines(&raw, None);
        assert!(prepared.lines.is_empty());
        assert_eq!(prepared.total_live, 0);
    }

    /// An ACU as the final line is dropped.
    #[test]
    fn prepare_replay_trailing_acu_dropped() {
        let u = acp_envelope_with_meta(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"hi"}}"#,
            r#"{"totalTokens":7}"#,
        );
        let acu =
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#);
        let raw = format!("{u}\n{acu}\n");
        let prepared = prepare_replay_lines(&raw, None);
        assert_eq!(prepared.lines.len(), 1);
        assert!(prepared.lines[0].contains("hi"));
        assert_eq!(prepared.total_live, 1);
    }

    /// Rewind + cursor + ACU together, with explicit expected values.
    #[test]
    fn prepare_replay_rewind_then_cursor_with_acu() {
        let u0 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p0"}}"#,
            r#"{"eventId":"e0","totalTokens":2}"#,
        );
        let a0 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"a0"}}"#,
            r#"{"eventId":"e1"}"#,
        );
        let acu0 =
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#);
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":0,"created_at":"2024-01-01"}"#,
        );
        let u1 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p1"}}"#,
            r#"{"eventId":"e2","totalTokens":9}"#,
        );
        let acu1 =
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#);
        let a1 = acp_envelope_with_meta(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"a1"}}"#,
            r#"{"eventId":"e3","totalTokens":12}"#,
        );
        let raw = format!("{u0}\n{a0}\n{acu0}\n{rw}\n{u1}\n{acu1}\n{a1}\n");

        // Rewind to 0 kills u0/a0/acu0; surviving live = [u1(e2), acu1, a1(e3)].
        // Cursor on e2 → tail = [acu1, a1]; drop acu1 → lines = [a1].
        let prepared = prepare_replay_lines(&raw, Some("e2"));
        assert!(!prepared.mark_replay);
        assert_eq!(prepared.lines.len(), 1);
        assert!(prepared.lines[0].contains("a1"));
        assert_eq!(prepared.total_live, 2); // ACU-free survivors: u1, a1
    }

    /// The delta-replay helper (shared with the initial path) drops blanks + ACUs
    /// and applies the canonical rewind filter.
    #[test]
    fn filter_delta_replay_drops_blank_acu_and_rewinds() {
        let u1 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p1"}}"#,
        );
        let acu =
            acp_envelope(r#"{"sessionUpdate":"available_commands_update","availableCommands":[]}"#);
        let a1 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"a1"}}"#,
        );
        // A second prompt that a trailing rewind_marker then discards.
        let u2 = acp_envelope(
            r#"{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"p2-dead"}}"#,
        );
        let a2 = acp_envelope(
            r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"a2-dead"}}"#,
        );
        let rw = grow_envelope(
            r#"{"sessionUpdate":"rewind_marker","target_prompt_index":1,"created_at":"2024-01-01"}"#,
        );
        let raw = format!("{u1}\n\n{acu}\n{a1}\n{u2}\n{a2}\n{rw}\n");

        let live = filter_delta_replay_lines(raw.lines().collect());
        // Blank + ACU dropped; the rewind to prompt 1 truncates the dead branch
        // (u2/a2) and consumes the marker, leaving only p1/a1.
        assert_eq!(live.len(), 2);
        assert!(
            live.iter()
                .all(|l| !l.contains("available_commands_update"))
        );
        assert!(live[0].contains("p1"));
        assert!(live[1].contains("a1"));
        assert!(live.iter().all(|l| !l.contains("dead")));
        assert!(live.iter().all(|l| !l.contains("rewind_marker")));
    }

    #[test]
    fn prepare_replay_reports_subagent_projection_coverage() {
        let spawn = |id: &str, child: &str| {
            format!(
                r#"{{"method":"_grow/session/update","params":{{"sessionId":"s","update":{{"sessionUpdate":"subagent_spawned","subagent_id":"{id}","parent_session_id":"s","child_session_id":"{child}","subagent_type":"general-purpose","description":"task"}},"_meta":{{"eventId":"s-1"}}}}}}"#
            )
        };
        let finish = |id: &str| {
            format!(
                r#"{{"method":"_grow/session/update","params":{{"sessionId":"s","update":{{"sessionUpdate":"subagent_finished","subagent_id":"{id}","child_session_id":"c{id}","status":"completed","tool_calls":0,"turns":0,"duration_ms":0,"tokens_used":0}},"_meta":{{"eventId":"s-2"}}}}}}"#
            )
        };
        let raw = format!(
            "{}\n{}\n{}\n",
            spawn("a", "ca"),
            finish("a"),
            spawn("b", "cb")
        );
        let prepared = prepare_replay_lines(&raw, None);
        assert_eq!(
            prepared.subagent_projections.spawned,
            ["a".to_string(), "b".to_string()].into_iter().collect()
        );
        assert_eq!(
            prepared.subagent_projections.finished,
            ["a".to_string()].into_iter().collect()
        );
    }

    /// Resume idempotency seam: the finish the projection repair emits must
    /// re-pair the spawn on the next resume (emit→serialize→collect),
    /// so a second resume doesn't re-emit. Guards a `SubagentFinished` shape drift.
    #[test]
    fn collect_tracks_spawn_and_finish_projections_independently() {
        use crate::extensions::notification::{SessionNotification, SessionUpdate};

        let spawn = grow_envelope(
            r#"{"sessionUpdate":"subagent_spawned","subagent_id":"sa","parent_session_id":"s","child_session_id":"ca","subagent_type":"general-purpose","description":"task"}"#,
        );
        // Build the finish exactly as the stream reconcile emits it.
        let finish_notification = SessionNotification {
            session_id: acp::SessionId::new("s"),
            update: SessionUpdate::SubagentFinished {
                subagent_id: "sa".into(),
                child_session_id: "ca".into(),
                status: "cancelled".into(),
                error: Some("interrupted by process restart".into()),
                tool_calls: 0,
                turns: 0,
                duration_ms: 0,
                tokens_used: 0,
                output: None,
            },
            meta: None,
        };
        let finish = serde_json::to_string(
            &SessionUpdateEnvelope::from_update(&super::SessionUpdate::Grow(Box::new(
                finish_notification,
            )))
            .unwrap(),
        )
        .unwrap();

        let state = collect_subagent_projection_state(&[spawn.as_str(), finish.as_str()]);
        assert!(state.spawned.contains("sa"));
        assert!(state.finished.contains("sa"));
    }

    #[test]
    fn from_str_unknown_grow_variant_is_rejected() {
        let line = grow_envelope(r#"{"sessionUpdate":"git_branch_update","branch":"main"}"#);
        assert!(SessionUpdateEnvelope::from_str(&line).is_err());
    }

    #[test]
    fn from_str_known_grow_variant_still_works() {
        let line = grow_envelope(r#"{"sessionUpdate":"memory_flush_started"}"#);
        let update = SessionUpdateEnvelope::from_str(&line).unwrap();
        match update {
            SessionUpdate::Grow(notif) => {
                assert_eq!(
                    notif.update,
                    crate::extensions::notification::SessionUpdate::MemoryFlushStarted
                );
            }
            SessionUpdate::Acp(_) => panic!("expected Grow variant"),
        }
    }
}
