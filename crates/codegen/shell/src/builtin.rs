//! Built-in files extracted to `~/.grow/` on startup.

use std::io::{self, Read as _, Write as _};
use std::path::{Component, Path, PathBuf};

const BUILTIN_FILES: &[(&str, &str)] = &[
    ("README.md", include_str!("../README.md")),
    (
        "workflows/deep-research.rhai",
        include_str!("../../../../.grow/workflows/deep-research.rhai"),
    ),
];

/// Extract built-in metadata files to `~/.grow/` on startup.
///
/// User skills under `~/.grow/skills/` are never managed here. Platform skills
/// are delivered separately through the bundled skill cache.
pub fn extract_builtin_files(grow_home: &std::path::Path) {
    if let Err(error) = extract_builtin_files_transaction(grow_home) {
        tracing::debug!(%error, path = %grow_home.display(), "Failed to extract built-in files");
    }
}

fn extract_builtin_files_transaction(grow_home: &Path) -> io::Result<()> {
    let version = version::VERSION;
    std::fs::create_dir_all(grow_home)?;
    let root_metadata = std::fs::symlink_metadata(grow_home)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Grow home must be a real directory",
        ));
    }
    let root = cap_std::fs::Dir::open_ambient_dir(grow_home, cap_std::ambient_authority())?;
    let extraction_lock = open_managed_lock(&root, Path::new(".builtin-extraction.lock"))?;
    fs2::FileExt::lock_exclusive(&extraction_lock)?;
    let marker = Path::new(".metadata_version");

    // Re-check only after the cross-process lease is held. Different Grow
    // versions may start concurrently against the same home directory.
    // Clean up cached changelog files from previous version so
    // /release-notes fetches fresh content for the new version.
    if !managed_file_matches(&root, marker, version.as_bytes())? {
        for stale in &["CHANGELOG.json", "CHANGELOG.md"] {
            let _ = root.remove_file(stale);
        }
    }

    for &(filename, content) in BUILTIN_FILES {
        let path = Path::new(filename);
        if path == Path::new("workflows/deep-research.rhai") {
            // Share the exact Definition publication lock used by Registry.
            // Publish and extraction are linear commits, so the later complete
            // operation wins without a mixed Definition/marker generation.
            let definition_lock =
                open_managed_lock(&root, Path::new("workflows/.deep-research.lock"))?;
            fs2::FileExt::lock_exclusive(&definition_lock)?;
            let result = write_managed_file(&root, path, content.as_bytes());
            let _ = fs2::FileExt::unlock(&definition_lock);
            result?;
        } else {
            write_managed_file(&root, path, content.as_bytes())?;
        }
    }

    // The marker is the transaction commit record. Publishing it last keeps a
    // partial extraction retryable after I/O failure or process interruption.
    write_managed_file(&root, marker, version.as_bytes())?;
    let _ = fs2::FileExt::unlock(&extraction_lock);
    tracing::debug!(version, "Extracted built-in files");
    Ok(())
}

fn open_managed_lock(root: &cap_std::fs::Dir, path: &Path) -> io::Result<std::fs::File> {
    use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt as _};

    ensure_real_parent_dirs(root, path.parent().unwrap_or_else(|| Path::new("")))?;
    let mut options = cap_std::fs::OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .follow(FollowSymlinks::No);
    let file = root.open_with(path, &options)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Managed lock must be a regular file",
        ));
    }
    Ok(file.into_std())
}

fn managed_file_matches(root: &cap_std::fs::Dir, path: &Path, expected: &[u8]) -> io::Result<bool> {
    use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt as _};

    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = match root.open_with(path, &options) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => return Ok(false),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    let expected_len = u64::try_from(expected.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "managed file is too large"))?;
    if !metadata.is_file() || metadata.len() != expected_len {
        return Ok(false);
    }
    let mut actual = Vec::with_capacity(expected.len());
    file.take(expected_len.saturating_add(1))
        .read_to_end(&mut actual)?;
    Ok(actual == expected)
}

fn write_managed_file(root: &cap_std::fs::Dir, path: &Path, content: &[u8]) -> io::Result<()> {
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Managed file path must stay beneath Grow home",
        ));
    }

    ensure_real_parent_dirs(root, path.parent().unwrap_or_else(|| Path::new("")))?;
    if managed_file_matches(root, path, content)? {
        return Ok(());
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid managed filename"))?;
    let temp_name = format!(".{file_name}.{}.tmp", uuid::Uuid::now_v7());
    let temp_path = path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(temp_name);
    let mut options = cap_std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    let result = (|| {
        let mut file = root.open_with(&temp_path, &options)?;
        file.write_all(content)?;
        file.sync_all()?;
        drop(file);
        root.rename(&temp_path, root, path)?;
        sync_managed_parent(root, path.parent().unwrap_or_else(|| Path::new("")))
    })();
    if result.is_err() {
        let _ = root.remove_file(&temp_path);
    }
    result
}

