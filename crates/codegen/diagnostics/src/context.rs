//! Git context collection for diagnostics events.

pub struct GitContext {
    pub is_git_repo: bool,
}

pub fn collect_git_context(cwd: &str) -> GitContext {
    use std::path::Path;

    GitContext {
        is_git_repo: git2::Repository::discover_path(
            Path::new(cwd),
            std::iter::empty::<&std::ffi::OsStr>(),
        )
        .is_ok(),
    }
}
