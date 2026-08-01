use indexmap::IndexMap;

use super::config::{ConfigModelOverride, EnvKeys};
use super::config_model_override_parse::{ConfigWarning, ConfigWarningKind};
use crate::sampling::ApiBackend;

/// Translate the public `[provider.<id>]` hierarchy into the flat internal
/// model catalog consumed by the runtime. The flat representation is an
/// implementation detail; configuration has one source of truth:
/// `provider -> options/models`.
pub(crate) fn normalize_provider_config(raw_config: &toml::Value) -> Result<toml::Value, String> {
    let mut normalized = raw_config.clone();
    let root = normalized
        .as_table_mut()
        .ok_or_else(|| "config root must be a TOML table".to_owned())?;

    // The previous public shape is deliberately not part of the new schema.
    root.remove("model");
    root.remove("model_providers");

    let Some(provider_value) = root.remove("provider") else {
        return Ok(normalized);
    };
    let providers = provider_value
        .as_table()
        .ok_or_else(|| "`provider` must be a table of [provider.<id>] entries".to_owned())?;

    let mut flat_providers = toml::map::Map::new();
    let mut flat_models = toml::map::Map::new();
    for (provider_id, provider_value) in providers {
        let mut provider = provider_value
            .as_table()
            .cloned()
            .ok_or_else(|| format!("`provider.{provider_id}` must be a table"))?;
        let models = provider
            .remove("models")
            .unwrap_or_else(|| toml::Value::Table(toml::map::Map::new()));
        let models = models
            .as_table()
            .ok_or_else(|| format!("`provider.{provider_id}.models` must be a table"))?;
        let mut options = provider
            .remove("options")
            .unwrap_or_else(|| toml::Value::Table(toml::map::Map::new()));
        let options = options
            .as_table_mut()
            .ok_or_else(|| format!("`provider.{provider_id}.options` must be a table"))?;

        // Protocol selection describes the provider adapter, while connection
        // and credential values stay grouped under `options`.
        if let Some(api_backend) = provider.remove("api_backend") {
            options.insert("api_backend".to_owned(), api_backend);
        }
        if !provider.is_empty() {
            let fields = provider.keys().cloned().collect::<Vec<_>>().join(", ");
            return Err(format!(
                "unknown field(s) in `provider.{provider_id}`: {fields}; use `api_backend`, `options`, or `models`"
            ));
        }
        flat_providers.insert(provider_id.clone(), options.clone().into());

        for (model_id, model_value) in models {
            let mut model = model_value.as_table().cloned().ok_or_else(|| {
                format!("`provider.{provider_id}.models.{model_id}` must be a table")
            })?;
            model
                .entry("model".to_owned())
                .or_insert_with(|| toml::Value::String(model_id.clone()));
            model.insert(
                "model_provider".to_owned(),
                toml::Value::String(provider_id.clone()),
            );
            flat_models.insert(format!("{provider_id}/{model_id}"), model.into());
        }
    }
    root.insert("model_providers".to_owned(), flat_providers.into());
    root.insert("model".to_owned(), flat_models.into());
    Ok(normalized)
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct ModelProviderConfig {
    pub base_url: Option<String>,
    pub api_base_url: Option<String>,
    pub env_key: Option<EnvKeys>,
    pub api_key: Option<String>,
    pub api_backend: Option<ApiBackend>,
    pub extra_headers: IndexMap<String, String>,
    /// Query parameters folded into every request URL; inherited by models.
    pub query_params: IndexMap<String, String>,
    /// Header name to environment variable; inherited by models, resolved at
    /// client build.
    pub env_http_headers: IndexMap<String, String>,
    pub auth_provider: Option<String>,
    pub auth: Option<crate::auth::AuthProviderConfig>,
    pub context_window: Option<u64>,
}

pub(crate) fn model_provider_auth_name(provider_id: &str) -> String {
    format!("model_provider:{provider_id}")
}

pub(crate) fn auth_config_issues(
    config: &crate::auth::AuthProviderConfig,
) -> Vec<(&'static str, ConfigWarningKind, String)> {
    let mut issues = Vec::new();
    if !config.is_usable() {
        issues.push((
            if config.is_oauth() {
                "issuer/client_id"
            } else {
                "command"
            },
            ConfigWarningKind::InvalidValue,
            if config.is_oauth() {
                "OAuth providers require non-empty issuer and client_id; models resolve with no credential"
                    .to_owned()
            } else {
                "missing or empty command; models resolve with no credential".to_owned()
            },
        ));
    }
    if config.is_oauth() {
        return issues;
    }
    let skew = crate::auth::PROVIDER_TOKEN_EXPIRY_SKEW_SECS;
    if config.token_ttl_secs.is_some_and(|ttl| ttl <= skew) {
        issues.push((
            "token_ttl_secs",
            ConfigWarningKind::InvalidValue,
            format!(
                "at or below the {skew}s refresh margin; the command will run before every turn"
            ),
        ));
    }
    if let Some(timeout) = config.timeout_secs
        && !(1..=crate::auth::PROVIDER_TIMEOUT_CEILING_SECS).contains(&timeout)
    {
        let ceiling = crate::auth::PROVIDER_TIMEOUT_CEILING_SECS;
        issues.push((
            "timeout_secs",
            ConfigWarningKind::InvalidValue,
            if timeout == 0 {
                "below the 1 second minimum; clamped to 1".to_owned()
            } else {
                format!("above the {ceiling}s maximum; clamped to {ceiling}")
            },
        ));
    }
    issues
}

pub(crate) fn parse_model_providers(
    raw_config: &toml::Value,
) -> (IndexMap<String, ModelProviderConfig>, Vec<ConfigWarning>) {
    let mut providers = IndexMap::new();
    let mut warnings = Vec::new();
    let Some(section) = raw_config.get("model_providers") else {
        return (providers, warnings);
    };
    let Some(table) = section.as_table() else {
        warnings.push(ConfigWarning::model_provider_section(
            ConfigWarningKind::NotATable,
            format!(
                "`model_providers` must be a table of [model_providers.<id>] entries, got {}; \
                 all model providers ignored",
                section.type_str()
            ),
        ));
        return (providers, warnings);
    };
    for (id, value) in table {
        let mut unknown = Vec::new();
        match serde_ignored::deserialize::<_, _, ModelProviderConfig>(value.clone(), |path| {
            unknown.push(path.to_string());
        }) {
            Ok(provider) => {
                for key in unknown {
                    warnings.push(ConfigWarning::model_provider(
                        id,
                        Some(key.as_str()),
                        ConfigWarningKind::UnknownField,
                        "unrecognized key; field ignored".to_owned(),
                    ));
                }
                if let Some(auth) = &provider.auth {
                    for (field, kind, reason) in auth_config_issues(auth) {
                        warnings.push(ConfigWarning::model_provider(
                            id,
                            Some(&format!("auth.{field}")),
                            kind,
                            reason,
                        ));
                    }
                }
                let has_helper = provider.auth.is_some() || provider.auth_provider.is_some();
                let has_static_api_key = provider
                    .api_key
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|k| !k.is_empty());
                if has_helper && has_static_api_key {
                    warnings.push(ConfigWarning::model_provider(
                        id,
                        Some("api_key"),
                        ConfigWarningKind::ConflictingFields,
                        "api_key shadows this provider's auth helper; the static key always \
                         takes precedence, so the helper never runs for inheriting models"
                            .to_owned(),
                    ));
                } else if has_helper
                    && provider
                        .env_key
                        .as_ref()
                        .and_then(EnvKeys::primary)
                        .is_some()
                {
                    warnings.push(ConfigWarning::model_provider(
                        id,
                        Some("env_key"),
                        ConfigWarningKind::ConflictingFields,
                        "env_key may shadow this provider's auth helper; env_key takes precedence \
                         when its variable resolves, otherwise the helper runs"
                            .to_owned(),
                    ));
                }
                if provider.auth_provider.is_some() && provider.auth.is_some() {
                    warnings.push(ConfigWarning::model_provider(
                        id,
                        Some("auth"),
                        ConfigWarningKind::ConflictingFields,
                        "inline auth is shadowed by auth_provider on this provider; the referenced \
                         provider takes precedence, so the inline helper never runs"
                            .to_owned(),
                    ));
                }
                providers.insert(id.clone(), provider);
            }
            Err(error) => {
                warnings.push(ConfigWarning::model_provider(
                    id,
                    None,
                    ConfigWarningKind::InvalidValue,
                    format!(
                        "failed to parse ({error}); provider skipped, inheriting models \
                         resolve with defaults"
                    ),
                ));
            }
        }
    }
    (providers, warnings)
}

