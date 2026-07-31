//! Ordered user roots shared by Agent and Skill discovery.
//!
//! Only Grow's user root (`~/.grow`) is used. Project-level discovery is
//! handled separately by callers.

use std::path::{Path, PathBuf};

pub(crate) fn user_roots(home: Option<&Path>) -> Vec<PathBuf> {
    home.into_iter().map(|home| home.join(".grow")).collect()
}

pub(crate) fn user_subdirs(home: Option<&Path>, subdir: &str) -> Vec<PathBuf> {
    user_roots(home)
        .into_iter()
        .map(|root| root.join(subdir))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_root_is_grow_only() {
        let home = Path::new("/home/test");
        assert_eq!(
            user_subdirs(Some(home), "agents"),
            vec![home.join(".grow/agents")]
        );
        assert!(user_subdirs(None, "agents").is_empty());
    }
}
