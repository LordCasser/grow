use toml::Value as TomlValue;

pub(crate) const ENV_CRASH_HANDLER: &str = "GROW_CRASH_HANDLER";

fn crash_handler_from_toml(v: Option<&TomlValue>) -> Option<bool> {
    v?.get("diagnostics")?.get("crash_handler")?.as_bool()
}

/// Resolve the crash-handler install gate from local config and its explicit
/// environment override. The default is disabled.
pub fn resolve_crash_handler_enabled(
    config: Option<&TomlValue>,
) -> crate::agent::config::Resolved<bool> {
    use crate::agent::config::BoolFlag;
    BoolFlag::env(ENV_CRASH_HANDLER)
        .config(crash_handler_from_toml(config))
        .default(false)
        .resolve()
}

/// Synchronous form used before the async runtime starts.
pub fn load_crash_handler_enabled_sync() -> bool {
    let config = crate::config::load_effective_config().ok();
    resolve_crash_handler_enabled(config.as_ref()).value
}
