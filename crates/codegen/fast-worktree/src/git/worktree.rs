//! Git worktree operations.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::git::checkout::git_command;

/// Create a git worktree with `--no-checkout`. Blocking.
pub(crate) fn worktree_add_no_checkout(source: &Path, dest: &str, git_ref: &str) -> Result<()> {
    let output = git_command()
        .current_dir(source)
        .args([
            "worktree",
            "add",
            "--detach",
            "--no-checkout",
            dest,
            git_ref,
        ])
        .output()
        .context("failed to run git worktree add")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git worktree add failed: {}", stderr);
    }

    Ok(())
}

/// Resolve a checkout's gitfile only when it identifies that checkout's linked
/// registration. A gitfile alone never authorizes deleting its target.
pub(crate) fn verified_worktree_registration(worktree_path: &Path) -> Result<Option<PathBuf>> {
    let gitfile = worktree_path.join(".git");
    match std::fs::symlink_metadata(&gitfile) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("inspect worktree gitfile"),
    }
    let content = std::fs::read_to_string(&gitfile).context("read worktree gitfile")?;
    let Some(gitdir) = content.trim().strip_prefix("gitdir: ") else {
        return Ok(None);
    };
    let candidate = worktree_path.join(gitdir);
    match std::fs::symlink_metadata(&candidate) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("inspect worktree registration candidate"),
    }
    let registration = dunce::canonicalize(&candidate).context("resolve registration candidate")?;
    let Some(worktrees_dir) = registration.parent() else {
        return Ok(None);
    };
    if worktrees_dir.file_name() != Some(std::ffi::OsStr::new("worktrees")) {
        return Ok(None);
    }
    let Some(common_dir) = worktrees_dir.parent() else {
        return Ok(None);
    };
    if !is_common_git_dir(common_dir) {
        return Ok(None);
    }
    if registration_matches_worktree(&registration, common_dir, worktree_path)? {
        Ok(Some(registration))
    } else {
        Ok(None)
    }
}

/// Structural discovery deliberately does not open repository configuration.
fn is_common_git_dir(path: &Path) -> bool {
    matches!(
        gix::discover::is_git(path),
        Ok(gix::discover::repository::Kind::PossiblyBare
            | gix::discover::repository::Kind::SubmoduleGitDir
            | gix::discover::repository::Kind::WorkTree {
                linked_git_dir: None
            })
    )
}

fn registration_matches_worktree(
    registration: &Path,
    common_dir: &Path,
    worktree_path: &Path,
) -> Result<bool> {
    // Compare physical entities, but require the registration's location as
    // well: a symlink or a matching backlink cannot turn another dir into one.
    if registration.parent() != Some(common_dir.join("worktrees").as_path()) {
        return Ok(false);
    }
    let common_ref = std::fs::read_to_string(registration.join("commondir"))
        .context("read registration commondir")?;
    if common_ref.trim().is_empty() {
        return Ok(false);
    }
    let recorded_common = dunce::canonicalize(registration.join(common_ref.trim()))
        .context("resolve registration commondir")?;
    if recorded_common != common_dir {
        return Ok(false);
    }
    let backlink = std::fs::read_to_string(registration.join("gitdir"))
        .context("read registration gitdir backlink")?;
    if backlink.trim().is_empty() {
        return Ok(false);
    }
    let recorded_gitfile = normalized_worktree_path(&registration.join(backlink.trim()))?;
    let expected_gitfile = normalized_worktree_path(worktree_path)?.join(".git");
    Ok(recorded_gitfile == expected_gitfile)
}

