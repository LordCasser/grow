use toml::Value as TomlValue;

pub(crate) const ENV_AUTO_PERMISSION_MODE: &str = "GROW_AUTO_PERMISSION_MODE";

const AUTO_MODE_CLASSIFY_TIMEOUT_MIN_MS: u64 = 1_000;
const AUTO_MODE_CLASSIFY_TIMEOUT_DEFAULT_MS: u64 = 30_000;
const AUTO_MODE_CLASSIFY_TIMEOUT_MAX_MS: u64 = 120_000;

#[cfg(test)]
pub(crate) static AUTO_PERMISSION_MODE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn auto_permission_mode_from_toml(v: Option<&TomlValue>) -> Option<bool> {
    v?.get("auto_mode")?.get("enabled")?.as_bool()
}

fn resolve_auto_permission_mode_layers(
    config: Option<bool>,
) -> crate::agent::config::Resolved<bool> {
    use crate::agent::config::BoolFlag;
    BoolFlag::env(ENV_AUTO_PERMISSION_MODE)
        .config(config)
        .default(true)
        .resolve()
}

/// Resolve `PermissionMode::Auto` from effective local TOML. Environment
/// override wins over `[auto_mode].enabled`, then the default is on.
pub fn resolve_auto_permission_mode_enabled(
    config: Option<&TomlValue>,
) -> crate::agent::config::Resolved<bool> {
    resolve_auto_permission_mode_layers(auto_permission_mode_from_toml(config))
}

fn auto_mode_config_from_toml(
    v: Option<&TomlValue>,
) -> Option<crate::agent::config::AutoModeConfig> {
    let table = v?.get("auto_mode")?.clone();
    table
        .try_into()
        .map_err(|e| tracing::warn!(error = %e, "[auto_mode]: dropped malformed local table"))
        .ok()
}

pub fn auto_permission_mode_enabled_from_disk() -> bool {
    let effective = crate::config::load_effective_config().ok();
    resolve_auto_permission_mode_layers(auto_permission_mode_from_toml(effective.as_ref())).value
}

pub fn resolve_auto_mode_config_from_disk() -> crate::agent::config::AutoModeConfig {
    let effective = crate::config::load_effective_config().ok();
    auto_mode_config_from_toml(effective.as_ref()).unwrap_or_default()
}

pub fn auto_mode_classify_timeout(
    cfg: &crate::agent::config::AutoModeConfig,
) -> std::time::Duration {
    let configured = cfg
        .classify_timeout_ms
        .unwrap_or(AUTO_MODE_CLASSIFY_TIMEOUT_DEFAULT_MS);
    let bounded = configured.clamp(
        AUTO_MODE_CLASSIFY_TIMEOUT_MIN_MS,
        AUTO_MODE_CLASSIFY_TIMEOUT_MAX_MS,
    );
    if bounded != configured {
        tracing::warn!(
            configured_ms = configured,
            bounded_ms = bounded,
            min_ms = AUTO_MODE_CLASSIFY_TIMEOUT_MIN_MS,
            max_ms = AUTO_MODE_CLASSIFY_TIMEOUT_MAX_MS,
            "[auto_mode] classify_timeout_ms outside supported range; clamped"
        );
    }
    std::time::Duration::from_millis(bounded)
}

pub fn auto_mode_classifier_defaults(
    cfg: &crate::agent::config::AutoModeConfig,
) -> (
    workspace::permission::ClassifierPromptType,
    Option<sampling_types::ReasoningEffort>,
) {
    let prompt_type = cfg
        .prompt_type
        .unwrap_or(workspace::permission::ClassifierPromptType::Full);
    (prompt_type, cfg.reasoning_effort)
}
