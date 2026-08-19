//! Plugin marketplace browse and index crate.
//!
//! Provides canonical indexed marketplace discovery and install integration
//! with the Grow plugin registry.

pub mod catalog;
pub mod config;
pub mod error;
pub mod git;
pub mod index;
pub mod install_resolve;
pub mod installer;
pub mod matcher;
pub mod scanner;
pub mod types;

pub use config::{env_require_sha, load_require_sha, load_sources};
pub use error::MarketplaceError;
pub use scanner::scan_marketplace;
pub use types::*;

/// Normalized lowercase `owner/repo` from a GitHub URL (HTTPS/http/ssh/scp,
/// `www.`, trailing `.git`/`/`), or `None` if not a GitHub URL.
pub(crate) fn canonical_github_owner_repo(url: &str) -> Option<String> {
    let s = url.trim();
    let s = s.strip_suffix('/').unwrap_or(s);
    let s = s.strip_suffix(".git").unwrap_or(s);
    let lower = s.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .or_else(|| lower.strip_prefix("ssh://"))
        .unwrap_or(&lower);
    let rest = rest.strip_prefix("git@").unwrap_or(rest);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let owner_repo = rest
        .strip_prefix("github.com/")
        .or_else(|| rest.strip_prefix("github.com:"))?;
    if owner_repo.is_empty() {
        None
    } else {
        Some(owner_repo.to_string())
    }
}
