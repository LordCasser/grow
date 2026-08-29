//! Config-value resolution leaf types and per-model laziness config,
//! extracted from shell for dependency inversion.

use config::env_bool;

/// Where a resolved config value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
#[strum(serialize_all = "snake_case")]
pub enum ConfigSource {
    Cli,
    Env,
    Config,
    Remote,
    Default,
}

/// A resolved config value with its source for diagnostics.
#[derive(Debug, Clone)]
pub struct Resolved<T> {
    pub value: T,
    pub source: ConfigSource,
}

impl<T> Resolved<T> {
    pub fn new(value: T, source: ConfigSource) -> Self {
        Self { value, source }
    }
}

impl<T: std::fmt::Display> std::fmt::Display for Resolved<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.value, self.source)
    }
}
/// Resolve a boolean feature flag: cli > env > config > feature flag > default.
pub struct BoolFlag<'a> {
    cli: Option<bool>,
    env_var: &'a str,
    config: Option<bool>,
    feature_flag: Option<bool>,
    default: bool,
}

impl<'a> BoolFlag<'a> {
    pub fn env(env_var: &'a str) -> Self {
        Self {
            cli: None,
            env_var,
            config: None,
            feature_flag: None,
            default: false,
        }
    }

    pub fn cli(mut self, v: Option<bool>) -> Self {
        self.cli = v;
        self
    }
    pub fn config(mut self, v: Option<bool>) -> Self {
        self.config = v;
        self
    }
    pub fn feature_flag(mut self, v: Option<bool>) -> Self {
        self.feature_flag = v;
        self
    }
    pub fn default(mut self, v: bool) -> Self {
        self.default = v;
        self
    }

    pub fn resolve(self) -> Resolved<bool> {
        resolve_bool_flag(
            self.cli,
            self.env_var,
            self.config,
            self.feature_flag,
            self.default,
        )
    }
}

fn resolve_bool_flag(
    cli_arg: Option<bool>,
    env_var: &str,
    config_val: Option<bool>,
    feature_flag_val: Option<bool>,
    default: bool,
) -> Resolved<bool> {
    if let Some(val) = cli_arg {
        return Resolved::new(val, ConfigSource::Cli);
    }
    if let Some(val) = env_bool(env_var) {
        return Resolved::new(val, ConfigSource::Env);
    }
    if let Some(val) = config_val {
        return Resolved::new(val, ConfigSource::Config);
    }
    if let Some(val) = feature_flag_val {
        return Resolved::new(val, ConfigSource::Remote);
    }
    Resolved::new(default, ConfigSource::Default)
}
/// Session search index gate (`[session_search]` table in `config.toml`).
///
/// The session search index is ON by default. `enabled = false` turns the
/// index off for the current process; the gate resolves
/// `GROW_SESSION_SEARCH` (env) above this table, mirroring [`BoolFlag`]
/// precedence.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(default)]
pub struct SessionSearchConfig {
    /// Master switch. `None` (unset) defers to env / default
    /// (`true`). `Some(false)` turns the index off for the process.
    pub enabled: Option<bool>,
}

#[cfg(test)]
mod session_search_config_tests {
    use super::*;

    #[test]
    fn session_search_config_defaults_to_unset() {
        let c = SessionSearchConfig::default();
        assert_eq!(c.enabled, None);
    }

    #[test]
    fn session_search_config_deserializes_enabled() {
        let off: SessionSearchConfig = serde_json::from_str(r#"{"enabled": false}"#).unwrap();
        assert_eq!(off.enabled, Some(false));

        let on: SessionSearchConfig = serde_json::from_str(r#"{"enabled": true}"#).unwrap();
        assert_eq!(on.enabled, Some(true));

        let absent: SessionSearchConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(absent.enabled, None);
    }
}

/// Per-model configuration for the Layer-3 LazinessDetector.
///
/// All fields default to the disabled state. Activation is a deliberate
/// two-step opt-in: setting `enabled = true` lets the classifier fire
/// (and emit `LazinessClassifierFired` diagnostics), but a nudge is only
/// injected when `max_nudges_per_session > 0` as well. This makes
/// observation-only rollout (classify-but-don't-act) the natural
/// intermediate state.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LazinessDetectorPerModelConfig {
    /// Master switch. When `false` (the default), the classifier never
    /// fires for this model and no per-classification cost is incurred.
    #[serde(default)]
    pub enabled: bool,
    /// Hard cap on `<system-reminder>` nudges injected per session for
    /// this model. Default `0` makes `enabled = true` alone an
    /// observation-only mode (classifier fires, no nudges).
    #[serde(default)]
    pub max_nudges_per_session: u32,
    /// How long the session must be idle before the classifier runs.
    /// `None` defers to the harness default (10 seconds).
    #[serde(default)]
    pub idle_threshold_ms: Option<u64>,
    /// Minimum classifier confidence required to inject a nudge. `None`
    /// defers to the harness default (0.7).
    #[serde(default)]
    pub min_confidence: Option<f32>,
    /// When `Some(true)` (or `None` — the default), the classifier sees
    /// the assistant's plain-text reasoning as `[assistant reasoning]`
    /// lines. `Some(false)` drops them (the pre-2026-05 behavior).
    /// `None` defers to the harness default (`LAZINESS_INCLUDE_REASONING`,
    /// currently `true`).
    #[serde(default)]
    pub include_reasoning: Option<bool>,
}
