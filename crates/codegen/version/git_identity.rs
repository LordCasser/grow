use std::path::{Path, PathBuf};
use std::process::Command;

/// Git metadata whose changes can alter the commit embedded by a build script.
pub fn watch_paths(manifest_dir: &Path) -> Vec<PathBuf> {
    let Some(git_dir) = git_path(manifest_dir, "--absolute-git-dir") else {
        return Vec::new();
    };
    let common_dir = git_path(manifest_dir, "--git-common-dir")
        .map(|path| resolve_from(manifest_dir, path))
        .unwrap_or_else(|| git_dir.clone());

    let mut paths = vec![git_dir.join("HEAD")];
    let packed_refs = common_dir.join("packed-refs");
    if packed_refs.exists() {
        paths.push(packed_refs);
    }
    if let Some(reference) = git_output(manifest_dir, &["symbolic-ref", "--quiet", "HEAD"])
        .and_then(|reference| String::from_utf8(reference).ok())
    {
        let reference = reference.trim();
        if !reference.is_empty() {
            let reference_path = common_dir.join(reference);
            if reference_path.exists() {
                paths.push(reference_path);
            } else {
                // Cargo re-runs continuously for a watched path that does not exist.
                // Watch the nearest existing ref ancestor to catch a packed ref
                // becoming loose, including when pack-refs removed nested parents.
                let mut ancestor = reference_path.parent();
                while let Some(path) = ancestor {
                    if path.exists() {
                        paths.push(path.to_path_buf());
                        break;
                    }
                    ancestor = path.parent();
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn git_path(manifest_dir: &Path, argument: &str) -> Option<PathBuf> {
    let output = git_output(manifest_dir, &["rev-parse", argument])?;
    let value = String::from_utf8(output).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| {
        let path = resolve_from(manifest_dir, PathBuf::from(value));
        path.canonicalize().unwrap_or(path)
    })
}

fn git_output(manifest_dir: &Path, arguments: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(manifest_dir)
        .args(arguments)
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

fn resolve_from(base: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::watch_paths;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("grow-git-identity-{}-{id}", std::process::id()));
            std::fs::create_dir(&path).expect("create temp directory");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn git(cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("git output is UTF-8")
            .trim()
            .to_owned()
    }

    fn init_repo(path: &Path) {
        std::fs::create_dir(path).expect("create repo");
        git(path, &["init", "--initial-branch=main"]);
        git(path, &["config", "user.name", "Test"]);
        git(path, &["config", "user.email", "test@example.invalid"]);
        std::fs::write(path.join("tracked"), "one\n").expect("write tracked file");
        git(path, &["add", "tracked"]);
        git(path, &["commit", "-m", "initial"]);
    }

    fn contains(paths: &[PathBuf], path: PathBuf) -> bool {
        paths.iter().any(|candidate| candidate == &path)
    }

    #[test]
    fn watches_normal_checkout_head_branch_and_packed_refs() {
        let temp = TempDir::new();
        let repo = temp.0.join("repo");
        init_repo(&repo);

        let git_dir = std::fs::canonicalize(git(&repo, &["rev-parse", "--absolute-git-dir"]))
            .expect("canonicalize git dir");
        let paths = watch_paths(&repo);
        assert!(contains(&paths, git_dir.join("HEAD")), "{paths:?}");
        assert!(
            contains(&paths, git_dir.join("refs/heads/main")),
            "{paths:?}"
        );
        assert!(!contains(&paths, git_dir.join("packed-refs")));
        assert!(!contains(&paths, git_dir.join("refs/heads")));

        git(&repo, &["pack-refs", "--all", "--prune"]);
        let paths = watch_paths(&repo);
        assert!(contains(&paths, git_dir.join("packed-refs")));
        assert!(!contains(&paths, git_dir.join("refs/heads/main")));
        assert!(contains(&paths, git_dir.join("refs/heads")));

        std::fs::write(repo.join("tracked"), "two\n").expect("update tracked file");
        git(&repo, &["add", "tracked"]);
        git(&repo, &["commit", "-m", "second"]);
        let paths = watch_paths(&repo);
        assert!(contains(&paths, git_dir.join("refs/heads/main")));
        assert!(!contains(&paths, git_dir.join("refs/heads")));
    }

    #[test]
    fn watches_nearest_existing_ancestor_for_nested_packed_branch() {
        let temp = TempDir::new();
        let repo = temp.0.join("repo");
        init_repo(&repo);
        git(&repo, &["switch", "-c", "feature/topic"]);
        git(&repo, &["pack-refs", "--all", "--prune"]);

        let git_dir = std::fs::canonicalize(git(&repo, &["rev-parse", "--absolute-git-dir"]))
            .expect("canonicalize git dir");
        let paths = watch_paths(&repo);
        assert!(!git_dir.join("refs/heads/feature").exists());
        assert!(contains(&paths, git_dir.join("refs/heads")), "{paths:?}");
        assert!(paths.iter().all(|path| path.exists()), "{paths:?}");
    }

    #[test]
    fn watches_linked_worktree_head_and_common_branch_ref() {
        let temp = TempDir::new();
        let repo = temp.0.join("repo");
        let worktree = temp.0.join("linked");
        init_repo(&repo);
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                "linked-branch",
                worktree.to_str().expect("worktree path is UTF-8"),
            ],
        );

        let worktree_git_dir =
            std::fs::canonicalize(git(&worktree, &["rev-parse", "--absolute-git-dir"]))
                .expect("canonicalize worktree git dir");
        let common_dir = PathBuf::from(git(&worktree, &["rev-parse", "--git-common-dir"]));
        let common_dir = if common_dir.is_absolute() {
            common_dir
        } else {
            worktree.join(common_dir)
        };
        let common_dir = std::fs::canonicalize(common_dir).expect("canonicalize common dir");
        let paths = watch_paths(&worktree);
        assert!(contains(&paths, worktree_git_dir.join("HEAD")));
        assert!(contains(
            &paths,
            common_dir.join("refs/heads/linked-branch")
        ));
        assert!(!contains(&paths, common_dir.join("packed-refs")));
    }

    #[test]
    fn detached_head_watches_only_its_head_for_identity_changes() {
        let temp = TempDir::new();
        let repo = temp.0.join("repo");
        init_repo(&repo);
        git(&repo, &["checkout", "--detach"]);

        let git_dir = std::fs::canonicalize(git(&repo, &["rev-parse", "--absolute-git-dir"]))
            .expect("canonicalize git dir");
        let paths = watch_paths(&repo);
        assert!(contains(&paths, git_dir.join("HEAD")));
        assert!(!paths.iter().any(|path| path.ends_with("refs/heads/main")));
    }
}
