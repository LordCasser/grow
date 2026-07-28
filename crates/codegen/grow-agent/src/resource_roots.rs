//! Ordered user roots shared by Agent and Skill discovery.
//!
//! Grow owns the primary root. The generic `.agent` root is a per-name
//! fallback; callers preserve this order during name deduplication.

use std::path::{Path, PathBuf};

pub(crate) fn user_roots(home: Option<&Path>) -> Vec<PathBuf> {
    home.into_iter()
        .flat_map(|home| [home.join(".config/.grow"), home.join(".agent")])
        .collect()
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
    fn grow_root_precedes_generic_fallback() {
        let home = Path::new("/home/test");
        assert_eq!(
            user_subdirs(Some(home), "agents"),
            vec![
                home.join(".config/.grow/agents"),
                home.join(".agent/agents"),
            ]
        );
    }
}
