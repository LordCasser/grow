use serde::Deserialize;
use toml::Value as TomlValue;

/// Read `[cli] show_tips` from config.toml. Returns `None` if not set.
/// When `Some(false)`, the tip-of-the-day is suppressed on startup.
pub fn show_tips_from_toml_opt(root: &TomlValue) -> Option<bool> {
    if let TomlValue::Table(table) = root
        && let Some(TomlValue::Table(cli)) = table.get("cli")
    {
        cli.get("show_tips").and_then(|v| v.as_bool())
    } else {
        None
    }
}
/// Local `[tips]` config section.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TipsOverride {
    pub tips: Vec<String>,
    /// When true, drop the built-in tips entirely.
    pub exclude_default: bool,
}

/// Parse `[tips]` from a TOML value.
pub fn tips_from_toml(root: &TomlValue) -> Option<TipsOverride> {
    root.get("tips")?.clone().try_into::<TipsOverride>().ok()
}

/// Merge tip sources in priority order.
///
/// Return local tips, or the built-in list when no local override is present.
pub fn merge_tips(local: Option<TipsOverride>) -> Vec<String> {
    let Some(local) = local else {
        return Vec::new();
    };
    if local.exclude_default {
        Vec::new()
    } else {
        local.tips
    }
}

/// Resolve the merged tip list from pre-loaded config layers.
///
/// `GROW_TIPS_OVERRIDE` env var overrides everything (debug builds only).
/// `[cli] show_tips = false` in the effective local config kills all tips.
pub fn resolve_tips(config: Option<&TomlValue>) -> Vec<String> {
    if config.and_then(show_tips_from_toml_opt) == Some(false) {
        return Vec::new();
    }

    #[cfg(debug_assertions)]
    if let Ok(raw) = std::env::var("GROW_TIPS_OVERRIDE") {
        return raw.split('|').map(str::to_string).collect();
    }

    merge_tips(config.and_then(tips_from_toml))
}

/// Convenience wrapper that loads config layers from disk and picks one tip.
/// Prefer [`resolve_tips`] when layers are already loaded.
pub fn resolve_tips_from_disk(
    raw_config: &TomlValue,
    grow_home: &std::path::Path,
) -> Option<String> {
    let all = resolve_tips(Some(raw_config));
    if all.is_empty() {
        return None;
    }
    crate::util::tips::pick_and_advance(&all, grow_home)
}

pub const SLASH_COMMAND_TAGS_CONFIG_PATH: &str = "slash_command_tags";

/// Parse `[slash_command_tags]` from a TOML value into a name → tag map.
/// Only string values are kept; non-string entries are ignored.
fn slash_command_tags_from_toml(root: &TomlValue) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    if let Some(TomlValue::Table(table)) = root.get(SLASH_COMMAND_TAGS_CONFIG_PATH) {
        for (name, value) in table {
            if let Some(tag) = value.as_str() {
                out.insert(name.clone(), tag.to_string());
            }
        }
    }
    out
}

/// Parse a `GROW_SLASH_COMMAND_TAGS` payload (a JSON object of string→string)
/// into a name → tag map. `None`/empty → empty; malformed → warn + empty. Split
/// from env-reading so the parse is unit-testable without mutating process env.
fn parse_slash_command_tags_json(raw: Option<&str>) -> std::collections::HashMap<String, String> {
    // Unset or empty/whitespace-only is the normal "no override" state, not an
    // error — only real, non-empty input is parsed (and warned on failure).
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return std::collections::HashMap::new();
    };
    match serde_json::from_str::<std::collections::BTreeMap<String, String>>(raw) {
        Ok(map) => map.into_iter().collect(),
        Err(e) => {
            tracing::warn!(
                error = %e,
                "ignoring malformed GROW_SLASH_COMMAND_TAGS; expected a JSON object of string values"
            );
            std::collections::HashMap::new()
        }
    }
}

/// Read per-command tags from the `GROW_SLASH_COMMAND_TAGS` env var. Unset →
/// empty; malformed → warn + empty.
fn slash_command_tags_from_env() -> std::collections::HashMap<String, String> {
    parse_slash_command_tags_json(std::env::var("GROW_SLASH_COMMAND_TAGS").ok().as_deref())
}