impl ConfigModelOverride {
    pub(crate) fn with_provider_defaults(
        &self,
        provider: &ModelProviderConfig,
        provider_id: &str,
    ) -> Self {
        let ModelProviderConfig {
            base_url,
            api_base_url,
            env_key,
            api_key,
            api_backend,
            extra_headers,
            query_params,
            env_http_headers,
            auth_provider,
            auth,
            context_window,
        } = provider;

        let mut merged = self.clone();
        merged.model_provider = None;
        merged.base_url = merged.base_url.or_else(|| base_url.clone());
        merged.api_base_url = merged.api_base_url.or_else(|| api_base_url.clone());
        merged.api_backend = merged.api_backend.or_else(|| api_backend.clone());
        merged.context_window = merged.context_window.or(*context_window);
        // Inherited wholesale only when the model sets none of its own.
        if merged.extra_headers.is_empty() {
            merged.extra_headers = extra_headers.clone();
        }
        if merged.query_params.is_empty() {
            merged.query_params = query_params.clone();
        }
        if merged.env_http_headers.is_empty() {
            merged.env_http_headers = env_http_headers.clone();
        }
        let model_sets_own_api_key = self
            .api_key
            .as_deref()
            .is_some_and(|k| !k.trim().is_empty());
        let model_sets_own_env_key = self.env_key.as_ref().and_then(EnvKeys::primary).is_some();
        let model_has_own_auth =
            model_sets_own_api_key || model_sets_own_env_key || self.auth_provider.is_some();
        if !model_has_own_auth {
            merged.api_key = api_key.clone();
            merged.env_key = env_key.clone();
            merged.auth_provider = auth_provider
                .clone()
                .or_else(|| auth.as_ref().map(|_| model_provider_auth_name(provider_id)));
        }
        merged
    }

