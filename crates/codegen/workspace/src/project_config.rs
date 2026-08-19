//! Project config-file discovery for repo-local `.grow/config.toml` files.
//!
//! These pure `git2` + filesystem walks are shared by the shell's config
//! loaders and the folder-trust gate's `repo_configs_present`.

use std::path::{Path, PathBuf};

use agent::repo::RepoDirChain;

/// True when `config_path` is `$GROW_HOME/config.toml` (user tier, not project).
fn is_user_config_file(config_path: &Path) -> bool {
    let Some(user_home) = config::user_grow_home() else {
        return false;
    };
    let user_config = user_home.join("config.toml");
    if config_path == user_config.as_path() {
        return true;
    }
    let Ok(canonical_config) = dunce::canonicalize(config_path) else {
        return false;
    };
    let canonical_user = dunce::canonicalize(&user_config).unwrap_or(user_config);
    canonical_config == canonical_user
}

/// Find all `.grow/config.toml` files from `cwd` upward to the git repo root.
/// Returns paths ordered from repo root (lowest priority) to cwd (highest priority),
/// matching the convention used by skills and AGENTS.md discovery.
///
/// If no git repo is found, only checks `cwd/.grow/config.toml`. Excludes the
/// user-global config so `cwd == $HOME` does not treat `~/.grow/config.toml` as
/// a project overlay.
pub fn find_project_configs(cwd: &Path) -> Vec<PathBuf> {
    find_project_configs_in(&RepoDirChain::resolve(cwd).dirs)
}

/// [`find_project_configs`] over a precomputed cwd→git-root dir chain
/// ([`RepoDirChain`]), repo-root-first. Excludes the user-global config so
/// `cwd == $HOME` does not treat `~/.grow/config.toml` as a project overlay.
/// `pub(crate)` — the gate (`repo_configs_present`) reaches it within this crate.
pub(crate) fn find_project_configs_in(chain_dirs: &[PathBuf]) -> Vec<PathBuf> {
    // `dirs` is cwd-first; reverse so repo root comes first (lowest priority)
    // and cwd last (highest), matching skills/AGENTS.md discovery order.
    chain_dirs
        .iter()
        .rev()
        .map(|dir| dir.join(".grow").join("config.toml"))
        .filter(|config_path| config_path.is_file() && !is_user_config_file(config_path))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_project_configs_excludes_user_config_file() {
        let Some(user_home) = config::user_grow_home() else {
            return;
        };
        let user_config = user_home.join("config.toml");
        if user_config.is_file() {
            #[allow(deprecated)]
            let home = std::env::home_dir().expect("home dir");
            let from_home = find_project_configs(&home);
            assert!(
                !from_home.iter().any(|p| is_user_config_file(p)),
                "user config leaked into project configs: {from_home:?}"
            );
            assert!(is_user_config_file(&user_config));
        }

        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("repo");
        std::fs::create_dir_all(project.join(".grow")).unwrap();
        std::fs::write(project.join(".grow/config.toml"), "# project\n").unwrap();
        let found = find_project_configs(&project);
        assert_eq!(found.len(), 1);
        assert!(!is_user_config_file(&found[0]));
    }
}