/// Pure per-key merge of local `[slash_command_tags]` and env. Env overrides
/// local values per key.
fn merge_command_tags(
    local: std::collections::HashMap<String, String>,
    env: std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, String> {
    let mut out = local;
    out.extend(env); // env overrides local
    out
}

/// Env-injectable core of [`resolve_slash_command_tags`]: local
/// `[slash_command_tags]` → `env` (highest). Takes the env map explicitly so the
/// TOML-extraction + merge composition is hermetically testable (no process env).
fn resolve_slash_command_tags_with_env(
    effective_config: &TomlValue,
    env: std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, String> {
    merge_command_tags(slash_command_tags_from_toml(effective_config), env)
}

/// Resolve per-command slash-dropdown tags. Local `[slash_command_tags]` is
/// overlaid by the `GROW_SLASH_COMMAND_TAGS` env var (which wins per key).
pub fn resolve_slash_command_tags(
    effective_config: &TomlValue,
) -> std::collections::HashMap<String, String> {
    resolve_slash_command_tags_with_env(effective_config, slash_command_tags_from_env())
}

/// Read `[cli] channel` from config.toml.
/// Returns `None` when absent.
pub fn channel_from_toml_opt(root: &TomlValue) -> Option<String> {
    if let TomlValue::Table(table) = root
        && let Some(TomlValue::Table(cli)) = table.get("cli")
    {
        cli.get("channel")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use toml::Value as TomlValue;

    #[test]
    fn show_tips_defaults_to_none() {
        let config = TomlValue::Table(toml::map::Map::new());
        assert_eq!(show_tips_from_toml_opt(&config), None);
    }

    #[test]
    fn show_tips_reads_false() {
        let config: TomlValue = toml::from_str("[cli]\nshow_tips = false").unwrap();
        assert_eq!(show_tips_from_toml_opt(&config), Some(false));
    }

    #[test]
    fn show_tips_reads_true() {
        let config: TomlValue = toml::from_str("[cli]\nshow_tips = true").unwrap();
        assert_eq!(show_tips_from_toml_opt(&config), Some(true));
    }

    // Hermetic: drive the resolver through `_with_env` with an EXPLICIT env map
    // so ambient `GROW_SLASH_COMMAND_TAGS` can't affect these assertions.
    #[test]
    fn resolve_slash_command_tags_reads_local_per_key() {
        let local: TomlValue =
            toml::from_str("[slash_command_tags]\nworkflows = \"new\"\nplan = \"local-only\"\n")
                .unwrap();

        let resolved =
            resolve_slash_command_tags_with_env(&local, std::collections::HashMap::new());
        assert_eq!(resolved.get("workflows").map(String::as_str), Some("new"));
        assert_eq!(resolved.get("plan").map(String::as_str), Some("local-only"));
        assert_eq!(resolved.len(), 2);
    }

    #[test]
    fn resolve_slash_command_tags_missing_is_empty() {
        let empty = TomlValue::Table(toml::map::Map::new());
        assert!(
            resolve_slash_command_tags_with_env(&empty, std::collections::HashMap::new())
                .is_empty()
        );
    }

    // Env wins through the public composition — proven hermetically via `_with_env`
    // (no process-env mutation).
    #[test]
    fn resolve_slash_command_tags_env_overrides_local() {
        let local: TomlValue =
            toml::from_str("[slash_command_tags]\nworkflows = \"local\"\n").unwrap();
        let mut env = std::collections::HashMap::new();
        env.insert("workflows".to_string(), "env".to_string());

        let resolved = resolve_slash_command_tags_with_env(&local, env);
        assert_eq!(resolved.get("workflows").map(String::as_str), Some("env"));
        assert_eq!(resolved.len(), 1);
    }

    #[test]
    fn merge_command_tags_env_beats_local_per_key() {
        let mut local = std::collections::HashMap::new();
        local.insert("a".to_string(), "local-a".to_string());
        local.insert("b".to_string(), "local-b".to_string());
        local.insert("l".to_string(), "local-only".to_string());

        let mut env = std::collections::HashMap::new();
        env.insert("a".to_string(), "env-a".to_string());
        env.insert("e".to_string(), "env-only".to_string());

        let merged = merge_command_tags(local, env);
        assert_eq!(merged.get("a").map(String::as_str), Some("env-a"));
        assert_eq!(merged.get("b").map(String::as_str), Some("local-b"));
        assert_eq!(merged.get("l").map(String::as_str), Some("local-only"));
        assert_eq!(merged.get("e").map(String::as_str), Some("env-only"));
        assert_eq!(merged.len(), 4);

        // All sources empty → empty map.
        assert!(
            merge_command_tags(
                std::collections::HashMap::new(),
                std::collections::HashMap::new()
            )
            .is_empty()
        );
    }

    #[test]
    fn parse_slash_command_tags_json_handles_none_valid_and_malformed() {
        // Unset → empty (no warn).
        assert!(parse_slash_command_tags_json(None).is_empty());
        // Empty / whitespace-only is the normal "no override" state → empty (no warn).
        assert!(parse_slash_command_tags_json(Some("")).is_empty());
        assert!(parse_slash_command_tags_json(Some("   ")).is_empty());
        // Valid JSON object of string→string → parsed.
        let parsed = parse_slash_command_tags_json(Some(r#"{"commit":"new","plan":"beta"}"#));
        assert_eq!(parsed.get("commit").map(String::as_str), Some("new"));
        assert_eq!(parsed.get("plan").map(String::as_str), Some("beta"));
        assert_eq!(parsed.len(), 2);
        // Array instead of object → empty (tolerated).
        assert!(parse_slash_command_tags_json(Some(r#"["oops"]"#)).is_empty());
        // Non-string value → whole parse fails → empty (only string values kept).
        assert!(parse_slash_command_tags_json(Some(r#"{"commit": 3}"#)).is_empty());
        // Not JSON → empty.
        assert!(parse_slash_command_tags_json(Some("garbage")).is_empty());
    }
}