/// Retry a partially removed, metadata-tracked linked checkout. The saved
/// source supplies the common repository; an absent checkout supplies no pointer.
/// Unlike stale cleanup, any uncertainty must keep the tracking record alive.
#[cfg(feature = "metadata")]
pub(crate) fn remove_missing_worktree_registration(
    source_repo: &Path,
    worktree_path: &Path,
) -> Result<()> {
    let source_repo = dunce::canonicalize(source_repo)
        .context("resolve source for incomplete worktree removal")?;
    let (repository, _) = gix::discover::upwards(&source_repo)
        .context("find source repository for incomplete worktree removal")?;
    let (git_dir, _) = repository.into_repository_and_work_tree_directories();
    let git_dir = dunce::canonicalize(git_dir).context("resolve source Git directory")?;
    let common_dir =
        match gix::discover::is_git(&git_dir).context("validate source Git directory")? {
            gix::discover::repository::Kind::WorkTreeGitDir { .. } => {
                let common_ref = std::fs::read_to_string(git_dir.join("commondir"))
                    .context("read source commondir")?;
                anyhow::ensure!(!common_ref.trim().is_empty(), "empty source commondir");
                dunce::canonicalize(git_dir.join(common_ref.trim()))
                    .context("resolve source commondir")?
            }
            _ => git_dir,
        };
    anyhow::ensure!(
        is_common_git_dir(&common_dir),
        "invalid source common Git directory"
    );
    let worktrees_dir = common_dir.join("worktrees");
    match std::fs::symlink_metadata(&worktrees_dir) {
        Ok(metadata) => anyhow::ensure!(metadata.is_dir(), "unverified source worktrees directory"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("inspect source worktrees directory"),
    }
    let entries =
        std::fs::read_dir(&worktrees_dir).context("scan source worktree registrations")?;
    let expected_gitfile = normalized_worktree_path(worktree_path)?.join(".git");
    for entry in entries {
        let entry = entry.context("read source worktree registration entry")?;
        let registration = entry.path();
        anyhow::ensure!(
            entry
                .file_type()
                .context("inspect source registration entry")?
                .is_dir(),
            "unverified worktree registration entry: {}",
            registration.display()
        );
        let backlink = std::fs::read_to_string(registration.join("gitdir"))
            .context("read source registration backlink")?;
        anyhow::ensure!(
            !backlink.trim().is_empty(),
            "empty source registration backlink"
        );
        if normalized_worktree_path(&registration.join(backlink.trim()))? != expected_gitfile {
            continue;
        }
        let registration =
            dunce::canonicalize(registration).context("resolve source registration")?;
        anyhow::ensure!(
            registration_matches_worktree(&registration, &common_dir, worktree_path)?,
            "registration identity does not match incomplete worktree removal"
        );
        std::fs::remove_dir_all(&registration).with_context(|| {
            format!(
                "worktree checkout is absent but registration removal is incomplete: {}",
                registration.display()
            )
        })?;
    }
    Ok(())
}

/// Which stale registrations [`remove_stale_worktree_registrations`] removes.
#[derive(Clone, Copy, Debug)]
pub enum StaleWorktreeMatch<'a> {
    /// Exactly the registration whose recorded worktree path is this path.
    Path(&'a Path),
    /// Every verified registration whose recorded worktree path is under this
    /// prefix (e.g. a tool-owned base directory).
    UnderPrefix(&'a Path),
}

/// Remove stale `.git/worktrees/<id>` registrations matching `match_rule` only
/// after verifying their direct location, common directory and exact backlink.
///
/// Deliberately not `git worktree prune`: prune deletes every registration
/// whose worktree path is not visible from the current mount namespace (git
/// applies no expiry protection to that case) and deletes `.git/worktrees`
/// itself once emptied — under a container that does not mount the user's
/// linked worktrees, that wiped them all. Best-effort: failures are logged,
/// never returned. Returns the number of registrations removed (git suffixes
/// ids on basename collisions, so an id may differ from the basename).
pub fn remove_stale_worktree_registrations(
    source_repo: &Path,
    match_rule: StaleWorktreeMatch<'_>,
) -> u64 {
    match try_remove_stale_worktree_registrations(source_repo, match_rule) {
        Ok(removed) => removed,
        Err(error) => {
            tracing::warn!(
                source_repo = %source_repo.display(),
                error = %error,
                "stale registration scrub skipped: cannot verify source or scope"
            );
            0
        }
    }
}

fn try_remove_stale_worktree_registrations(
    source_repo: &Path,
    match_rule: StaleWorktreeMatch<'_>,
) -> Result<u64> {
    if let StaleWorktreeMatch::Path(path) = match_rule
        && path.try_exists().context("inspect stale worktree target")?
    {
        return Ok(0);
    }

    let output = git_command()
        .current_dir(source_repo)
        .args(["rev-parse", "--git-common-dir"])
        .output()
        .context("resolve stale registration source common directory")?;
    anyhow::ensure!(
        output.status.success(),
        "git rev-parse --git-common-dir failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let common_ref = std::str::from_utf8(&output.stdout)
        .context("decode source common directory")?
        .trim();
    anyhow::ensure!(!common_ref.is_empty(), "empty source common directory");
    let common_dir = dunce::canonicalize(source_repo.join(common_ref))
        .context("resolve source common Git directory")?;
    anyhow::ensure!(
        is_common_git_dir(&common_dir),
        "invalid source common Git directory"
    );
    let worktrees_dir = common_dir.join("worktrees");
    match std::fs::symlink_metadata(&worktrees_dir) {
        Ok(metadata) => anyhow::ensure!(metadata.is_dir(), "unverified worktrees directory"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error).context("inspect worktrees directory"),
    }
    let entries = std::fs::read_dir(&worktrees_dir).context("scan stale worktree registrations")?;
    let normalized_target = match match_rule {
        StaleWorktreeMatch::Path(path) | StaleWorktreeMatch::UnderPrefix(path) => {
            normalized_worktree_path(path)?
        }
    };
    let mut removed = 0u64;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(%error, "cannot read stale registration entry; preserving it");
                continue;
            }
        };
        let registration = entry.path();
        let recorded = match verified_stale_registration_worktree(&registration, &common_dir) {
            Ok(Some(recorded)) => recorded,
            Ok(None) => continue,
            Err(error) => {
                tracing::warn!(
                    registration = %registration.display(),
                    error = %error,
                    "cannot verify stale registration ownership; preserving it"
                );
                continue;
            }
        };
        let matched = match match_rule {
            StaleWorktreeMatch::Path(_) => recorded == normalized_target,
            StaleWorktreeMatch::UnderPrefix(_) => recorded.starts_with(&normalized_target),
        };
        if !matched {
            continue;
        }
        match std::fs::remove_dir_all(&registration) {
            Ok(()) => {
                tracing::debug!(
                    registration = %registration.display(),
                    worktree = %recorded.display(),
                    "removed stale worktree registration"
                );
                removed += 1;
            }
            Err(error) => {
                tracing::warn!(
                    registration = %registration.display(),
                    error = %error,
                    "failed to remove stale worktree registration"
                );
            }
        }
    }
    Ok(removed)
}

