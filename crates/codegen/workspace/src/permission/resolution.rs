//! Canonical permission resolution.
//!
//! Permission policy has three native layers: system/user requirements,
//! managed Grow TOML, and global/project .grow/config.toml. Project rules are
//! admitted only after folder trust. Evaluation is deny > ask > allow.

use std::path::{Path, PathBuf};

use tracing::{debug, info, warn};

use crate::permission::rules::parse_permission_rule;
use crate::permission::types::{
    PatternMode, PermissionConfig, PermissionRule, PromptPolicy, RequirementSource, RuleAction,
    Sourced, ToolFilter,
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
    source: RequirementSource,
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

fn load_requirements_permissions() -> Vec<Sourced<PermissionRule>> {
    config::requirements_layers()
        .into_iter()
        .flat_map(|layer| {
            let path = PathBuf::from(layer.source.label().as_ref());
            let source = if layer.is_system {
                RequirementSource::SystemRequirements { path }
            } else {
                RequirementSource::Requirements { path }
            };
            extract_toml_permissions(&layer.value, source)
        })
        .collect()
}

fn load_managed_permissions() -> Vec<Sourced<PermissionRule>> {
    config::managed_config_layers()
        .into_iter()
        .flat_map(|layer| {
            extract_toml_permissions(
                &layer.value,
                RequirementSource::ManagedConfig {
                    path: layer.path.clone(),
                },
            )
        })
        .collect()
}

fn load_config_permissions(cwd: &Path, project_trusted: bool) -> Vec<Sourced<PermissionRule>> {
    let mut rules = Vec::new();

    if let Some(path) = config::user_grow_home().map(|home| home.join("config.toml"))
        && path.is_file()
    {
        match config::load_config_file(&path) {
            Ok(value) => rules.extend(extract_toml_permissions(
                &value,
                RequirementSource::Config { path: path.clone() },
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
                    RequirementSource::Config { path: path.clone() },
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
    pub sources: Vec<RequirementSource>,
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
    let policy_block = always_approve_disabled_by_policy();
    let mut skipped = Vec::new();
    let mut rules = load_requirements_permissions();
    rules.extend(load_managed_permissions());
    rules.extend(load_config_permissions(cwd, project_trusted));
    let rules = drop_untrusted_catchall_allows(rules, policy_block, &mut skipped);

    if rules.is_empty() && skipped.is_empty() {
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
        skipped,
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

pub fn is_catchall_allow(rule: &PermissionRule) -> bool {
    if rule.action != RuleAction::Allow {
        return false;
    }
    if matches!(
        rule.tool,
        ToolFilter::Read | ToolFilter::Edit | ToolFilter::Grep
    ) {
        return false;
    }
    crate::permission::policy::rule_is_catchall(rule)
}

fn is_admin_source(source: &RequirementSource) -> bool {
    matches!(source, RequirementSource::SystemRequirements { .. })
}

fn drop_untrusted_catchall_allows(
    rules: Vec<Sourced<PermissionRule>>,
    policy_block: Option<&'static str>,
    skipped: &mut Vec<SkippedPermission>,
) -> Vec<Sourced<PermissionRule>> {
    let Some(reason) = policy_block else {
        return rules;
    };
    rules
        .into_iter()
        .filter(|rule| {
            if is_catchall_allow(&rule.value) && !is_admin_source(&rule.source) {
                skipped.push(SkippedPermission {
                    rule: format!(
                        "allow {} (catch-all)",
                        rule.value.pattern.as_deref().unwrap_or("*"),
                    ),
                    reason: reason.to_owned(),
                });
                warn!(
                    source = %rule.source,
                    "catch-all allow ignored: always-approve disabled by managed policy",
                );
                false
            } else {
                true
            }
        })
        .collect()
}

pub const ALWAYS_APPROVE_PIN_REASON_REQUIREMENTS: &str = "always-approve disabled by managed policy ([ui] disable_bypass_permissions_mode = true in requirements.toml)";

pub fn always_approve_disabled_by_policy() -> Option<&'static str> {
    let layers = config::requirements_layers();
    let labeled: Vec<(PathBuf, &toml::Value)> = layers
        .iter()
        .map(|layer| (PathBuf::from(layer.source.label().as_ref()), &layer.value))
        .collect();
    resolve_always_approve_policy_block(
        labeled.iter().map(|(path, value)| (path.as_path(), *value)),
    )
}

fn requirements_lock_bool(ui: Option<&toml::Value>, key: &str, path: &Path) -> Option<bool> {
    let value = ui?.get(key)?;
    match value.as_bool() {
        Some(value) => Some(value),
        None => {
            warn!(
                path = %path.display(),
                key,
                "[ui] {key} must be a boolean; ignoring this requirements lock",
            );
            None
        }
    }
}

fn resolve_always_approve_policy_block<'a>(
    requirement_layers: impl Iterator<Item = (&'a Path, &'a toml::Value)>,
) -> Option<&'static str> {
    for (path, layer) in requirement_layers {
        if requirements_lock_bool(layer.get("ui"), "disable_bypass_permissions_mode", path)
            == Some(true)
        {
            return Some(ALWAYS_APPROVE_PIN_REASON_REQUIREMENTS);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sourced(rule: PermissionRule, source: RequirementSource) -> Sourced<PermissionRule> {
        Sourced {
            value: rule,
            source,
        }
    }

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
    fn catchall_pin_keeps_only_root_owned_grant() {
        let catchall = PermissionRule {
            action: RuleAction::Allow,
            tool: ToolFilter::Any,
            pattern: None,
            pattern_mode: PatternMode::Glob,
        };
        let mut skipped = Vec::new();
        let kept = drop_untrusted_catchall_allows(
            vec![
                sourced(
                    catchall.clone(),
                    RequirementSource::Config {
                        path: "/repo/.grow/config.toml".into(),
                    },
                ),
                sourced(
                    catchall,
                    RequirementSource::SystemRequirements {
                        path: "/etc/grow/requirements.toml".into(),
                    },
                ),
            ],
            Some(ALWAYS_APPROVE_PIN_REASON_REQUIREMENTS),
            &mut skipped,
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(skipped.len(), 1);
        assert!(matches!(
            kept[0].source,
            RequirementSource::SystemRequirements { .. },
        ));
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

    #[test]
    fn requirements_lock_is_exact() {
        let enabled: toml::Value =
            toml::from_str("[ui]\ndisable_bypass_permissions_mode = true\n").unwrap();
        assert_eq!(
            resolve_always_approve_policy_block(std::iter::once((
                Path::new("/etc/grow/requirements.toml"),
                &enabled
            )),),
            Some(ALWAYS_APPROVE_PIN_REASON_REQUIREMENTS),
        );

        let unrelated: toml::Value = toml::from_str("[ui]\ntheme = \"dark\"\n").unwrap();
        assert_eq!(
            resolve_always_approve_policy_block(std::iter::once((
                Path::new("/etc/grow/requirements.toml"),
                &unrelated
            )),),
            None,
        );
    }
}
