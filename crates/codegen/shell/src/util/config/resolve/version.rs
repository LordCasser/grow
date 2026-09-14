//! Version display helpers retained by shell.

pub use config::VersionPolicy;

/// Machine-readable channel name derived from the updater's local version cache.
///
/// Reads `stable_version` from `~/.grow/version.json` (written by the
/// auto-updater) and compares the compiled-in version against it. This display
/// helper stays in shell because update cannot depend on the interactive shell.
pub fn channel_name_from_cache() -> Option<&'static str> {
    use std::sync::OnceLock;
    static NAME: OnceLock<Option<&'static str>> = OnceLock::new();
    *NAME.get_or_init(|| {
        let version_path = config::grow_home().join("version.json");
        let content = std::fs::read_to_string(&version_path).ok()?;
        let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
        let stable = parsed.get("stable_version")?.as_str()?;
        let current = semver::Version::parse(version::VERSION).ok()?;
        let stable_v = semver::Version::parse(stable).ok()?;
        if current > stable_v {
            Some("alpha")
        } else {
            Some("stable")
        }
    })
}
