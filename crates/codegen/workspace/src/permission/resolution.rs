//! Canonical permission resolution.
//!
//! Permission policy is loaded from global/project `.grow/config.toml` files.
//! Project rules are admitted only after folder trust. Evaluation is deny > ask
//! > allow.

use std::path::Path;

use tracing::{debug, info, warn};

use crate::permission::rules::parse_permission_rule;
use crate::permission::types::{
    PermissionConfig, PermissionRule, PermissionSource, PromptPolicy, RuleAction, Sourced,
    ToolFilter,
};

fn parse_toml_permission_section(
    permission_value: &toml::Value,
) -> Result<Vec<PermissionRule>, String> {
    let mut rules = Vec::new();
    let mut found_compact = false;

    for (action, key) in [
        (RuleAction::Deny, "deny"),
        (RuleAction::Allow, "allow"),
        (RuleAction::Ask, "ask"),
    ] {
        let Some(value) = permission_value.get(key) else {
            continue;
        };
        found_compact = true;
        let Some(items) = value.as_array() else {
            warn!(
                "permission.{key}: expected an array of rule strings, got {}",
                toml_type_name(value),
            );
            continue;
        };
        for (index, item) in items.iter().enumerate() {
            let Some(text) = item.as_str() else {
                warn!(
                    "permission.{key}[{index}]: expected string, got {}",
                    toml_type_name(item),
                );
                continue;
            };
            match parse_permission_rule(text, action) {
                Ok(rule) => rules.push(rule),
                Err(error) => warn!("permission.{key}[{index}]: {text:?}: {error}"),
            }
        }
    }

    if found_compact {
        return Ok(rules);
    }

    permission_value
        .clone()
        .try_into::<PermissionConfig>()
        .map(|config| config.rules)
        .map_err(|error| error.to_string())
}

fn toml_type_name(value: &toml::Value) -> &'static str {
    match value {
        toml::Value::String(_) => "string",
        toml::Value::Integer(_) => "integer",
        toml::Value::Float(_) => "float",
        toml::Value::Boolean(_) => "boolean",
        toml::Value::Datetime(_) => "datetime",
        toml::Value::Array(_) => "array",
        toml::Value::Table(_) => "table",
    }
}

fn extract_toml_permissions(
    config: &toml::Value,
    source: PermissionSource,
) -> Vec<Sourced<PermissionRule>> {
    let Some(permission) = config.get("permission") else {
        return Vec::new();
    };
    match parse_toml_permission_section(permission) {
        Ok(rules) => {
            if !rules.is_empty() {
                info!(count = rules.len(), %source, "loaded permission rules");
            }
            rules
                .into_iter()
                .map(|value| Sourced {
                    value,
                    source: source.clone(),
                })
                .collect()
        }
        Err(error) => {
            warn!(%source, %error, "failed to parse [permission]");
            Vec::new()
        }
    }
}

fn load_config_permissions(cwd: &Path, project_trusted: bool) -> Vec<Sourced<PermissionRule>> {
    let mut rules = Vec::new();

    if let Some(path) = config::user_grow_home().map(|home| home.join("config.toml"))
        && path.is_file()
    {
        match config::load_config_file(&path) {
            Ok(value) => rules.extend(extract_toml_permissions(
                &value,
                PermissionSource::Config { path: path.clone() },
            )),
            Err(error) => {
                warn!(path = %path.display(), %error, "failed to load global config.toml");
            }
        }
    }

    if project_trusted {
        for path in crate::project_config::find_project_configs(cwd) {
            match config::load_config_file(&path) {
                Ok(value) => rules.extend(extract_toml_permissions(
                    &value,
                    PermissionSource::Config { path: path.clone() },
                )),
                Err(error) => {
                    warn!(path = %path.display(), %error, "failed to load project config.toml");
                }
            }
        }
    }

    rules
}

pub async fn resolve_permission_config(
    cwd: &Path,
    project_trusted: bool,
) -> Option<PermissionConfig> {
    resolve_permissions_with_provenance(cwd, project_trusted)
        .await
        .map(|resolved| resolved.config)
}

pub struct ResolvedPermissions {
    pub config: PermissionConfig,
    pub sources: Vec<PermissionSource>,
    pub skipped: Vec<SkippedPermission>,
}

pub struct SkippedPermission {
    pub rule: String,
    pub reason: String,
}

pub async fn resolve_permissions_with_provenance(
    cwd: &Path,
    project_trusted: bool,
) -> Option<ResolvedPermissions> {
    let rules = load_config_permissions(cwd, project_trusted);

    if rules.is_empty() {
        return None;
    }

    let (rules, sources): (Vec<_>, Vec<_>) = rules
        .into_iter()
        .map(|rule| (rule.value, rule.source))
        .unzip();
    debug!(count = rules.len(), "resolved permission rules");

    Some(ResolvedPermissions {
        config: PermissionConfig {
            rules,
            prompt_policy: PromptPolicy::Ask,
        },
        sources,
        skipped: Vec::new(),
    })
}

pub fn deny_read_globs_from_config(config: &PermissionConfig) -> Vec<String> {
    config
        .rules
        .iter()
        .filter(|rule| {
            rule.action == RuleAction::Deny
                && matches!(
                    rule.tool,
                    ToolFilter::Read | ToolFilter::Grep | ToolFilter::Any
                )
        })
        .filter_map(|rule| rule.pattern.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission::types::PatternMode;

    #[test]
    fn compact_permission_parser_is_canonical() {
        let value: toml::Value = toml::from_str(
            r#"
[permission]
deny = ["Read(**/.env)"]
ask = ["Bash(git push*)"]
allow = ["Read(src/**)"]
"#,
        )
        .unwrap();
        let rules = parse_toml_permission_section(value.get("permission").unwrap()).unwrap();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].action, RuleAction::Deny);
        assert_eq!(rules[1].action, RuleAction::Allow);
        assert_eq!(rules[2].action, RuleAction::Ask);
    }

    #[test]
    fn read_denies_are_derived_from_effective_policy() {
        let config = PermissionConfig {
            rules: vec![
                PermissionRule {
                    action: RuleAction::Deny,
                    tool: ToolFilter::Read,
                    pattern: Some("**/.env".into()),
                    pattern_mode: PatternMode::Glob,
                },
                PermissionRule {
                    action: RuleAction::Deny,
                    tool: ToolFilter::Bash,
                    pattern: Some("rm *".into()),
                    pattern_mode: PatternMode::Glob,
                },
            ],
            prompt_policy: PromptPolicy::Ask,
        };
        assert_eq!(deny_read_globs_from_config(&config), vec!["**/.env"]);
    }
}
