//! Environment variables and plugin-token expansion for tool execution.

/// Env var set on agent-spawned terminal processes so host tools (e.g. `x ban`)
/// can distinguish agent invocations from human interactive shells.
/// Note: the CLI also uses `GROW_AGENT` as an
/// optional agent-definition selector for launching `grow` itself; child terminal
/// processes only need the sentinel value `"1"`.
pub const GROW_AGENT_ENV: &str = "GROW_AGENT";

/// Sentinel value for [`GROW_AGENT_ENV`] on agent tool terminals.
pub const GROW_AGENT_ENV_VALUE: &str = "1";

/// Force `GROW_AGENT=1` on an agent terminal child so request-scoped env cannot
/// clear the agent marker.
pub fn apply_agent_marker(cmd: &mut tokio::process::Command) {
    cmd.env(GROW_AGENT_ENV, GROW_AGENT_ENV_VALUE);
}

/// Expand the canonical plugin-path tokens (`${GROW_PLUGIN_ROOT}` and
/// `${GROW_PLUGIN_DATA}`) in `s` when their values are provided. Single source
/// of truth for plugin agent bodies,
/// plugin skill/command bodies, and plugin MCP/hook config substitution.
pub fn substitute_plugin_tokens(
    s: &str,
    plugin_root: Option<&str>,
    plugin_data: Option<&str>,
) -> String {
    let mut out = s.to_string();
    if let Some(root) = plugin_root {
        out = out.replace("${GROW_PLUGIN_ROOT}", root);
    }
    if let Some(data) = plugin_data {
        out = out.replace("${GROW_PLUGIN_DATA}", data);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{GROW_AGENT_ENV, GROW_AGENT_ENV_VALUE, substitute_plugin_tokens};

    const ALL_TOKENS: &str = "${GROW_PLUGIN_ROOT}/a ${GROW_PLUGIN_DATA}/b";

    #[test]
    fn expands_canonical_tokens_when_both_provided() {
        let out = substitute_plugin_tokens(ALL_TOKENS, Some("/root"), Some("/data"));
        assert_eq!(out, "/root/a /data/b");
    }

    #[test]
    fn leaves_tokens_literal_when_both_none() {
        let out = substitute_plugin_tokens(ALL_TOKENS, None, None);
        assert_eq!(out, ALL_TOKENS);
    }

    #[test]
    fn expands_only_root_when_data_none() {
        let out = substitute_plugin_tokens(ALL_TOKENS, Some("/root"), None);
        assert_eq!(out, "/root/a ${GROW_PLUGIN_DATA}/b");
    }

    #[test]
    fn agent_marker_constants_match_cursor_parity() {
        assert_eq!(GROW_AGENT_ENV, "GROW_AGENT");
        assert_eq!(GROW_AGENT_ENV_VALUE, "1");
    }
}
