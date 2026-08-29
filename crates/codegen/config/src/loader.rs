//! Loading `$GROW_HOME/config.toml` and its local overlays.

use std::path::Path;

use crate::paths::user_grow_home;
use crate::version_overrides::{self, apply_version_overrides};

fn read_toml_file(path: &Path) -> std::io::Result<toml::Value> {
    match std::fs::read_to_string(path) {
        Ok(s) => match toml::from_str::<toml::Value>(&s) {
            Ok(v) => Ok(v),
            Err(e) => {
                let detail = toml_error_detail(&s, &e);
                tracing::error!(file = %path.display(), "config toml has syntax errors: {detail}");
                Err(std::io::Error::other(detail))
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(toml::Value::Table(toml::map::Map::new()))
        }
        Err(e) => {
            tracing::error!(file = %path.display(), "config file unreadable: {e}");
            Err(e)
        }
    }
}

pub fn load_toml_file(path: &Path) -> std::io::Result<toml::Value> {
    let mut v = read_toml_file(path)?;
    expand_env_vars_in_toml(&mut v);
    Ok(v)
}

pub fn toml_error_detail(src: &str, e: &toml::de::Error) -> String {
    match e.span() {
        Some(span) => {
            let (line, col) = line_col(src, span.start);
            format!(
                "TOML parse error at line {line}, column {col}: {}",
                e.message()
            )
        }
        None => e.message().to_owned(),
    }
}

fn line_col(src: &str, byte: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in src.char_indices() {
        if i >= byte {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

pub fn load_config_file(path: &Path) -> std::io::Result<toml::Value> {
    let mut v = load_toml_file(path)?;
    apply_version_overrides_with_registered(&mut v)?;
    Ok(v)
}

pub const USER_CONFIG_FILENAME: &str = "config.toml";

/// Load `$GROW_HOME/config.toml`. No cwd-relative fallback is permitted.
pub fn load_from_disk() -> std::io::Result<toml::Value> {
    match user_grow_home() {
        Some(home) => load_config_file(&home.join(USER_CONFIG_FILENAME)),
        None => Ok(toml::Value::Table(toml::map::Map::new())),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookProvenance {
    User,
    File,
    Plugin,
}

impl HookProvenance {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::File => "file",
            Self::Plugin => "plugin",
        }
    }
}

#[derive(Debug, Clone)]
pub struct HookConfigLayer {
    provenance: HookProvenance,
    source_name: String,
    path: std::path::PathBuf,
    hooks: toml::Value,
}

impl HookConfigLayer {
    pub fn new(
        provenance: HookProvenance,
        source_name: impl Into<String>,
        hooks: toml::Value,
    ) -> Self {
        let source_name = source_name.into();
        let path = std::path::PathBuf::from(&source_name);
        Self {
            provenance,
            source_name,
            path,
            hooks,
        }
    }

    pub fn provenance(&self) -> HookProvenance {
        self.provenance
    }

    pub fn source_name(&self) -> &str {
        &self.source_name
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn hooks(&self) -> &toml::Value {
        &self.hooks
    }
}

/// Read the global `hooks` table from `$GROW_HOME/config.toml` without
/// expanding environment variables. Project hooks are discovered separately
/// from the trust-gated `.grow/hooks/` directory.
pub fn hook_config_layers() -> Vec<HookConfigLayer> {
    hook_config_layers_at(user_grow_home().as_deref())
}

pub fn hook_config_layers_at(user_home: Option<&Path>) -> Vec<HookConfigLayer> {
    let Some(path) = user_home.map(|home| home.join(USER_CONFIG_FILENAME)) else {
        return Vec::new();
    };
    if !path.is_file() {
        return Vec::new();
    }
    let mut value = match read_toml_file(&path) {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "skipping config hooks that could not be read");
            return Vec::new();
        }
    };
    if let Err(error) = apply_version_overrides_with_registered(&mut value) {
        tracing::warn!(path = %path.display(), %error, "skipping config hooks whose version_overrides failed to apply");
        return Vec::new();
    }
    let Some(hooks) = value.get("hooks") else {
        return Vec::new();
    };
    if !hooks.is_table() {
        tracing::warn!(path = %path.display(), "ignoring non-table `hooks` value in config");
        return Vec::new();
    }
    vec![HookConfigLayer {
        provenance: HookProvenance::User,
        source_name: "user".to_owned(),
        path,
        hooks: hooks.clone(),
    }]
}

#[derive(Clone)]
pub struct ConfigLayers {
    pub user: toml::Value,
    pub campaigns: crate::campaigns::CampaignOverrides,
}

impl Default for ConfigLayers {
    fn default() -> Self {
        Self {
            user: toml::Value::Table(toml::map::Map::new()),
            campaigns: crate::campaigns::CampaignOverrides::default(),
        }
    }
}

impl ConfigLayers {
    pub fn load() -> std::io::Result<Self> {
        let mut user = load_from_disk()?;
        let campaigns = crate::campaigns::CampaignOverrides {
            user: crate::campaigns::take_campaign_entries(&mut user, "user"),
        };
        Ok(Self { user, campaigns })
    }

    pub fn effective_config_base(&self) -> toml::Value {
        self.user.clone()
    }

    pub fn resolve_campaigns(
        &self,
        base: &toml::Value,
        dismissed_ids: &std::collections::HashSet<String>,
    ) -> Vec<crate::campaigns::CampaignEntry> {
        if campaigns_application_disabled(base) {
            return Vec::new();
        }
        crate::campaigns::filter_active_campaigns(self.campaigns.user.clone(), dismissed_ids)
    }

    pub fn apply_campaign_overrides(
        &self,
        merged: &mut toml::Value,
        active: &[crate::campaigns::CampaignEntry],
    ) {
        crate::campaigns::apply_active_campaign_patches(merged, active);
    }

    pub fn effective_config_disk_only(&self) -> toml::Value {
        let mut merged = self.effective_config_base();
        let active = self.resolve_campaigns(&merged, &load_dismissed_ids_from_home());
        self.apply_campaign_overrides(&mut merged, &active);
        merged
    }
}

pub fn campaigns_application_disabled(base_effective: &toml::Value) -> bool {
    if crate::env_bool("GROW_CAMPAIGNS") == Some(false) {
        return true;
    }
    base_effective
        .get("features")
        .and_then(|f| f.get("campaigns"))
        .and_then(|c| c.as_bool())
        == Some(false)
}

pub fn load_effective_config_disk_only() -> std::io::Result<toml::Value> {
    Ok(ConfigLayers::load()?.effective_config_disk_only())
}

pub const CAMPAIGNS_STATE_FILE: &str = "campaigns_state.json";

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CampaignsState {
    #[serde(default)]
    pub dismissed_ids: Vec<String>,
}

pub fn campaigns_state_path(home: &std::path::Path) -> std::path::PathBuf {
    home.join(CAMPAIGNS_STATE_FILE)
}

pub fn load_dismissed_ids_from_home() -> std::collections::HashSet<String> {
    let Some(home) = crate::user_grow_home() else {
        return std::collections::HashSet::new();
    };
    let Ok(contents) = std::fs::read_to_string(campaigns_state_path(&home)) else {
        return std::collections::HashSet::new();
    };
    serde_json::from_str::<CampaignsState>(&contents)
        .map(|s| s.dismissed_ids.into_iter().collect())
        .unwrap_or_default()
}

pub fn apply_version_overrides_with_registered(value: &mut toml::Value) -> std::io::Result<()> {
    match version::installed_semver() {
        Ok(version) => apply_version_overrides(value, &version)
            .map_err(|e| std::io::Error::other(e.to_string())),
        Err(_) => {
            if let Some(table) = value.as_table_mut() {
                table.remove(version_overrides::VERSION_OVERRIDES_KEY);
            }
            Ok(())
        }
    }
}

pub fn deep_merge_toml(base: &mut toml::Value, overrides: &toml::Value) {
    if let toml::Value::Table(overrides_table) = overrides
        && let toml::Value::Table(base_table) = base
    {
        for (key, value) in overrides_table {
            if let Some(existing) = base_table.get_mut(key) {
                deep_merge_toml(existing, value);
            } else {
                base_table.insert(key.clone(), value.clone());
            }
        }
    } else {
        *base = overrides.clone();
    }
}

pub fn expand_env_vars_in_toml(value: &mut toml::Value) {
    match value {
        toml::Value::String(s) => {
            let expanded = expand_env_vars_in_string(s);
            if expanded != *s {
                *s = expanded;
            }
        }
        toml::Value::Array(items) => {
            for item in items {
                expand_env_vars_in_toml(item);
            }
        }
        toml::Value::Table(table) => {
            for (_, item) in table.iter_mut() {
                expand_env_vars_in_toml(item);
            }
        }
        _ => {}
    }
}

pub fn expand_env_vars_in_string(input: &str) -> String {
    let context = |name: &str| std::env::var(name).ok();
    shellexpand::env_with_context_no_errors(input, context).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_config_layers_reads_only_user_config() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join(USER_CONFIG_FILENAME),
            "[[hooks.pre_tool_use]]\n[[hooks.pre_tool_use.hooks]]\ntype = \"command\"\ncommand = \"${HOME}/u.sh\"\n",
        )
        .unwrap();

        let layers = hook_config_layers_at(Some(home.path()));

        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].provenance(), HookProvenance::User);
        assert_eq!(
            layers[0].hooks()["pre_tool_use"][0]["hooks"][0]["command"].as_str(),
            Some("${HOME}/u.sh")
        );
    }

    #[test]
    fn config_layers_are_the_user_file() {
        let user: toml::Value = toml::from_str("[features]\nweb_fetch = true\n").unwrap();
        let layers = ConfigLayers {
            user: user.clone(),
            ..Default::default()
        };
        assert_eq!(layers.effective_config_base(), user);
    }
}
