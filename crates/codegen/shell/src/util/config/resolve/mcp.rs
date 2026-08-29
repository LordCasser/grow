use toml::Value as TomlValue;

fn feature_from_toml(v: Option<&TomlValue>, name: &str) -> Option<bool> {
    v?.get("features")?.get(name)?.as_bool()
}

pub fn resolve_mcp_liveness_watchers(user: Option<&TomlValue>) -> bool {
    crate::agent::config::resolve_mcp_liveness_watchers(
        None,
        feature_from_toml(user, "mcp_liveness_watchers"),
        None,
    )
    .value
}

pub fn resolve_mcp_auto_restart(user: Option<&TomlValue>) -> bool {
    crate::agent::config::resolve_mcp_auto_restart(
        None,
        feature_from_toml(user, "mcp_auto_restart"),
        None,
    )
    .value
}

pub fn resolve_mcp_recursive_config_watch(user: Option<&TomlValue>) -> bool {
    crate::agent::config::resolve_mcp_recursive_config_watch(
        None,
        feature_from_toml(user, "mcp_recursive_config_watch"),
        None,
    )
    .value
}

pub const DEFAULT_MCP_STARTUP_TIMEOUT_SECS: u64 = 30;
const ENV_MCP_TIMEOUT_MS: &str = "MCP_TIMEOUT";
const ENV_MCP_STARTUP_TIMEOUT_SECS: &str = "GROW_MCP_STARTUP_TIMEOUT_SECS";

pub fn resolved_mcp_startup_timeout_secs() -> u64 {
    resolve_mcp_startup_timeout_secs()
}

pub fn resolve_mcp_startup_timeout_secs() -> u64 {
    fn extract(v: &toml::Value) -> Option<u64> {
        v.get("mcp")?
            .get("startup_timeout_sec")?
            .as_integer()
            .and_then(|n| u64::try_from(n).ok())
            .filter(|n| *n > 0)
    }
    let config = crate::config::load_effective_config()
        .ok()
        .as_ref()
        .and_then(extract);
    mcp_startup_timeout_from_env()
        .or(config)
        .unwrap_or(DEFAULT_MCP_STARTUP_TIMEOUT_SECS)
}

fn mcp_startup_timeout_from_env() -> Option<u64> {
    if let Some(ms) = std::env::var(ENV_MCP_TIMEOUT_MS)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|n| *n > 0)
    {
        return Some(ms.div_ceil(1000));
    }
    std::env::var(ENV_MCP_STARTUP_TIMEOUT_SECS)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|n| *n > 0)
}

pub const DEFAULT_MAX_MCP_OUTPUT_BYTES: usize = tools::MCP_MAX_OUTPUT_BYTES;

pub fn cache_mcp_max_output_bytes() {
    tools::set_mcp_max_output_bytes(resolve_max_mcp_output_bytes());
}

fn max_mcp_output_bytes_from_toml(v: &toml::Value) -> Option<usize> {
    let raw = v.get("mcp")?.get("max_output_bytes")?.as_integer()?;
    u64::try_from(raw)
        .ok()
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n > 0)
}

pub fn resolve_max_mcp_output_bytes() -> usize {
    let config = crate::config::load_effective_config()
        .ok()
        .as_ref()
        .and_then(max_mcp_output_bytes_from_toml);
    tools::mcp_max_output_bytes_from_env()
        .or(config)
        .unwrap_or(DEFAULT_MAX_MCP_OUTPUT_BYTES)
}

fn project_max_mcp_output_bytes(cwd: &std::path::Path) -> Option<usize> {
    if !crate::agent::folder_trust::project_scope_allowed(cwd) {
        return None;
    }
    let mut value = None;
    for config_path in crate::config::find_project_configs(cwd) {
        if let Ok(toml_val) = config::load_config_file(&config_path)
            && let Some(v) = max_mcp_output_bytes_from_toml(&toml_val)
        {
            value = Some(v);
        }
    }
    value
}

pub fn resolve_max_mcp_output_bytes_for_cwd(cwd: &std::path::Path) -> Option<usize> {
    if tools::mcp_max_output_bytes_from_env().is_some() {
        None
    } else {
        project_max_mcp_output_bytes(cwd)
    }
}