/// Establish the same ownership proof as checkout removal before stale cleanup
/// can delete an entry. A matching path only selects among verified owners.
fn verified_stale_registration_worktree(
    registration: &Path,
    common_dir: &Path,
) -> Result<Option<PathBuf>> {
    anyhow::ensure!(
        std::fs::symlink_metadata(registration)
            .context("inspect stale registration")?
            .is_dir(),
        "unverified registration directory"
    );
    if registration
        .join("locked")
        .try_exists()
        .context("inspect stale registration lock")?
    {
        return Ok(None);
    }
    let backlink = std::fs::read_to_string(registration.join("gitdir"))
        .context("read stale registration backlink")?;
    anyhow::ensure!(!backlink.trim().is_empty(), "empty registration backlink");
    // Relative backlinks are owned by the registration dir, including after
    // the checkout has disappeared. Normalize the whole gitfile, not its parent.
    let recorded_gitfile = normalized_worktree_path(&registration.join(backlink.trim()))?;
    let recorded = recorded_gitfile
        .parent()
        .context("registration backlink has no checkout parent")?;
    match std::fs::symlink_metadata(recorded) {
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("inspect recorded stale checkout"),
    }
    anyhow::ensure!(
        registration_matches_worktree(registration, common_dir, recorded)?,
        "stale registration identity does not match source common directory and checkout"
    );
    Ok(Some(recorded.to_path_buf()))
}

/// [`remove_stale_worktree_registrations`] scoped to exactly one worktree path.
pub fn remove_stale_worktree_registration(source_repo: &Path, worktree_path: &Path) -> u64 {
    remove_stale_worktree_registrations(source_repo, StaleWorktreeMatch::Path(worktree_path))
}

/// [`remove_stale_worktree_registrations`] scoped to a tool-owned base directory.
pub fn remove_stale_worktree_registrations_under(source_repo: &Path, prefix: &Path) -> u64 {
    remove_stale_worktree_registrations(source_repo, StaleWorktreeMatch::UnderPrefix(prefix))
}

