use super::mcp::*;
use toml::Value as TomlValue;
pub async fn load_config() -> Config {
    let root: TomlValue = match crate::config::load_effective_config() {
        Ok(v) => v,
        Err(_) => return Config::default(),
    };
    load_config_from_toml(&root)
}
/// Parse `Config` from a pre-loaded TOML value. Used by both async and sync paths.
pub fn load_config_from_toml(root: &TomlValue) -> Config {
    let table = match root.as_table() {
        Some(t) => t,
        None => return Config::default(),
    };
    fn section<T: serde::de::DeserializeOwned + Default>(
        table: &toml::map::Map<String, TomlValue>,
        key: &str,
    ) -> T {
        table
            .get(key)
            .and_then(|v| v.clone().try_into().ok())
            .unwrap_or_default()
    }
    let management_api_key = table
        .get("endpoints")
        .and_then(|v| v.get("management_api_key"))
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let permission = table
        .get("permission")
        .and_then(|v| v.clone().try_into::<PermissionConfig>().ok());
    Config {
        cli: section(table, "cli"),
        models: section(table, "models"),
        ui: section(table, "ui"),
        skills: section(table, "skills"),
        management_api_key,
        permission,
        diagnostics: section(table, "diagnostics"),
        session: section(table, "session"),
        ask_user_question: table
            .get("toolset")
            .and_then(|t| t.get("ask_user_question"))
            .and_then(|v| v.clone().try_into().ok())
            .unwrap_or_default(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use toml::Value as TomlValue;
    #[test]
    fn test_models_default_parsing() {
        let toml_str = r#"
[models]
default = "grow-code-fast-1"
"#;
        let root: TomlValue = toml::from_str(toml_str).unwrap();
        if let TomlValue::Table(table) = root
            && let Some(TomlValue::Table(models)) = table.get("models")
        {
            let default = models
                .get("default")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            assert_eq!(default.as_deref(), Some("grow-code-fast-1"));
        } else {
            panic!("Expected models table");
        }
    }
    #[test]
    fn test_remote_secret_parsing() {
        let toml_str = r#"
[remote]
secret = "my-secret-token"
"#;
        let root: TomlValue = toml::from_str(toml_str).unwrap();
        if let TomlValue::Table(table) = root
            && let Some(TomlValue::Table(remote)) = table.get("remote")
        {
            let secret = remote
                .get("secret")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            assert_eq!(secret, Some("my-secret-token".to_string()));
        } else {
            panic!("Expected remote table");
        }
    }
    #[test]
    fn test_remote_secret_empty_section() {
        let toml_str = r#"
[remote]
"#;
        let root: TomlValue = toml::from_str(toml_str).unwrap();
        if let TomlValue::Table(table) = root
            && let Some(TomlValue::Table(remote)) = table.get("remote")
        {
            let secret = remote
                .get("secret")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            assert!(secret.is_none());
        } else {
            panic!("Expected remote table");
        }
    }
    #[test]
    fn test_remote_secret_no_section() {
        let toml_str = r#"
[models]
default = "grow-code-fast-1"
"#;
        let root: TomlValue = toml::from_str(toml_str).unwrap();
        if let TomlValue::Table(table) = root {
            let has_remote = table.get("remote").is_some();
            assert!(!has_remote);
        }
    }
}
