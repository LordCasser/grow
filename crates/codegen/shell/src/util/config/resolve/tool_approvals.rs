use toml::Value as TomlValue;

pub(crate) const ENV_REMEMBER_TOOL_APPROVALS: &str = "GROW_REMEMBER_TOOL_APPROVALS";

fn remember_tool_approvals_from_toml(v: Option<&TomlValue>) -> Option<bool> {
    v?.get("ui")?.get("remember_tool_approvals")?.as_bool()
}

/// Resolve the local per-tool approval persistence gate. Environment override
/// wins over `[ui].remember_tool_approvals`; the default is disabled.
pub fn resolve_remember_tool_approvals(
    config: Option<&TomlValue>,
) -> crate::agent::config::Resolved<bool> {
    use crate::agent::config::BoolFlag;
    BoolFlag::env(ENV_REMEMBER_TOOL_APPROVALS)
        .config(remember_tool_approvals_from_toml(config))
        .default(false)
        .resolve()
}

pub fn remember_tool_approvals_from_disk() -> bool {
    let config = crate::config::load_effective_config().ok();
    resolve_remember_tool_approvals(config.as_ref()).value
}