/// Match a recorded checkout even after it has been removed, without turning
/// access errors or unresolved ancestors into evidence of identity or absence.
pub(crate) fn normalized_worktree_path(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path).context("make worktree identity absolute")?;
    let mut missing = Vec::new();
    let mut cursor = absolute.as_path();
    loop {
        match dunce::canonicalize(cursor) {
            Ok(mut canonical) => {
                for component in missing.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("resolve worktree identity"),
        }
        match (cursor.parent(), cursor.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name.to_os_string());
                cursor = parent;
            }
            _ => anyhow::bail!("cannot resolve worktree identity: {}", path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_git(cwd: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn init_repo_with_worktrees() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        run_git(&repo, &["init"]);
        run_git(&repo, &["config", "user.email", "t@test"]);
        run_git(&repo, &["config", "user.name", "t"]);
        std::fs::write(repo.join("f.txt"), b"x").unwrap();
        run_git(&repo, &["add", "f.txt"]);
        run_git(&repo, &["commit", "-m", "init"]);
        (tmp, repo)
    }

    fn add_worktree(repo: &Path, wt: &Path) {
        run_git(
            repo,
            &["worktree", "add", "--detach", wt.to_str().unwrap(), "HEAD"],
        );
    }

    #[test]
    fn removes_only_the_matching_stale_registration() {
        let (tmp, repo) = init_repo_with_worktrees();
        let git_worktrees = repo.join(".git").join("worktrees");

        let target = tmp.path().join("target-wt");
        add_worktree(&repo, &target);
        std::fs::remove_dir_all(&target).unwrap();

        let hidden = tmp.path().join("hidden-wt");
        add_worktree(&repo, &hidden);
        std::fs::rename(&hidden, tmp.path().join("hidden-wt-moved")).unwrap();

        std::fs::create_dir(git_worktrees.join("bare-entry")).unwrap();

        let removed = remove_stale_worktree_registration(&repo, &target);

        assert_eq!(removed, 1);
        assert!(!git_worktrees.join("target-wt").exists());
        assert!(
            git_worktrees.join("hidden-wt").exists(),
            "non-matching registration must survive even when its path is gone"
        );
        assert!(git_worktrees.join("bare-entry").exists());
        assert!(git_worktrees.exists());
    }

    /// Rewrite a registration's `gitdir` backlink to the relative layout
    /// `worktree.useRelativePaths` (git >= 2.48) produces, without requiring
    /// that git version on the test host.
    fn make_backlink_relative(repo: &Path, reg_name: &str, worktree: &Path) {
        let reg_dir = repo.join(".git").join("worktrees").join(reg_name);
        let target = worktree.join(".git");
        let mut ups = PathBuf::new();
        let mut cursor = reg_dir.as_path();
        loop {
            if let Ok(rest) = target.strip_prefix(cursor) {
                std::fs::write(
                    reg_dir.join("gitdir"),
                    format!("{}\n", ups.join(rest).display()),
                )
                .unwrap();
                return;
            }
            cursor = cursor.parent().expect("shared ancestor");
            ups.push("..");
        }
    }

    #[test]
    fn resolves_relative_backlink_against_registration_dir() {
        let (tmp, repo) = init_repo_with_worktrees();

        let stale = tmp.path().join("rel-stale");
        add_worktree(&repo, &stale);
        make_backlink_relative(&repo, "rel-stale", &stale);
        std::fs::remove_dir_all(&stale).unwrap();

        let live = tmp.path().join("rel-live");
        add_worktree(&repo, &live);
        make_backlink_relative(&repo, "rel-live", &live);

        let removed_live = remove_stale_worktree_registrations_under(&repo, tmp.path());
        assert_eq!(removed_live, 1, "only the stale relative entry is removed");
        assert!(!repo.join(".git/worktrees/rel-stale").exists());
        assert!(
            repo.join(".git/worktrees/rel-live").exists(),
            "live worktree with relative backlink must survive"
        );
    }

    #[test]
    fn keeps_registration_when_worktree_still_exists() {
        let (tmp, repo) = init_repo_with_worktrees();
        let wt = tmp.path().join("live-wt");
        add_worktree(&repo, &wt);

        let removed = remove_stale_worktree_registration(&repo, &wt);

        assert_eq!(removed, 0);
        assert!(repo.join(".git/worktrees/live-wt").exists());
    }

    #[test]
    fn keeps_locked_registration() {
        let (tmp, repo) = init_repo_with_worktrees();
        let wt = tmp.path().join("locked-wt");
        add_worktree(&repo, &wt);
        run_git(&repo, &["worktree", "lock", wt.to_str().unwrap()]);
        std::fs::remove_dir_all(&wt).unwrap();

        let removed = remove_stale_worktree_registration(&repo, &wt);

        assert_eq!(removed, 0);
        assert!(repo.join(".git/worktrees/locked-wt").exists());
    }

    #[test]
    fn under_prefix_removes_only_owned_stale_registrations() {
        let (tmp, repo) = init_repo_with_worktrees();
        let git_worktrees = repo.join(".git").join("worktrees");

        let owned_base = tmp.path().join("owned-base");
        let owned_stale = owned_base.join("instance").join("wt-stale");
        let owned_locked = owned_base.join("instance").join("wt-locked");
        let owned_live = owned_base.join("instance").join("wt-live");
        std::fs::create_dir_all(owned_stale.parent().unwrap()).unwrap();
        add_worktree(&repo, &owned_stale);
        add_worktree(&repo, &owned_locked);
        add_worktree(&repo, &owned_live);
        std::fs::remove_dir_all(&owned_stale).unwrap();
        run_git(&repo, &["worktree", "lock", owned_locked.to_str().unwrap()]);
        std::fs::remove_dir_all(&owned_locked).unwrap();

        let foreign = tmp.path().join("foreign-wt");
        add_worktree(&repo, &foreign);
        std::fs::rename(&foreign, tmp.path().join("foreign-wt-moved")).unwrap();

        std::fs::create_dir(git_worktrees.join("bare-entry")).unwrap();

        let removed = remove_stale_worktree_registrations_under(&repo, &owned_base);

        assert_eq!(removed, 1);
        assert!(!git_worktrees.join("wt-stale").exists());
        assert!(
            git_worktrees.join("wt-locked").exists(),
            "locked registration must survive even when owned and stale"
        );
        assert!(
            git_worktrees.join("wt-live").exists(),
            "live owned registration must survive"
        );
        assert!(
            git_worktrees.join("foreign-wt").exists(),
            "foreign registration must survive even when its path is hidden"
        );
        assert!(git_worktrees.join("bare-entry").exists());
        assert!(git_worktrees.exists());
    }

    #[cfg(unix)]
    #[test]
    fn under_prefix_matches_symlinked_base_spelling() {
        let (tmp, repo) = init_repo_with_worktrees();

        let real_base = tmp.path().join("real-base");
        std::fs::create_dir(&real_base).unwrap();
        let wt = real_base.join("wt");
        add_worktree(&repo, &wt);
        std::fs::remove_dir_all(&wt).unwrap();

        let alias_base = tmp.path().join("alias-base");
        std::os::unix::fs::symlink(&real_base, &alias_base).unwrap();

        let removed = remove_stale_worktree_registrations_under(&repo, &alias_base);
        assert_eq!(removed, 1, "symlinked base spelling must match");
        assert!(!repo.join(".git/worktrees/wt").exists());
    }

    #[test]
    fn matches_across_symlinked_parent_spelling() {
        let (tmp, repo) = init_repo_with_worktrees();
        let real_parent = tmp.path().join("real-parent");
        std::fs::create_dir(&real_parent).unwrap();
        let wt = real_parent.join("wt");
        add_worktree(&repo, &wt);
        std::fs::remove_dir_all(&wt).unwrap();

        #[cfg(unix)]
        {
            let alias = tmp.path().join("alias-parent");
            std::os::unix::fs::symlink(&real_parent, &alias).unwrap();
            let removed = remove_stale_worktree_registration(&repo, &alias.join("wt"));
            assert_eq!(removed, 1, "symlinked spelling of the parent must match");
            assert!(!repo.join(".git/worktrees/wt").exists());
        }
    }
}
