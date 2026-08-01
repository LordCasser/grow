//! Local workspace activity settings.

use std::time::Duration;

const DEFAULT_SESSION_IDLE_PRUNE_SECS: u64 = 30 * 60;

#[derive(Debug, Clone)]
pub struct StatusConfig {
    /// Idle duration after which an inactive session disappears from the
    /// activity index.
    pub session_idle_prune: Duration,
    /// Whether background tasks are excluded when calculating local idle time.
    pub idle_ignores_background: bool,
}

impl Default for StatusConfig {
    fn default() -> Self {
        Self {
            session_idle_prune: Duration::from_secs(DEFAULT_SESSION_IDLE_PRUNE_SECS),
            idle_ignores_background: false,
        }
    }
}

impl StatusConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            session_idle_prune: Duration::from_secs(parse_env(
                "GROW_WORKSPACE_SESSION_IDLE_PRUNE_SECS",
                defaults.session_idle_prune.as_secs(),
            )),
            idle_ignores_background: parse_env(
                "GROW_WORKSPACE_IDLE_IGNORE_BACKGROUND_TASKS",
                defaults.idle_ignores_background,
            ),
        }
    }
}

fn parse_env<T>(name: &str, default: T) -> T
where
    T: std::str::FromStr,
{
    match std::env::var(name) {
        Ok(raw) => raw.parse().unwrap_or_else(|_| {
            tracing::warn!(name, value = %raw, "invalid local workspace setting; using default");
            default
        }),
        Err(_) => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_local_only() {
        let config = StatusConfig::default();
        assert_eq!(config.session_idle_prune, Duration::from_secs(30 * 60));
        assert!(!config.idle_ignores_background);
    }
}