#[cfg(unix)]
fn sync_managed_parent(root: &cap_std::fs::Dir, parent: &Path) -> io::Result<()> {
    let directory = if parent.as_os_str().is_empty() {
        root.try_clone()?
    } else {
        root.open_dir(parent)?
    };
    directory.into_std_file().sync_all()
}

#[cfg(not(unix))]
fn sync_managed_parent(_root: &cap_std::fs::Dir, _parent: &Path) -> io::Result<()> {
    Ok(())
}

fn ensure_real_parent_dirs(root: &cap_std::fs::Dir, parent: &Path) -> io::Result<()> {
    let mut current = PathBuf::new();
    for component in parent.components() {
        let Component::Normal(component) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Managed directory path must stay beneath Grow home",
            ));
        };
        current.push(component);
        match root.create_dir(&current) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        let metadata = root.symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Managed directory is not a real directory: {}",
                    current.display()
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_bump_reextracts_managed_files_without_touching_user_content() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();

        extract_builtin_files(home);
        std::fs::write(home.join("README.md"), "old").unwrap();
        std::fs::write(home.join(".metadata_version"), "0.0.0-stale").unwrap();

        let skill_names = [
            "help",
            "create-skill",
            "code-review",
            "check-work",
            "check",
            "best-of-n",
            "docx",
            "pptx",
            "xlsx",
        ];
        for name in skill_names {
            let dir = home.join("skills").join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("SKILL.md"), format!("custom {name}")).unwrap();
            std::fs::write(dir.join("user-file.txt"), "keep").unwrap();
        }

        extract_builtin_files(home);

        assert_ne!(
            std::fs::read_to_string(home.join("README.md")).unwrap(),
            "old"
        );
        for name in skill_names {
            let dir = home.join("skills").join(name);
            assert_eq!(
                std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
                format!("custom {name}")
            );
            assert_eq!(
                std::fs::read_to_string(dir.join("user-file.txt")).unwrap(),
                "keep"
            );
        }
        let deep_research =
            std::fs::read_to_string(home.join("workflows").join("deep-research.rhai")).unwrap();
        let meta = workflow::extract_meta(&deep_research).unwrap();
        assert_eq!(meta.name, "deep-research");
    }

    #[test]
    fn same_version_reconciles_managed_files_without_touching_user_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        std::fs::create_dir_all(home.join("skills/check")).unwrap();
        std::fs::write(home.join("skills/check/SKILL.md"), "custom check").unwrap();
        std::fs::write(home.join(".metadata_version"), version::VERSION).unwrap();

        extract_builtin_files(home);

        assert!(!home.join("skills/help/SKILL.md").exists());
        assert_eq!(
            std::fs::read_to_string(home.join("skills/check/SKILL.md")).unwrap(),
            "custom check"
        );
        assert!(home.join("workflows/deep-research.rhai").is_file());
    }

    #[test]
    fn failed_extraction_does_not_publish_marker_and_retries() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        std::fs::write(home.join("workflows"), "not a directory").unwrap();

        extract_builtin_files(home);

        assert!(!home.join(".metadata_version").exists());
        std::fs::remove_file(home.join("workflows")).unwrap();
        extract_builtin_files(home);
        assert_eq!(
            std::fs::read_to_string(home.join(".metadata_version")).unwrap(),
            version::VERSION
        );
        assert!(home.join("workflows/deep-research.rhai").is_file());
    }

    #[test]
    fn concurrent_extractors_publish_one_complete_generation() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().to_path_buf();
        let workers = (0..4)
            .map(|_| {
                let home = home.clone();
                std::thread::spawn(move || extract_builtin_files(&home))
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }

        assert_eq!(
            std::fs::read_to_string(home.join(".metadata_version")).unwrap(),
            version::VERSION
        );
        for (path, expected) in BUILTIN_FILES {
            assert_eq!(std::fs::read_to_string(home.join(path)).unwrap(), *expected);
        }
    }

    #[cfg(unix)]
    #[test]
    fn extraction_rejects_symlinked_managed_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, home.join("workflows")).unwrap();

        extract_builtin_files(&home);

        assert!(!home.join(".metadata_version").exists());
        assert!(!outside.join("deep-research.rhai").exists());
    }
}