    pub(crate) fn with_missing_provider(&self) -> Self {
        let mut merged = self.clone();
        merged.model_provider = None;
        merged
    }
}

#[cfg(test)]
mod tests {
    use crate::agent::config::{Config, resolve_credentials, resolve_model_list};

    #[test]
    fn provider_hierarchy_builds_the_byok_catalog() {
        let raw: toml::Value = toml::from_str(
            r#"
            [models]
            default = "deepseek/deepseek-chat"

            [provider.deepseek]
            api_backend = "chat_completions"

            [provider.deepseek.options]
            base_url = "https://gateway.example/v1"
            api_key = "secret"

            [provider.deepseek.models.deepseek-chat]
            name = "DeepSeek Chat"
            context_window = 200000
            "#,
        )
        .unwrap();

        let cfg = Config::new_from_toml_cfg(&raw).expect("provider config should parse");
        cfg.validate_llm_configuration()
            .expect("provider config should be complete");
        let models = resolve_model_list(&cfg);
        let model = models
            .get("deepseek/deepseek-chat")
            .expect("canonical provider/model id should exist");
        assert_eq!(model.info.model, "deepseek-chat");
        assert_eq!(model.info.base_url, "https://gateway.example/v1");
        assert_eq!(
            model.info.api_backend,
            crate::sampling::ApiBackend::ChatCompletions
        );
        assert_eq!(
            resolve_credentials(model, Some("product-session-token"))
                .api_key
                .as_deref(),
            Some("secret")
        );
    }

    #[test]
    fn product_credentials_never_become_inference_credentials() {
        let raw: toml::Value = toml::from_str(
            r#"
            [models]
            default = "local/model"

            [provider.local]
            api_backend = "chat_completions"

            [provider.local.options]
            base_url = "http://localhost:11434/v1"

            [provider.local.models.model]
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw).unwrap();
        cfg.validate_llm_configuration()
            .expect("keyless local providers are valid BYOK");
        let models = resolve_model_list(&cfg);
        let model = models.get("local/model").unwrap();
        assert_eq!(
            resolve_credentials(model, Some("product-session-token")).api_key,
            None
        );
    }

    #[test]
    fn missing_provider_catalog_is_rejected_before_connect() {
        let raw: toml::Value = toml::from_str("[models]\ndefault = \"missing/model\"").unwrap();
        let cfg = Config::new_from_toml_cfg(&raw).unwrap();

        assert!(
            cfg.validate_llm_configuration()
                .unwrap_err()
                .contains("no LLM is configured")
        );
    }

    #[test]
    fn global_default_must_be_an_exact_provider_model_id() {
        let raw: toml::Value = toml::from_str(
            r#"
            [models]
            default = "upstream-name"

            [provider.gateway]
            api_backend = "chat_completions"

            [provider.gateway.options]
            base_url = "https://gateway.example/v1"

            [provider.gateway.models.local-id]
            model = "upstream-name"
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw).unwrap();

        assert!(
            cfg.validate_llm_configuration()
                .unwrap_err()
                .contains("does not exist")
        );
    }
}
