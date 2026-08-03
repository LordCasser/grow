use crate::agent::auth_method::ModelByok;
use crate::agent::model_providers::{
    ModelProviderConfig, auth_config_issues, model_provider_auth_name, parse_model_providers,
};
use crate::remote::DEFAULT_CONTEXT_WINDOW;
use crate::{sampling::ApiBackend, tools::config::ShellToolsetConfig};
use agent::prompt::skills::SkillsConfig;
use agent_client_protocol as acp;
use indexmap::IndexMap;
use sampler::{AuthScheme, SamplerConfig};
use sampling_types::{
    CompactionAtTokens, CompactionsRemaining, REASONING_EFFORT_META_KEY,
    REASONING_EFFORTS_META_KEY, ReasoningEffort, ReasoningEffortOption,
    reasoning_effort_meta_value, reasoning_efforts_meta_value,
};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;
use std::path::PathBuf;
use std::sync::Arc;
use tools::types::compat::{
    COMPAT_CELLS, CompatConfig, CompatConfigToml, CompatRemoteKey, CompatSurface, CompatVendor,
};
/// The mode in which the agent is running.
/// Identifies the local product surface that owns the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentMode {
    /// TUI interactive mode.
    Tui,
    /// One-shot non-interactive mode.
    Headless,
    /// Stdio mode - JSON-RPC over stdin/stdout
    Stdio,
    /// Server mode - WebSocket server for external clients
    Serve,
    /// Leader mode - IPC server for follower clients
    Leader,
    /// Generic/unknown mode
    #[default]
    Generic,
}
/// Default agent type when the server or user config doesn't specify one.
pub const DEFAULT_AGENT_TYPE: &str = "grow";
/// Serde default for `ModelInfo.agent_type` and `ModelEntryConfig.agent_type`.
pub fn default_agent_type() -> String {
    DEFAULT_AGENT_TYPE.to_owned()
}
/// Grow ships without a service backend. These empty defaults are retained as
/// resolver sentinels until the optional service subsystem is made fully
/// `Option`-typed.
pub const CLI_CHAT_PROXY_BASE_URL_DEFAULT: &str = "";
pub const INFERENCE_BASE_URL_DEFAULT: &str = "";
pub const ASSET_SERVER_URL_DEFAULT: &str = "";
/// One or more environment variable names that may hold a model API key.
///
/// Serde `untagged`: accepts a string or an array in TOML/JSON.
///
/// ```toml
/// env_key = "ANTHROPIC_AUTH_TOKEN"
/// # or
/// env_key = ["ANTHROPIC_AUTH_TOKEN", "LC_ANTHROPIC_AUTH_TOKEN"]
/// ```
///
/// At resolve time the **first set, non-blank** value wins (e.g. SSH
/// `AcceptEnv LC_*` forwarding of the Bottlerocket token).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EnvKeys {
    One(String),
    Many(Vec<String>),
}
impl EnvKeys {
    /// Single-name convenience constructor.
    pub fn single(name: impl Into<String>) -> Self {
        Self::One(name.into())
    }
    /// Construct from an ordered list (empty names dropped; 0/1/N → Many/One/Many).
    pub fn new(names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let names: Vec<String> = names
            .into_iter()
            .map(Into::into)
            .filter(|s| !s.is_empty())
            .collect();
        match names.as_slice() {
            [] => Self::Many(Vec::new()),
            [_] => Self::One(names.into_iter().next().expect("len 1")),
            _ => Self::Many(names),
        }
    }
    pub fn is_empty(&self) -> bool {
        match self {
            Self::One(s) => s.is_empty(),
            Self::Many(v) => v.is_empty(),
        }
    }
    /// Configured names in priority order.
    pub fn names(&self) -> Vec<&str> {
        match self {
            Self::One(s) => vec![s.as_str()],
            Self::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }
    /// First name only (useful for single-key assertions / display).
    pub fn primary(&self) -> Option<&str> {
        match self {
            Self::One(s) if !s.is_empty() => Some(s.as_str()),
            Self::One(_) => None,
            Self::Many(v) => v.iter().map(String::as_str).find(|s| !s.is_empty()),
        }
    }
    /// Resolve the first set, non-blank process env value among configured names.
    pub fn resolve_value(&self) -> Option<String> {
        self.resolve_value_with(|name| std::env::var(name).ok())
    }
    /// Testable resolve with an injected getenv.
    pub fn resolve_value_with(
        &self,
        mut getenv: impl FnMut(&str) -> Option<String>,
    ) -> Option<String> {
        for name in self.names() {
            if let Some(value) = getenv(name)
                && !value.trim().is_empty()
            {
                return Some(value);
            }
        }
        None
    }
}
/// Semantic equality: compares the ordered name lists, so `One("X")` and
/// `Many(["X"])` (the shape serde produces for `["X"]`) compare equal.
impl PartialEq for EnvKeys {
    fn eq(&self, other: &Self) -> bool {
        self.names() == other.names()
    }
}
impl Eq for EnvKeys {}
impl std::fmt::Display for EnvKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.names().join(", "))
    }
}
/// Configuration for API endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EndpointsConfig {
    /// cli chat proxy base URL. `None` = unset (resolvers apply the default);
    /// `Some` = explicitly configured. Tracking explicitness (vs comparing to the
    /// default value) lets an org pin the proxy to the default on purpose.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_chat_proxy_base_url: Option<String>,
    /// Optional service inference base URL. Normal LLM requests use the
    /// selected provider/model `base_url` instead.
    pub inference_base_url: String,
    /// Optional extra access-header value (applied only with the optional
    /// non-production feature, and only for matching first-party hosts).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_test_key: Option<String>,
    /// Env: `GROW_DEPLOYMENT_KEY`. Management API key for enterprise deployments.
    /// Sent on managed service requests for deployment-level attribution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment_key: Option<String>,
    /// Env: `GROW_MANAGED_CONFIG_URL`. Override the managed config endpoint.
    /// Defaults to `{proxy_url()}/deployment/config`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub managed_config_url: Option<String>,
    /// Base URL for the asset server (profile images, etc.).
    /// Env: `GROW_ASSET_SERVER_URL`.
    #[serde(default = "default_asset_server_url")]
    pub asset_server_url: String,
    /// Read by `load_management_api_key_sync()`. Declared for `serde_ignored`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub management_api_key: Option<String>,
}
pub(crate) fn default_asset_server_url() -> String {
    std::env::var("GROW_ASSET_SERVER_URL").unwrap_or_else(|_| ASSET_SERVER_URL_DEFAULT.to_owned())
}
/// A blank or whitespace-only override counts as unset. Single source of truth
/// for the "empty value = not configured" rule shared by the endpoint resolvers.
fn blank_as_unset(opt: &Option<String>) -> Option<String> {
    opt.as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
}
impl EndpointsConfig {
    /// `default()` plus merged managed/requirements endpoint overrides, so
    /// startup fetches use the configured (not public) endpoints. Only merges
    /// layers — never derives one endpoint from another. Falls back to
    /// `default()` on load failure.
    pub fn from_effective_config() -> Self {
        match crate::config::load_effective_config() {
            Ok(cfg) => Self::from_config_value(&cfg),
            Err(_) => Self::default(),
        }
    }
    /// Layer the `[endpoints]` table from `config` over the env/default base.
    /// No field is derived from another — defaulting is done by the resolvers.
    pub fn from_config_value(config: &toml::Value) -> Self {
        let default = Self::default();
        let mut base = match toml::Value::try_from(default) {
            Ok(v) => v,
            Err(_) => return Self::default(),
        };
        if let Some(endpoints) = config.get("endpoints") {
            crate::config::deep_merge_toml(&mut base, endpoints);
        }
        base.try_into().unwrap_or_default()
    }
    /// The explicitly configured auxiliary-service base URL. Empty means the
    /// optional service subsystem is unavailable.
    pub fn proxy_url(&self) -> String {
        blank_as_unset(&self.cli_chat_proxy_base_url)
            .unwrap_or_else(|| CLI_CHAT_PROXY_BASE_URL_DEFAULT.to_owned())
    }
    /// Managed deployment-config URL (`grow setup`): explicit `managed_config_url`,
    /// else `proxy_url` + `/deployment/config`. Never `inference_base_url`, so the
    /// deployment key reaches the proxy, not the inference host.
    pub fn resolve_managed_config_url(&self) -> Option<String> {
        blank_as_unset(&self.managed_config_url).or_else(|| {
            blank_as_unset(&self.cli_chat_proxy_base_url)
                .map(|proxy| format!("{}/deployment/config", proxy.trim_end_matches('/')))
        })
    }
}
impl Default for EndpointsConfig {
    fn default() -> Self {
        Self {
            cli_chat_proxy_base_url: std::env::var("GROW_CLI_CHAT_PROXY_BASE_URL").ok(),
            inference_base_url: std::env::var("GROW_INFERENCE_BASE_URL")
                .unwrap_or_else(|_| INFERENCE_BASE_URL_DEFAULT.to_owned()),
            alpha_test_key: None,
            deployment_key: env_string("GROW_DEPLOYMENT_KEY"),
            managed_config_url: env_string("GROW_MANAGED_CONFIG_URL"),
            asset_server_url: default_asset_server_url(),
            management_api_key: None,
        }
    }
}
pub use config_types::{BoolFlag, ConfigSource, LazinessDetectorPerModelConfig, Resolved};
/// Resolution result for a `/goal` role's model selection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum GoalRoleModelChoice {
    /// Use the current (parent) model + the parent's agent type.
    #[default]
    InheritCurrent,
    /// Use this explicit pair (subject to auth/fail-open at spawn time).
    Explicit(crate::util::config::GoalRoleModel),
}
/// A requirement pin from `requirements.toml`. Wins over all other sources.
#[derive(Debug, Clone, Default)]
pub struct Constrained<T> {
    pin: Option<T>,
    source: Option<crate::config::RequirementSource>,
}
impl<T: Clone> Constrained<T> {
    pub fn pin(&mut self, value: T, source: crate::config::RequirementSource) {
        self.pin = Some(value);
        self.source = Some(source);
    }
    pub fn pinned(&self) -> Option<T> {
        self.pin.clone()
    }
    pub fn source(&self) -> Option<&crate::config::RequirementSource> {
        self.source.as_ref()
    }
}
/// Enforced requirements from `requirements.toml`. Pinned values win over all other sources.
#[derive(Debug, Clone, Default)]
pub struct Requirements {
    pub lsp_tools: Constrained<bool>,
    pub tool_search: Constrained<bool>,
    pub web_fetch: Constrained<bool>,
    pub ask_user_question: Constrained<bool>,
    pub write_file: Constrained<bool>,
    pub sandbox_auto_allow_bash: Constrained<bool>,
    pub sandbox_profile: Constrained<String>,
    pub respect_gitignore: Constrained<bool>,
}
/// Inputs for resolving `#[serde(skip)]` runtime fields after `new_from_toml_cfg()`.
///
/// Constructed by each binary from its CLI args and startup state, then passed
/// to [`Config::resolve_runtime_fields`].
pub struct RuntimeResolutionContext<'a> {
    pub raw_config: &'a toml::Value,
    pub remote_settings: Option<&'a crate::util::config::RemoteSettings>,
    pub is_headless: bool,
    /// `Some(true)` = CLI explicitly enabled, `None` = defer to config/env/remote.
    pub cli_subagents: Option<bool>,
    pub cli_session_summary_model: Option<&'a str>,
    /// CLI `--experimental-memory` flag. Enables cross-session memory.
    pub cli_experimental_memory: bool,
    /// CLI `--no-memory` flag. Overrides all other memory settings.
    pub cli_no_memory: bool,
    /// CLI `--todo-gate` flag. Session-scoped — not persisted.
    pub todo_gate: bool,
    /// CLI `--laziness-debug-log <path>`. When `Some`, the Layer-3
    /// classifier fires after every turn (bypassing the idle wait /
    /// per-model gate / nudge cap) and writes a JSONL line per fire.
    /// Observation-only. Session-scoped — not persisted.
    pub laziness_debug_log: Option<&'a std::path::Path>,
}
/// First-party credential env vars scrubbed from a BYOK auth-provider helper's
/// environment so it can't inherit the keys Grow uses for its own first-party
/// requests. Keep in sync with every first-party credential env read across the
/// crate: `auth::manager` (`GROW_AUTH`/`GROW_AUTH_PATH`), `auth_method`
/// (`GROW_API_KEY`/legacy), and the credential-bearing `env_string(...)` reads in
/// `EndpointsConfig::default`. The `provider_helper_env_scrubs_first_party_credentials`
/// test pins this against an independent audited literal, so any change here must
/// be mirrored (and re-audited) there.
pub(crate) const FIRST_PARTY_CREDENTIAL_ENV_VARS: &[&str] = &[
    crate::agent::auth_method::GROW_API_KEY_ENV_VAR,
    "GROW_AUTH",
    "GROW_AUTH_PATH",
    "GROW_DEPLOYMENT_KEY",
    "GROW_EXTRA_AUTH_KEY",
];
/// Read an env var as a trimmed string. Returns `None` if unset or empty/whitespace-only.
pub(crate) fn env_string(name: &str) -> Option<String> {
    let value = std::env::var(name).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
pub use config::env_bool;
/// Compaction-mode precedence (env > config > remote settings > default, with
/// unrecognized values at each source falling through). `remote` sits just
/// above the default, mirroring `feature_flag` in `resolve_bool_flag`. Pure so
/// it's unit-testable without mutating process env.
fn resolve_compaction_mode_from(
    env: Option<&str>,
    config: Option<&str>,
    remote: Option<&str>,
) -> chat_state::CompactionMode {
    use chat_state::CompactionMode;
    env.and_then(CompactionMode::parse)
        .or_else(|| config.and_then(CompactionMode::parse))
        .or_else(|| remote.and_then(CompactionMode::parse))
        .unwrap_or_default()
}
/// Compaction-detail precedence (env > config > remote settings > default). Pure.
/// Controls the per-turn verbatim detail in `segments` mode (default `verbose`).
fn resolve_compaction_detail_from(
    env: Option<&str>,
    config: Option<&str>,
    remote: Option<&str>,
) -> chat_state::CompactionDetail {
    use chat_state::CompactionDetail;
    env.and_then(CompactionDetail::parse)
        .or_else(|| config.and_then(CompactionDetail::parse))
        .or_else(|| remote.and_then(CompactionDetail::parse))
        .unwrap_or_default()
}
/// Resolve a single vendor-compat cell: env > `[compat]` TOML > remote settings
/// remote flag > default ON.
fn resolve_compat_cell(
    env: &str,
    cfg: Option<bool>,
    remote: Option<bool>,
    default: bool,
) -> Resolved<bool> {
    resolve_compat_cell_with_env(config::env_bool(env), cfg, remote, default)
}
pub(crate) fn resolve_compat_cell_with_env(
    env: Option<bool>,
    cfg: Option<bool>,
    remote: Option<bool>,
    default: bool,
) -> Resolved<bool> {
    if let Some(value) = env {
        Resolved::new(value, ConfigSource::Env)
    } else if let Some(value) = cfg {
        Resolved::new(value, ConfigSource::Config)
    } else if let Some(value) = remote {
        Resolved::new(value, ConfigSource::Remote)
    } else {
        Resolved::new(default, ConfigSource::Default)
    }
}
fn remote_compat_value(
    remote: Option<&crate::util::config::RemoteSettings>,
    key: Option<CompatRemoteKey>,
) -> Option<bool> {
    let remote = remote?;
    match key? {
        CompatRemoteKey::CursorSkills => remote.cursor_skills_enabled,
        CompatRemoteKey::CursorRules => remote.cursor_rules_enabled,
        CompatRemoteKey::CursorAgents => remote.cursor_agents_enabled,
        CompatRemoteKey::CursorMcps => remote.cursor_mcps_enabled,
        CompatRemoteKey::CursorHooks => remote.cursor_hooks_enabled,
        CompatRemoteKey::ClaudeSkills => remote.claude_skills_enabled,
        CompatRemoteKey::ClaudeRules => remote.claude_rules_enabled,
        CompatRemoteKey::ClaudeAgents => remote.claude_agents_enabled,
        CompatRemoteKey::ClaudeMcps => remote.claude_mcps_enabled,
        CompatRemoteKey::ClaudeHooks => remote.claude_hooks_enabled,
    }
}
/// Resolve vendor compatibility cells from TOML and remote settings.
fn resolve_compat_config(
    config: &CompatConfigToml,
    remote: Option<&crate::util::config::RemoteSettings>,
) -> CompatConfig {
    let defaults = CompatConfig::default();
    let mut resolved = defaults;
    for cell in COMPAT_CELLS {
        resolved.set(
            cell,
            resolve_compat_cell(
                cell.env_var(),
                config.value(cell),
                remote_compat_value(remote, cell.remote_key()),
                defaults.value(cell),
            )
            .value,
        );
    }
    resolved
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompatConfigCellError {
    Unavailable,
    Malformed,
}
pub(crate) fn compat_config_cell(
    raw_config: Result<&toml::Value, ()>,
    cell: tools::types::compat::CompatCell,
) -> Result<Option<bool>, CompatConfigCellError> {
    let raw = raw_config.map_err(|()| CompatConfigCellError::Unavailable)?;
    let Some(compat) = raw.get("compat") else {
        return Ok(None);
    };
    let compat = compat.as_table().ok_or(CompatConfigCellError::Malformed)?;
    let Some(vendor) = compat.get(cell.vendor().as_str()) else {
        return Ok(None);
    };
    let vendor = vendor.as_table().ok_or(CompatConfigCellError::Malformed)?;
    let Some(value) = vendor.get(cell.surface().as_str()) else {
        return Ok(None);
    };
    value
        .as_bool()
        .map(Some)
        .ok_or(CompatConfigCellError::Malformed)
}
/// Resolve a string setting: cli > env > config > feature flag. `None` if no source provides a value.
pub(crate) fn resolve_string_flag(
    cli_arg: Option<&str>,
    env_var: &str,
    config_val: Option<&str>,
    feature_flag_val: Option<&str>,
) -> Option<Resolved<String>> {
    if let Some(val) = cli_arg.filter(|s| !s.is_empty()) {
        return Some(Resolved::new(val.to_owned(), ConfigSource::Cli));
    }
    if let Some(val) = env_string(env_var) {
        return Some(Resolved::new(val, ConfigSource::Env));
    }
    if let Some(val) = config_val.filter(|s| !s.is_empty()) {
        return Some(Resolved::new(val.to_owned(), ConfigSource::Config));
    }
    if let Some(val) = feature_flag_val.filter(|s| !s.is_empty()) {
        return Some(Resolved::new(val.to_owned(), ConfigSource::Remote));
    }
    None
}
/// Resolve `enabled` for section-based configs (memory, subagents, etc.).
/// Feature flag only applies when the TOML section is absent.
pub(crate) fn resolve_enabled(
    cli_flag: Option<bool>,
    env_var: &str,
    config_enabled: bool,
    has_local_section: bool,
    feature_flag_val: Option<bool>,
    default: bool,
) -> Resolved<bool> {
    let config_val = if has_local_section {
        Some(config_enabled)
    } else {
        None
    };
    BoolFlag::env(env_var)
        .cli(cli_flag)
        .config(config_val)
        .feature_flag(feature_flag_val)
        .default(default)
        .resolve()
}
/// Plugin system configuration from `[plugins]` section in config.toml.
///
/// ```toml
/// [plugins]
/// paths = ["~/my-plugins/custom-tools"]
/// disabled = ["user/a1b2c3d4/noisy-plugin"]
/// ```
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PluginsConfig {
    /// Additional plugin directory paths to load.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Plugin IDs or names to disable. Disabled plugins are discovered
    /// but their components are not loaded into the session.
    #[serde(default)]
    pub disabled: Vec<String>,
    /// Plugin IDs or names to explicitly enable. Used for project-scope plugins
    /// which are disabled by default — adding a plugin here overrides that default.
    #[serde(default)]
    pub enabled: Vec<String>,
    /// CLI `--plugin-dir` paths (populated by CLI arg processing, not config file).
    #[serde(skip)]
    pub cli_plugin_dirs: Vec<std::path::PathBuf>,
}
impl PluginsConfig {
    /// Merge `enabledPlugins` from Claude settings files into this config.
    ///
    /// Reads `enabledPlugins` from `~/.claude/settings.json` only (user scope).
    /// Project-level `<git_root>/.claude/settings.json` is intentionally NOT
    /// read here: a malicious repo could pre-populate `enabledPlugins` to
    /// bypass the project-plugin auto-disable logic in `populate_plugin_lists`,
    /// enabling attacker-controlled hooks (e.g. SessionStart → RCE).
    /// Native `.grow/config.toml` entries already present take precedence:
    /// a name is only added if it isn't already in the opposite list.
    pub fn merge_claude_enabled_plugins(&mut self, _cwd: Option<&std::path::Path>) {
        if crate::claude_import::is_claude_import_marked_with_log("merge_claude_enabled_plugins") {
            return;
        }
        let mut paths = Vec::new();
        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(".claude").join("settings.json"));
        }
        for path in &paths {
            let (claude_enabled, claude_disabled) =
                agent::plugins::marketplace::load_enabled_disabled_plugins(path);
            for name in claude_enabled {
                if !self.disabled.contains(&name) && !self.enabled.contains(&name) {
                    self.enabled.push(name);
                }
            }
            for name in claude_disabled {
                if !self.enabled.contains(&name) && !self.disabled.contains(&name) {
                    self.disabled.push(name);
                }
            }
        }
    }
    /// Build a `DiscoveryConfig` from this plugins config.
    pub fn to_discovery_config(&self) -> agent::plugins::discovery::DiscoveryConfig {
        agent::plugins::discovery::DiscoveryConfig {
            cli_plugin_dirs: self.cli_plugin_dirs.clone(),
            config_paths: self.paths.iter().map(std::path::PathBuf::from).collect(),
            disabled: self.disabled.clone(),
            enabled: self.enabled.clone(),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CompactionConfig {
    pub memory_flush: Option<crate::config::MemoryFlushConfig>,
    pub pruning: Option<crate::config::PruningConfig>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CliConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_update: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dismissed_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_leader: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_tips: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_type: Option<String>,
    /// Env `GROW_MINIMUM_VERSION`. See [`crate::util::config::VersionPolicy`] for
    /// the version-policy knobs. (Unrelated to
    /// `version_overrides[].maximum_version`, which gates config patches.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_version: Option<String>,
    /// Env `GROW_MAXIMUM_VERSION`. See [`crate::util::config::VersionPolicy`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_version: Option<String>,
    /// Env `GROW_REQUIRED_MINIMUM_VERSION`. See [`crate::util::config::VersionPolicy`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_minimum_version: Option<String>,
    /// Env `GROW_REQUIRED_MAXIMUM_VERSION`. See [`crate::util::config::VersionPolicy`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_maximum_version: Option<String>,
    /// Group sessions by repo in the picker and CLI listings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_picker_grouped: Option<bool>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DiagnosticsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crash_handler: Option<bool>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// The pre-campaign `models.default` (merged user/managed/requirements)
    /// captured when a campaign is overriding the default, so model resolution can
    /// recover if the campaign points at a model missing from the catalog. `None`
    /// when there is nothing to recover to. Runtime-only; never serialized.
    #[serde(skip)]
    pub pre_campaign_default: Option<String>,
    /// Whether an active campaign is currently overriding `models.default`. The
    /// authoritative campaign-driven-default signal (set from the resolved active
    /// set), correct even when the user has no base default. Runtime-only.
    #[serde(skip)]
    pub default_is_campaign_driven: bool,
    /// Persisted effort for the default model; applied in `resolve_model_catalog`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_reasoning_effort: Option<ReasoningEffort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_summary: Option<String>,
    /// Optional vision model used to describe images returned by `read_file`.
    /// When unset, the active model receives those images directly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_description: Option<String>,
    /// Model pin for next-prompt suggestions (tab-autocomplete ghost text).
    /// Unset disables model-backed prompt suggestions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_suggestion: Option<String>,
    /// Restricts which models are user-selectable for normal chat (picker,
    /// `/model`, `-m`). Non-matching models stay in the catalog but are never
    /// shown, defaulted to, or selectable. Special/internal models
    /// (image_description, subagents, fork secondary) are exempt.
    ///
    /// Glob patterns (`*`, `?`, `[...]`) match the model id or catalog key,
    /// case-sensitive. Empty = no restriction; an excluded explicit `default`/`-m`
    /// is rejected at startup.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_models: Option<Vec<String>>,
    /// Force `hidden = true` on these model IDs (still usable via `-m`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden_models: Option<Vec<String>>,
    /// Remove these model IDs from the catalog entirely. Wins over `hidden_models`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_models: Option<Vec<String>>,
    /// Fallback `agent_type` for models without a per-model override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    /// Global default request headers applied to every model. A per-model
    /// `[model.<id>].extra_headers` entry overrides per key (case-insensitive).
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub extra_headers: IndexMap<String, String>,
    /// Global default values applied to every model that leaves the field
    /// unset; a per-model `[model.<id>]` value always wins. A deliberately
    /// small, allow-listed subset of the per-model fields (only `Option` ones,
    /// so "unset" is unambiguous). Future: these could consolidate into a
    /// `[models.defaults]` sub-table mirroring the per-model schema 1:1; kept
    /// flat for now as that is a larger refactor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inference_idle_timeout_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_tool_calls: Option<bool>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorktreePoolConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_size: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_count_threshold: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallelism: Option<usize>,
}
/// `[worktree]` section from config.toml (auto-GC policy lives under `auto_gc`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorktreeConfigSection {
    #[serde(default)]
    pub auto_gc: crate::util::config::WorktreeAutoGcSettings,
}
/// `[sandbox]` section from config.toml.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SandboxSettingsConfig {
    /// "off", "workspace", "devbox", "read-only", "strict", or custom name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Skip bash permission prompts when sandbox is active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_allow_bash: Option<bool>,
}
impl SandboxSettingsConfig {
    pub fn from_effective_config() -> Self {
        crate::config::load_effective_config()
            .ok()
            .and_then(|v| v.get("sandbox")?.clone().try_into().ok())
            .unwrap_or_default()
    }
    /// Resolve sandbox profile: requirement > CLI > env > config > "off".
    pub fn resolve_profile(
        &self,
        cli_arg: Option<&str>,
        requirement: Option<&str>,
    ) -> Resolved<String> {
        if let Some(val) = requirement {
            return Resolved::new(val.to_owned(), ConfigSource::Requirement);
        }
        resolve_string_flag(cli_arg, "GROW_SANDBOX", self.profile.as_deref(), None)
            .unwrap_or_else(|| Resolved::new("off".to_owned(), ConfigSource::Default))
    }
    /// Resolve auto_allow_bash: requirement > env > config > default (false).
    pub fn resolve_auto_allow_bash(&self, requirement: Option<bool>) -> Resolved<bool> {
        BoolFlag::env("GROW_SANDBOX_AUTO_ALLOW_BASH")
            .requirement(requirement)
            .config(self.auto_allow_bash)
            .resolve()
    }
}
/// `[marketplace]` section from config.toml (plugin marketplace sources).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct MarketplaceConfig {
    /// Optional source loaded automatically before manually registered sources.
    #[serde(default)]
    pub bootstrap: Option<MarketplaceSourceEntry>,
    /// `[[marketplace.sources]]` entries.
    #[serde(default)]
    pub sources: Vec<MarketplaceSourceEntry>,
    /// Written/read out-of-band by `extensions::marketplace`, opaque so a wrong-typed value can't fail load.
    #[serde(default)]
    pub default_skills_installs_purged: Option<toml::Value>,
}
/// A single `[[marketplace.sources]]` entry.
#[derive(Clone, Debug, Deserialize)]
pub struct MarketplaceSourceEntry {
    pub name: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub git: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
}
/// `[suggestions]` section from config.toml.
///
/// Controls the shell command suggestion pipeline (history, path, AI).
///
/// ```toml
/// [suggestions]
/// enabled = true
/// ai_enabled = true
/// ai_model = "grow-build"
/// debounce_ms = 50
/// ```
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SuggestionsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debounce_ms: Option<u64>,
}
impl SuggestionsConfig {
    pub fn resolve_enabled(
        &self,
        remote: Option<&crate::util::config::RemoteSettings>,
    ) -> Resolved<bool> {
        BoolFlag::env("GROW_SUGGESTIONS")
            .config(self.enabled)
            .feature_flag(remote.and_then(|r| r.suggestions_enabled))
            .default(false)
            .resolve()
    }
    pub fn resolve_ai_enabled(
        &self,
        remote: Option<&crate::util::config::RemoteSettings>,
    ) -> Resolved<bool> {
        BoolFlag::env("GROW_SUGGESTIONS_AI")
            .config(self.ai_enabled)
            .feature_flag(remote.and_then(|r| r.suggestions_ai_enabled))
            .default(false)
            .resolve()
    }
    pub fn resolve_ai_model(&self) -> String {
        resolve_string_flag(
            None,
            "GROW_SUGGESTIONS_AI_MODEL",
            self.ai_model.as_deref(),
            None,
        )
        .map(|r| r.value)
        .unwrap_or_else(|| "grow-build".to_owned())
    }
}
/// `[storage]` section from config.toml.
///
/// Controls session persistence settings like cleanup TTL.
/// Read by `resolve_cleanup_ttl_days()` in `session/persistence.rs`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    /// Number of days to keep stale sessions before cleanup. Default: 30.
    pub cleanup_ttl_days: Option<u32>,
}
/// `[paths]` configuration: extra directories to scan for skills, rules, etc.
///
/// These supplement the built-in scan locations (`.grow/skills/`,
/// `.agents/skills/`, `~/.grow/skills/`). They're written by `/import-claude`
/// to preserve previously-discovered Claude directories after the runtime
/// `.claude/` cutoff (see `[claude_compat] imported`).
///
/// Example:
/// ```toml
/// [paths]
/// extra_skill_dirs = ["~/.claude/skills", "/path/to/.claude/skills"]
/// extra_rule_dirs = ["~/.claude/rules"]
/// ```
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PathsConfig {
    /// Additional directories to scan for skills (each contains `<skill>/SKILL.md`).
    pub extra_skill_dirs: Vec<String>,
    /// Additional directories to scan for rules (each contains `*.md`).
    pub extra_rule_dirs: Vec<String>,
}
/// `[permission]` known keys, declared for the unrecognized-key scan only;
/// consumed out-of-band. Keys stay typed so a typo (e.g. `denny`) still warns.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct PermissionKnownKeys {
    /// Compact rule arrays (`parse_toml_permission_section`).
    pub allow: Option<toml::Value>,
    pub deny: Option<toml::Value>,
    pub ask: Option<toml::Value>,
    /// Verbose `[[permission.rules]]` form.
    pub rules: Option<toml::Value>,
}
/// `[shell_environment_policy]` known keys, for the unrecognized-key scan only;
/// the value is parsed at spawn by [`crate::util::config::resolve_shell_env_policy`].
/// `Option<toml::Value>` (no `deny_unknown_fields`) keeps a typo a warning, not a
/// load failure, like [`PermissionKnownKeys`].
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ShellEnvironmentPolicyKnownKeys {
    pub inherit: Option<toml::Value>,
    pub ignore_default_excludes: Option<toml::Value>,
    pub exclude: Option<toml::Value>,
    pub set: Option<toml::Value>,
    pub include_only: Option<toml::Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub features: Features,
    /// `[goal]` section: canonical `/goal` configuration. See [`GoalConfig`].
    #[serde(default)]
    pub goal: GoalConfig,
    #[serde(default)]
    pub workflows: WorkflowsConfig,
    /// `[doom_loop_recovery]` section: the shared settings struct — ONE type
    /// serves this TOML table and the remote remote settings `doom_loop_recovery`
    /// object. See [`crate::util::config::DoomLoopRecoverySettings`].
    #[serde(default)]
    pub doom_loop_recovery: crate::util::config::DoomLoopRecoverySettings,
    /// `[worktree]` section (currently `[worktree.auto_gc]` only).
    #[serde(default)]
    pub worktree: WorktreeConfigSection,
    /// `[auto_mode]` section: Auto permission-mode configuration. See [`AutoModeConfig`].
    #[serde(default)]
    pub auto_mode: AutoModeConfig,
    /// Flattened `[provider.*.models.*]` entries. Resolve via `resolve_model_list()`.
    #[serde(skip)]
    pub config_models: IndexMap<String, ConfigModelOverride>,
    #[serde(skip)]
    pub config_warnings: Vec<super::config_model_override_parse::ConfigWarning>,
    /// `[auth_provider.<name>]` tables, populated by
    /// [`parse_auth_providers`] from trusted config layers only.
    #[serde(skip)]
    pub auth_providers: IndexMap<String, crate::auth::AuthProviderConfig>,
    #[serde(skip)]
    pub model_providers: IndexMap<String, ModelProviderConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcuts: Option<toml::Value>,
    /// Written by the client via `config_toml_edit`; absorbed so it isn't
    /// flagged as an unrecognized key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hints: Option<toml::Value>,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub toolset: ShellToolsetConfig,
    /// Validation only; the value is parsed at spawn by `resolve_shell_env_policy`.
    #[serde(default, skip_serializing)]
    pub shell_environment_policy: ShellEnvironmentPolicyKnownKeys,
    #[serde(default)]
    pub endpoints: EndpointsConfig,
    /// Session behavior configuration.
    #[serde(default)]
    pub session: SessionConfig,
    /// Agent definition selection configuration.
    /// Set in `config.toml` under `[agent]` to choose which agent definition
    /// is used for all sessions (unless overridden by CLI flag or ACP meta).
    #[serde(default)]
    pub agent: AgentSelectionConfig,
    #[serde(default)]
    pub repo_changes_dedup: RepoChangesDedupConfig,
    /// Skills discovery configuration.
    #[serde(default)]
    pub skills: SkillsConfig,
    /// Raw `[compat]` vendor-compatibility config (per-vendor × per-surface
    /// toggles). Resolved into [`Config::compat_resolved`] by
    /// `resolve_runtime_fields`.
    #[serde(default)]
    pub compat: CompatConfigToml,
    /// Plugin system configuration.
    #[serde(default)]
    pub plugins: PluginsConfig,
    /// Filesystem path overrides (`[paths]` in config.toml).
    #[serde(default)]
    pub paths: PathsConfig,
    #[serde(default, skip_serializing)]
    pub cli: CliConfig,
    #[serde(default, skip_serializing)]
    pub models: ModelsConfig,
    #[serde(default, skip_serializing)]
    pub remote: RemoteConfig,
    #[serde(default, skip_serializing)]
    pub worktree_pool: WorktreePoolConfig,
    #[serde(default, skip_serializing)]
    pub sandbox: SandboxSettingsConfig,
    #[serde(default, skip_serializing)]
    pub mcp_servers: std::collections::HashMap<String, crate::util::config::McpServerConfig>,
    #[serde(default, skip_serializing)]
    pub disabled_mcp_servers: Vec<String>,
    #[serde(default, skip_serializing)]
    pub disabled_mcp_tools: std::collections::HashMap<String, Vec<String>>,
    #[serde(default, skip_serializing)]
    pub subagents: crate::config::SubagentsConfig,
    #[serde(default, skip_serializing)]
    pub memory: crate::config::MemoryConfig,
    #[serde(default, skip_serializing)]
    pub compaction: CompactionConfig,
    #[serde(default, skip_serializing)]
    pub managed_mcps: crate::config::ManagedMcpsConfig,
    /// Final top-level local `announcements` array. The default is Grow's
    /// built-in notice; an explicitly configured empty array disables it.
    #[serde(default = "announcements::default_announcements", skip_serializing)]
    pub announcements: Vec<announcements::Announcement>,
    /// `[tips]` section — consumed by `merge_tips`.
    #[serde(default, skip_serializing)]
    pub tips: Option<crate::util::config::TipsOverride>,
    /// `[permission]` — consumed out-of-band; see [`PermissionKnownKeys`].
    #[serde(default, skip_serializing)]
    pub permission: PermissionKnownKeys,
    /// `[tools]` — also read by `ToolsConfig::resolve()`.
    #[serde(default, skip_serializing)]
    pub tools: crate::config::ToolsConfig,
    /// `[storage]` — also read by `resolve_cleanup_ttl_days()`.
    #[serde(default, skip_serializing)]
    pub storage: StorageConfig,
    /// `[suggestions]` — shell command suggestion pipeline settings.
    #[serde(default, skip_serializing)]
    pub suggestions: SuggestionsConfig,
    /// `[marketplace]` — also read by `plugin_marketplace::load_sources()`.
    #[serde(default, skip_serializing)]
    pub marketplace: MarketplaceConfig,
    /// `[diagnostics]` — crash handler toggle (`load_crash_handler_enabled_sync`).
    #[serde(default, skip_serializing)]
    pub diagnostics: DiagnosticsConfig,
    /// CLI override for the default model ID.
    #[serde(skip)]
    pub default_model_override: Option<String>,
    /// CLI override for reasoning effort.
    #[serde(skip)]
    pub reasoning_effort_override: Option<ReasoningEffort>,
    /// CLI override for the session summary model ID.
    #[serde(skip)]
    pub session_summary_model_override: Option<String>,
    /// CLI override for YOLO mode (auto-approve all permissions).
    /// Takes precedence over default settings.
    #[serde(skip)]
    pub default_yolo_mode: bool,
    /// Start sessions in auto permission mode (classifier) when no per-session override.
    pub default_auto_mode: bool,
    /// CLI `--experimental-memory` flag. Stored for `ConfigReloader` hot-reload re-resolution.
    #[serde(skip)]
    pub cli_experimental_memory: bool,
    /// CLI `--no-memory` flag. Stored for `ConfigReloader` hot-reload re-resolution.
    #[serde(skip)]
    pub cli_no_memory: bool,
    /// Original CLI `--subagents` tri-state, preserved for re-resolution
    /// when remote settings settings are refreshed on /new.
    #[serde(skip)]
    pub cli_subagents: Option<bool>,
    /// Resolved memory configuration. `None` when memory is disabled.
    /// Resolved by [`RuntimeResolutionContext`] in [`Config::resolve_runtime_fields`].
    #[serde(skip)]
    pub memory_config: Option<crate::config::MemoryConfig>,
    /// CLI override: path to an agent profile (.md file with YAML frontmatter).
    #[serde(skip)]
    pub agent_profile_path: Option<PathBuf>,
    /// Client version string (e.g., "0.1.77 (abc1234)").
    /// Set by the TUI/CLI launcher and used as fallback when clients don't provide clientVersion.
    #[serde(skip)]
    pub client_version: Option<String>,
    /// The mode in which the agent is running.
    /// Identifies the local product surface that owns the agent.
    #[serde(skip)]
    pub mode: AgentMode,
    /// Remote settings fetched from cli-chat-proxy at startup.
    #[serde(skip)]
    pub remote_settings: Option<crate::util::config::RemoteSettings>,
    #[serde(skip)]
    pub cli_agents: Vec<agent::config::AgentDefinition>,
    #[serde(skip)]
    pub cli_agent_overrides: CliAgentOverrides,
    /// Whether subagent (task tool) support is enabled. Enabled by default;
    /// disabled only via `GROW_SUBAGENTS=0` or `[subagents] enabled = false`.
    /// Not remotely gated.
    #[serde(skip)]
    pub subagents_enabled: bool,
    /// Resolved max subagent nesting depth (see
    /// [`crate::config::SubagentsConfig::resolve_max_depth`]).
    #[serde(skip)]
    pub subagents_max_depth: u32,
    /// Per-subagent model ID overrides from `[subagents.models]` in config.toml.
    /// Keys are agent names, values are model IDs. Set alongside `subagents_enabled`
    /// from `SubagentsConfig::resolve()`.
    #[serde(skip)]
    pub subagent_model_overrides: std::collections::HashMap<String, String>,
    /// Per-subagent enable/disable toggles from `[subagents.toggle]` in config.toml.
    /// Keys are agent names, values are booleans. Omitted agents default to enabled.
    #[serde(skip)]
    pub subagent_toggle: std::collections::HashMap<String, bool>,
    /// Trust-independent roles from inline, user, and bundled sources.
    #[serde(skip)]
    pub subagent_roles: std::collections::HashMap<String, crate::config::SubagentRole>,
    /// Trust-independent personas from inline, user, and bundled sources.
    #[serde(skip)]
    pub subagent_personas: std::collections::HashMap<String, crate::config::SubagentPersona>,
    /// Whether the runtime turn-end TodoGate is force-enabled via the
    /// `--todo-gate` CLI flag. Session-scoped — not persisted. When
    /// true, flips the runtime policy's `enabled` bit on regardless of
    /// remote settings or the built-in default (which is `false`).
    /// The gate runs only while a `/goal` is active (goal reminders
    /// inject `<task_completion_discipline>`); global built-in templates
    /// do not activate it.
    #[serde(skip)]
    pub todo_gate: bool,
    /// Path for the Layer-3 LazinessDetector debug log
    /// (`--laziness-debug-log`). When `Some`, the classifier fires
    /// after every turn (bypassing the idle wait, the per-model
    /// enable gate, and the nudge cap) and appends a JSONL line per
    /// fire to this file. Observation-only — no nudges are injected
    /// in this mode. Session-scoped, not persisted.
    #[serde(skip)]
    pub laziness_debug_log: Option<std::path::PathBuf>,
    /// Whether tools should respect `.gitignore` patterns.
    /// When `true`, all tools including `read_file` block gitignored files.
    /// When `false` (default), each tool applies its own default
    /// (`read_file` allows, others block).
    /// Resolved by [`crate::config::ToolsConfig::resolve`].
    #[serde(skip)]
    pub respect_gitignore: bool,
    /// Whether to enrich path-not-found errors with CWD reminders,
    /// "dropped repo folder" correction, and similar-name suggestions.
    /// Default `false`. Enabled via remote settings and retained in local
    /// session configuration for diagnostics.
    #[serde(default)]
    pub path_not_found_hints: bool,
    /// Whether to fetch managed MCP configs from the managed connectors service at startup.
    /// Resolved by [`crate::config::ManagedMcpsConfig::resolve`]: env var >
    /// config.toml > remote settings > default (off in headless, on in interactive).
    #[serde(skip)]
    pub managed_mcps_enabled: bool,
    #[serde(skip)]
    pub managed_mcp_gateway_tools_enabled: bool,
    /// Whether auto-wake is enabled: when a background task or subagent
    /// completes, immediately inject a synthetic prompt instead of waiting
    /// for the idle-gated notification drain.
    #[serde(skip)]
    pub auto_wake_enabled: bool,
    /// Resolved vendor-compat config (env → `[compat]` TOML → feature flag →
    /// default ON), built from `compat` + `remote_settings` in
    /// `resolve_runtime_fields`. Threaded into skills / rules / AGENTS.md
    /// discovery.
    #[serde(skip)]
    pub compat_resolved: CompatConfig,
    /// Enforced requirement pins from `requirements.toml`.
    #[serde(skip)]
    pub requirements: Requirements,
    /// Session title model. `None` means use the active model.
    #[serde(skip)]
    pub session_summary_model: Option<String>,
    /// Image description model. `None` keeps image reads on the active model.
    #[serde(skip)]
    pub image_description_model: Option<String>,
    /// Next-prompt suggestion model pin (`env > [models] prompt_suggestion >
    /// remote`), consumed catalog-guarded by `handle_suggest_prompt`; see
    /// `ModelOverrideConfig::resolve`.
    #[serde(skip)]
    pub prompt_suggest_model_pin: crate::config::PromptSuggestModelPin,
}
#[derive(Debug, Clone, Default)]
pub struct CliAgentOverrides {
    pub tools: Option<Vec<String>>,
    pub disallowed_tools: Option<Vec<String>>,
    pub permission_rules: Vec<workspace::permission::types::PermissionRule>,
    pub max_turns: Option<u32>,
    pub permission_mode: Option<agent::config::PermissionMode>,
}
impl CliAgentOverrides {
    /// Apply to the *main-session* agent, which the operator defines directly:
    /// the flags are authoritative, so they replace the agent's own fields.
    /// Spawned subagents instead layer these on top of an author's definition —
    /// see [`Self::apply_to_subagent_definition`].
    pub fn apply_to_definition(&self, def: &mut agent::config::AgentDefinition) {
        if let Some(ref tools) = self.tools {
            def.tools = tools.clone();
        }
        if let Some(ref dt) = self.disallowed_tools {
            def.disallowed_tools = dt.clone();
        }
        if let Some(ref pm) = self.permission_mode {
            def.permission_mode = pm.clone();
        }
    }
    /// Subagent variant of [`Self::apply_to_definition`]: records the flags as
    /// session-clamp state (see [`AgentDefinition::session_tools_allowlist`])
    /// instead of overwriting the agent author's own fields.
    pub fn apply_to_subagent_definition(&self, def: &mut agent::config::AgentDefinition) {
        def.session_tools_allowlist = self.tools.clone();
        def.session_tools_denylist = self.disallowed_tools.clone();
        if let Some(ref parent_mode) = self.permission_mode
            && def.plugin_name.is_none()
        {
            def.permission_mode =
                resolve_subagent_permission_mode(def.permission_mode.clone(), parent_mode);
        }
    }
    pub fn has_definition_overrides(&self) -> bool {
        self.tools.is_some() || self.disallowed_tools.is_some() || self.permission_mode.is_some()
    }
}
/// Parent bypassPermissions/acceptEdits/auto override the subagent's own mode
/// (spec); any other parent mode keeps it.
fn resolve_subagent_permission_mode(
    own: PermissionMode,
    parent: &PermissionMode,
) -> PermissionMode {
    match parent {
        PermissionMode::BypassPermissions | PermissionMode::AcceptEdits | PermissionMode::Auto => {
            parent.clone()
        }
        _ => own,
    }
}
pub use agent::config::AgentDefinition;
pub use agent::config::Effort;
pub use agent::config::PermissionMode;
pub use client_support::ui_config::{ContextualHints, UiConfig};
/// Configuration for selecting the agent definition.
///
/// Set in `config.toml` under `[agent]`:
///
/// ```toml
/// [agent]
/// # Use a named agent (looked up via discovery: .grow/agents/, ~/.grow/agents/, built-ins)
/// name = "my-custom-agent"
///
/// # OR: path to an agent definition file (.md with YAML frontmatter)
/// definition = "/path/to/my-agent.md"
/// ```
///
/// Priority (highest to lowest):
/// 1. ACP session-level `_meta.agentProfile`
/// 2. CLI `--agent-profile` flag
/// 3. `[agent]` config.toml section (this config)
/// 4. `GROW_AGENT` env var
/// 5. Default `grow-build` agent
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSelectionConfig {
    /// Name of a built-in or discovered agent definition.
    /// Looked up via `agent::discovery::by_name_in_cwd()`.
    /// Examples: "grow-build", "browser-use", or a custom agent name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Path to an agent definition file (.md with YAML frontmatter).
    /// When set, the agent is loaded from this file.
    /// Supports environment variable expansion (e.g., `$HOME/.grow/agents/my-agent.md`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition: Option<PathBuf>,
    /// Global system-prompt identity label. Per-model override wins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt_label: Option<String>,
}
/// Configuration for session behavior.
pub const DEFAULT_PERMISSION_PROMPT_TIMEOUT_SECS: u64 = 60;
pub const DEFAULT_NON_INTERACTIVE_PERMISSION_PROMPT_TIMEOUT_SECS: u64 = 10;

fn deserialize_positive_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = u64::deserialize(deserializer)?;
    if value == 0 {
        return Err(serde::de::Error::custom("value must be greater than 0"));
    }
    Ok(value)
}

fn is_default_permission_prompt_timeout(value: &u64) -> bool {
    *value == DEFAULT_PERMISSION_PROMPT_TIMEOUT_SECS
}

fn is_default_non_interactive_permission_prompt_timeout(value: &u64) -> bool {
    *value == DEFAULT_NON_INTERACTIVE_PERMISSION_PROMPT_TIMEOUT_SECS
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    /// Context window usage percentage (0-100) at which auto-compact is triggered.
    /// When the session's token usage exceeds this percentage of the model's context window,
    /// the conversation will be automatically summarized to free up space.
    ///
    /// `None` means "user didn't set it"; the resolver in
    /// `crate::util::config::resolve_auto_compact_threshold_percent` falls
    /// through to remote tiers and ultimately the hardcoded default 85.
    /// Read this field via the resolver — not directly — to honor the full
    /// precedence chain (env, per-model, remote, default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_compact_threshold_percent: Option<u8>,
    /// Whether to load environment variables from .envrc files.
    /// When enabled, the session will parse .envrc in the workspace directory
    /// and inject the environment variables into bash commands.
    /// Defaults to `true` when unset. `Option<bool>` so `None`
    /// round-trips as absent on disk (managed config wins over default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_envrc: Option<bool>,
    /// Maximum time to wait for an interactive permission prompt response.
    #[serde(
        deserialize_with = "deserialize_positive_u64",
        skip_serializing_if = "is_default_permission_prompt_timeout"
    )]
    pub permission_prompt_timeout_secs: u64,
    /// Maximum time to wait for a permission response in non-interactive sessions.
    #[serde(
        deserialize_with = "deserialize_positive_u64",
        skip_serializing_if = "is_default_non_interactive_permission_prompt_timeout"
    )]
    pub non_interactive_permission_prompt_timeout_secs: u64,
}

impl SessionConfig {
    pub fn permission_prompt_timeout(&self, non_interactive: bool) -> std::time::Duration {
        let seconds = if non_interactive {
            self.non_interactive_permission_prompt_timeout_secs
        } else {
            self.permission_prompt_timeout_secs
        };
        std::time::Duration::from_secs(seconds)
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            auto_compact_threshold_percent: None,
            load_envrc: None,
            permission_prompt_timeout_secs: DEFAULT_PERMISSION_PROMPT_TIMEOUT_SECS,
            non_interactive_permission_prompt_timeout_secs:
                DEFAULT_NON_INTERACTIVE_PERMISSION_PROMPT_TIMEOUT_SECS,
        }
    }
}
/// Configuration for change-archive deduplication.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RepoChangesDedupConfig {
    pub enabled: bool,
    /// Include inline content even when references exist.
    pub include_inline_fallback: bool,
    /// Omit inline content larger than this (0 = no limit).
    pub max_inline_bytes: usize,
    /// Deduplicate untracked file content.
    pub dedup_untracked: bool,
    /// Deduplicate binary file blobs.
    pub dedup_binary: bool,
    /// Skip untracked files larger than this (0 = no limit).
    pub untracked_max_bytes: usize,
    /// Optional glob patterns to exclude untracked paths.
    pub untracked_exclude_globs: Vec<String>,
}
impl RepoChangesDedupConfig {}
impl Default for RepoChangesDedupConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            include_inline_fallback: false,
            max_inline_bytes: 0,
            dedup_untracked: true,
            dedup_binary: true,
            untracked_max_bytes: 0,
            untracked_exclude_globs: Vec::new(),
        }
    }
}
impl Default for Config {
    fn default() -> Self {
        let endpoints = EndpointsConfig::default();
        let mut cfg = Self {
            features: Features::default(),
            goal: GoalConfig::default(),
            workflows: WorkflowsConfig::default(),
            doom_loop_recovery: crate::util::config::DoomLoopRecoverySettings::default(),
            worktree: WorktreeConfigSection::default(),
            auto_mode: AutoModeConfig::default(),
            config_models: IndexMap::new(),
            config_warnings: Vec::new(),
            auth_providers: IndexMap::new(),
            model_providers: IndexMap::new(),
            shortcuts: None,
            hints: None,
            ui: UiConfig::default(),
            toolset: ShellToolsetConfig::default(),
            shell_environment_policy: ShellEnvironmentPolicyKnownKeys::default(),
            endpoints,
            session: SessionConfig::default(),
            agent: AgentSelectionConfig::default(),
            repo_changes_dedup: RepoChangesDedupConfig::default(),
            skills: SkillsConfig::default(),
            compat: CompatConfigToml::default(),
            plugins: PluginsConfig::default(),
            paths: PathsConfig::default(),
            cli: CliConfig::default(),
            models: ModelsConfig::default(),
            remote: RemoteConfig::default(),
            worktree_pool: WorktreePoolConfig::default(),
            sandbox: SandboxSettingsConfig::default(),
            mcp_servers: std::collections::HashMap::new(),
            disabled_mcp_servers: Vec::new(),
            disabled_mcp_tools: std::collections::HashMap::new(),
            subagents: crate::config::SubagentsConfig::default(),
            memory: crate::config::MemoryConfig::default(),
            compaction: CompactionConfig::default(),
            managed_mcps: crate::config::ManagedMcpsConfig::default(),
            announcements: announcements::default_announcements(),
            tips: None,
            permission: PermissionKnownKeys::default(),
            tools: crate::config::ToolsConfig::default(),
            storage: StorageConfig::default(),
            suggestions: SuggestionsConfig::default(),
            marketplace: MarketplaceConfig::default(),
            diagnostics: DiagnosticsConfig::default(),
            default_model_override: None,
            reasoning_effort_override: None,
            session_summary_model_override: None,
            default_yolo_mode: false,
            default_auto_mode: false,
            agent_profile_path: None,
            client_version: Some(version::VERSION.to_string()),
            mode: AgentMode::default(),
            remote_settings: None,
            cli_agents: Vec::new(),
            cli_agent_overrides: CliAgentOverrides::default(),
            subagents_enabled: true,
            subagents_max_depth: crate::config::SubagentsConfig::DEFAULT_MAX_DEPTH,
            subagent_model_overrides: std::collections::HashMap::new(),
            subagent_toggle: std::collections::HashMap::new(),
            subagent_roles: std::collections::HashMap::new(),
            subagent_personas: std::collections::HashMap::new(),
            todo_gate: false,
            laziness_debug_log: None,
            respect_gitignore: false,
            path_not_found_hints: false,
            cli_experimental_memory: false,
            cli_no_memory: false,
            cli_subagents: None,
            memory_config: None,
            managed_mcps_enabled: true,
            managed_mcp_gateway_tools_enabled: false,
            auto_wake_enabled: true,
            compat_resolved: CompatConfig::default(),
            requirements: Requirements::default(),
            session_summary_model: None,
            image_description_model: None,
            prompt_suggest_model_pin: crate::config::PromptSuggestModelPin::Unpinned,
        };
        cfg.apply_env_overrides();
        cfg
    }
}
/// Config paths read by raw-layer resolvers, not [`Config`] serde fields, so
/// `serde_ignored` must not report them as unrecognized keys.
const NON_SERDE_CONFIG_PATHS: &[&str] = &[crate::util::config::SLASH_COMMAND_TAGS_CONFIG_PATH];
/// Parse `[auth_provider.<name>]` tables leniently: a malformed entry warns
/// (surfaced by `grow inspect`) and is skipped, so it fails closed for the
/// models referencing it instead of failing the whole config.
fn parse_auth_providers(
    raw_config: &toml::Value,
) -> (
    IndexMap<String, crate::auth::AuthProviderConfig>,
    Vec<super::config_model_override_parse::ConfigWarning>,
) {
    use super::config_model_override_parse::{ConfigWarning, ConfigWarningKind};
    let mut providers = IndexMap::new();
    let mut warnings = Vec::new();
    let Some(section) = raw_config.get("auth_provider") else {
        return (providers, warnings);
    };
    let Some(table) = section.as_table() else {
        warnings.push(ConfigWarning::auth_provider_section(
            ConfigWarningKind::NotATable,
            format!(
                "`auth_provider` must be a table of [auth_provider.<name>] entries, got {}; \
                 all auth providers ignored",
                section.type_str()
            ),
        ));
        return (providers, warnings);
    };
    for (name, value) in table {
        let mut unknown = Vec::new();
        match serde_ignored::deserialize::<_, _, crate::auth::AuthProviderConfig>(
            value.clone(),
            |path| unknown.push(path.to_string()),
        ) {
            Ok(provider) => {
                for key in unknown {
                    warnings.push(ConfigWarning::auth_provider(
                        name,
                        Some(key.as_str()),
                        ConfigWarningKind::UnknownField,
                        "unrecognized key; field ignored".to_owned(),
                    ));
                }
                for (field, kind, reason) in auth_config_issues(&provider) {
                    warnings.push(ConfigWarning::auth_provider(
                        name,
                        Some(field),
                        kind,
                        reason,
                    ));
                }
                providers.insert(name.clone(), provider);
            }
            Err(error) => {
                warnings.push(ConfigWarning::auth_provider(
                    name,
                    None,
                    ConfigWarningKind::InvalidValue,
                    format!(
                        "failed to parse ({error}); provider skipped, referencing models \
                         resolve with no credential"
                    ),
                ));
            }
        }
    }
    (providers, warnings)
}
impl Config {
    /// Reject invalid glob patterns in the model-filter lists at config load, so
    /// a typo fails loudly instead of silently changing availability.
    pub fn validate_model_filters(&self) -> Result<(), String> {
        for (field, list) in [
            ("allowed_models", &self.models.allowed_models),
            ("disabled_models", &self.models.disabled_models),
            ("hidden_models", &self.models.hidden_models),
        ] {
            if let Err(bad) = crate::agent::models::ModelGlobSet::compile(list.as_ref()) {
                return Err(format!(
                    "{field} has an invalid pattern: {}. Patterns use * and ? wildcards.",
                    bad.join(", ")
                ));
            }
        }
        Ok(())
    }
    /// Deserialize the merged `base` document, also returning the ignored key
    /// paths whose top-level key appears in `user_config`. Paths outside it
    /// can only come from the serialized-defaults half of the merge and must
    /// not be blamed on the user.
    fn deserialize_collecting_unrecognized(
        base: toml::Value,
        user_config: &toml::Value,
    ) -> Result<(Self, Vec<String>), String> {
        let mut unused_keys = Vec::new();
        let config: Self = serde_ignored::deserialize(base, |path| {
            unused_keys.push(path.to_string());
        })
        .map_err(|e| e.to_string())?;
        let unrecognized_keys = match user_config.as_table() {
            Some(user_table) => unused_keys
                .into_iter()
                .filter(|path| {
                    let top_level = path.split('.').next().unwrap_or(path);
                    user_table.contains_key(top_level)
                })
                .filter(|path| !NON_SERDE_CONFIG_PATHS.contains(&path.as_str()))
                .collect(),
            None => Vec::new(),
        };
        Ok((config, unrecognized_keys))
    }
    pub fn new_from_toml_cfg(raw_config: &toml::Value) -> Result<Self, String> {
        let normalized_model_config =
            super::model_providers::normalize_provider_config(raw_config)?;
        let super::config_model_override_parse::ParsedModelOverrides {
            models: config_models,
            warnings: config_warnings,
        } = super::config_model_override_parse::parse_model_overrides(&normalized_model_config);
        let (mut auth_providers, auth_provider_warnings) = parse_auth_providers(raw_config);
        let (model_providers, mut model_provider_warnings) =
            parse_model_providers(&normalized_model_config);
        for (id, provider) in &model_providers {
            if let Some(auth) = &provider.auth {
                let synthetic = model_provider_auth_name(id);
                if auth_providers.contains_key(&synthetic) {
                    model_provider_warnings
                        .push(
                            super::config_model_override_parse::ConfigWarning::model_provider(
                                id,
                                Some("auth"),
                                super::config_model_override_parse::ConfigWarningKind::ConflictingFields,
                                format!(
                                "inline auth overwrites a hand-written \
                                 [auth_provider.\"{synthetic}\"]; the `model_provider:` prefix is \
                                 a reserved namespace"
                            ),
                            ),
                        );
                }
                auth_providers.insert(synthetic, auth.clone());
            }
        }
        let mut base = toml::Value::try_from(Self::default()).map_err(|e| e.to_string())?;
        if let toml::Value::Table(ref mut t) = base {
            t.remove("model");
        }
        let mut raw_without_model_sections = raw_config.clone();
        if let toml::Value::Table(ref mut t) = raw_without_model_sections {
            t.remove("model");
            t.remove("auth_provider");
            t.remove("model_providers");
            t.remove("provider");
        }
        let parsed_mcp_servers =
            crate::util::config::parse_mcp_servers_from_toml(&raw_without_model_sections);
        if let toml::Value::Table(ref mut t) = raw_without_model_sections {
            t.remove("mcp_servers");
        }
        crate::config::deep_merge_toml(&mut base, &raw_without_model_sections);
        if let toml::Value::Table(ref mut t) = base {
            t.remove("mcp_servers");
        }
        let (mut config, mut unrecognized_keys) =
            Self::deserialize_collecting_unrecognized(base, &raw_without_model_sections)?;
        config.mcp_servers = parsed_mcp_servers.into_iter().collect();
        config.config_models = config_models;
        config.config_warnings = config_warnings;
        config.auth_providers = auth_providers;
        config.model_providers = model_providers;
        config.config_warnings.extend(auth_provider_warnings);
        config.config_warnings.extend(model_provider_warnings);
        unrecognized_keys.sort();
        for key in unrecognized_keys {
            config.config_warnings.push(
                super::config_model_override_parse::ConfigWarning::config_key(
                    key,
                    super::config_model_override_parse::ConfigWarningKind::UnknownField,
                    "unrecognized config key".to_owned(),
                ),
            );
        }
        let declared_provider_names: std::collections::HashSet<&str> = raw_config
            .get("auth_provider")
            .and_then(toml::Value::as_table)
            .map(|t| t.keys().map(String::as_str).collect())
            .unwrap_or_default();
        let declared_model_provider_names: std::collections::HashSet<&str> =
            normalized_model_config
                .get("model_providers")
                .and_then(toml::Value::as_table)
                .map(|t| t.keys().map(String::as_str).collect())
                .unwrap_or_default();
        for (model_key, model) in &config.config_models {
            if let Some(ref name) = model.auth_provider
                && !config.auth_providers.contains_key(name)
                && !declared_provider_names.contains(name.as_str())
            {
                config.config_warnings.push(
                    super::config_model_override_parse::ConfigWarning::model(
                        model_key,
                        Some("auth_provider"),
                        super::config_model_override_parse::ConfigWarningKind::InvalidValue,
                        format!(
                            "references [auth_provider.{name}], which is not defined; \
                             the model resolves with no provider credential"
                        ),
                    ),
                );
            }
            if let Some(ref id) = model.model_provider
                && !config.model_providers.contains_key(id)
                && !declared_model_provider_names.contains(id.as_str())
            {
                config.config_warnings.push(
                    super::config_model_override_parse::ConfigWarning::model(
                        model_key,
                        Some("model_provider"),
                        super::config_model_override_parse::ConfigWarningKind::InvalidValue,
                        format!(
                            "references [model_providers.{id}], which is not defined; \
                             provider defaults are not applied — the model uses its own \
                             credential if set, otherwise fails closed on a custom endpoint"
                        ),
                    ),
                );
            }
        }
        for (id, provider) in &config.model_providers {
            if let Some(ref name) = provider.auth_provider
                && !config.auth_providers.contains_key(name)
                && !declared_provider_names.contains(name.as_str())
            {
                config.config_warnings.push(
                    super::config_model_override_parse::ConfigWarning::model_provider(
                        id,
                        Some("auth_provider"),
                        super::config_model_override_parse::ConfigWarningKind::InvalidValue,
                        format!(
                            "references [auth_provider.{name}], which is not defined; \
                             inheriting models fail closed with no provider credential"
                        ),
                    ),
                );
            }
        }
        super::config_model_override_parse::log_config_warnings(&config.config_warnings);
        if config.client_version.is_none() {
            config.client_version = Self::default().client_version;
        }
        let model_overrides = crate::config::ModelOverrideConfig::resolve(None, raw_config, None);
        config.session_summary_model = model_overrides.session_summary;
        config.image_description_model = model_overrides.image_description;
        config.prompt_suggest_model_pin = model_overrides.prompt_suggestion;
        config.apply_env_overrides();
        Ok(config)
    }

    /// Validate the inference configuration before any authentication,
    /// catalog fetch, or ACP connection is attempted.
    pub fn validate_llm_configuration(&self) -> Result<(), String> {
        if self.config_models.is_empty() {
            return Err(
                "no LLM is configured; add at least one [provider.<id>.models.<model>] entry"
                    .to_owned(),
            );
        }
        let default = self
            .models
            .default
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                "no global default model is configured; set [models] default = \"provider/model\""
                    .to_owned()
            })?;
        if !self.config_models.contains_key(default) {
            return Err(format!(
                "global default model `{default}` does not exist; expected a configured provider/model id"
            ));
        }
        for (id, model) in resolve_model_list(self) {
            if model.info.base_url.trim().is_empty() {
                return Err(format!(
                    "model `{id}` has no base_url; set it under [provider.{}.options]",
                    id.split('/').next().unwrap_or("<id>")
                ));
            }
        }
        Ok(())
    }
    /// Populate trust-independent `#[serde(skip)]` subagent base fields.
    ///
    /// Must be called after `new_from_toml_cfg` on the **primary startup path**
    /// before the config is handed to `MvpAgent`. Project definitions are overlaid
    /// per cwd after that cwd's authoritative folder-trust resolve.
    pub fn resolve_subagents(&mut self, cli_flag: bool, raw_config: &toml::Value) {
        let sa = crate::config::SubagentsConfig::resolve(cli_flag, raw_config);
        self.subagents_enabled = sa.enabled;
        self.subagent_model_overrides = sa.models;
        self.subagent_toggle = sa.toggle;
        self.subagent_roles = sa.roles;
        self.subagent_personas = sa.personas;
        let env = std::env::var(crate::config::SubagentsConfig::ENV_MAX_DEPTH).ok();
        let remote = self
            .remote_settings
            .as_ref()
            .and_then(|r| r.subagents_max_depth);
        self.subagents_max_depth =
            crate::config::SubagentsConfig::resolve_max_depth(env.as_deref(), sa.max_depth, remote);
    }
    /// Resolve all `#[serde(skip)]` runtime fields that have resolver functions.
    ///
    /// Call immediately after `new_from_toml_cfg()`. Fields resolved:
    /// - subagents base layers (6 fields) via `SubagentsConfig::resolve`
    /// - respect_gitignore via `ToolsConfig::resolve`
    /// - managed_mcps_enabled via `ManagedMcpsConfig::resolve`
    /// - session_summary_model / image_description_model /
    ///   prompt_suggest_model_pin via `ModelOverrideConfig::resolve`
    /// - memory_config via `MemoryConfig::resolve`
    /// - path_not_found_hints from remote_settings
    ///
    /// Note: `worktree_type` is resolved directly in `MvpAgent::new` via
    /// `resolve_worktree_type` since it's an agent-level field, not a Config field.
    pub fn resolve_runtime_fields(&mut self, ctx: &RuntimeResolutionContext<'_>) {
        self.cli_subagents = ctx.cli_subagents;
        self.session_summary_model_override = ctx.cli_session_summary_model.map(|s| s.to_owned());
        let cli_flag = ctx.cli_subagents.unwrap_or(false);
        self.resolve_subagents(cli_flag, ctx.raw_config);
        let env = std::env::var(crate::config::SubagentsConfig::ENV_MAX_DEPTH).ok();
        let toml_max = ctx
            .raw_config
            .get("subagents")
            .and_then(|s| s.get("max_depth"))
            .and_then(|v| v.as_integer());
        let remote = ctx.remote_settings.and_then(|r| r.subagents_max_depth);
        self.subagents_max_depth =
            crate::config::SubagentsConfig::resolve_max_depth(env.as_deref(), toml_max, remote);
        let tools = crate::config::ToolsConfig::resolve(ctx.raw_config);
        self.respect_gitignore = match self.requirements.respect_gitignore.pinned() {
            Some(pinned) => pinned,
            None => tools.respect_gitignore,
        };
        let mcps = crate::config::ManagedMcpsConfig::resolve(
            ctx.raw_config,
            ctx.remote_settings,
            ctx.is_headless,
        );
        self.managed_mcps_enabled = mcps.enabled;
        self.managed_mcp_gateway_tools_enabled = mcps.gateway_tools_enabled;
        let models = crate::config::ModelOverrideConfig::resolve(
            ctx.cli_session_summary_model,
            ctx.raw_config,
            ctx.remote_settings,
        );
        self.session_summary_model = models.session_summary;
        self.image_description_model = models.image_description;
        self.prompt_suggest_model_pin = models.prompt_suggestion;
        self.cli_experimental_memory = ctx.cli_experimental_memory;
        self.cli_no_memory = ctx.cli_no_memory;
        let mem = crate::config::MemoryConfig::resolve(
            ctx.cli_experimental_memory,
            ctx.cli_no_memory,
            ctx.raw_config,
            ctx.remote_settings,
        );
        self.memory_config = if mem.enabled { Some(mem) } else { None };
        self.todo_gate = ctx.todo_gate;
        self.laziness_debug_log = ctx.laziness_debug_log.map(std::path::Path::to_path_buf);
        if let Some(v) = ctx.remote_settings.and_then(|s| s.path_not_found_hints) {
            self.path_not_found_hints = v;
        }
        self.auto_wake_enabled = BoolFlag::env("GROW_AUTO_WAKE")
            .config(self.features.auto_wake)
            .feature_flag(ctx.remote_settings.and_then(|r| r.auto_wake_enabled))
            .default(true)
            .resolve()
            .value;
        self.compat_resolved = resolve_compat_config(&self.compat, ctx.remote_settings);
    }
    /// Re-resolve eagerly-resolved runtime fields using the current `Config`
    /// state and fresh `raw_config`. Builds a [`RuntimeResolutionContext`] from
    /// the CLI flags already stored on this `Config`.
    ///
    /// Integration test coverage: `tests/test_settings_refresh.rs`.
    pub fn re_resolve_runtime_fields(&mut self, raw_config: &toml::Value) {
        let remote_settings = self.remote_settings.clone();
        let cli_session_summary_model = self.session_summary_model_override.clone();
        let laziness_debug_log = self.laziness_debug_log.clone();
        let ctx = RuntimeResolutionContext {
            raw_config,
            remote_settings: remote_settings.as_ref(),
            is_headless: self.mode == AgentMode::Headless,
            cli_subagents: self.cli_subagents,
            cli_session_summary_model: cli_session_summary_model.as_deref(),
            cli_experimental_memory: self.cli_experimental_memory,
            cli_no_memory: self.cli_no_memory,
            todo_gate: self.todo_gate,
            laziness_debug_log: laziness_debug_log.as_deref(),
        };
        self.resolve_runtime_fields(&ctx);
        crate::util::config::set_remote_campaigns_from_settings(self.remote_settings.as_ref());
    }
    fn apply_env_overrides(&mut self) {}
    pub fn is_session_recap_enabled(&self) -> bool {
        self.resolve_session_recap().value
    }
    /// Two-pass (prefire) compaction gate. Default OFF (opt-in) — enable via
    /// remote settings `two_pass_compaction_enabled`, the `[features] two_pass_compaction`
    /// config.toml key, or `GROW_TWO_PASS_COMPACTION` env.
    pub fn is_two_pass_compaction_enabled(&self) -> bool {
        self.resolve_two_pass_compaction().value
    }
    pub(crate) fn resolve_two_pass_compaction(&self) -> Resolved<bool> {
        let ff = self
            .remote_settings
            .as_ref()
            .and_then(|s| s.two_pass_compaction_enabled);
        BoolFlag::env("GROW_TWO_PASS_COMPACTION")
            .config(self.features.two_pass_compaction)
            .feature_flag(ff)
            .default(false)
            .resolve()
    }
    /// Server-side doom-loop check policy (the `x-grow-doom-loop-check`
    /// header, trigger parsing, and confident-signal resampling, all
    /// applied by the sampler). Merged
    /// PER-FIELD across the `[doom_loop_recovery]` TOML table and the
    /// remote settings `doom_loop_recovery` object (a partial remote object only
    /// overrides the fields it sets). Gate precedence: env
    /// `GROW_DOOM_LOOP_RECOVERY` > TOML `enabled` > remote `enabled` >
    /// default ON — each layer's `false` is an independent kill switch, and
    /// `None` IS the off state, so disabled has exactly one spelling.
    /// Tunables have no env layer (TOML > remote > default) and are clamped
    /// to their documented ranges. Returns the composite runtime policy
    /// rather than `Resolved` because each knob resolves from its own
    /// source (the `resolve_reminder_policy` pattern).
    pub(crate) fn resolve_doom_loop_recovery(
        &self,
    ) -> Option<sampling_types::DoomLoopRecoveryPolicy> {
        use sampling_types::DoomLoopRecoveryPolicy as Policy;
        let remote = self
            .remote_settings
            .as_ref()
            .and_then(|s| s.doom_loop_recovery.as_ref());
        let enabled = BoolFlag::env("GROW_DOOM_LOOP_RECOVERY")
            .config(self.doom_loop_recovery.enabled)
            .feature_flag(remote.and_then(|s| s.enabled))
            .default(true)
            .resolve()
            .value;
        enabled.then(|| Policy {
            max_threshold: self
                .doom_loop_recovery
                .max_threshold
                .or(remote.and_then(|s| s.max_threshold))
                .map_or(Policy::DEFAULT_MAX_THRESHOLD, Policy::clamp_max_threshold),
            max_retries: self
                .doom_loop_recovery
                .max_retries
                .or(remote.and_then(|s| s.max_retries))
                .map_or(Policy::DEFAULT_MAX_RETRIES, Policy::clamp_max_retries),
        })
    }
    /// Automatic worktree GC policy. Precedence: env kill/dry-run >
    /// `[worktree.auto_gc]` TOML > remote `worktree_auto_gc` > defaults.
    /// Platform age-expiry (non-Linux dead-only) is enforced inside
    /// `fast_worktree::maybe_auto_gc`, not here.
    pub fn resolve_worktree_auto_gc(&self) -> fast_worktree::ResolvedWorktreeAutoGc {
        crate::util::config::resolve_worktree_auto_gc_from_settings(
            Some(&self.worktree.auto_gc),
            self.remote_settings
                .as_ref()
                .and_then(|s| s.worktree_auto_gc.as_ref()),
        )
    }
    pub(crate) fn resolve_lsp_tools(&self) -> Resolved<bool> {
        let ff = self
            .remote_settings
            .as_ref()
            .and_then(|s| s.lsp_tools_enabled);
        BoolFlag::env("GROW_LSP_TOOLS")
            .requirement(self.requirements.lsp_tools.pinned())
            .config(self.features.lsp_tools)
            .feature_flag(ff)
            .resolve()
    }
    pub(crate) fn resolve_web_fetch(&self) -> Resolved<bool> {
        let ff = self
            .remote_settings
            .as_ref()
            .and_then(|s| s.web_fetch_enabled);
        BoolFlag::env("GROW_WEB_FETCH")
            .requirement(self.requirements.web_fetch.pinned())
            .config(self.features.web_fetch)
            .feature_flag(ff)
            .resolve()
    }
    /// `ask_user_question` tool gate; default ON. remote settings
    /// `ask_user_question_enabled: false` (or `[features]` / env) is a remote
    /// kill-switch. The `_meta.askUserQuestion` override (`--no-ask-user`) is
    /// applied at the spawn site and outranks this resolver.
    pub(crate) fn resolve_ask_user_question(&self) -> Resolved<bool> {
        let ff = self
            .remote_settings
            .as_ref()
            .and_then(|s| s.ask_user_question_enabled);
        BoolFlag::env("GROW_ASK_USER_QUESTION")
            .requirement(self.requirements.ask_user_question.pinned())
            .config(self.features.ask_user_question)
            .feature_flag(ff)
            .default(true)
            .resolve()
    }
    /// Session recap gate (the `/recap` command + automatic return-from-away
    /// recap). Default ON — disable via remote settings `session_recap`, the
    /// `[features] session_recap` config.toml key, or `GROW_SESSION_RECAP` env.
    pub(crate) fn resolve_session_recap(&self) -> Resolved<bool> {
        let ff = self.remote_settings.as_ref().and_then(|s| s.session_recap);
        BoolFlag::env("GROW_SESSION_RECAP")
            .config(self.features.session_recap)
            .feature_flag(ff)
            .default(true)
            .resolve()
    }
    /// Goal mode (`/goal`) master switch. Default ON; an explicitly supplied
    /// remote setting may still disable it for a managed deployment.
    pub(crate) fn resolve_goal(&self) -> Resolved<bool> {
        let ff = self.remote_settings.as_ref().and_then(|s| s.goal_enabled);
        if ff == Some(false) {
            return Resolved::new(false, ConfigSource::Remote);
        }
        BoolFlag::env("GROW_GOAL")
            .config(self.goal.enabled)
            .feature_flag(ff)
            .default(true)
            .resolve()
    }
    /// Background workflows (`workflow` tool, `.grow/workflows/*.rhai`,
    /// `/deep-research`, host-owned `/goal` driver). Default ON: deployments
    /// that never receive remote settings still get workflows; `Some(false)`
    /// remote / config / env remains a kill-switch.
    pub(crate) fn resolve_workflows(&self) -> Resolved<bool> {
        let ff = self
            .remote_settings
            .as_ref()
            .and_then(|s| s.workflows_enabled);
        if ff == Some(false) {
            return Resolved::new(false, ConfigSource::Remote);
        }
        BoolFlag::env("GROW_WORKFLOWS")
            .config(self.workflows.enabled)
            .feature_flag(ff)
            .default(true)
            .resolve()
    }
    /// Classifier, planner, and summary all default to goal mode itself: when
    /// `/goal` is on they are on unless config/env/remote says otherwise.
    /// `goal_enabled` is the session's already-resolved master switch (the same
    /// value the actor stores), passed in so a sub-role default can never
    /// disagree with whether `/goal` is on.
    pub(crate) fn resolve_goal_classifier_enabled(&self, goal_enabled: bool) -> Resolved<bool> {
        BoolFlag::env("GROW_GOAL_CLASSIFIER")
            .config(self.goal.classifier_enabled)
            .feature_flag(
                self.remote_settings
                    .as_ref()
                    .and_then(|s| s.goal_classifier_enabled),
            )
            .default(goal_enabled)
            .resolve()
    }
    pub(crate) fn resolve_goal_planner_enabled(&self, goal_enabled: bool) -> Resolved<bool> {
        BoolFlag::env("GROW_GOAL_PLANNER")
            .config(self.goal.planner_enabled)
            .feature_flag(
                self.remote_settings
                    .as_ref()
                    .and_then(|s| s.goal_planner_enabled),
            )
            .default(goal_enabled)
            .resolve()
    }
    pub(crate) fn resolve_goal_summary_enabled(&self, goal_enabled: bool) -> Resolved<bool> {
        BoolFlag::env("GROW_GOAL_SUMMARY")
            .config(self.goal.summary_enabled)
            .feature_flag(
                self.remote_settings
                    .as_ref()
                    .and_then(|s| s.goal_summary_enabled),
            )
            .default(goal_enabled)
            .resolve()
    }
    /// Goal count resolver: env(parse) > config > remote > default, then clamp.
    /// An unparseable env value falls through to the next source.
    fn resolve_goal_u32(
        env_var: &str,
        config: Option<u32>,
        remote: Option<u32>,
        default: u32,
        clamp: impl Fn(u32) -> u32,
    ) -> Resolved<u32> {
        if let Some(env_value) = env_string(env_var)
            && let Ok(parsed) = env_value.parse::<u32>()
        {
            return Resolved::new(clamp(parsed), ConfigSource::Env);
        }
        if let Some(v) = config {
            return Resolved::new(clamp(v), ConfigSource::Config);
        }
        if let Some(v) = remote {
            return Resolved::new(clamp(v), ConfigSource::Remote);
        }
        Resolved::new(default, ConfigSource::Default)
    }
    /// Per-attempt adversarial-skeptic count, clamped to
    /// `[GOAL_VERIFIER_SKEPTIC_MIN, GOAL_VERIFIER_SKEPTIC_MAX]`.
    pub(crate) fn resolve_goal_verifier_count(&self) -> Resolved<u32> {
        use crate::session::goal_classifier::{
            GOAL_VERIFIER_SKEPTIC_COUNT, GOAL_VERIFIER_SKEPTIC_MAX, GOAL_VERIFIER_SKEPTIC_MIN,
        };
        Self::resolve_goal_u32(
            "GROW_GOAL_VERIFIER_N",
            self.goal.verifier_count,
            self.remote_settings
                .as_ref()
                .and_then(|s| s.goal_verifier_count),
            GOAL_VERIFIER_SKEPTIC_COUNT,
            |v| v.clamp(GOAL_VERIFIER_SKEPTIC_MIN, GOAL_VERIFIER_SKEPTIC_MAX),
        )
    }
    /// Per-goal classifier run cap, floored at `GOAL_CLASSIFIER_MAX_RUNS_MIN`
    /// with no upper ceiling.
    pub(crate) fn resolve_goal_classifier_max_runs(&self) -> Resolved<u32> {
        use crate::session::goal_classifier::{
            GOAL_CLASSIFIER_MAX_RUNS_DEFAULT, GOAL_CLASSIFIER_MAX_RUNS_MIN,
        };
        Self::resolve_goal_u32(
            "GROW_GOAL_CLASSIFIER_MAX",
            self.goal.classifier_max_runs,
            self.remote_settings
                .as_ref()
                .and_then(|s| s.goal_classifier_max_runs),
            GOAL_CLASSIFIER_MAX_RUNS_DEFAULT,
            |v| v.max(GOAL_CLASSIFIER_MAX_RUNS_MIN),
        )
    }
    /// Stall-triggered strategist cadence N (fires every N consecutive
    /// `NotAchieved`). Default tracks the resolved classifier cap
    /// (`max(1, cap / 2)`); floored at 1 so it can never silently disable.
    pub(crate) fn resolve_goal_strategist_every(&self, classifier_max_runs: u32) -> Resolved<u32> {
        Self::resolve_goal_u32(
            "GROW_GOAL_STRATEGIST_EVERY",
            self.goal.strategist_every,
            self.remote_settings
                .as_ref()
                .and_then(|s| s.goal_strategist_every),
            (classifier_max_runs / 2).max(1),
            |v| v.max(1),
        )
    }
    /// Re-verify escalation threshold; floored at 1. No remote layer.
    pub(crate) fn resolve_goal_reverify_after(&self) -> Resolved<u32> {
        Self::resolve_goal_u32(
            "GROW_GOAL_REVERIFY_AFTER",
            self.goal.reverify_after,
            None,
            crate::session::acp_session::GOAL_REVERIFY_AFTER_DEFAULT,
            |v| v.max(1),
        )
    }
    /// When `true`, every `/goal` role inherits the current model regardless of
    /// configured pairs.
    pub(crate) fn resolve_goal_use_current_model_only(&self) -> Resolved<bool> {
        BoolFlag::env("GROW_GOAL_USE_CURRENT_MODEL_ONLY")
            .config(self.goal.use_current_model_only)
            .default(false)
            .resolve()
    }
    /// Shared single-pair resolution. Precedence: kill-switch ⇒
    /// `InheritCurrent`/`Config` > `config_pair` ⇒ `Explicit`/`Config` >
    /// `remote_pair` ⇒ `Explicit`/`Remote` > `InheritCurrent`/`Default`. The
    /// chosen pair is cloned only on its branch.
    fn resolve_single_role_model(
        use_current_only: bool,
        config_pair: Option<&crate::util::config::GoalRoleModel>,
        remote_pair: Option<&crate::util::config::GoalRoleModel>,
    ) -> Resolved<GoalRoleModelChoice> {
        if use_current_only {
            return Resolved::new(GoalRoleModelChoice::InheritCurrent, ConfigSource::Config);
        }
        if let Some(pair) = config_pair {
            return Resolved::new(
                GoalRoleModelChoice::Explicit(pair.clone()),
                ConfigSource::Config,
            );
        }
        match remote_pair {
            Some(pair) => Resolved::new(
                GoalRoleModelChoice::Explicit(pair.clone()),
                ConfigSource::Remote,
            ),
            None => Resolved::new(GoalRoleModelChoice::InheritCurrent, ConfigSource::Default),
        }
    }
    /// Planner role model: `[goal]` config then remote. No env layer (only the
    /// kill-switch reads env).
    ///
    /// An `Explicit` pair is applied as `runtime_overrides.model`, resolved before
    /// `resolve_subagent_sampling_config`, so it wins over a user
    /// `[subagents.models]` pin; `InheritCurrent` hands precedence back to that pin.
    pub(crate) fn resolve_goal_planner_model(
        &self,
        use_current_only: bool,
    ) -> Resolved<GoalRoleModelChoice> {
        Self::resolve_single_role_model(
            use_current_only,
            self.goal.planner_model.as_ref(),
            self.remote_settings
                .as_ref()
                .and_then(|s| s.goal_planner_model.as_ref()),
        )
    }
    /// Strategist role model; same precedence as [`Self::resolve_goal_planner_model`].
    pub(crate) fn resolve_goal_strategist_model(
        &self,
        use_current_only: bool,
    ) -> Resolved<GoalRoleModelChoice> {
        Self::resolve_single_role_model(
            use_current_only,
            self.goal.strategist_model.as_ref(),
            self.remote_settings
                .as_ref()
                .and_then(|s| s.goal_strategist_model.as_ref()),
        )
    }
    /// Skeptic pool; same precedence as [`Self::resolve_goal_planner_model`] but
    /// over a pool. Pool order is preserved for the round-robin expansion in
    /// `expand_skeptic_assignment`.
    pub(crate) fn resolve_goal_skeptic_models(
        &self,
        use_current_only: bool,
    ) -> Resolved<Vec<GoalRoleModelChoice>> {
        if use_current_only {
            return Resolved::new(Vec::new(), ConfigSource::Config);
        }
        let to_choices = |pool: &[crate::util::config::GoalRoleModel]| {
            pool.iter()
                .cloned()
                .map(GoalRoleModelChoice::Explicit)
                .collect::<Vec<_>>()
        };
        if !self.goal.skeptic_models.is_empty() {
            return Resolved::new(to_choices(&self.goal.skeptic_models), ConfigSource::Config);
        }
        match self
            .remote_settings
            .as_ref()
            .map(|s| s.goal_skeptic_models.as_slice())
        {
            Some(pool) if !pool.is_empty() => Resolved::new(to_choices(pool), ConfigSource::Remote),
            _ => Resolved::new(Vec::new(), ConfigSource::Default),
        }
    }
    pub(crate) fn resolve_write_file(&self) -> Resolved<bool> {
        let ff = self
            .remote_settings
            .as_ref()
            .and_then(|s| s.write_file_enabled);
        BoolFlag::env("GROW_WRITE_FILE")
            .requirement(self.requirements.write_file.pinned())
            .config(self.features.write_file)
            .feature_flag(ff)
            .default(true)
            .resolve()
    }
    /// Resolve the mode (env `GROW_COMPACTION_MODE` > config > remote settings >
    /// default, unrecognized falling through) and, for `Segments`, attach the
    /// separately-resolved detail level.
    pub(crate) fn resolve_compaction_mode(&self) -> chat_state::CompactionMode {
        resolve_compaction_mode_from(
            env_string("GROW_COMPACTION_MODE").as_deref(),
            self.features.compaction_mode.as_deref(),
            self.remote_settings
                .as_ref()
                .and_then(|r| r.compaction_mode.as_deref()),
        )
        .with_segment_detail(self.resolve_compaction_detail())
    }
    /// Resolve verbatim-input flag: env `GROW_COMPACTION_VERBATIM_INPUT` > config > remote settings > default `true`.
    pub(crate) fn resolve_compaction_verbatim_input(&self) -> bool {
        BoolFlag::env("GROW_COMPACTION_VERBATIM_INPUT")
            .config(self.features.compaction_verbatim_input)
            .feature_flag(
                self.remote_settings
                    .as_ref()
                    .and_then(|r| r.compaction_verbatim_input),
            )
            .default(true)
            .resolve()
            .value
    }
    pub(crate) fn resolve_compaction_tool_choice(
        &self,
    ) -> crate::util::config::CompactionToolChoice {
        crate::util::config::resolve_compaction_tool_choice_from(
            env_string(crate::util::config::ENV_COMPACTION_TOOL_CHOICE).as_deref(),
            self.features.compaction_tool_choice.as_deref(),
            self.remote_settings
                .as_ref()
                .and_then(|r| r.compaction_tool_choice.as_deref()),
        )
    }
    /// Precedence: env `GROW_COMPACTION_DETAIL`, then config
    /// `features.compaction_detail`, then remote settings
    /// `remote_settings.compaction_detail`, then default (`verbose`). Drives the
    /// `segments` verbatim detail level.
    fn resolve_compaction_detail(&self) -> chat_state::CompactionDetail {
        resolve_compaction_detail_from(
            env_string("GROW_COMPACTION_DETAIL").as_deref(),
            self.features.compaction_detail.as_deref(),
            self.remote_settings
                .as_ref()
                .and_then(|r| r.compaction_detail.as_deref()),
        )
    }
    pub fn resolve_cancel_rewind(&self) -> Resolved<bool> {
        let ff = self
            .remote_settings
            .as_ref()
            .and_then(|s| s.cancel_rewind_enabled);
        BoolFlag::env("GROW_CANCEL_REWIND")
            .config(self.features.cancel_rewind)
            .feature_flag(ff)
            .default(true)
            .resolve()
    }
    /// Resolve whether to spawn the per-`Ready`-client transport
    /// liveness pollers and the session-actor `StatusDispatcher`.
    ///
    /// Thin delegate to the canonical
    /// [`resolve_mcp_liveness_watchers`] free function, which unifies
    /// the two previous implementations so they can't drift. CLI / managed / feature-flag inputs are
    /// `None` here because the `Config` method only has visibility
    /// into the embedded `Features` table; richer call sites (e.g.
    /// the session-actor spawn path) go through
    /// [`crate::util::config::resolve_mcp_liveness_watchers`] which
    /// stacks all 7 layers.
    pub fn resolve_mcp_liveness_watchers(&self) -> Resolved<bool> {
        resolve_mcp_liveness_watchers(None, None, self.features.mcp_liveness_watchers, None, None)
    }
    /// Resolve whether the bounded stdio auto-restart task is allowed
    /// to fire. Thin delegate to
    /// [`resolve_mcp_auto_restart`]; mirrors
    /// [`Self::resolve_mcp_liveness_watchers`]. The 7-step precedence
    /// stack lives in the canonical free function. CLI / managed /
    /// feature-flag inputs are `None` here because the `Config`
    /// method only has visibility into the embedded `Features`
    /// table; richer call sites go through
    /// [`crate::util::config::resolve_mcp_auto_restart`] which stacks
    /// all 7 layers.
    pub fn resolve_mcp_auto_restart(&self) -> Resolved<bool> {
        resolve_mcp_auto_restart(None, None, self.features.mcp_auto_restart, None, None)
    }
    /// Resolve whether the pager subscribes to the per-server
    /// `grow/mcp/server_status` push.
    ///
    /// Thin delegate to the canonical
    /// [`resolve_mcp_push_server_status`] free function — mirrors the
    /// `resolve_mcp_liveness_watchers` pattern so the two
    /// implementations can't drift. CLI / managed / feature-flag
    /// inputs are `None` here because the `Config` method only has
    /// visibility into the embedded `Features` table; richer call
    /// sites go through
    /// [`crate::util::config::resolve_mcp_push_server_status`] which
    /// stacks all 7 layers.
    pub fn resolve_mcp_push_server_status(&self) -> Resolved<bool> {
        resolve_mcp_push_server_status(None, None, self.features.mcp_push_server_status, None, None)
    }
    /// Resolve whether the leader's `ConfigFileWatcher` adds the two
    /// narrow non-recursive watches for `<cwd>/` and `<cwd>/.grow/`.
    ///
    /// Thin delegate to the canonical
    /// [`resolve_mcp_recursive_config_watch`] free function — mirrors
    /// the same delegation pattern. CLI / managed /
    /// feature-flag inputs are `None` here because the `Config`
    /// method only sees the embedded `Features` table; richer call
    /// sites (notably the leader's watcher spawn path) go through
    /// [`crate::util::config::resolve_mcp_recursive_config_watch`]
    /// which stacks all 7 layers.
    pub fn resolve_mcp_recursive_config_watch(&self) -> Resolved<bool> {
        resolve_mcp_recursive_config_watch(
            None,
            None,
            self.features.mcp_recursive_config_watch,
            None,
            None,
        )
    }
}
/// Canonical resolver for `mcp.liveness_watchers`. Stacks the full
/// 7-step `BoolFlag` precedence:
///
/// `requirement > cli > env (GROW_MCP_LIVENESS_WATCHERS) > config >
/// managed > feature_flag > default (true)`.
///
/// Both `Config::resolve_mcp_liveness_watchers` and
/// `util::config::resolve_mcp_liveness_watchers` delegate here so the
/// precedence is single-sourced.
///
/// The default is `true` — it gates the watcher + dispatcher
/// default-on, with this flag existing primarily as a kill switch
/// during the rollout.
pub fn resolve_mcp_liveness_watchers(
    requirement: Option<bool>,
    cli: Option<bool>,
    config: Option<bool>,
    managed: Option<bool>,
    feature_flag: Option<bool>,
) -> Resolved<bool> {
    BoolFlag::env("GROW_MCP_LIVENESS_WATCHERS")
        .requirement(requirement)
        .cli(cli)
        .config(config)
        .managed(managed)
        .feature_flag(feature_flag)
        .default(true)
        .resolve()
}
/// Canonical resolver for `mcp.auto_restart`. Stacks the full 7-step
/// `BoolFlag` precedence:
///
/// `requirement > cli > env (GROW_MCP_AUTO_RESTART) > config >
/// managed > feature_flag > default (true)`.
///
/// Mirrors [`resolve_mcp_liveness_watchers`]. Both
/// `Config::resolve_mcp_auto_restart` and
/// `util::config::resolve_mcp_auto_restart` delegate here so the
/// precedence is single-sourced.
///
/// Recovery is on by default; opt out via `GROW_MCP_AUTO_RESTART=false`,
/// `[features] mcp_auto_restart`, or `requirements.toml`.
pub fn resolve_mcp_auto_restart(
    requirement: Option<bool>,
    cli: Option<bool>,
    config: Option<bool>,
    managed: Option<bool>,
    feature_flag: Option<bool>,
) -> Resolved<bool> {
    BoolFlag::env("GROW_MCP_AUTO_RESTART")
        .requirement(requirement)
        .cli(cli)
        .config(config)
        .managed(managed)
        .feature_flag(feature_flag)
        .default(true)
        .resolve()
}
/// Canonical resolver for `mcp.push_server_status`. Stacks the same
/// 7-step `BoolFlag` precedence as
/// [`resolve_mcp_liveness_watchers`]:
///
/// `requirement > cli > env (GROW_MCP_PUSH_SERVER_STATUS) > config >
/// managed > feature_flag > default (true)`.
///
/// Both `Config::resolve_mcp_push_server_status` and
/// `util::config::resolve_mcp_push_server_status` delegate here so
/// the precedence is single-sourced.
///
/// The default is `true` — the pager's subscription to
/// `grow/mcp/server_status` is wired default-on, with this
/// flag existing primarily as a kill switch.
pub fn resolve_mcp_push_server_status(
    requirement: Option<bool>,
    cli: Option<bool>,
    config: Option<bool>,
    managed: Option<bool>,
    feature_flag: Option<bool>,
) -> Resolved<bool> {
    BoolFlag::env("GROW_MCP_PUSH_SERVER_STATUS")
        .requirement(requirement)
        .cli(cli)
        .config(config)
        .managed(managed)
        .feature_flag(feature_flag)
        .default(true)
        .resolve()
}
/// Canonical resolver for `mcp.recursive_config_watch`. Stacks the
/// same 7-step `BoolFlag` precedence as
/// [`resolve_mcp_liveness_watchers`]:
///
/// `requirement > cli > env (GROW_MCP_RECURSIVE_CONFIG_WATCH) >
/// config > managed > feature_flag > default (true)`.
///
/// Both `Config::resolve_mcp_recursive_config_watch` and
/// `util::config::resolve_mcp_recursive_config_watch` delegate here
/// so the precedence is single-sourced.
///
/// The default is `true`. It enables the two narrow
/// non-recursive cwd watches default-on. The flag exists primarily
/// as a kill switch during the rollout: if the FSEvents flakiness
/// on macOS or an inotify-quota issue on Linux causes a regression,
/// operators flip this flag (e.g. via `GROW_MCP_RECURSIVE_CONFIG_
/// WATCH=0`) and the leader falls back to the prior behavior (no cwd
/// watches; user-triggered refresh is the only project-config
/// reload path).
///
/// Note the **name is a slight misnomer**: the watches themselves
/// are non-recursive (by design, to avoid blowing through
/// `fs.inotify.max_user_watches` on large repos). The flag name
/// follows the rollout-gate naming convention.
pub fn resolve_mcp_recursive_config_watch(
    requirement: Option<bool>,
    cli: Option<bool>,
    config: Option<bool>,
    managed: Option<bool>,
    feature_flag: Option<bool>,
) -> Resolved<bool> {
    BoolFlag::env("GROW_MCP_RECURSIVE_CONFIG_WATCH")
        .requirement(requirement)
        .cli(cli)
        .config(config)
        .managed(managed)
        .feature_flag(feature_flag)
        .default(true)
        .resolve()
}
/// Load `~/.grow/requirements.toml` standalone so the admin pin can beat
/// env vars. The merged config layer can't express that — last-merge-wins
/// loses provenance.
pub(crate) fn read_requirements_toml() -> Option<toml::Value> {
    let path = crate::util::grow_home::grow_home().join("requirements.toml");
    let content = std::fs::read_to_string(&path).ok()?;
    toml::from_str(&content).ok()
}
/// Seed free-function remote caches after writing `Config.remote_settings`.
///
/// Called from `init.rs` at boot and from the agent when backgrounded settings
/// arrive later, so every side effect here must be idempotent and safe to
/// re-apply.
pub fn apply_remote_settings_side_effects(settings: Option<&crate::util::config::RemoteSettings>) {
    if let Some(s) = settings {
        config::signed_policy::apply_remote_managed_config_signature_verification(
            s.managed_config_signature_verification,
            false,
        );
    }
    crate::util::config::cache_remote_mcp_startup_timeout_secs(
        settings.and_then(|s| s.mcp_startup_timeout_secs),
    );
    crate::util::config::cache_remote_max_mcp_output_bytes(
        settings.and_then(|s| s.max_mcp_output_bytes),
    );
    crate::util::config::cache_remote_auto_mode(settings.and_then(|s| s.auto_mode.clone()));
    crate::util::config::cache_remote_remember_tool_approvals(
        settings.and_then(|s| s.remember_tool_approvals),
    );
    crate::util::config::cache_remote_crash_handler_enabled(
        settings.and_then(|s| s.crash_handler_enabled),
    );
    let image_normalize_cache_enabled = settings
        .and_then(|r| r.image_normalize_cache_enabled)
        .unwrap_or(false);
    crate::session::normalize_cache::NormalizeCache::global()
        .set_enabled(image_normalize_cache_enabled);
}
/// Read `env.<key>` from Claude-compat `managed_settings.json`. `Some(true)`
/// indicates a force-off signal from a Mac-MDM-style admin policy.
fn managed_settings_env_flag(key: &str) -> Option<bool> {
    let path = config::claude_managed_settings_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    workspace::permission::resolution::json_env_flag(json.get("env"), key)
}
/// Assemble the final model map from the configured provider hierarchy.
/// Remote catalogs and compiled presets are intentionally not model sources.
pub fn resolve_model_list(cfg: &Config) -> IndexMap<String, ModelEntry> {
    let mut resolved: IndexMap<String, ModelEntry> = IndexMap::new();
    for (key, model_override) in &cfg.config_models {
        let had_base = resolved.contains_key(key);
        let base = resolved.shift_remove(key);
        if !had_base {
            tracing::debug!(model_key = %key, "adding configured provider model");
            if model_override.context_window.is_none() {
                tracing::debug!(
                    model_key = %key,
                    default = 200_000,
                    "model missing context_window, defaulting to 200000 — set context_window in [provider.<id>.models.<model>]",
                );
            }
        }
        let with_provider = model_override.model_provider.as_deref().map(|pid| {
            match cfg.model_providers.get(pid) {
                Some(provider) => model_override.with_provider_defaults(provider, pid),
                None => model_override.with_missing_provider(),
            }
        });
        let effective = with_provider.as_ref().unwrap_or(model_override);
        let mut entry = effective.apply(key, base);
        tracing::debug!(
            model_key = %key,
            base_url = %entry.info.base_url,
            has_api_key = entry.api_key.is_some(),
            env_key = ?entry.env_key,
            auth_provider = entry.auth_provider.as_ref().map(|p| p.name.as_str()),
            model_provider = model_override.model_provider.as_deref(),
            had_base,
            "config model override applied"
        );
        resolved.insert(key.clone(), entry);
    }
    for (key, entry) in resolved.iter_mut() {
        if let Some(ref mut provider) = entry.auth_provider {
            if provider.is_fail_closed() {
                continue;
            }
            let config = cfg.auth_providers.get(&provider.name);
            if config.is_none() {
                tracing::debug!(
                    model_key = %key,
                    provider = %provider.name,
                    "provider ref has no trusted config; failing closed with an empty command"
                );
            }
            provider.attach_trusted_config(config);
        }
    }
    {
        let default_cw = DEFAULT_CONTEXT_WINDOW;
        let donors: std::collections::HashMap<String, (std::num::NonZeroU64, ApiBackend)> =
            resolved
                .values()
                .filter(|e| e.info.context_window.get() != default_cw)
                .map(|e| {
                    (
                        e.info.model.clone(),
                        (e.info.context_window, e.info.api_backend.clone()),
                    )
                })
                .collect();
        for entry in resolved.values_mut() {
            if let Some((donor_cw, donor_backend)) = donors.get(&entry.info.model) {
                if entry.info.context_window.get() == default_cw {
                    tracing::debug!(
                        model = %entry.info.model,
                        from = default_cw,
                        to = donor_cw.get(),
                        "slug-match: inheriting context_window from sibling catalog entry"
                    );
                    entry.info.context_window = *donor_cw;
                }
                if entry.info.api_backend == ApiBackend::default()
                    && *donor_backend != ApiBackend::default()
                {
                    entry.info.api_backend.clone_from(donor_backend);
                }
            }
        }
    }
    if let Some(ref global_agent_type) = cfg.models.agent_type {
        tracing::warn!(
            global_agent_type = %global_agent_type,
            "[models] agent_type is deprecated. Set agent_type on each [model.X] entry instead."
        );
        for entry in resolved.values_mut() {
            if entry.info.agent_type == DEFAULT_AGENT_TYPE {
                entry.info.agent_type = global_agent_type.clone();
            }
        }
    }
    apply_global_extra_headers(&mut resolved, &cfg.models);
    apply_global_scalar_defaults(&mut resolved, &cfg.models);
    for entry in resolved.values_mut() {
        entry.info.derive_reasoning_effort_fields();
    }
    resolved
}
/// Layer 6 of [`resolve_model_list`]: fold the global `[models].extra_headers`
/// into every model as a base. The presence check is case-insensitive because
/// the sampler lowers these into an `http::HeaderMap`, so a global `X-Foo` must
/// not shadow a per-model `x-foo`; a per-model `[model.<id>].extra_headers`
/// (applied earlier) therefore wins per key.
fn apply_global_extra_headers(resolved: &mut IndexMap<String, ModelEntry>, models: &ModelsConfig) {
    if models.extra_headers.is_empty() {
        return;
    }
    tracing::debug!(
        header_keys = ?models.extra_headers.keys().collect::<Vec<_>>(),
        model_count = resolved.len(),
        "applying global [models].extra_headers default to all models"
    );
    for entry in resolved.values_mut() {
        for (k, v) in &models.extra_headers {
            let present = entry
                .info
                .extra_headers
                .keys()
                .any(|ek| ek.eq_ignore_ascii_case(k));
            if !present {
                entry.info.extra_headers.insert(k.clone(), v.clone());
            }
        }
    }
}
/// Layer 7 of [`resolve_model_list`]: fill scalar `[models]` defaults into any
/// model that left the field unset. Per-model (Layer 3) and remote-prefetched
/// (Layer 2) values already populated theirs, so they win via `get_or_insert`
/// (the global default is a fallback, not a clamp).
fn apply_global_scalar_defaults(
    resolved: &mut IndexMap<String, ModelEntry>,
    models: &ModelsConfig,
) {
    for entry in resolved.values_mut() {
        let info = &mut entry.info;
        if let Some(v) = models.temperature {
            info.temperature.get_or_insert(v);
        }
        if let Some(v) = models.top_p {
            info.top_p.get_or_insert(v);
        }
        if let Some(v) = models.output_limit {
            info.output_limit.get_or_insert(v);
        }
        if let Some(v) = models.max_retries {
            info.max_retries.get_or_insert(v);
        }
        if let Some(v) = models.inference_idle_timeout_secs {
            info.inference_idle_timeout_secs.get_or_insert(v);
        }
        if let Some(v) = models.stream_tool_calls {
            info.stream_tool_calls.get_or_insert(v);
        }
    }
}
/// Resolve a model against the available model map.
/// Checks the map key (id) first, then falls back to a slug scan.
pub fn find_model_by_id<'a>(
    models: &'a IndexMap<String, ModelEntry>,
    model_id: &str,
) -> Option<&'a ModelEntry> {
    models
        .get(model_id)
        .or_else(|| models.values().find(|m| m.model == model_id))
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntryConfig {
    /// Stable unique identifier for this catalog entry. When present,
    /// used as the catalog map key. Falls back to `model` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The routing slug sent in API requests.
    pub model: String,
    /// The base URL of the model. e.g. "https://api.example.com/v1"
    pub base_url: String,
    /// Human-readable display name of the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// The API key for this model's provider.
    /// If not set, falls back to env_key, then GROW_API_KEY.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Environment variable name(s) that hold the provider API key.
    /// Accepts a string or an array (first set, non-empty value wins).
    /// If not set, falls back to GROW_API_KEY.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_key: Option<EnvKeys>,
    /// Which API backend to use for this model.
    /// Values: "chat_completions" (default), "responses"
    #[serde(default)]
    pub api_backend: ApiBackend,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_scheme: Option<AuthScheme>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub supports_reasoning_effort: bool,
    /// Per-model reasoning-effort menu (source of truth). The two legacy fields
    /// above are derived from this list when it is non-empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasoning_efforts: Vec<ReasoningEffortOption>,
    /// Extra headers to send with requests to this model's endpoint.
    /// Useful for BYOK (Bring Your Own Key) scenarios.
    /// Example: { "x-anthropic-api-key" = "sk-ant-..." }
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub extra_headers: IndexMap<String, String>,
    /// The total context window size in tokens for this model.
    /// Used for auto-compact threshold calculations.
    /// Required — BYOK users must explicitly set this in config.toml.
    pub context_window: NonZeroU64,
    /// Per-model auto-compact threshold (0-100). When the session's token
    /// usage exceeds this percentage of `context_window`, the conversation
    /// is summarized. Resolver precedence:
    /// requirements > env > user (per-model > global) > managed (per-model > global)
    /// > remote per-model (this field) > remote global > 85.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_compact_threshold_percent: Option<u8>,
    /// Per-model system-prompt identity label (not UI `name`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt_label: Option<String>,
    /// When true, this model uses concise mode (compact system prompt,
    /// concise tool output, concise user message prefix, reduced toolset).
    /// Defaults to false — when omitted or false, nothing changes.
    #[serde(default, skip_serializing_if = "is_false")]
    pub use_concise: bool,
    /// The type of system prompt to use for this model.
    /// e.g. "grow-build", "browser-use".
    #[serde(default = "default_agent_type")]
    pub agent_type: String,
    /// Maximum seconds to wait between SSE chunks during inference streaming.
    /// When no chunk is received within this duration, the request fails with
    /// a non-retryable `IdleTimeout` error. This is a per-chunk deadline that
    /// resets on every received chunk — NOT a total-turn timeout.
    /// Default: 300 seconds (5 minutes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference_idle_timeout_secs: Option<u64>,
    /// Maximum number of retries for transient API errors (429, 500, 502, etc.)
    /// during a single inference request. Default: 5.
    /// Can also be set via the `GROW_MAX_RETRIES` environment variable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
    /// Exclude from the client model picker; still usable by internal tasks.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    /// Per-model config for the `x-compactions-remaining` header; `None` disables it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compactions_remaining: Option<CompactionsRemaining>,
    /// Per-model config for the `x-compaction-at` header; `None` disables it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction_at_tokens: Option<CompactionAtTokens>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub show_model_fingerprint: bool,
    /// Inject `stream_tool_calls: true` into the request body
    /// so the upstream emits per-chunk `function_call_arguments.delta`
    /// Without this set, backends using this extension send args as one delta
    /// event, defeating the purpose of streaming.
    ///
    /// Per-model opt-in -- BYOK endpoints that don't understand the
    /// flag should leave this unset to avoid request errors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_tool_calls: Option<bool>,
    /// Per-model Layer-3 LazinessDetector configuration. Defaults to
    /// the all-disabled state via `#[serde(default)]`.
    #[serde(default, skip_serializing_if = "is_default_laziness_detector")]
    pub laziness_detector: LazinessDetectorPerModelConfig,
}
/// True when `cfg` equals the all-disabled default. Derives `PartialEq`
/// on `f32`, which is fine for the current shape because both `f32`
/// fields default to `None` — there's no parsed-vs-literal `0.7` float
/// equality footgun. If a future default introduces `Some(0.7)`, this
/// helper must be reworked (e.g. compare on tolerance, or switch to a
/// bit-pattern compare) so `skip_serializing_if` doesn't start emitting
/// `[laziness_detector]` blocks for every model in `config.toml`.
fn is_default_laziness_detector(cfg: &LazinessDetectorPerModelConfig) -> bool {
    cfg == &LazinessDetectorPerModelConfig::default()
}
/// A `[model.foo]` entry from config.toml, parsed directly from raw TOML
/// (bypassing deep merge). Scalar fields are `Option` so absent means "inherit
/// from defaults/prefetched"; the collection fields (`extra_headers`,
/// `reasoning_efforts`) merge only when non-empty and so cannot express
/// "override to empty."
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ConfigModelOverride {
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub api_key: Option<String>,
    /// Env var name(s) for the provider key — string or array in config.toml.
    pub env_key: Option<EnvKeys>,
    /// Name of a `[auth_provider.<name>]` credential helper that mints
    /// this model's bearer token. Static `api_key` / `env_key` win when both
    /// are set.
    pub auth_provider: Option<String>,
    pub model_provider: Option<String>,
    pub output_limit: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub api_backend: Option<ApiBackend>,
    #[serde(default)]
    pub extra_headers: IndexMap<String, String>,
    #[serde(default)]
    pub query_params: IndexMap<String, String>,
    #[serde(default)]
    pub env_http_headers: IndexMap<String, String>,
    pub context_window: Option<u64>,
    /// Per-model auto-compact threshold override (0-100) from `[model.<id>]`.
    /// Read directly by `resolve_auto_compact_threshold_percent`; intentionally
    /// NOT merged into `ModelInfo.auto_compact_threshold_percent` so the
    /// resolver can keep user-per-model distinct from GB-per-model.
    pub auto_compact_threshold_percent: Option<u8>,
    /// Per-model system-prompt identity; not merged into `ModelInfo` (tiered resolve).
    pub system_prompt_label: Option<String>,
    pub use_concise: Option<bool>,
    pub agent_type: Option<String>,
    pub inference_idle_timeout_secs: Option<u64>,
    pub max_retries: Option<u32>,
    pub hidden: Option<bool>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub supports_reasoning_effort: Option<bool>,
    pub reasoning_efforts: Vec<ReasoningEffortOption>,
    /// Aliases must be registered in `config_model_override_parse::ALIASES`;
    /// serde rejects a table that contains both spellings otherwise.
    #[serde(alias = "send_compactions_remaining")]
    pub compactions_remaining: Option<CompactionsRemaining>,
    pub compaction_at_tokens: Option<CompactionAtTokens>,
    pub show_model_fingerprint: Option<bool>,
    pub stream_tool_calls: Option<bool>,
}
impl ConfigModelOverride {
    pub(crate) fn apply(&self, key: &str, base: Option<ModelEntry>) -> ModelEntry {
        let mut entry = base.unwrap_or_else(|| ModelEntry::fallback(key));
        if let Some(ref v) = self.model {
            entry.info.model = v.clone();
        }
        if let Some(ref v) = self.base_url {
            entry.info.base_url = v.clone();
        }
        if self.name.is_some() {
            entry.info.name.clone_from(&self.name);
        }
        if self.description.is_some() {
            entry.info.description.clone_from(&self.description);
        }
        if self.output_limit.is_some() {
            entry.info.output_limit = self.output_limit;
        }
        if self.temperature.is_some() {
            entry.info.temperature = self.temperature;
        }
        if self.top_p.is_some() {
            entry.info.top_p = self.top_p;
        }
        if let Some(ref v) = self.api_backend {
            entry.info.api_backend = v.clone();
        }
        if !self.extra_headers.is_empty() {
            entry.info.extra_headers = self.extra_headers.clone();
        }
        if !self.query_params.is_empty() {
            entry.info.query_params = self.query_params.clone();
        }
        if !self.env_http_headers.is_empty() {
            entry.info.env_http_headers = self.env_http_headers.clone();
        }
        if let Some(cw) = self.context_window.and_then(NonZeroU64::new) {
            entry.info.context_window = cw;
        }
        if let Some(v) = self.use_concise {
            entry.info.use_concise = v;
        }
        if let Some(ref at) = self.agent_type {
            entry.info.agent_type.clone_from(at);
        }
        if self.inference_idle_timeout_secs.is_some() {
            entry.info.inference_idle_timeout_secs = self.inference_idle_timeout_secs;
        }
        if self.max_retries.is_some() {
            entry.info.max_retries = self.max_retries;
        }
        if let Some(v) = self.hidden {
            entry.info.hidden = v;
        }
        if self.reasoning_effort.is_some() {
            entry.info.reasoning_effort = self.reasoning_effort;
        }
        if let Some(v) = self.supports_reasoning_effort {
            entry.info.supports_reasoning_effort = v;
        } else if !entry.info.supports_reasoning_effort
            && matches!(entry.info.api_backend, ApiBackend::Messages)
        {
            entry.info.supports_reasoning_effort = true;
        }
        if !self.reasoning_efforts.is_empty() {
            entry.info.reasoning_efforts = self.reasoning_efforts.clone();
        }
        if self.compactions_remaining.is_some() {
            entry.info.compactions_remaining = self.compactions_remaining;
        }
        if self.compaction_at_tokens.is_some() {
            entry.info.compaction_at_tokens = self.compaction_at_tokens;
        }
        if let Some(v) = self.show_model_fingerprint {
            entry.info.show_model_fingerprint = v;
        }
        if self.stream_tool_calls.is_some() {
            entry.info.stream_tool_calls = self.stream_tool_calls;
        }
        if self.api_key.is_some() {
            entry.api_key.clone_from(&self.api_key);
        }
        if self.env_key.is_some() {
            entry.env_key.clone_from(&self.env_key);
        }
        if let Some(ref name) = self.auth_provider {
            entry.auth_provider = Some(crate::auth::AuthProviderRef::unresolved(name.clone()));
        }
        entry
    }
}
/// Shared model metadata — the common fields across all model sources.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelInfo {
    /// Stable unique identifier for this catalog entry.
    /// Falls back to `model` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The routing slug sent in API requests.
    pub model: String,
    /// The base URL of the model (session endpoint). e.g. "https://service.example.com/v1"
    pub base_url: String,
    /// Human-readable name of the model. Honored by both the picker
    /// (`/model`) and `/session-info` -- when set, that's the label shown
    /// to users in either consumer.
    pub name: Option<String>,
    pub description: Option<String>,
    pub output_limit: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub api_backend: ApiBackend,
    pub auth_scheme: AuthScheme,
    pub extra_headers: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub query_params: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub env_http_headers: IndexMap<String, String>,
    pub context_window: NonZeroU64,
    /// Per-model auto-compact threshold (0-100). `None` defers to the
    /// global / default tiers in `resolve_auto_compact_threshold_percent`.
    pub auto_compact_threshold_percent: Option<u8>,
    /// Per-model system-prompt identity (not UI picker `name`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt_label: Option<String>,
    /// When true, this model uses concise mode (compact system prompt,
    /// concise tool output, concise user message prefix, reduced toolset).
    pub use_concise: bool,
    /// The type of agent configuration to use for this model.
    /// Always has a value; defaults to `"grow"` when the server
    /// or user config doesn't specify one.
    #[serde(default = "default_agent_type")]
    pub agent_type: String,
    /// Per-chunk idle timeout for inference streaming (see `ModelEntryConfig`).
    pub inference_idle_timeout_secs: Option<u64>,
    pub max_retries: Option<u32>,
    /// Never show in picker.
    pub hidden: bool,
    /// May the user select this model for normal chat? Derived from
    /// `allowed_models` in `resolve_model_catalog`; never persisted.
    #[serde(skip_serializing, default = "default_true")]
    pub user_selectable: bool,
    pub reasoning_effort: Option<ReasoningEffort>,
    /// When true, the UI shows effort controls for this model.
    pub supports_reasoning_effort: bool,
    /// Per-model reasoning-effort menu (source of truth); legacy fields derived from it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasoning_efforts: Vec<ReasoningEffortOption>,
    /// Per-model config for the `x-compactions-remaining` header; `None` disables it.
    pub compactions_remaining: Option<CompactionsRemaining>,
    /// Per-model config for the `x-compaction-at` header; `None` disables it.
    pub compaction_at_tokens: Option<CompactionAtTokens>,
    pub show_model_fingerprint: bool,
    /// When `Some(true)`, the sampler injects `stream_tool_calls: true`
    pub stream_tool_calls: Option<bool>,
    /// Per-model Layer-3 LazinessDetector configuration. Defaults to
    /// the all-disabled state — the feature is per-model opt-in with a
    /// second-step `max_nudges_per_session > 0` opt-in for actually
    /// injecting nudges. See [`LazinessDetectorPerModelConfig`].
    #[serde(default)]
    pub laziness_detector: LazinessDetectorPerModelConfig,
}
impl ModelInfo {
    /// Minimal fallback descriptor for an unknown model slug.
    /// Used when a configured model ID isn't found in presets or remote models.
    pub fn fallback(slug: &str) -> Self {
        ModelInfo {
            user_selectable: true,
            id: None,
            model: slug.to_owned(),
            base_url: String::new(),
            name: None,
            description: None,
            output_limit: None,
            temperature: None,
            top_p: None,
            api_backend: ApiBackend::default(),
            auth_scheme: Default::default(),
            extra_headers: IndexMap::new(),
            query_params: IndexMap::new(),
            env_http_headers: IndexMap::new(),
            context_window: NonZeroU64::new(200_000).unwrap(),
            auto_compact_threshold_percent: None,
            system_prompt_label: None,
            use_concise: false,
            agent_type: default_agent_type(),
            inference_idle_timeout_secs: None,
            max_retries: None,
            hidden: false,
            reasoning_effort: None,
            supports_reasoning_effort: false,
            reasoning_efforts: Vec::new(),
            compactions_remaining: None,
            compaction_at_tokens: None,
            show_model_fingerprint: false,
            stream_tool_calls: None,
            laziness_detector: LazinessDetectorPerModelConfig::default(),
        }
    }
    /// Extract shared model metadata from a flat config entry.
    pub fn from_config(entry: &ModelEntryConfig) -> Self {
        ModelInfo {
            user_selectable: true,
            id: entry.id.clone(),
            model: entry.model.clone(),
            base_url: entry.base_url.clone(),
            name: entry.name.clone(),
            description: entry.description.clone(),
            output_limit: entry.output_limit,
            temperature: entry.temperature,
            top_p: entry.top_p,
            api_backend: entry.api_backend.clone(),
            auth_scheme: entry.auth_scheme.unwrap_or_default(),
            extra_headers: entry.extra_headers.clone(),
            query_params: IndexMap::new(),
            env_http_headers: IndexMap::new(),
            context_window: entry.context_window,
            auto_compact_threshold_percent: entry.auto_compact_threshold_percent,
            system_prompt_label: entry.system_prompt_label.clone(),
            use_concise: entry.use_concise,
            agent_type: entry.agent_type.clone(),
            inference_idle_timeout_secs: entry.inference_idle_timeout_secs,
            max_retries: entry.max_retries,
            hidden: entry.hidden,
            reasoning_effort: entry.reasoning_effort,
            supports_reasoning_effort: entry.supports_reasoning_effort,
            reasoning_efforts: entry.reasoning_efforts.clone(),
            compactions_remaining: entry.compactions_remaining,
            compaction_at_tokens: entry.compaction_at_tokens,
            show_model_fingerprint: entry.show_model_fingerprint,
            stream_tool_calls: entry.stream_tool_calls,
            laziness_detector: entry.laziness_detector.clone(),
        }
    }
    /// Derive the legacy effort gate and an explicitly marked model default
    /// from `reasoning_efforts`. A configured global fallback is resolved later
    /// by the catalog so a model-level default always wins.
    /// The empty-list path leaves both legacy fields untouched.
    fn derive_reasoning_effort_fields(&mut self) {
        if self.reasoning_efforts.is_empty() {
            return;
        }
        self.supports_reasoning_effort = true;
        if self.reasoning_effort.is_none() {
            let default = self
                .reasoning_efforts
                .iter()
                .find(|opt| opt.default)
                .map(|opt| opt.value);
            self.reasoning_effort = default;
        }
    }
}
/// Flat struct so credential and endpoint fields coexist after deep-merge.
/// Routing reads fields, not provenance.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelEntry {
    pub info: ModelInfo,
    pub api_key: Option<String>,
    pub env_key: Option<EnvKeys>,
    /// Named credential helper (`[model.<id>] auth_provider = "<name>"`),
    /// resolved against `[auth_provider.<name>]` by `resolve_model_list`.
    /// Config-file models only: the built-in catalog never carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_provider: Option<crate::auth::AuthProviderRef>,
}
impl ModelEntry {
    /// Minimal fallback entry for an unknown model slug.
    pub fn fallback(slug: &str) -> Self {
        let mut info = ModelInfo::fallback(slug);
        info.base_url.clear();
        Self {
            info,
            api_key: None,
            env_key: None,
            auth_provider: None,
        }
    }
    pub fn info(&self) -> &ModelInfo {
        &self.info
    }
    pub fn from_config_entry(entry: &ModelEntryConfig) -> Self {
        Self {
            info: ModelInfo::from_config(entry),
            api_key: entry.api_key.clone(),
            env_key: entry.env_key.clone(),
            auth_provider: None,
        }
    }
    /// Non-empty `api_key`, else first non-empty resolved `env_key`.
    /// `None` → fall through to session / global key. Static only: never
    /// consults auth-provider tokens.
    pub(crate) fn own_credential(&self) -> Option<String> {
        first_own_credential(self.api_key.as_deref(), self.env_key.as_ref())
    }
    /// The provider governing this model's bearer: `None` when a static
    /// `api_key`/`env_key` resolves. The turn paths consult this, so a
    /// shadowed provider never runs.
    pub(crate) fn effective_auth_provider(&self) -> Option<&crate::auth::AuthProviderRef> {
        if self.own_credential().is_some() {
            return None;
        }
        self.auth_provider.as_ref()
    }
    /// `true` when the model has a non-empty `api_key`, an `env_key` that
    /// resolves to a non-empty value, or a named auth provider.
    /// Probes `std::env::var` at call time: result is not stable across env
    /// changes. Never executes a provider command.
    pub fn has_own_credentials(&self) -> bool {
        self.own_credential().is_some() || self.auth_provider.is_some()
    }
}
impl std::ops::Deref for ModelEntry {
    type Target = ModelInfo;
    fn deref(&self) -> &ModelInfo {
        &self.info
    }
}
fn is_false(v: &bool) -> bool {
    !v
}
fn default_true() -> bool {
    true
}
/// Codebase indexing setting for `[features] codebase_indexing`.
///
/// Patterns are matched against the git root when available, otherwise the cwd,
/// which allows explicitly indexing non-git directories.
///
/// ```toml
/// codebase_indexing = false                                          # disable
/// codebase_indexing = true                                           # any git repo (default)
/// codebase_indexing = ["/Users/*/grow*", "!/Users/*/old-*"]           # globs, ! to exclude
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CodebaseIndexingSetting {
    Enabled(bool),
    Patterns(Vec<String>),
}
impl Default for CodebaseIndexingSetting {
    fn default() -> Self {
        Self::Enabled(true)
    }
}
impl CodebaseIndexingSetting {
    /// Should `path` be indexed? For `Enabled(true)`, always yes (caller gates on git-root).
    /// For `Patterns`, path must match an include and not match any `!exclude`.
    pub fn should_index(&self, path: &std::path::Path) -> bool {
        match self {
            Self::Enabled(b) => *b,
            Self::Patterns(patterns) => {
                let path_str = path.to_string_lossy();
                let matches_any = |pats: &[&str]| {
                    pats.iter()
                        .any(|p| glob::Pattern::new(p).is_ok_and(|pat| pat.matches(&path_str)))
                };
                let (excludes, includes): (Vec<_>, Vec<_>) =
                    patterns.iter().partition(|p| p.starts_with('!'));
                let excludes: Vec<&str> = excludes
                    .iter()
                    .map(|p| p.strip_prefix('!').unwrap_or(p.as_str()))
                    .collect();
                let includes: Vec<&str> = includes.iter().map(|p| p.as_str()).collect();
                let included = includes.is_empty() || matches_any(&includes);
                let excluded = matches_any(&excludes);
                included && !excluded
            }
        }
    }
}
/// Optional role pair that drops a malformed value to `None` (with a warn)
/// instead of failing the whole config parse — one typo must not wipe the
/// config. Mirrors the remote tolerance in `util::config::remote`.
fn de_tolerant_goal_role_model<'de, D>(
    deserializer: D,
) -> Result<Option<crate::util::config::GoalRoleModel>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<toml::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|v| {
        v.try_into()
            .map_err(|e| tracing::warn!(error = %e, "[goal] role model: dropped malformed value"))
            .ok()
    }))
}
/// Skeptic pool variant of [`de_tolerant_goal_role_model`]: a non-array yields
/// an empty pool; malformed entries are dropped, survivor order preserved (the
/// skeptic round-robin depends on it).
fn de_tolerant_goal_role_models<'de, D>(
    deserializer: D,
) -> Result<Vec<crate::util::config::GoalRoleModel>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<toml::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(toml::Value::Array(arr)) => arr
            .into_iter()
            .filter_map(|v| {
                v.try_into()
                    .map_err(|e| {
                        tracing::warn!(error = %e, "[goal] skeptic model: dropped malformed entry");
                    })
                    .ok()
            })
            .collect(),
        _ => Vec::new(),
    })
}
/// `[goal]` section: the canonical home for `/goal` configuration. Field names
/// mirror the remote `goal_*` keys with the prefix dropped, so config and remote
/// stay 1:1. Per-key precedence is env > this config > remote > default.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GoalConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classifier_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planner_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_current_model_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verifier_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classifier_max_runs: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategist_every: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reverify_after: Option<u32>,
    #[serde(
        default,
        deserialize_with = "de_tolerant_goal_role_model",
        skip_serializing_if = "Option::is_none"
    )]
    pub planner_model: Option<crate::util::config::GoalRoleModel>,
    #[serde(
        default,
        deserialize_with = "de_tolerant_goal_role_model",
        skip_serializing_if = "Option::is_none"
    )]
    pub strategist_model: Option<crate::util::config::GoalRoleModel>,
    #[serde(
        default,
        deserialize_with = "de_tolerant_goal_role_models",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub skeptic_models: Vec<crate::util::config::GoalRoleModel>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkflowsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}
/// `[auto_mode]` section: server-side configuration for Auto permission mode.
/// ONE struct serves both the local `[auto_mode]` TOML table and the remote
/// remote settings `auto_mode` JSON object (coerced via `serde_json::from_value`), so
/// the two stay 1:1. All fields are plain scalars/enums, so they deserialize
/// cleanly from both formats (no custom tolerant deser needed). Unset fields stay
/// `None` here; the wire fn applies the built-in defaults once auto mode is
/// enabled (current model, upstream reasoning default, `full` prompt).
/// Precedence: local config > remote > those built-in defaults.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoModeConfig {
    /// The Auto-mode gate. Lowest-precedence layer of the gate chain (env and
    /// local `[auto_mode] enabled` config win over this remote value).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// How much context the classifier prompt includes. `None` ⇒ the wire fn's
    /// built-in default (`full`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_type: Option<workspace::permission::ClassifierPromptType>,
    /// Routing slug for a dedicated classifier model. `None` ⇒ inherit the
    /// session model. Resolved via `resolve_aux_model_sampling_config`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classifier_model: Option<String>,
    /// Classifier side-query duration in milliseconds; resolved with bounded defaults.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classify_timeout_ms: Option<u64>,
    /// Classifier reasoning effort. Applies on BOTH the routed-model path and the
    /// inherited session-model path; `None` leaves the field unset so model
    /// configuration or the upstream service can choose the default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Features {
    /// when set, the agent may ask permission for tool executions
    #[serde(default)]
    pub support_permission: bool,
    /// Codebase graph indexing for go-to-definition/references.
    /// Accepts: true | false | ["glob", "!negative-glob", ...]
    /// Default: true (index any git repo). Patterns can explicitly match non-git directories.
    #[serde(default)]
    pub codebase_indexing: CodebaseIndexingSetting,
    /// Show a blocking warning when Grow starts outside a Git repository.
    /// Default: false. Used as the local fallback when the `non_git_warning` remote settings
    /// flag in `grow_build_settings` is absent. When the remote flag is present it takes
    /// precedence — `Some(false)` from remote settings overrides `true` here.
    #[serde(default)]
    pub non_git_warning: bool,
    /// Managed config fetching (managed_config.toml + requirements.toml).
    /// `None` = defer to env / default (true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_config: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lsp_tools: Option<bool>,
    /// MCP tool search/discovery. `None` = defer to remote settings / env / default (true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_search: Option<bool>,
    /// Web fetch tool. `None` = defer to remote settings / env / default (false).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_fetch: Option<bool>,
    /// Ask-user-question tool. `None` = defer to remote settings / env / default (true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask_user_question: Option<bool>,
    /// Session recap (`/recap` + automatic return-from-away recap).
    /// `None` = defer to remote settings / env / default (`true`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_recap: Option<bool>,
    /// Two-pass (prefire) compaction: speculatively summarize the history
    /// prefix in the background, then summarize NOTE₁ + recent tail at
    /// compaction. `None` = defer to remote settings / env / default (`false`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub two_pass_compaction: Option<bool>,
    /// Write file tool. `None` = defer to remote settings / env / default (true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_file: Option<bool>,
    /// Cancel-rewind: Ctrl+C before first activity restores the prompt.
    /// `None` = defer to remote settings / env / default (true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_rewind: Option<bool>,
    /// Auto-wake: immediately inject a synthetic prompt when a background
    /// task or subagent completes, instead of waiting for the idle drain.
    /// `None` = defer to remote settings / env / default (true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_wake: Option<bool>,
    /// `summary` (default) | `transcript` | `segments`. `None` = defer to CLI /
    /// env (`GROW_COMPACTION_MODE`). Parsed via `CompactionMode::parse`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction_mode: Option<String>,
    /// `none` | `minimal` | `balanced` | `verbose` (default). `None` = defer to
    /// env (`GROW_COMPACTION_DETAIL`). The `segments` verbatim detail level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction_detail: Option<String>,
    /// Feed the summarizer the verbatim conversation instead of the lossy rewrite; `None` = defer to env/remote settings/default (true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction_verbatim_input: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction_tool_choice: Option<String>,
    /// Snapshot a completed subagent's isolated worktree into a durable git ref
    /// and delete its directory (resume rehydrates from the ref). This is the
    /// per-deployment rollout lever (set in managed_config.toml `[features]`).
    /// `None` = defer to remote settings / default (false).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_worktree_snapshot: Option<bool>,
    /// Per-`Ready`-client transport-liveness pollers + the
    /// session-actor `StatusDispatcher`.
    ///
    /// When `true` (default), each successfully-handshaken MCP
    /// client gets a poller that detects rmcp service-loop
    /// termination and pushes `grow/mcp/server_status` updates to
    /// the client. When `false`, neither watchers nor the
    /// dispatcher are spawned — useful as an emergency kill switch
    /// for the rollout. `None` = defer to env / default (true).
    ///
    /// Resolved via [`Config::resolve_mcp_liveness_watchers`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_liveness_watchers: Option<bool>,
    /// Bounded stdio auto-restart task.
    ///
    /// When `true`, the session-actor `StatusDispatcher` reacts to
    /// `TransportClosed` / `HandshakeFailed` events on stdio MCP
    /// servers by scheduling up to 3 respawn attempts with
    /// `[1s, 4s, 16s]` backoff. HTTP / HttpAuth servers are NOT
    /// auto-restarted (their existing `reset_transport` path
    /// covers the recovery). `None` = defer to env / default
    /// (recovery is on by default; set `false` here / via
    /// `GROW_MCP_AUTO_RESTART` to opt out).
    ///
    /// Resolved via [`Config::resolve_mcp_auto_restart`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_auto_restart: Option<bool>,
    /// Pager-side subscription to the `grow/mcp/server_status` push.
    ///
    /// When `true` (default), the pager subscribes to the per-server
    /// status delta the shell emits via the dispatcher and
    /// patches the MCP servers modal in-place (no re-fetch round
    /// trip). When `false`, the pager ignores the push and falls
    /// back to the legacy `grow/mcp/tools_changed` debounced refetch
    /// path. `None` = defer to env / default (true).
    ///
    /// The pager-side gate
    /// (`acp_handler::push_server_status_enabled`) uses an
    /// **env-only** OnceLock cache via
    /// [`crate::util::config::resolve_mcp_push_server_status(None, None, None)`].
    /// That function consults `BoolFlag::env` and the default `true`
    /// — it does NOT read this `Features` field. The shell-side
    /// `Config::resolve_mcp_push_server_status` does delegate
    /// through this field, but the pager never holds a `Config`.
    ///
    /// Practical consequence: setting
    /// `[features] mcp_push_server_status = false` in
    /// `~/.grow/config.toml` will NOT disable the pager's
    /// subscription on a freshly-launched process. To disable the
    /// pager subscription, set `GROW_MCP_PUSH_SERVER_STATUS=0` in
    /// the env before launch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_push_server_status: Option<bool>,
    /// Whether the leader's `ConfigFileWatcher` adds the two narrow
    /// non-recursive watches for `<cwd>/` and `<cwd>/.grow/`.
    ///
    /// When `true` (default), edits to `<cwd>/.mcp.json`,
    /// `<cwd>/.grow/config.toml`, or `<cwd>/.claude.json` flow
    /// through the watcher → reloader → `ConfigUpdate::
    /// ProjectMcpServersChanged { cwd }` → `app.rs` ACP-injection
    /// pipeline and the affected sessions reload their MCP servers
    /// within the debounce window (~ 1 s). When `false`, the leader
    /// skips the cwd watches entirely and the only way to pick up a
    /// project-config edit is the user-triggered refresh button.
    ///
    /// The watches are **always non-recursive** — the name follows
    /// the convention for the rollout-gate flag. See
    /// `crate::config::watcher::ConfigFileWatcher::watch_path` for
    /// the inotify-quota rationale.
    ///
    /// The name is a documented misnomer — it gates
    /// the existence of the **cwd** watches, NOT their recursion
    /// mode. A future rename to `mcp_cwd_config_watch` would align
    /// name and behavior; deferred to a follow-up to avoid widening
    /// the config surface across requirements.toml / managed configs.
    ///
    /// Resolved via [`Config::resolve_mcp_recursive_config_watch`].
    /// `None` = defer to env / default (true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_recursive_config_watch: Option<bool>,
}
/// Resolved credentials for a model session.
pub struct ResolvedCredentials {
    pub api_key: Option<String>,
    pub base_url: String,
    pub auth_scheme: AuthScheme,
}
/// First usable BYOK credential: a non-empty (trimmed) api_key, else the first
/// set, non-empty env_key value. Single source of truth for has_own_credentials,
/// resolve_credentials, and the JWT-reload path.
pub(crate) fn first_own_credential(
    api_key: Option<&str>,
    env_key: Option<&EnvKeys>,
) -> Option<String> {
    api_key
        .filter(|k| !k.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| env_key.and_then(EnvKeys::resolve_value))
}
/// Resolve only credentials owned by the configured provider/model.
/// Product login credentials are never inference credentials.
pub fn resolve_credentials(model: &ModelEntry) -> ResolvedCredentials {
    let info = model.info();
    let (api_key, base_url) = if let Some(key) = model.own_credential() {
        (Some(key), info.base_url.clone())
    } else if let Some(provider) = model.auth_provider.as_ref() {
        debug_assert!(model.effective_auth_provider().is_some());
        (provider.cached_token(), info.base_url.clone())
    } else {
        if let Some(ref env_keys) = model.env_key
            && !env_keys.is_empty()
        {
            tracing::warn!(
                model = %info.model,
                env_key = %env_keys,
                "model has env_key configured but none of the environment variables are set — \
                 requests will have no API key",
            );
        }
        (None, info.base_url.clone())
    };
    let auth_scheme = info.auth_scheme;
    ResolvedCredentials {
        api_key,
        base_url,
        auth_scheme,
    }
}
/// Derive a stable local identifier from a deployment key without exposing the key.
pub fn deployment_id_from_key(key: &str) -> String {
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, key.as_bytes()).to_string()
}
/// Try to resolve credentials for a model by loading the effective config.
/// Returns `None` (with a warning) if config loading, parsing, or model
/// lookup fails.
pub fn try_resolve_model_credentials(model_id: &str) -> Option<ResolvedCredentials> {
    let raw = crate::config::load_effective_config()
        .map_err(|e| tracing::warn!(error = %e, "config load failed for credential resolution"))
        .ok()?;
    let cfg = Config::new_from_toml_cfg(&raw)
        .map_err(|e| tracing::warn!(error = %e, "config parse failed for credential resolution"))
        .ok()?;
    let models = resolve_model_list(&cfg);
    let entry = find_model_by_id(&models, model_id)?;
    Some(resolve_credentials(entry))
}
/// Per-model auth facts (BYOK status + auth scheme) from one effective-config
/// load, memoized by the session actor.
#[derive(Clone, Copy)]
pub struct ModelAuthFacts {
    pub byok: ModelByok,
    pub auth_scheme: AuthScheme,
}
/// Resolve `model_id` to its auth facts and auth-provider reference from one
/// effective-config load; both ride the same memo (see
/// `SessionActor::model_auth_memo`). Load/parse failure → `byok = Unknown`;
/// model absent from the catalog → `NotByok`. An empty `model_id` (no sampling
/// config yet) → `Unknown`, not `NotByok`, so the gate isn't activated for an
/// unidentified model.
pub fn resolve_model_auth_facts_and_provider(
    model_id: &str,
) -> (ModelAuthFacts, Option<crate::auth::AuthProviderRef>) {
    if model_id.is_empty() {
        return (
            ModelAuthFacts {
                byok: ModelByok::Unknown,
                auth_scheme: AuthScheme::default(),
            },
            None,
        );
    }
    with_resolved_model(model_id, |lookup| {
        let facts = ModelAuthFacts {
            byok: byok_from_lookup(&lookup),
            auth_scheme: match lookup {
                ModelLookup::Loaded(Some(e)) => e.info().auth_scheme,
                _ => AuthScheme::default(),
            },
        };
        let provider = match lookup {
            ModelLookup::Loaded(Some(e)) => e.effective_auth_provider().cloned(),
            _ => None,
        };
        (facts, provider)
    })
}
fn byok_from_lookup(lookup: &ModelLookup) -> ModelByok {
    match lookup {
        ModelLookup::ConfigUnavailable => ModelByok::Unknown,
        // Every selectable model now originates in the user's provider
        // catalog. Keyless local providers are BYOK too; BYOK describes
        // ownership of the endpoint, not merely the presence of a secret.
        ModelLookup::Loaded(Some(_)) => ModelByok::Byok,
        ModelLookup::Loaded(None) => ModelByok::NotByok,
    }
}
enum ModelLookup<'a> {
    /// `None` if `model_id` is absent from the catalog.
    Loaded(Option<&'a ModelEntry>),
    ConfigUnavailable,
}
/// Load + parse the effective config and hand the `model_id` lookup to `f`,
/// keeping "config unavailable" distinct from "model absent" so callers can
/// stay conservative on a transient config failure.
fn with_resolved_model<T>(model_id: &str, f: impl FnOnce(ModelLookup) -> T) -> T {
    let Some(raw) = crate::config::load_effective_config()
        .map_err(|e| tracing::warn!(error = %e, "config load failed for model auth lookup"))
        .ok()
    else {
        return f(ModelLookup::ConfigUnavailable);
    };
    let Some(cfg) = Config::new_from_toml_cfg(&raw)
        .map_err(|e| tracing::warn!(error = %e, "config parse failed for model auth lookup"))
        .ok()
    else {
        return f(ModelLookup::ConfigUnavailable);
    };
    let models = resolve_model_list(&cfg);
    f(ModelLookup::Loaded(find_model_by_id(&models, model_id)))
}
/// Resolve a standalone `SamplerConfig` for an auxiliary model slug (image
/// description, session summary, ...), resolved through the catalog so a
/// provider/model configuration redirects it to its own endpoint, credentials, and
/// routing `model`. `None` → caller falls back to the active session's model.
pub fn resolve_aux_model_sampling_config(
    model_id: &str,
    models: &IndexMap<String, ModelEntry>,
    alpha_test_key: Option<String>,
) -> Option<SamplerConfig> {
    if model_id.trim().is_empty() {
        return None;
    }
    let catalog_entry = find_model_by_id(models, model_id).cloned();
    if let Some(entry) = &catalog_entry {
        let credentials = resolve_credentials(entry);
        let sampler = sampling_config_for_model(entry, credentials, alpha_test_key.clone());
        if sampler.api_key.is_some() {
            return Some(sampler);
        }
        if entry.effective_auth_provider().is_some() {
            tracing::warn!(
                model = %model_id,
                "aux model uses an auth provider with no cached token; the caller falls back to its session default"
            );
            return None;
        }
    }
    tracing::warn!(
        aux_model = %model_id,
        "auxiliary model is not explicitly configured; falling back to active model",
    );
    None
}
/// Stamp the session-local attribution, bearer resolver, and retry policy
/// from the active session onto a routed aux `SamplerConfig` so a
/// helper model keeps the session's auth/attribution. Shared by image-describe
/// and the auto-mode classifier so the two can't drift.
///
/// The resolver gate is host-based so an auxiliary sampler cannot leak a
/// managed-service bearer to an arbitrary provider endpoint.
pub fn stamp_session_local_sampler_fields(
    cfg: &mut SamplerConfig,
    active_session_config: &SamplerConfig,
    max_retries: Option<u32>,
) {
    cfg.attribution_callback = active_session_config.attribution_callback.clone();
    if crate::util::is_service_api_bearer_url(&cfg.base_url) {
        cfg.bearer_resolver = active_session_config.bearer_resolver.clone();
    }
    cfg.max_retries = max_retries;
}
pub fn sampling_config_for_model(
    model: &ModelEntry,
    credentials: ResolvedCredentials,
    alpha_test_key: Option<String>,
) -> SamplerConfig {
    let info = model.info();
    let model_name = info.model.clone();
    let output_limit = info.output_limit;
    let temperature = info.temperature;
    let top_p = info.top_p;
    let mut extra_headers = info.extra_headers.clone();
    inject_url_derived_headers(
        &mut extra_headers,
        alpha_test_key.as_deref(),
        &credentials.base_url,
    );
    let api_backend = info.api_backend.clone();
    SamplerConfig {
        api_key: credentials.api_key,
        model: model_name,
        base_url: credentials.base_url,
        output_limit,
        temperature,
        top_p,
        api_backend,
        auth_scheme: credentials.auth_scheme,
        extra_headers,
        query_params: info.query_params.clone(),
        env_http_headers: info.env_http_headers.clone(),
        context_window: info.context_window.get(),
        reasoning_effort: info.reasoning_effort,
        force_http1: false,
        max_retries: info.max_retries,
        stream_tool_calls: info.stream_tool_calls.unwrap_or(false),
        idle_timeout_secs: None,
        origin_client: None,
        attribution_callback: None,
        bearer_resolver: None,
        compactions_remaining: info.compactions_remaining,
        compaction_at_tokens: info.compaction_at_tokens,
        doom_loop_recovery: None,
    }
}
/// Fold URL-derived headers into `extra_headers`.
///
/// The sampler crate is intentionally URL-agnostic: it does not inspect
/// `base_url` to decide which auth or staging headers to add. Replicate the
/// URL-derived header logic at the shell boundary so callers downstream see a
/// single homogenous header bag.
///
/// * cli-chat-proxy bases get `X-Grow-Token-Auth` and
///   `x-authenticateresponse` headers (mirrors the inline match in the legacy
///   `sampling::Client::new` on `is_cli_chat_proxy_url`).
/// * With the optional non-production feature, matching first-party hosts may
///   get an extra access header from the corresponding key argument.
///
/// Existing entries are never overwritten so callers can pre-set a value.
pub fn inject_url_derived_headers(
    headers: &mut IndexMap<String, String>,
    alpha_test_key: Option<&str>,
    base_url: &str,
) {
    if crate::util::is_cli_chat_proxy_url(base_url) {
        headers
            .entry("X-Grow-Token-Auth".to_string())
            .or_insert_with(|| "grow-cli".to_string());
        headers
            .entry("x-authenticateresponse".to_string())
            .or_insert_with(|| "authenticate-response".to_string());
        headers
            .entry(crate::http::CLIENT_MODE_HEADER.to_string())
            .or_insert_with(|| crate::http::process_client_mode().to_string());
    }
    let _ = (alpha_test_key, base_url);
}
pub fn to_acp_model_info(
    models: &IndexMap<String, ModelEntry>,
) -> IndexMap<acp::ModelId, acp::ModelInfo> {
    models
        .iter()
        .map(|(key, model)| {
            let info = model.info();
            let model_id = acp::ModelId::new(Arc::from(key.clone()));
            let total_context_tokens = info.context_window.get();
            let meta = {
                let mut map = serde_json::Map::new();
                map.insert(
                    "totalContextTokens".to_string(),
                    serde_json::Value::Number(total_context_tokens.into()),
                );
                map.insert(
                    "agentType".to_string(),
                    serde_json::Value::String(info.agent_type.clone()),
                );
                if info.supports_reasoning_effort {
                    map.insert(
                        "supportsReasoningEffort".to_string(),
                        serde_json::Value::Bool(true),
                    );
                    if let Some(effort) = info.reasoning_effort {
                        map.insert(
                            REASONING_EFFORT_META_KEY.to_string(),
                            reasoning_effort_meta_value(effort),
                        );
                    }
                }
                if !info.reasoning_efforts.is_empty() {
                    map.insert(
                        REASONING_EFFORTS_META_KEY.to_string(),
                        reasoning_efforts_meta_value(&info.reasoning_efforts),
                    );
                }
                if map.is_empty() { None } else { Some(map) }
            };
            (
                model_id.clone(),
                acp::ModelInfo::new(
                    model_id,
                    info.name.clone().unwrap_or_else(|| info.model.clone()),
                )
                .description(info.description.clone())
                .meta(meta),
            )
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use test_support::EnvGuard;
    #[test]
    fn main_cli_tools_override_preserves_profile_injection_policy() {
        let overrides = CliAgentOverrides {
            tools: Some(vec!["read_file".into()]),
            ..Default::default()
        };
        let mut cases = vec![(AgentDefinition::default_grow_build(), true)];
        for (mut definition, expected_injection) in cases {
            overrides.apply_to_definition(&mut definition);
            assert_eq!(definition.tools, vec!["read_file".to_string()]);
            assert_eq!(definition.inject_default_tools, expected_injection);
        }
    }
    /// `AutoModeConfig` parses identically from a local `[auto_mode]` TOML table
    /// and an equivalent remote settings JSON object (serde is format-agnostic). The
    /// lean shape is all scalars/enums, so no custom tolerant deser is needed.
    #[test]
    fn auto_mode_config_parses_from_toml_and_json_equivalently() {
        use workspace::permission::ClassifierPromptType;
        let toml_src = r#"
enabled = true
prompt_type = "no_user_tool_prefix"
classifier_model = "grow-4.5"
classify_timeout_ms = 45000
reasoning_effort = "low"
"#;
        let from_toml: AutoModeConfig = toml::from_str(toml_src).unwrap();
        let json = serde_json::json!({
            "enabled": true,
            "prompt_type": "no_user_tool_prefix",
            "classifier_model": "grow-4.5",
            "classify_timeout_ms": 45000,
            "reasoning_effort": "low"
        });
        let from_json: AutoModeConfig = serde_json::from_value(json).unwrap();
        assert_eq!(
            serde_json::to_value(&from_toml).unwrap(),
            serde_json::to_value(&from_json).unwrap()
        );
        for cfg in [&from_toml, &from_json] {
            assert_eq!(cfg.enabled, Some(true));
            assert_eq!(
                cfg.prompt_type,
                Some(ClassifierPromptType::NoUserToolPrefix)
            );
            assert_eq!(cfg.classifier_model.as_deref(), Some("grow-4.5"));
            assert_eq!(cfg.classify_timeout_ms, Some(45_000));
            assert_eq!(cfg.reasoning_effort, Some(ReasoningEffort::Low));
        }
        let empty: AutoModeConfig = toml::from_str("").unwrap();
        assert_eq!(serde_json::to_value(&empty).unwrap(), serde_json::json!({}));
    }
    /// `prompt_type` wire values are the snake_case `ClassifierPromptType` names.
    #[test]
    fn auto_mode_prompt_type_parses_snake_case() {
        use workspace::permission::ClassifierPromptType;
        for (s, variant) in [
            ("full", ClassifierPromptType::Full),
            (
                "no_user_tool_prefix",
                ClassifierPromptType::NoUserToolPrefix,
            ),
            ("bare_instructions", ClassifierPromptType::BareInstructions),
            ("just_command", ClassifierPromptType::JustCommand),
        ] {
            let cfg: AutoModeConfig = toml::from_str(&format!("prompt_type = \"{s}\"")).unwrap();
            assert_eq!(cfg.prompt_type, Some(variant));
        }
    }
    #[test]
    fn laziness_detector_default_is_all_disabled() {
        let cfg = LazinessDetectorPerModelConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.max_nudges_per_session, 0);
        assert_eq!(cfg.idle_threshold_ms, None);
        assert_eq!(cfg.min_confidence, None);
        assert_eq!(
            cfg.include_reasoning, None,
            "include_reasoning defaults to None so the harness default applies",
        );
    }
    #[test]
    fn laziness_detector_absent_block_deserializes_to_default() {
        let json = serde_json::json!({
            "model": "test",
            "base_url": "https://test.api/v1",
            "context_window": 200_000,
        });
        let entry: ModelEntryConfig =
            serde_json::from_value(json).expect("ModelEntryConfig deserializes without detector");
        assert_eq!(
            entry.laziness_detector,
            LazinessDetectorPerModelConfig::default()
        );
        let info = ModelInfo::from_config(&entry);
        assert!(!info.laziness_detector.enabled);
    }
    #[test]
    fn laziness_detector_fallback_modelinfo_is_disabled() {
        let info = ModelInfo::fallback("unknown-model");
        assert_eq!(
            info.laziness_detector,
            LazinessDetectorPerModelConfig::default(),
        );
        assert!(!info.laziness_detector.enabled);
        assert_eq!(info.laziness_detector.max_nudges_per_session, 0);
    }
    #[test]
    fn laziness_detector_block_round_trips_through_serde() {
        let json = serde_json::json!({
            "enabled": true,
            "max_nudges_per_session": 3,
            "idle_threshold_ms": 15_000,
            "min_confidence": 0.8,
            "include_reasoning": false,
        });
        let cfg: LazinessDetectorPerModelConfig =
            serde_json::from_value(json).expect("deserialize populated block");
        assert!(cfg.enabled);
        assert_eq!(cfg.max_nudges_per_session, 3);
        assert_eq!(cfg.idle_threshold_ms, Some(15_000));
        assert_eq!(cfg.min_confidence, Some(0.8));
        assert_eq!(cfg.include_reasoning, Some(false));
    }
    /// Pins all three states of the per-model `include_reasoning`
    /// override (`Some(true)`, `Some(false)`, absent → `None`) so a
    /// future drift on the `#[serde(default)]` attribute or the field
    /// type fails the test rather than silently changing the resolved
    /// default.
    #[test]
    fn laziness_detector_include_reasoning_serde_states() {
        let some_true: LazinessDetectorPerModelConfig =
            serde_json::from_value(serde_json::json!({ "include_reasoning": true }))
                .expect("Some(true)");
        assert_eq!(some_true.include_reasoning, Some(true));
        let some_false: LazinessDetectorPerModelConfig =
            serde_json::from_value(serde_json::json!({ "include_reasoning": false }))
                .expect("Some(false)");
        assert_eq!(some_false.include_reasoning, Some(false));
        let absent: LazinessDetectorPerModelConfig =
            serde_json::from_value(serde_json::json!({})).expect("absent → None");
        assert_eq!(absent.include_reasoning, None);
    }
    #[test]
    fn subagent_permission_mode_precedence() {
        let own = PermissionMode::DontAsk;
        let cases = [
            (
                PermissionMode::BypassPermissions,
                PermissionMode::BypassPermissions,
            ),
            (PermissionMode::AcceptEdits, PermissionMode::AcceptEdits),
            (PermissionMode::Auto, PermissionMode::Auto),
            (PermissionMode::Default, own.clone()),
            (PermissionMode::DontAsk, own.clone()),
        ];
        for (parent, expected) in cases {
            assert_eq!(
                resolve_subagent_permission_mode(own.clone(), &parent),
                expected,
                "parent={parent:?}"
            );
        }
    }
    #[test]
    fn inject_url_derived_headers_skips_proxy_headers_for_external_url() {
        let mut headers = IndexMap::new();
        inject_url_derived_headers(&mut headers, None, "https://api.example.com/v1");
        assert!(headers.get("X-Grow-Token-Auth").is_none());
        assert!(headers.get("x-authenticateresponse").is_none());
    }
    #[test]
    fn inject_url_derived_headers_does_not_overwrite_existing_entries() {
        let mut headers = IndexMap::new();
        headers.insert("X-Grow-Token-Auth".to_string(), "caller-set".to_string());
        inject_url_derived_headers(&mut headers, None, "https://external.example/v1");
        assert_eq!(
            headers.get("X-Grow-Token-Auth").map(String::as_str),
            Some("caller-set"),
        );
    }
    #[test]
    fn parses_toolset_overrides() {
        let raw_config: toml::Value = toml::from_str(
            r#"
            [toolset.bash]
            timeout_secs = 123

            [toolset.ask_user_question]
            timeout_enabled = false
            timeout_secs = 30
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw_config).expect("config should parse");
        assert_eq!(cfg.toolset.bash.timeout_secs, Some(123.0));
        assert_eq!(cfg.toolset.ask_user_question.timeout_enabled, Some(false));
        assert_eq!(cfg.toolset.ask_user_question.timeout_secs, Some(30));
    }
    #[test]
    fn parses_toolset_bash_float_timeout() {
        let raw_config: toml::Value = toml::from_str(
            r#"
            [toolset.bash]
            timeout_secs = 30.5
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw_config).expect("config should parse");
        assert_eq!(cfg.toolset.bash.timeout_secs, Some(30.5));
    }
    #[test]
    fn resolve_aux_model_honors_grow_build_override() {
        let endpoints = EndpointsConfig::default();
        let mut catalog = IndexMap::new();
        catalog.insert(
            "grow-build".to_string(),
            test_model_entry(
                "v9m-rl-learnability-tp8",
                "https://vendor.example/v1",
                Some("vendor-key"),
                None,
                None,
            ),
        );
        let resolved = resolve_aux_model_sampling_config("grow-build", &catalog, None)
            .expect("override entry has an API key, so resolution succeeds");
        assert_eq!(resolved.model, "v9m-rl-learnability-tp8");
        assert_eq!(resolved.base_url, "https://vendor.example/v1");
        assert_eq!(resolved.api_key.as_deref(), Some("vendor-key"));
    }
    /// Cold cache falls back to the session model, never the configured service proxy;
    /// warm cache serves the provider token at the provider endpoint.
    #[tokio::test]
    async fn aux_model_with_auth_provider_never_reroutes() {
        let provider = crate::auth::AuthProviderRef::new(
            "aux-provider-test".into(),
            crate::auth::AuthProviderConfig {
                command: "printf aux-token".into(),
                args: None,
                token_ttl_secs: Some(3600),
                timeout_secs: None,
                cwd: None,
                ..Default::default()
            },
        );
        let mut entry = test_model_entry("m", "https://litellm.example/v1", None, None, None);
        entry.auth_provider = Some(provider.clone());
        let mut catalog = IndexMap::new();
        catalog.insert("proxied-aux".to_string(), entry);
        assert!(
            resolve_aux_model_sampling_config("proxied-aux", &catalog, None).is_none(),
            "cold provider cache must not reroute the aux model through the configured service proxy"
        );
        let _ = provider.ensure_fresh_token(None).await;
        let resolved = resolve_aux_model_sampling_config("proxied-aux", &catalog, None)
            .expect("warm cache resolves");
        assert_eq!(resolved.base_url, "https://litellm.example/v1");
        assert_eq!(resolved.api_key.as_deref(), Some("aux-token"));
    }
    #[test]
    fn invalid_mcp_server_stub_does_not_fail_config_load() {
        let raw_config: toml::Value = toml::from_str(
            r#"
            [mcp_servers.github]
            enabled = false

            mcp_servers.broken = "not-a-table"

            [mcp_servers.also_broken]
            enabled = "yes"

            [mcp_servers.linear]
            command = "npx"
            args = ["-y", "mcp-remote", "https://mcp.linear.app/mcp"]
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw_config)
            .expect("bad mcp stubs must be dropped, not fail whole config");
        assert!(
            !cfg.mcp_servers.contains_key("broken"),
            "non-table entry is dropped"
        );
        assert!(
            !cfg.mcp_servers.contains_key("also_broken"),
            "wrong-type enabled is dropped"
        );
        assert!(
            !cfg.mcp_servers.contains_key("github"),
            "transport-less stub is dropped (disable via disabled_mcp_servers)"
        );
        assert!(
            cfg.mcp_servers.contains_key("linear"),
            "valid MCP neighbor must still load"
        );
        assert!(cfg.mcp_servers["linear"].enabled);
    }
    /// The lenient parser warns per problem and never fails the whole
    /// config.
    #[test]
    fn auth_provider_parse_warnings_are_lenient_and_specific() {
        use super::super::config_model_override_parse::{ConfigWarningKind, WarningTarget};
        let raw_config: toml::Value = toml::from_str(
            r#"
            [auth_provider.good]
            command = "printf ok"

            [auth_provider.bad-type]
            command = "printf x"
            token_ttl_secs = "not-a-number"

            [auth_provider.typo]
            command = "printf y"
            timeout_seconds = 5

            [auth_provider.commandless]
            token_ttl_secs = 60

            [auth_provider.short-ttl]
            command = "printf x"
            token_ttl_secs = 60

            [auth_provider.zero-timeout]
            command = "printf x"
            timeout_secs = 0

            [auth_provider.slow]
            command = "printf x"
            timeout_secs = 601

            [provider.orphaned.options]
            base_url = "https://x.example/v1"
            auth_provider = "does-not-exist"

            [provider.orphaned.models.m]
            context_window = 200000
            "#,
        )
        .unwrap();
        let cfg =
            Config::new_from_toml_cfg(&raw_config).expect("one bad table must not fail the config");
        assert!(cfg.auth_providers.contains_key("good"));
        assert!(
            !cfg.auth_providers.contains_key("bad-type"),
            "malformed entry is skipped (fails closed)"
        );
        let has_provider = |name: &str, field: Option<&str>, kind: ConfigWarningKind| {
            cfg.config_warnings.iter().any(|w| {
                w.kind == kind
                    && matches!(
                        &w.target,
                        WarningTarget::AuthProvider { name: n, field: f }
                            if n == name && f.as_deref() == field
                    )
            })
        };
        assert!(has_provider(
            "bad-type",
            None,
            ConfigWarningKind::InvalidValue
        ));
        assert!(has_provider(
            "typo",
            Some("timeout_seconds"),
            ConfigWarningKind::UnknownField
        ));
        assert!(has_provider(
            "commandless",
            Some("command"),
            ConfigWarningKind::InvalidValue
        ));
        assert!(has_provider(
            "short-ttl",
            Some("token_ttl_secs"),
            ConfigWarningKind::InvalidValue
        ));
        assert!(has_provider(
            "zero-timeout",
            Some("timeout_secs"),
            ConfigWarningKind::InvalidValue
        ));
        assert!(has_provider(
            "slow",
            Some("timeout_secs"),
            ConfigWarningKind::InvalidValue
        ));
        let provider_reason = |name: &str| {
            cfg.config_warnings
                .iter()
                .find(|w| {
                    matches!(&w.target, WarningTarget::AuthProvider { name: n, field: f }
                        if n == name && f.as_deref() == Some("timeout_secs"))
                })
                .map(|w| w.reason.as_str())
                .unwrap_or_default()
                .to_owned()
        };
        assert!(provider_reason("zero-timeout").contains("clamped to 1"));
        assert!(provider_reason("slow").contains("clamped to 600"));
        assert!(
            cfg.config_warnings.iter().any(|w| {
                w.kind == ConfigWarningKind::InvalidValue
                    && matches!(
                        &w.target,
                        WarningTarget::ModelProvider { field, .. }
                            if field.as_deref() == Some("auth_provider")
                    )
            }),
            "undefined reference warns at parse time: {:?}",
            cfg.config_warnings
        );
        let raw_config: toml::Value = toml::from_str(r#"auth_provider = "oops""#).unwrap();
        let cfg = Config::new_from_toml_cfg(&raw_config)
            .expect("a non-table auth_provider must not fail the config");
        assert!(cfg.auth_providers.is_empty());
        assert!(
            cfg.config_warnings.iter().any(|w| {
                matches!(w.target, WarningTarget::AuthProviderSection)
                    && w.kind == ConfigWarningKind::NotATable
            }),
            "non-table section warns: {:?}",
            cfg.config_warnings
        );
    }
    #[test]
    fn shell_environment_policy_typo_does_not_fail_config() {
        let cfg: toml::Value = toml::from_str(
            r#"
            [shell_environment_policy]
            inhert = "core"
            exclude = 123
            "#,
        )
        .unwrap();
        Config::new_from_toml_cfg(&cfg).expect("a policy typo must not fail the config");
    }
    #[test]
    fn shell_environment_policy_known_keys_track_the_policy_struct() {
        let tools::util::ShellEnvironmentPolicy {
            inherit: _,
            ignore_default_excludes: _,
            exclude: _,
            set: _,
            include_only: _,
        } = tools::util::ShellEnvironmentPolicy::default();
        let ShellEnvironmentPolicyKnownKeys {
            inherit: _,
            ignore_default_excludes: _,
            exclude: _,
            set: _,
            include_only: _,
        } = ShellEnvironmentPolicyKnownKeys::default();
    }

    #[test]
    fn provider_model_output_limit_overrides_global_default() {
        let raw_config: toml::Value = toml::from_str(
            r#"
            [models]
            output_limit = 65536

            [provider.deepseek.options]
            base_url = "https://api.deepseek.com/v1"
            api_key = "sk-test"

            [provider.deepseek.models.deepseek-v4-pro]
            context_window = 1048576
            output_limit = 131072

            [provider.deepseek.models.deepseek-chat]
            context_window = 131072
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw_config).expect("config should parse");
        let resolved = resolve_model_list(&cfg);
        let model = resolved
            .get("deepseek/deepseek-v4-pro")
            .expect("provider model should exist");

        assert_eq!(model.info.context_window.get(), 1_048_576);
        assert_eq!(model.info.output_limit, Some(131_072));
        assert_eq!(
            resolved
                .get("deepseek/deepseek-chat")
                .expect("provider model should inherit global defaults")
                .info
                .output_limit,
            Some(65_536)
        );
    }

    #[test]
    fn provider_model_without_output_limit_keeps_it_unset() {
        let raw_config: toml::Value = toml::from_str(
            r#"
            [provider.local.options]
            base_url = "http://localhost:11434/v1"
            api_key = "local"

            [provider.local.models.qwen]
            context_window = 131072
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw_config).expect("config should parse");
        let resolved = resolve_model_list(&cfg);
        assert_eq!(
            resolved
                .get("local/qwen")
                .expect("provider model should exist")
                .info
                .output_limit,
            None
        );
    }

    #[test]
    fn parses_auth_provider_tables_and_model_reference() {
        let raw_config: toml::Value = toml::from_str(
            r#"
            [auth_provider.litellm]
            command = "/usr/local/bin/litellm-token"
            args = ["--scope", "corp"]
            token_ttl_secs = 3600
            timeout_secs = 10

            [provider.corp.options]
            base_url = "https://litellm.corp.example/v1"
            auth_provider = "litellm"

            [provider.corp.models.claude-sonnet-4-5]
            context_window = 200000
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw_config).expect("config should parse");
        let configured = cfg.auth_providers.get("litellm").expect("provider");
        assert_eq!(configured.command, "/usr/local/bin/litellm-token");
        assert_eq!(configured.args, Some(vec!["--scope".into(), "corp".into()]));
        assert_eq!(configured.token_ttl_secs, Some(3600));
        assert_eq!(configured.timeout_secs, Some(10));
        let resolved = resolve_model_list(&cfg);
        let model = resolved
            .get("corp/claude-sonnet-4-5")
            .expect("model should exist");
        let provider = model
            .auth_provider
            .as_ref()
            .expect("model should reference the provider");
        assert_eq!(provider.name, "litellm");
        assert_eq!(provider.config.command, "/usr/local/bin/litellm-token");
        assert_eq!(provider.config.token_ttl_secs, Some(3600));
        assert!(
            model.has_own_credentials(),
            "provider-backed models classify as BYOK (session token must not leak)"
        );
    }
    #[test]
    fn undefined_auth_provider_fails_closed() {
        let raw_config: toml::Value = toml::from_str(
            r#"
            [provider.orphan.options]
            base_url = "https://third-party.example/v1"
            auth_provider = "nope"

            [provider.orphan.models.m]
            context_window = 200000
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw_config).expect("config should parse");
        let resolved = resolve_model_list(&cfg);
        let model = resolved.get("orphan/m").expect("model should exist");
        let provider = model.auth_provider.as_ref().unwrap();
        assert_eq!(provider.name, "nope");
        assert!(
            provider.config.command.is_empty(),
            "undefined provider keeps an empty command"
        );
        assert!(model.has_own_credentials());
        let creds = resolve_credentials(model);
        assert_eq!(creds.api_key, None);
    }
    #[tokio::test]
    async fn resolve_credentials_serves_cached_provider_token() {
        let mut model = test_model_entry("m", "https://litellm.example/v1", None, None, None);
        let provider = crate::auth::AuthProviderRef::new(
            "resolve-creds-test".into(),
            crate::auth::AuthProviderConfig {
                command: "printf provider-minted-token".into(),
                args: None,
                token_ttl_secs: Some(3600),
                timeout_secs: None,
                cwd: None,
                ..Default::default()
            },
        );
        model.auth_provider = Some(provider.clone());
        let creds = resolve_credentials(&model);
        assert_eq!(creds.api_key, None, "cold cache must not run the command");
        let _ = provider.ensure_fresh_token(None).await;
        let creds = resolve_credentials(&model);
        assert_eq!(creds.api_key.as_deref(), Some("provider-minted-token"));
        assert_eq!(creds.base_url, "https://litellm.example/v1");
    }
    /// A set `env_key` shadows even a warm provider cache at resolve time, so
    /// the static credential wins on the wire and the provider never governs.
    #[tokio::test]
    async fn set_env_key_shadows_warm_provider_at_resolve_time() {
        use test_support::EnvGuard;
        let var = "GROW_TEST_ENVKEY_SHADOW";
        let _guard = EnvGuard::set(var, "env-token");
        let mut model = test_model_entry("m", "https://litellm.example/v1", None, Some(var), None);
        let provider = crate::auth::AuthProviderRef::new(
            "env-shadow-test".into(),
            crate::auth::AuthProviderConfig {
                command: "printf provider-token".into(),
                args: None,
                token_ttl_secs: Some(3600),
                timeout_secs: None,
                cwd: None,
                ..Default::default()
            },
        );
        model.auth_provider = Some(provider.clone());
        let _ = provider.ensure_fresh_token(None).await;
        assert_eq!(
            model.effective_auth_provider().map(|p| p.name.as_str()),
            None,
            "a resolvable env_key shadows the provider"
        );
        let creds = resolve_credentials(&model);
        assert_eq!(
            creds.api_key.as_deref(),
            Some("env-token"),
            "a set env_key must win over a warm provider cache"
        );
    }
    fn test_model_entry(
        model: &str,
        base_url: &str,
        api_key: Option<&str>,
        env_key: Option<&str>,
        _api_base_url: Option<&str>,
    ) -> ModelEntry {
        ModelEntry {
            info: ModelInfo {
                user_selectable: true,
                id: None,
                model: model.to_string(),
                base_url: base_url.to_string(),
                name: None,
                description: None,
                output_limit: None,
                temperature: None,
                top_p: None,
                api_backend: ApiBackend::default(),
                auth_scheme: Default::default(),
                extra_headers: IndexMap::new(),
                query_params: IndexMap::new(),
                env_http_headers: IndexMap::new(),
                context_window: NonZeroU64::new(200_000).unwrap(),
                auto_compact_threshold_percent: None,
                system_prompt_label: None,
                use_concise: false,
                agent_type: default_agent_type(),
                inference_idle_timeout_secs: None,
                max_retries: None,
                hidden: false,
                reasoning_effort: None,
                supports_reasoning_effort: false,
                reasoning_efforts: Vec::new(),
                compactions_remaining: None,
                compaction_at_tokens: None,
                show_model_fingerprint: false,
                stream_tool_calls: None,
                laziness_detector: LazinessDetectorPerModelConfig::default(),
            },
            api_key: api_key.map(|s| s.to_string()),
            env_key: env_key.map(EnvKeys::single),
            auth_provider: None,
        }
    }
    #[test]
    fn sampling_config_uses_model_api_key_over_fallback() {
        let model = test_model_entry(
            "test-model",
            "https://test.api/v1",
            Some("model-specific-key"),
            None,
            None,
        );
        let sampling_config = sampling_config_for_model(&model, resolve_credentials(&model), None);
        assert_eq!(
            sampling_config.api_key,
            Some("model-specific-key".to_string())
        );
        assert_eq!(sampling_config.base_url, "https://test.api/v1");
    }
    #[test]
    fn sampling_config_uses_fallback_when_no_model_api_key() {
        let model = test_model_entry("test-model", "https://test.api/v1", None, None, None);
        let sampling_config = sampling_config_for_model(
            &model,
            ResolvedCredentials {
                api_key: Some("fallback-key".to_string()),
                base_url: model.info().base_url.clone(),
                auth_scheme: AuthScheme::Bearer,
            },
            None,
        );
        assert_eq!(sampling_config.api_key, Some("fallback-key".to_string()));
    }
    #[test]
    fn env_keys_deser_string_or_array() {
        let one: EnvKeys = serde_json::from_str(r#""ANTHROPIC_AUTH_TOKEN""#).unwrap();
        assert_eq!(one.names(), vec!["ANTHROPIC_AUTH_TOKEN"]);
        let many: EnvKeys =
            serde_json::from_str(r#"["ANTHROPIC_AUTH_TOKEN", "LC_ANTHROPIC_AUTH_TOKEN"]"#).unwrap();
        assert_eq!(
            many.names(),
            vec!["ANTHROPIC_AUTH_TOKEN", "LC_ANTHROPIC_AUTH_TOKEN"]
        );
        let ser = serde_json::to_value(&one).unwrap();
        assert_eq!(ser, serde_json::json!("ANTHROPIC_AUTH_TOKEN"));
        let ser_many = serde_json::to_value(&many).unwrap();
        assert_eq!(
            ser_many,
            serde_json::json!(["ANTHROPIC_AUTH_TOKEN", "LC_ANTHROPIC_AUTH_TOKEN"])
        );
    }
    #[test]
    fn env_keys_resolve_first_set_wins() {
        let keys = EnvKeys::new(["GROW_TEST_ENV_KEY_PRIMARY", "GROW_TEST_ENV_KEY_FALLBACK"]);
        assert_eq!(keys.resolve_value_with(|_| None), None, "none set");
        assert_eq!(
            keys.resolve_value_with(
                |n| (n == "GROW_TEST_ENV_KEY_FALLBACK").then(|| "from-fallback".into())
            ),
            Some("from-fallback".into())
        );
        assert_eq!(
            keys.resolve_value_with(|n| match n {
                "GROW_TEST_ENV_KEY_PRIMARY" => Some("from-primary".into()),
                "GROW_TEST_ENV_KEY_FALLBACK" => Some("from-fallback".into()),
                _ => None,
            }),
            Some("from-primary".into()),
            "primary wins when both set"
        );
        assert_eq!(
            keys.resolve_value_with(|n| match n {
                "GROW_TEST_ENV_KEY_PRIMARY" => Some(String::new()),
                "GROW_TEST_ENV_KEY_FALLBACK" => Some("from-fallback".into()),
                _ => None,
            }),
            Some("from-fallback".into())
        );
    }
    #[test]
    fn env_keys_single_and_array_are_semantically_equal() {
        let from_array: EnvKeys = serde_json::from_str(r#"["X"]"#).unwrap();
        assert_eq!(EnvKeys::new(["X"]), from_array);
        let from_string: EnvKeys = serde_json::from_str(r#""X""#).unwrap();
        assert_eq!(EnvKeys::new(["X"]), from_string);
    }
    #[test]
    fn env_keys_resolve_skips_whitespace_only_value() {
        let keys = EnvKeys::new(["GROW_TEST_WS_PRIMARY", "GROW_TEST_WS_FALLBACK"]);
        assert_eq!(
            keys.resolve_value_with(|n| match n {
                "GROW_TEST_WS_PRIMARY" => Some("   ".into()),
                "GROW_TEST_WS_FALLBACK" => Some("real".into()),
                _ => None,
            }),
            Some("real".into())
        );
        assert_eq!(
            EnvKeys::single("GROW_TEST_WS_ONLY").resolve_value_with(|_| Some("   ".into())),
            None
        );
        assert_eq!(
            EnvKeys::single("GROW_TEST_WS_PAD").resolve_value_with(|_| Some("  tok  ".into())),
            Some("  tok  ".into())
        );
    }
    #[test]
    #[serial]
    fn first_own_credential_empty_api_key_falls_through_to_env_key() {
        use test_support::EnvGuard;
        let var = "GROW_TEST_FIRST_OWN_CRED_ENV";
        let _guard = EnvGuard::set(var, "env-token");
        let env_key = EnvKeys::single(var);
        assert_eq!(
            first_own_credential(Some("   "), Some(&env_key)).as_deref(),
            Some("env-token")
        );
        assert_eq!(
            first_own_credential(Some("real-key"), Some(&env_key)).as_deref(),
            Some("real-key")
        );
    }
    #[test]
    #[serial]
    fn resolve_credentials_multi_env_key_uses_lc_alias() {
        let primary = "GROW_TEST_MULTI_ENV_PRIMARY";
        let alias = "GROW_TEST_MULTI_ENV_LC_ALIAS";
        unsafe {
            std::env::remove_var(primary);
            std::env::set_var(alias, "token-via-lc-alias");
        }
        let mut model = test_model_entry("m", "https://inference.example/v1", None, None, None);
        model.env_key = Some(EnvKeys::new([primary, alias]));
        assert!(
            model.has_own_credentials(),
            "alias alone should satisfy has_own_credentials"
        );
        let creds = resolve_credentials(&model);
        assert_eq!(creds.api_key.as_deref(), Some("token-via-lc-alias"));
        unsafe {
            std::env::remove_var(alias);
            std::env::set_var(primary, "token-via-primary");
            std::env::set_var(alias, "token-via-lc-alias");
        }
        let creds = resolve_credentials(&model);
        assert_eq!(
            creds.api_key.as_deref(),
            Some("token-via-primary"),
            "exact primary wins over LC alias when both set"
        );
        unsafe {
            std::env::remove_var(primary);
            std::env::remove_var(alias);
        }
    }
    /// Regression: BYOK env-var auth must stay ApiKey even when signed in,
    /// otherwise the bearer resolver overwrites the BYOK key with a session JWT.
    #[test]
    #[serial_test::serial]
    fn resolve_credentials_env_key_byok_keeps_api_key_auth_with_session() {
        let env_var = "REGRESSION_BYOK_TOKEN_FOR_AUTH_TYPE_TEST";
        unsafe {
            std::env::set_var(env_var, "sk-byok-test-value");
        }
        let model = test_model_entry(
            "byok-gpt-test",
            "https://llm.example.com/v1",
            None,
            Some(env_var),
            None,
        );
        assert!(model.has_own_credentials());
        let creds = resolve_credentials(&model);
        assert_eq!(
            creds.api_key.as_deref(),
            Some("sk-byok-test-value"),
            "api_key must be the env value, not the session JWT",
        );
        unsafe {
            std::env::remove_var(env_var);
        }
    }
    fn api_key_creds(base_url: &str) -> ResolvedCredentials {
        ResolvedCredentials {
            api_key: Some("provider-secret".to_string()),
            base_url: base_url.to_string(),
            auth_scheme: Default::default(),
        }
    }
    #[test]
    fn x_api_key_auth_scheme_flows_from_config_to_sampler() {
        let mut model = test_model_entry(
            "messages-compatible-model",
            "https://messages.example.com/v1",
            Some("sk-ant-test-key"),
            None,
            None,
        );
        model.info.api_backend = ApiBackend::Messages;
        model.info.auth_scheme = AuthScheme::XApiKey;
        let creds = resolve_credentials(&model);
        assert_eq!(creds.auth_scheme, AuthScheme::XApiKey);
        assert_eq!(creds.api_key, Some("sk-ant-test-key".to_string()));
        let config = sampling_config_for_model(&model, creds, None);
        assert_eq!(config.auth_scheme, AuthScheme::XApiKey);
        assert_eq!(config.api_backend, ApiBackend::Messages);
        let client = sampler::SamplingClient::new(config).expect("client should build");
        let info = client.auth_info();
        assert_eq!(info.auth_type, "x-api-key");
    }
    #[test]
    fn auth_scheme_defaults_to_bearer_when_not_set_in_config() {
        let model = test_model_entry(
            "grow-4.5",
            "https://api.example.com/v1",
            Some("sk-openai-test"),
            None,
            None,
        );
        assert_eq!(model.info.auth_scheme, AuthScheme::Bearer);
        let creds = resolve_credentials(&model);
        assert_eq!(creds.auth_scheme, AuthScheme::Bearer);
        let config = sampling_config_for_model(&model, creds, None);
        assert_eq!(config.auth_scheme, AuthScheme::Bearer);
        let client = sampler::SamplingClient::new(config).expect("client should build");
        let info = client.auth_info();
        assert_eq!(info.auth_type, "bearer");
    }
    #[test]
    fn has_own_credentials_guards_session_vs_external_key() {
        let config_model = test_model_entry(
            "my-model",
            "https://api.example.com/v1",
            Some("sk-external"),
            None,
            None,
        );
        assert!(config_model.has_own_credentials());
    }
    #[test]
    fn resolve_model_auth_facts_empty_model_id_is_unknown() {
        assert_eq!(
            resolve_model_auth_facts_and_provider("").0.byok,
            ModelByok::Unknown
        );
    }
    #[test]
    fn config_override_applies_show_model_fingerprint() {
        let override_on = ConfigModelOverride {
            show_model_fingerprint: Some(true),
            ..Default::default()
        };
        let entry = override_on.apply("some-model", None);
        assert!(
            entry.info.show_model_fingerprint,
            "Some(true) override should enable show_model_fingerprint"
        );
        let mut base = ModelEntry::fallback("some-model");
        base.info.show_model_fingerprint = true;
        let override_absent = ConfigModelOverride::default();
        let entry = override_absent.apply("some-model", Some(base));
        assert!(
            entry.info.show_model_fingerprint,
            "None override should preserve the base entry's show_model_fingerprint"
        );
        let mut base = ModelEntry::fallback("some-model");
        base.info.show_model_fingerprint = true;
        let override_off = ConfigModelOverride {
            show_model_fingerprint: Some(false),
            ..Default::default()
        };
        let entry = override_off.apply("some-model", Some(base));
        assert!(
            !entry.info.show_model_fingerprint,
            "Some(false) override should disable show_model_fingerprint over a true base"
        );
    }
    #[test]
    fn default_auto_compact_threshold_is_none() {
        let cfg = Config::default();
        assert_eq!(cfg.session.auto_compact_threshold_percent, None);
    }
    #[test]
    fn parses_auto_compact_threshold_percent() {
        let raw_config: toml::Value = toml::from_str(
            r#"
            [session]
            auto_compact_threshold_percent = 75
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw_config).expect("config should parse");
        assert_eq!(cfg.session.auto_compact_threshold_percent, Some(75));
    }
    #[test]
    fn permission_prompt_timeouts_resolve_defaults_and_session_kind() {
        let session = SessionConfig::default();
        assert_eq!(
            session.permission_prompt_timeout(false),
            std::time::Duration::from_secs(DEFAULT_PERMISSION_PROMPT_TIMEOUT_SECS)
        );
        assert_eq!(
            session.permission_prompt_timeout(true),
            std::time::Duration::from_secs(DEFAULT_NON_INTERACTIVE_PERMISSION_PROMPT_TIMEOUT_SECS)
        );
    }
    #[test]
    fn parses_custom_permission_prompt_timeouts() {
        let raw_config: toml::Value = toml::from_str(
            r#"
            [session]
            permission_prompt_timeout_secs = 120
            non_interactive_permission_prompt_timeout_secs = 7
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw_config).expect("config should parse");
        assert_eq!(cfg.session.permission_prompt_timeout_secs, 120);
        assert_eq!(
            cfg.session.non_interactive_permission_prompt_timeout_secs,
            7
        );
    }
    #[test]
    fn rejects_zero_permission_prompt_timeouts() {
        for field in [
            "permission_prompt_timeout_secs",
            "non_interactive_permission_prompt_timeout_secs",
        ] {
            let raw_config: toml::Value =
                toml::from_str(&format!("[session]\n{field} = 0\n")).unwrap();
            let error = Config::new_from_toml_cfg(&raw_config)
                .expect_err("zero must not restore an infinite wait");
            assert!(
                error.contains("greater than 0"),
                "unexpected error: {error}"
            );
        }
    }
    #[test]
    fn compaction_mode_precedence_env_over_config_over_remote_over_default() {
        use chat_state::CompactionMode;
        assert_eq!(
            resolve_compaction_mode_from(Some("transcript"), Some("segments"), Some("summary")),
            CompactionMode::Transcript
        );
        assert_eq!(
            resolve_compaction_mode_from(None, Some("segments"), Some("summary")),
            CompactionMode::Segments(chat_state::CompactionDetail::default())
        );
        assert_eq!(
            resolve_compaction_mode_from(None, None, Some("segments")),
            CompactionMode::Segments(chat_state::CompactionDetail::default())
        );
        assert_eq!(
            resolve_compaction_mode_from(Some("garbage"), None, Some("segments")),
            CompactionMode::Segments(chat_state::CompactionDetail::default())
        );
        assert_eq!(
            resolve_compaction_mode_from(None, None, None),
            CompactionMode::Summary
        );
    }
    /// Detail shares the env>config>remote>default combinator that the mode
    /// test exercises; the detail-specific facts are remote settings routing and the
    /// `Verbose` default (with unrecognized values falling through).
    #[test]
    fn compaction_detail_resolves_remote_settings_and_verbose_default() {
        use chat_state::CompactionDetail;
        assert_eq!(
            resolve_compaction_detail_from(None, None, Some("minimal")),
            CompactionDetail::Minimal
        );
        assert_eq!(
            resolve_compaction_detail_from(Some("garbage"), None, None),
            CompactionDetail::Verbose
        );
    }
    #[test]
    fn auto_compact_threshold_percent_defaults_when_not_specified() {
        let raw_config: toml::Value = toml::from_str(
            r#"
            [toolset.bash]
            timeout_secs = 123
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw_config).expect("config should parse");
        assert_eq!(cfg.session.auto_compact_threshold_percent, None);
    }
    #[test]
    fn parses_repo_changes_dedup_config() {
        let raw_config: toml::Value = toml::from_str(
            r#"
            [repo_changes_dedup]
            enabled = false
            include_inline_fallback = true
            max_inline_bytes = 1024
            dedup_untracked = false
            dedup_binary = false
            untracked_max_bytes = 2048
            untracked_exclude_globs = ["*.zip", "tmp/**"]
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw_config).expect("config should parse");
        let dedup = cfg.repo_changes_dedup;
        assert!(!dedup.enabled);
        assert!(dedup.include_inline_fallback);
        assert_eq!(dedup.max_inline_bytes, 1024);
        assert!(!dedup.dedup_untracked);
        assert!(!dedup.dedup_binary);
        assert_eq!(dedup.untracked_max_bytes, 2048);
        assert_eq!(dedup.untracked_exclude_globs, vec!["*.zip", "tmp/**"]);
    }
    #[test]
    fn sampling_config_context_window_from_entry_or_default() {
        let model = test_model_entry("any-model", "https://api.example.com/v1", None, None, None);
        let config = sampling_config_for_model(&model, resolve_credentials(&model), None);
        assert_eq!(config.context_window, 200_000);
        let mut model =
            test_model_entry("any-model", "https://api.example.com/v1", None, None, None);
        model.info.context_window = NonZeroU64::new(256_000).unwrap();
        let config = sampling_config_for_model(&model, resolve_credentials(&model), None);
        assert_eq!(config.context_window, 256_000);
    }
    #[test]
    fn sampling_config_uses_model_api_backend() {
        let mut model =
            test_model_entry("test-model", "https://api.example.com/v1", None, None, None);
        model.info.api_backend = ApiBackend::Responses;
        let sampling_config = sampling_config_for_model(&model, resolve_credentials(&model), None);
        assert_eq!(sampling_config.api_backend, ApiBackend::Responses);
    }
    #[test]
    fn model_info_from_config_propagates_use_concise() {
        let entry = ModelEntryConfig {
            id: None,
            model: "test".to_string(),
            base_url: "https://test.api/v1".to_string(),
            name: None,
            description: None,
            output_limit: None,
            temperature: None,
            top_p: None,
            api_key: None,
            env_key: None,
            api_backend: ApiBackend::default(),
            auth_scheme: None,
            extra_headers: IndexMap::new(),
            context_window: NonZeroU64::new(200_000).unwrap(),
            auto_compact_threshold_percent: None,
            system_prompt_label: None,
            use_concise: true,
            agent_type: default_agent_type(),
            inference_idle_timeout_secs: None,
            max_retries: None,
            hidden: false,
            reasoning_effort: None,
            supports_reasoning_effort: false,
            reasoning_efforts: Vec::new(),
            compactions_remaining: None,
            compaction_at_tokens: None,
            show_model_fingerprint: false,
            stream_tool_calls: None,
            laziness_detector: LazinessDetectorPerModelConfig::default(),
        };
        let info = ModelInfo::from_config(&entry);
        assert!(info.use_concise);
    }
    #[test]
    fn agent_selection_config_defaults_to_none() {
        let cfg = Config::default();
        assert!(cfg.agent.name.is_none());
        assert!(cfg.agent.definition.is_none());
    }
    #[test]
    fn parses_agent_selection_name() {
        let raw_config: toml::Value = toml::from_str(
            r#"
            [agent]
            name = "my-custom-agent"
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw_config).expect("config should parse");
        assert_eq!(cfg.agent.name.as_deref(), Some("my-custom-agent"));
        assert!(cfg.agent.definition.is_none());
    }
    #[test]
    fn parses_agent_selection_definition_path() {
        let raw_config: toml::Value = toml::from_str(
            r#"
            [agent]
            definition = "/path/to/my-agent.md"
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw_config).expect("config should parse");
        assert!(cfg.agent.name.is_none());
        assert_eq!(
            cfg.agent.definition.as_deref(),
            Some(std::path::Path::new("/path/to/my-agent.md"))
        );
    }
    #[test]
    fn parses_agent_selection_both_name_and_definition() {
        let raw_config: toml::Value = toml::from_str(
            r#"
            [agent]
            name = "fallback-agent"
            definition = "/path/to/primary-agent.md"
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw_config).expect("config should parse");
        assert_eq!(cfg.agent.name.as_deref(), Some("fallback-agent"));
        assert_eq!(
            cfg.agent.definition.as_deref(),
            Some(std::path::Path::new("/path/to/primary-agent.md"))
        );
    }
    #[test]
    fn agent_selection_not_specified_uses_defaults() {
        let raw_config: toml::Value = toml::from_str(
            r#"
            [toolset.bash]
            timeout_secs = 123
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw_config).expect("config should parse");
        assert!(cfg.agent.name.is_none());
        assert!(cfg.agent.definition.is_none());
    }
    #[test]
    fn model_info_from_config_propagates_agent_type() {
        let entry = ModelEntryConfig {
            id: None,
            model: "test".to_string(),
            base_url: "https://test.api/v1".to_string(),
            name: None,
            description: None,
            output_limit: None,
            temperature: None,
            top_p: None,
            api_key: None,
            env_key: None,
            api_backend: ApiBackend::default(),
            auth_scheme: None,
            extra_headers: IndexMap::new(),
            context_window: NonZeroU64::new(200_000).unwrap(),
            auto_compact_threshold_percent: None,
            system_prompt_label: None,
            use_concise: false,
            agent_type: "custom-harness".to_string(),
            inference_idle_timeout_secs: None,
            max_retries: None,
            hidden: false,
            reasoning_effort: None,
            supports_reasoning_effort: false,
            reasoning_efforts: Vec::new(),
            compactions_remaining: None,
            compaction_at_tokens: None,
            show_model_fingerprint: false,
            stream_tool_calls: None,
            laziness_detector: LazinessDetectorPerModelConfig::default(),
        };
        let info = ModelInfo::from_config(&entry);
        assert_eq!(info.agent_type, "custom-harness");
    }
    #[test]
    fn acp_model_meta_includes_agent_type_when_present() {
        let mut models = IndexMap::new();
        let mut entry = test_model_entry("test-model", "https://test.api/v1", None, None, None);
        entry.info.name = Some("Test Model".to_string());
        entry.info.context_window = NonZeroU64::new(256_000).unwrap();
        entry.info.agent_type = "custom-harness".to_string();
        models.insert("test-model".to_string(), entry);
        let acp_models = to_acp_model_info(&models);
        let acp_model = acp_models.values().next().expect("should have one model");
        let meta = acp_model.meta.as_ref().expect("meta should be present");
        assert_eq!(meta["agentType"], "custom-harness");
        assert_eq!(meta["totalContextTokens"], 256_000);
    }
    #[test]
    fn acp_model_meta_always_includes_agent_type() {
        let mut models = IndexMap::new();
        let mut entry = test_model_entry("plain-model", "https://test.api/v1", None, None, None);
        entry.info.name = Some("Plain Model".to_string());
        entry.info.context_window = NonZeroU64::new(256_000).unwrap();
        models.insert("plain-model".to_string(), entry);
        let acp_models = to_acp_model_info(&models);
        let acp_model = acp_models.values().next().expect("should have one model");
        let meta = acp_model.meta.as_ref().expect("meta should be present");
        assert_eq!(meta["totalContextTokens"], 256_000);
        assert_eq!(
            meta["agentType"], DEFAULT_AGENT_TYPE,
            "agentType should always be in meta, defaulting to DEFAULT_AGENT_TYPE"
        );
    }
    #[test]
    fn acp_model_meta_emits_reasoning_effort_when_supported() {
        let mut models = IndexMap::new();
        let mut entry = test_model_entry("m", "https://test.api/v1", None, None, None);
        entry.info.supports_reasoning_effort = true;
        entry.info.reasoning_effort = Some(ReasoningEffort::High);
        models.insert("m".to_string(), entry);
        let meta = to_acp_model_info(&models)
            .values()
            .next()
            .unwrap()
            .meta
            .clone()
            .unwrap();
        assert_eq!(meta["supportsReasoningEffort"], true);
        assert_eq!(meta["reasoningEffort"], "high");
    }
    #[test]
    fn acp_model_meta_supports_without_default_effort() {
        let mut models = IndexMap::new();
        let mut entry = test_model_entry("m", "https://test.api/v1", None, None, None);
        entry.info.supports_reasoning_effort = true;
        models.insert("m".to_string(), entry);
        let meta = to_acp_model_info(&models)
            .values()
            .next()
            .unwrap()
            .meta
            .clone()
            .unwrap();
        assert_eq!(meta["supportsReasoningEffort"], true);
        assert!(meta.get("reasoningEffort").is_none());
    }
    #[test]
    fn acp_model_meta_emits_reasoning_efforts_and_derives_legacy() {
        let mut models = IndexMap::new();
        let mut entry = test_model_entry("m", "https://test.api/v1", None, None, None);
        entry.info.reasoning_efforts = vec![
            ReasoningEffortOption {
                id: "deep".to_string(),
                value: ReasoningEffort::Xhigh,
                label: "Deep".to_string(),
                description: None,
                default: false,
            },
            ReasoningEffortOption {
                id: "high".to_string(),
                value: ReasoningEffort::High,
                label: "High".to_string(),
                description: None,
                default: true,
            },
        ];
        entry.info.derive_reasoning_effort_fields();
        models.insert("m".to_string(), entry);
        let meta = to_acp_model_info(&models)
            .values()
            .next()
            .unwrap()
            .meta
            .clone()
            .unwrap();
        assert_eq!(meta[REASONING_EFFORTS_META_KEY][0]["id"], "deep");
        assert_eq!(meta[REASONING_EFFORTS_META_KEY][0]["value"], "xhigh");
        assert_eq!(meta["supportsReasoningEffort"], true);
        assert_eq!(meta["reasoningEffort"], "high");
    }
    #[test]
    fn acp_model_meta_omits_reasoning_efforts_when_list_empty() {
        let mut models = IndexMap::new();
        let mut entry = test_model_entry("m", "https://test.api/v1", None, None, None);
        entry.info.supports_reasoning_effort = true;
        entry.info.reasoning_effort = Some(ReasoningEffort::Medium);
        models.insert("m".to_string(), entry);
        let meta = to_acp_model_info(&models)
            .values()
            .next()
            .unwrap()
            .meta
            .clone()
            .unwrap();
        assert!(meta.get(REASONING_EFFORTS_META_KEY).is_none());
        assert_eq!(meta["supportsReasoningEffort"], true);
        assert_eq!(meta["reasoningEffort"], "medium");
    }
    #[test]
    fn acp_model_meta_keeps_explicit_scalar_when_list_present() {
        let mut models = IndexMap::new();
        let mut entry = test_model_entry("m", "https://test.api/v1", None, None, None);
        entry.info.reasoning_effort = Some(ReasoningEffort::Low);
        entry.info.reasoning_efforts = vec![ReasoningEffortOption {
            id: "high".to_string(),
            value: ReasoningEffort::High,
            label: "High".to_string(),
            description: None,
            default: true,
        }];
        entry.info.derive_reasoning_effort_fields();
        models.insert("m".to_string(), entry);
        let meta = to_acp_model_info(&models)
            .values()
            .next()
            .unwrap()
            .meta
            .clone()
            .unwrap();
        assert_eq!(meta["supportsReasoningEffort"], true);
        assert_eq!(meta["reasoningEffort"], "low");
    }
    #[test]
    fn acp_model_meta_leaves_default_unset_for_catalog_fallback() {
        let mut models = IndexMap::new();
        let mut entry = test_model_entry("m", "https://test.api/v1", None, None, None);
        entry.info.reasoning_efforts = vec![
            ReasoningEffortOption {
                id: "balanced".to_string(),
                value: ReasoningEffort::Medium,
                label: "Balanced".to_string(),
                description: None,
                default: false,
            },
            ReasoningEffortOption {
                id: "deep".to_string(),
                value: ReasoningEffort::Xhigh,
                label: "Deep".to_string(),
                description: None,
                default: false,
            },
        ];
        entry.info.derive_reasoning_effort_fields();
        models.insert("m".to_string(), entry);
        let meta = to_acp_model_info(&models)
            .values()
            .next()
            .unwrap()
            .meta
            .clone()
            .unwrap();
        assert_eq!(meta["supportsReasoningEffort"], true);
        assert!(meta.get("reasoningEffort").is_none());
    }
    #[test]
    fn acp_model_meta_omits_reasoning_when_unsupported() {
        let mut models = IndexMap::new();
        let mut entry = test_model_entry("m", "https://test.api/v1", None, None, None);
        entry.info.reasoning_effort = Some(ReasoningEffort::High);
        models.insert("m".to_string(), entry);
        let meta = to_acp_model_info(&models)
            .values()
            .next()
            .unwrap()
            .meta
            .clone();
        if let Some(meta) = meta {
            assert!(meta.get("supportsReasoningEffort").is_none());
            assert!(meta.get("reasoningEffort").is_none());
        }
    }
    #[test]
    fn acp_model_meta_always_has_context_window() {
        let mut models = IndexMap::new();
        let mut entry = test_model_entry("unknown-model", "https://test.api/v1", None, None, None);
        entry.info.name = Some("Unknown Model".to_string());
        models.insert("unknown-model".to_string(), entry);
        let acp_models = to_acp_model_info(&models);
        let meta = acp_models.values().next().unwrap().meta.as_ref().unwrap();
        assert_eq!(meta["totalContextTokens"], 200_000);
    }
    #[test]
    fn disabled_models_removed_from_catalog() {
        use crate::agent::models::resolve_model_catalog;
        let raw: toml::Value = toml::from_str(
            r#"
            [models]
            disabled_models = ["to-disable"]
            [model.to-disable]
            model = "to-disable"
            base_url = "https://api.example.com/v1"
            context_window = 200000
            "#,
        )
        .unwrap();
        let catalog = resolve_model_catalog(&Config::new_from_toml_cfg(&raw).unwrap());
        assert!(!catalog.contains_key("to-disable"));
    }
    #[test]
    fn invalid_glob_is_rejected_by_validation() {
        use crate::agent::models::ModelGlobSet;
        assert!(ModelGlobSet::compile(Some(&vec!["grow[".to_string()])).is_err());
        let raw: toml::Value = toml::from_str(
            r#"
            [models]
            allowed_models = ["grow["]
            "#,
        )
        .unwrap();
        let err = Config::new_from_toml_cfg(&raw)
            .unwrap()
            .validate_model_filters()
            .unwrap_err();
        assert!(
            err.contains("allowed_models"),
            "error should name the offending field: {err}"
        );
    }
    #[test]
    fn inference_idle_timeout_propagates_to_model_info() {
        let entry = ModelEntryConfig {
            id: None,
            model: "test".to_string(),
            base_url: "https://test.api/v1".to_string(),
            name: None,
            description: None,
            output_limit: None,
            temperature: None,
            top_p: None,
            api_key: None,
            env_key: None,
            api_backend: ApiBackend::default(),
            auth_scheme: None,
            extra_headers: IndexMap::new(),
            context_window: NonZeroU64::new(200_000).unwrap(),
            auto_compact_threshold_percent: None,
            system_prompt_label: None,
            use_concise: false,
            agent_type: default_agent_type(),
            inference_idle_timeout_secs: Some(120),
            max_retries: None,
            hidden: false,
            reasoning_effort: None,
            supports_reasoning_effort: false,
            reasoning_efforts: Vec::new(),
            compactions_remaining: None,
            compaction_at_tokens: None,
            show_model_fingerprint: false,
            stream_tool_calls: None,
            laziness_detector: LazinessDetectorPerModelConfig::default(),
        };
        let info = ModelInfo::from_config(&entry);
        assert_eq!(info.inference_idle_timeout_secs, Some(120));
    }

    #[test]
    fn parsed_config_has_models_config() {
        let raw: toml::Value = toml::from_str(
            r#"
            [models]
            default = "my-enterprise-model"
            session_summary = "title-model"
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw).expect("config should parse");
        assert_eq!(cfg.models.default.as_deref(), Some("my-enterprise-model"));
        assert_eq!(cfg.models.session_summary.as_deref(), Some("title-model"));
    }
    #[test]
    fn config_models_default_beats_optional_remote_setting() {
        let config_default = Some("custom-byok-model");
        let remote_settings_default = Some("remote-settings-model");
        let resolved = resolve_string_flag(
            None,
            "GROW_DEFAULT_MODEL_TEST_NONEXISTENT",
            config_default,
            remote_settings_default,
        );
        let resolved = resolved.expect("should resolve to a value");
        assert_eq!(resolved.value, "custom-byok-model");
        assert_eq!(
            resolved.source,
            ConfigSource::Config,
            "[models] default from config.toml must beat remote settings"
        );
    }
    #[test]
    fn e2e_acp_model_info_no_dedup_on_model_field() {
        let mut models = IndexMap::new();
        models.insert(
            "default-grow".to_string(),
            test_model_entry(
                "same-upstream-model",
                "https://service.example.com/v1",
                None,
                None,
                Some("https://api.example.com/v1"),
            ),
        );
        models.insert(
            "acme-grow".to_string(),
            test_model_entry(
                "same-upstream-model",
                "https://inference.example.com/v1",
                Some("enterprise-key"),
                None,
                None,
            ),
        );
        let acp_models = to_acp_model_info(&models);
        assert_eq!(
            acp_models.len(),
            2,
            "both entries should survive in ACP model list"
        );
        assert!(
            acp_models.contains_key(&acp::ModelId::new("default-grow")),
            "default entry should be addressable by map key"
        );
        assert!(
            acp_models.contains_key(&acp::ModelId::new("acme-grow")),
            "user entry should be addressable by map key"
        );
    }
    /// Unset every env var that `EndpointsConfig::default()` reads for endpoints,
    /// so the cli-chat-proxy resolver tests below are deterministic regardless of
    /// the ambient environment. Gated behind `#[serial]`.
    fn unset_endpoint_env_vars() {
        for k in [
            "GROW_CLI_CHAT_PROXY_BASE_URL",
            "GROW_INFERENCE_BASE_URL",
            "GROW_MANAGED_CONFIG_URL",
        ] {
            unsafe { std::env::remove_var(k) };
        }
    }
    /// INVARIANT: auxiliary-service resolvers resolve to the cli-chat-proxy, never
    /// `inference_base_url` — overriding ONLY inference keeps every aux endpoint on
    /// the proxy; explicit per-service overrides win verbatim.
    #[test]
    #[serial]
    fn aux_endpoints_resolve_to_proxy_never_inference() {
        unset_endpoint_env_vars();
        let inference = "https://inference.acme-corp.example/provider/v1";
        let cfg = EndpointsConfig {
            inference_base_url: inference.to_string(),
            cli_chat_proxy_base_url: None,
            ..Default::default()
        };
        assert_eq!(cfg.proxy_url(), CLI_CHAT_PROXY_BASE_URL_DEFAULT);
        assert_eq!(cfg.resolve_managed_config_url(), None);
        assert_eq!(cfg.inference_base_url, inference);
        let overridden = EndpointsConfig {
            cli_chat_proxy_base_url: Some("https://proxy.enterprise.example/v1".to_string()),
            managed_config_url: Some(
                "https://control.enterprise.example/deployment/config".to_string(),
            ),
            ..Default::default()
        };
        assert_eq!(
            overridden.proxy_url(),
            "https://proxy.enterprise.example/v1"
        );
        assert_eq!(
            overridden.resolve_managed_config_url().as_deref(),
            Some("https://control.enterprise.example/deployment/config")
        );
    }
    /// REGRESSION: the managed-config URL never follows `inference_base_url`
    /// through the full loader `Config::new_from_toml_cfg` — a distinct construction
    /// path from `from_config_value`, so the deployment key never reaches the
    /// inference host on either.
    #[test]
    #[serial]
    fn loader_managed_config_url_never_follows_inference_endpoint() {
        unset_endpoint_env_vars();
        let cfg = Config::new_from_toml_cfg(
            &toml::from_str(
                r#"[endpoints]
                inference_base_url = "https://inference.acme-corp.example/provider/v1""#,
            )
            .unwrap(),
        )
        .expect("config should parse");
        assert!(cfg.endpoints.cli_chat_proxy_base_url.is_none());
        assert_eq!(cfg.endpoints.resolve_managed_config_url(), None);
    }
    #[test]
    #[serial]
    fn resolve_session_recap_defaults_to_true_when_unset() {
        unsafe { std::env::remove_var("GROW_SESSION_RECAP") };
        let cfg = Config::default();
        let r = cfg.resolve_session_recap();
        assert!(r.value, "session_recap should be true by default");
        assert_eq!(r.source, ConfigSource::Default);
    }
    #[test]
    #[serial]
    fn resolve_session_recap_config_off_overrides_default() {
        unsafe { std::env::remove_var("GROW_SESSION_RECAP") };
        let cfg = Config {
            features: Features {
                session_recap: Some(false),
                ..Default::default()
            },
            ..Default::default()
        };
        let r = cfg.resolve_session_recap();
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::Config);
    }
    #[test]
    #[serial]
    fn resolve_session_recap_env_off_overrides_default() {
        unsafe { std::env::set_var("GROW_SESSION_RECAP", "0") };
        let cfg = Config::default();
        let r = cfg.resolve_session_recap();
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::Env);
        unsafe { std::env::remove_var("GROW_SESSION_RECAP") };
    }
    #[test]
    #[serial]
    fn resolve_session_recap_remote_off_overrides_default() {
        unsafe { std::env::remove_var("GROW_SESSION_RECAP") };
        let cfg = Config {
            remote_settings: Some(crate::util::config::RemoteSettings {
                session_recap: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let r = cfg.resolve_session_recap();
        assert!(
            !r.value,
            "remote settings/remote false must kill-switch default on"
        );
        assert_eq!(r.source, ConfigSource::Remote);
    }
    /// Precedence: env > config.toml > remote settings > default(false). One test
    /// covers the full ladder so we do not maintain a matrix of flag cases.
    #[test]
    #[serial]
    fn resolve_two_pass_compaction_precedence() {
        unsafe { std::env::remove_var("GROW_TWO_PASS_COMPACTION") };
        let default_cfg = Config::default();
        let r = default_cfg.resolve_two_pass_compaction();
        assert!(!r.value, "default is opt-in off");
        assert_eq!(r.source, ConfigSource::Default);
        let remote_on = Config {
            remote_settings: Some(crate::util::config::RemoteSettings {
                two_pass_compaction_enabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        let r = remote_on.resolve_two_pass_compaction();
        assert!(r.value);
        assert_eq!(r.source, ConfigSource::Remote);
        let config_over_remote = Config {
            features: Features {
                two_pass_compaction: Some(true),
                ..Default::default()
            },
            remote_settings: Some(crate::util::config::RemoteSettings {
                two_pass_compaction_enabled: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let r = config_over_remote.resolve_two_pass_compaction();
        assert!(r.value);
        assert_eq!(r.source, ConfigSource::Config);
        unsafe { std::env::set_var("GROW_TWO_PASS_COMPACTION", "0") };
        let r = config_over_remote.resolve_two_pass_compaction();
        assert!(!r.value, "env wins over config + remote");
        assert_eq!(r.source, ConfigSource::Env);
        unsafe { std::env::remove_var("GROW_TWO_PASS_COMPACTION") };
    }
    /// Gate precedence: env > `[doom_loop_recovery]` > remote settings >
    /// default(ON), with the remote layer merged PER-FIELD from the nested
    /// `doom_loop_recovery` object and each layer's `false` an independent
    /// kill switch. One test covers the full ladder (the
    /// `resolve_two_pass_compaction_precedence` pattern).
    #[test]
    #[serial]
    fn resolve_doom_loop_recovery_precedence() {
        use crate::util::config::DoomLoopRecoverySettings;
        unsafe { std::env::remove_var("GROW_DOOM_LOOP_RECOVERY") };
        let default_cfg = Config::default();
        let p = default_cfg
            .resolve_doom_loop_recovery()
            .expect("default is ON");
        assert_eq!(p.max_threshold, 8, "default tunables unchanged");
        assert_eq!(p.max_retries, 2, "default tunables unchanged");
        let toml_off = Config {
            doom_loop_recovery: DoomLoopRecoverySettings {
                enabled: Some(false),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(
            toml_off.resolve_doom_loop_recovery().is_none(),
            "TOML kill switch"
        );
        let remote_off = Config {
            remote_settings: Some(crate::util::config::RemoteSettings {
                doom_loop_recovery: Some(DoomLoopRecoverySettings {
                    enabled: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(
            remote_off.resolve_doom_loop_recovery().is_none(),
            "remote settings kill switch"
        );
        unsafe { std::env::set_var("GROW_DOOM_LOOP_RECOVERY", "0") };
        assert!(
            default_cfg.resolve_doom_loop_recovery().is_none(),
            "env kill switch"
        );
        unsafe { std::env::remove_var("GROW_DOOM_LOOP_RECOVERY") };
        let remote_on = Config {
            remote_settings: Some(crate::util::config::RemoteSettings {
                doom_loop_recovery: Some(DoomLoopRecoverySettings {
                    enabled: Some(true),
                    max_threshold: Some(16),
                    max_retries: Some(1),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let p = remote_on.resolve_doom_loop_recovery().expect("remote on");
        assert_eq!(p.max_threshold, 16);
        assert_eq!(p.max_retries, 1);
        let partial_remote = Config {
            remote_settings: Some(crate::util::config::RemoteSettings {
                doom_loop_recovery: Some(DoomLoopRecoverySettings {
                    max_threshold: Some(16),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let p = partial_remote
            .resolve_doom_loop_recovery()
            .expect("default-on gate despite remote object omitting enabled");
        assert_eq!(p.max_threshold, 16, "remote tunable applies");
        assert_eq!(p.max_retries, 2, "unset field falls to the default");
        let config_over_remote = Config {
            doom_loop_recovery: DoomLoopRecoverySettings {
                enabled: Some(true),
                max_threshold: Some(4),
                max_retries: Some(3),
            },
            remote_settings: Some(crate::util::config::RemoteSettings {
                doom_loop_recovery: Some(DoomLoopRecoverySettings {
                    enabled: Some(false),
                    max_threshold: Some(16),
                    max_retries: Some(1),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let p = config_over_remote
            .resolve_doom_loop_recovery()
            .expect("config on beats remote kill-switch");
        assert_eq!(p.max_threshold, 4);
        assert_eq!(p.max_retries, 3);
        unsafe { std::env::set_var("GROW_DOOM_LOOP_RECOVERY", "0") };
        assert!(
            config_over_remote.resolve_doom_loop_recovery().is_none(),
            "env wins over config + remote"
        );
        unsafe { std::env::remove_var("GROW_DOOM_LOOP_RECOVERY") };
    }
    /// The `[doom_loop_recovery]` TOML section deserializes through the
    /// standard config path (no bespoke parser).
    #[test]
    #[serial]
    fn doom_loop_recovery_section_parses_from_toml() {
        unsafe { std::env::remove_var("GROW_DOOM_LOOP_RECOVERY") };
        let raw: toml::Value = toml::from_str(
            r#"
            [doom_loop_recovery]
            enabled = true
            max_threshold = 12
            max_retries = 1
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw).unwrap();
        assert_eq!(cfg.doom_loop_recovery.enabled, Some(true));
        let p = cfg.resolve_doom_loop_recovery().expect("enabled via toml");
        assert_eq!(p.max_threshold, 12);
        assert_eq!(p.max_retries, 1);
    }
    /// `[worktree.auto_gc]` deserializes through Config and resolve honors it.
    #[test]
    #[serial]
    fn worktree_auto_gc_section_parses_from_toml() {
        unsafe {
            std::env::remove_var(fast_worktree::ENV_AUTO_GC);
            std::env::remove_var(fast_worktree::ENV_AUTO_GC_DRY_RUN);
            std::env::remove_var(fast_worktree::ENV_AUTO_GC_MAX_AGE);
        }
        let raw: toml::Value = toml::from_str(
            r#"
            [worktree.auto_gc]
            enabled = true
            max_age_secs = 7200
            min_interval_secs = 120
            dry_run = true
            [worktree.auto_gc.max_age_by_kind]
            subagent = 3600
            manual = "never"
            "#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw).unwrap();
        assert_eq!(cfg.worktree.auto_gc.enabled, Some(true));
        assert_eq!(cfg.worktree.auto_gc.max_age_secs, Some(7200));
        let p = cfg.resolve_worktree_auto_gc();
        assert!(p.enabled);
        assert_eq!(p.max_age_secs, 7200);
        assert_eq!(p.min_interval_secs, 120);
        assert!(p.dry_run);
        assert_eq!(
            p.max_age_by_kind
                .get(&fast_worktree::WorktreeKind::Subagent),
            Some(&Some(3600))
        );
        assert_eq!(
            p.max_age_by_kind.get(&fast_worktree::WorktreeKind::Manual),
            Some(&None)
        );
    }
    /// Out-of-range tunables clamp instead of being honored or dropped.
    #[test]
    #[serial]
    fn resolve_doom_loop_recovery_clamps_tunables() {
        use crate::util::config::DoomLoopRecoverySettings;
        unsafe { std::env::remove_var("GROW_DOOM_LOOP_RECOVERY") };
        let cfg = Config {
            doom_loop_recovery: DoomLoopRecoverySettings {
                enabled: Some(true),
                max_threshold: Some(1_000),
                max_retries: Some(99),
            },
            ..Default::default()
        };
        let p = cfg.resolve_doom_loop_recovery().expect("enabled");
        assert_eq!(p.max_threshold, 64);
        assert_eq!(p.max_retries, 5);
        let cfg = Config {
            doom_loop_recovery: DoomLoopRecoverySettings {
                enabled: Some(true),
                max_threshold: Some(0),
                max_retries: Some(0),
            },
            ..Default::default()
        };
        let p = cfg.resolve_doom_loop_recovery().expect("enabled");
        assert_eq!(p.max_threshold, 2);
        assert_eq!(p.max_retries, 0, "0 retries is valid (observe-only)");
    }
    #[test]
    #[serial]
    fn resolve_goal_defaults_to_true_when_unset() {
        unsafe { std::env::remove_var("GROW_GOAL") };
        let cfg = Config::default();
        let r = cfg.resolve_goal();
        assert!(r.value, "goal should be on by default");
        assert_eq!(r.source, ConfigSource::Default);
    }
    #[test]
    #[serial]
    fn resolve_goal_env_overrides_config_without_remote_kill_switch() {
        unsafe { std::env::set_var("GROW_GOAL", "1") };
        let mut cfg = Config::default();
        cfg.goal.enabled = Some(false);
        let r = cfg.resolve_goal();
        assert_eq!(r.source, ConfigSource::Env);
        assert!(r.value);
        unsafe { std::env::remove_var("GROW_GOAL") };
    }
    #[test]
    #[serial]
    fn resolve_goal_remote_false_kills_local_opt_in() {
        unsafe { std::env::set_var("GROW_GOAL", "1") };
        let mut cfg = Config::default();
        cfg.goal.enabled = Some(true);
        cfg.remote_settings = Some(crate::util::config::RemoteSettings {
            goal_enabled: Some(false),
            ..Default::default()
        });
        let r = cfg.resolve_goal();
        assert_eq!(r.source, ConfigSource::Remote);
        assert!(!r.value);
        unsafe { std::env::remove_var("GROW_GOAL") };
    }
    #[test]
    #[serial]
    fn resolve_goal_remote_settings_used_when_no_local() {
        unsafe { std::env::remove_var("GROW_GOAL") };
        let cfg = Config {
            remote_settings: Some(crate::util::config::RemoteSettings {
                goal_enabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        let r = cfg.resolve_goal();
        assert_eq!(r.source, ConfigSource::Remote);
        assert!(r.value);
    }
    /// The remote settings `goal_enabled: false` kill-switch must still win over
    /// the default-on fallback.
    #[test]
    #[serial]
    fn resolve_goal_remote_settings_kill_switch_overrides_default_on() {
        unsafe { std::env::remove_var("GROW_GOAL") };
        let cfg = Config {
            remote_settings: Some(crate::util::config::RemoteSettings {
                goal_enabled: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let r = cfg.resolve_goal();
        assert_eq!(r.source, ConfigSource::Remote);
        assert!(!r.value);
    }
    #[test]
    #[serial]
    fn background_workflows_default_on_without_affecting_goal() {
        unsafe { std::env::remove_var("GROW_WORKFLOWS") };
        let cfg = Config::default();
        let r = cfg.resolve_workflows();
        assert!(r.value);
        assert_eq!(r.source, ConfigSource::Default);
        assert!(cfg.resolve_goal().value);
    }
    #[test]
    #[serial]
    fn resolve_workflows_remote_settings_enables() {
        unsafe { std::env::remove_var("GROW_WORKFLOWS") };
        let cfg = Config {
            remote_settings: Some(crate::util::config::RemoteSettings {
                workflows_enabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        let r = cfg.resolve_workflows();
        assert_eq!(r.source, ConfigSource::Remote);
        assert!(r.value);
    }
    #[test]
    #[serial]
    fn resolve_workflows_remote_false_kills_local_opt_in() {
        unsafe { std::env::set_var("GROW_WORKFLOWS", "1") };
        let mut cfg = Config::default();
        cfg.workflows.enabled = Some(true);
        cfg.remote_settings = Some(crate::util::config::RemoteSettings {
            workflows_enabled: Some(false),
            ..Default::default()
        });
        let r = cfg.resolve_workflows();
        assert_eq!(r.source, ConfigSource::Remote);
        assert!(!r.value);
        unsafe { std::env::remove_var("GROW_WORKFLOWS") };
    }
    #[test]
    #[serial]
    fn resolve_workflows_env_wins() {
        unsafe { std::env::set_var("GROW_WORKFLOWS", "0") };
        let cfg = Config::default();
        let r = cfg.resolve_workflows();
        assert_eq!(r.source, ConfigSource::Env);
        assert!(
            !r.value,
            "env must be able to kill the default-on workflows"
        );
        unsafe { std::env::remove_var("GROW_WORKFLOWS") };
    }
    #[test]
    #[serial]
    fn resolve_ask_user_question_defaults_to_true_when_unset() {
        unsafe { std::env::remove_var("GROW_ASK_USER_QUESTION") };
        let cfg = Config::default();
        let r = cfg.resolve_ask_user_question();
        assert!(r.value, "ask_user_question should be on by default");
        assert_eq!(r.source, ConfigSource::Default);
    }
    #[test]
    #[serial]
    fn resolve_ask_user_question_remote_settings_enables() {
        unsafe { std::env::remove_var("GROW_ASK_USER_QUESTION") };
        let cfg = Config {
            remote_settings: Some(crate::util::config::RemoteSettings {
                ask_user_question_enabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        let r = cfg.resolve_ask_user_question();
        assert_eq!(r.source, ConfigSource::Remote);
        assert!(r.value);
    }
    #[test]
    #[serial]
    fn resolve_ask_user_question_env_overrides_remote_settings() {
        unsafe { std::env::set_var("GROW_ASK_USER_QUESTION", "1") };
        let cfg = Config {
            remote_settings: Some(crate::util::config::RemoteSettings {
                ask_user_question_enabled: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let r = cfg.resolve_ask_user_question();
        assert_eq!(r.source, ConfigSource::Env);
        assert!(r.value);
        unsafe { std::env::remove_var("GROW_ASK_USER_QUESTION") };
    }
    #[test]
    #[serial]
    fn resolve_ask_user_question_config_overrides_remote_settings() {
        unsafe { std::env::remove_var("GROW_ASK_USER_QUESTION") };
        let mut cfg = Config::default();
        cfg.features.ask_user_question = Some(true);
        cfg.remote_settings = Some(crate::util::config::RemoteSettings {
            ask_user_question_enabled: Some(false),
            ..Default::default()
        });
        let r = cfg.resolve_ask_user_question();
        assert_eq!(r.source, ConfigSource::Config);
        assert!(r.value);
    }
    /// remote settings `ask_user_question_enabled: false` is a kill-switch: it must
    /// win over the default-on fallback.
    #[test]
    #[serial]
    fn resolve_ask_user_question_remote_settings_kill_switch_overrides_default_on() {
        unsafe { std::env::remove_var("GROW_ASK_USER_QUESTION") };
        let cfg = Config {
            remote_settings: Some(crate::util::config::RemoteSettings {
                ask_user_question_enabled: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let r = cfg.resolve_ask_user_question();
        assert_eq!(r.source, ConfigSource::Remote);
        assert!(!r.value);
    }
    /// Clear every env var the goal/companion resolvers read so tests
    /// start from a known baseline regardless of run order.
    fn clear_goal_envs() {
        unsafe {
            std::env::remove_var("GROW_GOAL");
            std::env::remove_var("GROW_GOAL_CLASSIFIER");
            std::env::remove_var("GROW_GOAL_PLANNER");
            std::env::remove_var("GROW_GOAL_SUMMARY");
            std::env::remove_var("GROW_GOAL_VERIFIER_N");
            std::env::remove_var("GROW_GOAL_CLASSIFIER_MAX");
            std::env::remove_var("GROW_GOAL_STRATEGIST_EVERY");
            std::env::remove_var("GROW_GOAL_REVERIFY_AFTER");
        }
    }
    fn cfg_with_goal(goal: bool) -> Config {
        Config {
            goal: GoalConfig {
                enabled: Some(goal),
                ..Default::default()
            },
            ..Default::default()
        }
    }
    fn cfg_with_goal_and_remote(goal: bool, remote: crate::util::config::RemoteSettings) -> Config {
        Config {
            goal: GoalConfig {
                enabled: Some(goal),
                ..Default::default()
            },
            remote_settings: Some(remote),
            ..Default::default()
        }
    }
    fn remote_classifier(v: bool) -> crate::util::config::RemoteSettings {
        crate::util::config::RemoteSettings {
            goal_classifier_enabled: Some(v),
            ..Default::default()
        }
    }
    fn remote_planner(v: bool) -> crate::util::config::RemoteSettings {
        crate::util::config::RemoteSettings {
            goal_planner_enabled: Some(v),
            ..Default::default()
        }
    }
    fn remote_summary(v: bool) -> crate::util::config::RemoteSettings {
        crate::util::config::RemoteSettings {
            goal_summary_enabled: Some(v),
            ..Default::default()
        }
    }
    fn cfg_with_goal_config(goal: GoalConfig) -> Config {
        Config {
            goal,
            ..Default::default()
        }
    }
    fn cfg_with_goal_config_and_remote(
        goal: GoalConfig,
        remote: crate::util::config::RemoteSettings,
    ) -> Config {
        Config {
            goal,
            remote_settings: Some(remote),
            ..Default::default()
        }
    }
    #[test]
    #[serial]
    fn resolve_goal_classifier_default_tracks_goal_enabled() {
        clear_goal_envs();
        assert!(
            !cfg_with_goal(false)
                .resolve_goal_classifier_enabled(false)
                .value
        );
        let on = cfg_with_goal(true).resolve_goal_classifier_enabled(true);
        assert!(on.value);
        assert_eq!(on.source, ConfigSource::Default);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_classifier_remote_forces_either_way() {
        clear_goal_envs();
        let off = cfg_with_goal_and_remote(true, remote_classifier(false))
            .resolve_goal_classifier_enabled(true);
        assert!(!off.value);
        assert_eq!(off.source, ConfigSource::Remote);
        let on = cfg_with_goal_and_remote(false, remote_classifier(true))
            .resolve_goal_classifier_enabled(false);
        assert!(on.value);
        assert_eq!(on.source, ConfigSource::Remote);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_classifier_env_overrides_default_and_remote() {
        clear_goal_envs();
        unsafe { std::env::set_var("GROW_GOAL_CLASSIFIER", "0") };
        let r = cfg_with_goal_and_remote(true, remote_classifier(true))
            .resolve_goal_classifier_enabled(true);
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::Env);
        unsafe { std::env::set_var("GROW_GOAL_CLASSIFIER", "1") };
        let r = cfg_with_goal_and_remote(false, remote_classifier(false))
            .resolve_goal_classifier_enabled(false);
        assert!(r.value);
        assert_eq!(r.source, ConfigSource::Env);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_planner_default_tracks_goal_enabled() {
        clear_goal_envs();
        assert!(
            !cfg_with_goal(false)
                .resolve_goal_planner_enabled(false)
                .value
        );
        let on = cfg_with_goal(true).resolve_goal_planner_enabled(true);
        assert!(on.value);
        assert_eq!(on.source, ConfigSource::Default);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_planner_remote_forces_either_way() {
        clear_goal_envs();
        let off = cfg_with_goal_and_remote(true, remote_planner(false))
            .resolve_goal_planner_enabled(true);
        assert!(!off.value);
        assert_eq!(off.source, ConfigSource::Remote);
        let on = cfg_with_goal_and_remote(false, remote_planner(true))
            .resolve_goal_planner_enabled(false);
        assert!(on.value);
        assert_eq!(on.source, ConfigSource::Remote);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_planner_env_overrides_default_and_remote() {
        clear_goal_envs();
        unsafe { std::env::set_var("GROW_GOAL_PLANNER", "0") };
        let r =
            cfg_with_goal_and_remote(true, remote_planner(true)).resolve_goal_planner_enabled(true);
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::Env);
        unsafe { std::env::set_var("GROW_GOAL_PLANNER", "1") };
        let r = cfg_with_goal_and_remote(false, remote_planner(false))
            .resolve_goal_planner_enabled(false);
        assert!(r.value);
        assert_eq!(r.source, ConfigSource::Env);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_summary_default_tracks_goal_enabled() {
        clear_goal_envs();
        assert!(
            !cfg_with_goal(false)
                .resolve_goal_summary_enabled(false)
                .value
        );
        let on = cfg_with_goal(true).resolve_goal_summary_enabled(true);
        assert!(on.value);
        assert_eq!(on.source, ConfigSource::Default);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_summary_remote_forces_either_way() {
        clear_goal_envs();
        let off = cfg_with_goal_and_remote(true, remote_summary(false))
            .resolve_goal_summary_enabled(true);
        assert!(!off.value);
        assert_eq!(off.source, ConfigSource::Remote);
        let on = cfg_with_goal_and_remote(false, remote_summary(true))
            .resolve_goal_summary_enabled(false);
        assert!(on.value);
        assert_eq!(on.source, ConfigSource::Remote);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_summary_env_overrides_default_and_remote() {
        clear_goal_envs();
        unsafe { std::env::set_var("GROW_GOAL_SUMMARY", "0") };
        let r =
            cfg_with_goal_and_remote(true, remote_summary(true)).resolve_goal_summary_enabled(true);
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::Env);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_classifier_config_honored_when_env_unset() {
        clear_goal_envs();
        let r = cfg_with_goal_config(GoalConfig {
            classifier_enabled: Some(true),
            ..Default::default()
        })
        .resolve_goal_classifier_enabled(false);
        assert_eq!(r.source, ConfigSource::Config);
        assert!(r.value);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_classifier_env_beats_config() {
        clear_goal_envs();
        unsafe { std::env::set_var("GROW_GOAL_CLASSIFIER", "0") };
        let r = cfg_with_goal_config(GoalConfig {
            classifier_enabled: Some(true),
            ..Default::default()
        })
        .resolve_goal_classifier_enabled(false);
        assert_eq!(r.source, ConfigSource::Env);
        assert!(!r.value);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_classifier_config_beats_remote() {
        clear_goal_envs();
        let r = cfg_with_goal_config_and_remote(
            GoalConfig {
                classifier_enabled: Some(true),
                ..Default::default()
            },
            remote_classifier(false),
        )
        .resolve_goal_classifier_enabled(false);
        assert_eq!(r.source, ConfigSource::Config);
        assert!(r.value);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_classifier_config_beats_default() {
        clear_goal_envs();
        let r = cfg_with_goal_config(GoalConfig {
            enabled: Some(true),
            classifier_enabled: Some(false),
            ..Default::default()
        })
        .resolve_goal_classifier_enabled(false);
        assert_eq!(r.source, ConfigSource::Config);
        assert!(!r.value);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_planner_config_honored_when_env_unset() {
        clear_goal_envs();
        let r = cfg_with_goal_config(GoalConfig {
            planner_enabled: Some(true),
            ..Default::default()
        })
        .resolve_goal_planner_enabled(false);
        assert_eq!(r.source, ConfigSource::Config);
        assert!(r.value);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_planner_env_beats_config() {
        clear_goal_envs();
        unsafe { std::env::set_var("GROW_GOAL_PLANNER", "0") };
        let r = cfg_with_goal_config(GoalConfig {
            planner_enabled: Some(true),
            ..Default::default()
        })
        .resolve_goal_planner_enabled(false);
        assert_eq!(r.source, ConfigSource::Env);
        assert!(!r.value);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_planner_config_beats_remote() {
        clear_goal_envs();
        let r = cfg_with_goal_config_and_remote(
            GoalConfig {
                planner_enabled: Some(true),
                ..Default::default()
            },
            remote_planner(false),
        )
        .resolve_goal_planner_enabled(false);
        assert_eq!(r.source, ConfigSource::Config);
        assert!(r.value);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_planner_config_beats_default() {
        clear_goal_envs();
        let r = cfg_with_goal_config(GoalConfig {
            enabled: Some(true),
            planner_enabled: Some(false),
            ..Default::default()
        })
        .resolve_goal_planner_enabled(false);
        assert_eq!(r.source, ConfigSource::Config);
        assert!(!r.value);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_summary_config_honored_when_env_unset() {
        clear_goal_envs();
        let r = cfg_with_goal_config(GoalConfig {
            summary_enabled: Some(true),
            ..Default::default()
        })
        .resolve_goal_summary_enabled(false);
        assert_eq!(r.source, ConfigSource::Config);
        assert!(r.value);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_summary_env_beats_config() {
        clear_goal_envs();
        unsafe { std::env::set_var("GROW_GOAL_SUMMARY", "0") };
        let r = cfg_with_goal_config(GoalConfig {
            summary_enabled: Some(true),
            ..Default::default()
        })
        .resolve_goal_summary_enabled(false);
        assert_eq!(r.source, ConfigSource::Env);
        assert!(!r.value);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_summary_config_beats_remote() {
        clear_goal_envs();
        let r = cfg_with_goal_config_and_remote(
            GoalConfig {
                summary_enabled: Some(true),
                ..Default::default()
            },
            remote_summary(false),
        )
        .resolve_goal_summary_enabled(false);
        assert_eq!(r.source, ConfigSource::Config);
        assert!(r.value);
        clear_goal_envs();
    }
    #[test]
    #[serial]
    fn resolve_goal_summary_config_beats_default() {
        clear_goal_envs();
        let r = cfg_with_goal_config(GoalConfig {
            enabled: Some(true),
            summary_enabled: Some(false),
            ..Default::default()
        })
        .resolve_goal_summary_enabled(false);
        assert_eq!(r.source, ConfigSource::Config);
        assert!(!r.value);
        clear_goal_envs();
    }
    #[test]
    fn goal_keys_round_trip_from_toml() {
        let raw: toml::Value = toml::from_str(
            r#"
[goal]
enabled = true
classifier_enabled = true
planner_enabled = false
summary_enabled = true
verifier_count = 4
classifier_max_runs = 7
strategist_every = 3
reverify_after = 6
"#,
        )
        .expect("test TOML should parse");
        let cfg = Config::new_from_toml_cfg(&raw).expect("config should parse");
        assert_eq!(cfg.goal.enabled, Some(true));
        assert_eq!(cfg.goal.classifier_enabled, Some(true));
        assert_eq!(cfg.goal.planner_enabled, Some(false));
        assert_eq!(cfg.goal.summary_enabled, Some(true));
        assert_eq!(cfg.goal.verifier_count, Some(4));
        assert_eq!(cfg.goal.classifier_max_runs, Some(7));
        assert_eq!(cfg.goal.strategist_every, Some(3));
        assert_eq!(cfg.goal.reverify_after, Some(6));
        let empty = Config::new_from_toml_cfg(&toml::from_str("").unwrap()).unwrap();
        assert_eq!(empty.goal.classifier_enabled, None);
        assert_eq!(empty.goal.verifier_count, None);
    }
    const GOAL_USE_CURRENT_ENV: &str = "GROW_GOAL_USE_CURRENT_MODEL_ONLY";
    fn clear_goal_model_env() {
        unsafe { std::env::remove_var(GOAL_USE_CURRENT_ENV) };
    }
    fn planner_pair() -> crate::util::config::GoalRoleModel {
        crate::util::config::GoalRoleModel {
            model: "grow-4".to_string(),
            agent_type: "general-purpose".to_string(),
        }
    }
    fn strategist_pair() -> crate::util::config::GoalRoleModel {
        crate::util::config::GoalRoleModel {
            model: "grow-4.5".to_string(),
            agent_type: "cursor".to_string(),
        }
    }
    #[test]
    #[serial]
    fn goal_use_current_model_only_env_true() {
        clear_goal_model_env();
        unsafe { std::env::set_var(GOAL_USE_CURRENT_ENV, "1") };
        let r = Config::default().resolve_goal_use_current_model_only();
        assert!(r.value);
        assert_eq!(r.source, ConfigSource::Env);
        clear_goal_model_env();
    }
    #[test]
    #[serial]
    fn goal_use_current_model_only_config_true() {
        clear_goal_model_env();
        let cfg = cfg_with_goal_config(GoalConfig {
            use_current_model_only: Some(true),
            ..Default::default()
        });
        let r = cfg.resolve_goal_use_current_model_only();
        assert!(r.value);
        assert_eq!(r.source, ConfigSource::Config);
        clear_goal_model_env();
    }
    #[test]
    #[serial]
    fn goal_use_current_model_only_default_false() {
        clear_goal_model_env();
        let r = Config::default().resolve_goal_use_current_model_only();
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::Default);
        clear_goal_model_env();
    }
    #[test]
    #[serial]
    fn goal_use_current_model_only_env_overrides_config_false() {
        clear_goal_model_env();
        unsafe { std::env::set_var(GOAL_USE_CURRENT_ENV, "1") };
        let cfg = cfg_with_goal_config(GoalConfig {
            use_current_model_only: Some(false),
            ..Default::default()
        });
        let r = cfg.resolve_goal_use_current_model_only();
        assert!(r.value);
        assert_eq!(r.source, ConfigSource::Env);
        clear_goal_model_env();
    }
    fn remote_planner_model(
        p: crate::util::config::GoalRoleModel,
    ) -> crate::util::config::RemoteSettings {
        crate::util::config::RemoteSettings {
            goal_planner_model: Some(p),
            ..Default::default()
        }
    }
    fn remote_strategist_model(
        p: crate::util::config::GoalRoleModel,
    ) -> crate::util::config::RemoteSettings {
        crate::util::config::RemoteSettings {
            goal_strategist_model: Some(p),
            ..Default::default()
        }
    }
    #[test]
    fn resolve_goal_planner_model_kill_switch_inherits() {
        let cfg = cfg_with_goal_config_and_remote(
            GoalConfig::default(),
            remote_planner_model(planner_pair()),
        );
        let r = cfg.resolve_goal_planner_model(true);
        assert_eq!(r.value, GoalRoleModelChoice::InheritCurrent);
        assert_eq!(r.source, ConfigSource::Config);
    }
    #[test]
    fn resolve_goal_planner_model_remote_pair_explicit() {
        let cfg = cfg_with_goal_config_and_remote(
            GoalConfig::default(),
            remote_planner_model(planner_pair()),
        );
        let r = cfg.resolve_goal_planner_model(false);
        assert_eq!(r.value, GoalRoleModelChoice::Explicit(planner_pair()));
        assert_eq!(r.source, ConfigSource::Remote);
    }
    #[test]
    fn resolve_goal_planner_model_config_overrides_remote() {
        let cfg = cfg_with_goal_config_and_remote(
            GoalConfig {
                planner_model: Some(planner_pair()),
                ..Default::default()
            },
            remote_planner_model(strategist_pair()),
        );
        let r = cfg.resolve_goal_planner_model(false);
        assert_eq!(r.value, GoalRoleModelChoice::Explicit(planner_pair()));
        assert_eq!(r.source, ConfigSource::Config);
    }
    #[test]
    fn resolve_goal_planner_model_default_inherits() {
        let r = Config::default().resolve_goal_planner_model(false);
        assert_eq!(r.value, GoalRoleModelChoice::InheritCurrent);
        assert_eq!(r.source, ConfigSource::Default);
    }
    #[test]
    fn resolve_goal_planner_model_remote_present_but_field_absent_inherits() {
        let cfg = cfg_with_goal_config_and_remote(
            GoalConfig::default(),
            remote_strategist_model(strategist_pair()),
        );
        let r = cfg.resolve_goal_planner_model(false);
        assert_eq!(r.value, GoalRoleModelChoice::InheritCurrent);
        assert_eq!(r.source, ConfigSource::Default);
    }
    #[test]
    fn resolve_goal_strategist_model_remote_pair_explicit() {
        let cfg = cfg_with_goal_config_and_remote(
            GoalConfig::default(),
            remote_strategist_model(strategist_pair()),
        );
        let r = cfg.resolve_goal_strategist_model(false);
        assert_eq!(r.value, GoalRoleModelChoice::Explicit(strategist_pair()));
        assert_eq!(r.source, ConfigSource::Remote);
    }
    #[test]
    fn resolve_goal_strategist_model_config_overrides_remote() {
        let cfg = cfg_with_goal_config_and_remote(
            GoalConfig {
                strategist_model: Some(strategist_pair()),
                ..Default::default()
            },
            remote_strategist_model(planner_pair()),
        );
        let r = cfg.resolve_goal_strategist_model(false);
        assert_eq!(r.value, GoalRoleModelChoice::Explicit(strategist_pair()));
        assert_eq!(r.source, ConfigSource::Config);
    }
    #[test]
    fn resolve_goal_skeptic_models_kill_switch_inherits() {
        let cfg = cfg_with_goal_config(GoalConfig {
            skeptic_models: vec![planner_pair(), strategist_pair()],
            ..Default::default()
        });
        let r = cfg.resolve_goal_skeptic_models(true);
        assert!(r.value.is_empty(), "kill-switch ⇒ all skeptics inherit");
        assert_eq!(r.source, ConfigSource::Config);
    }
    #[test]
    fn resolve_goal_skeptic_models_remote_pool_explicit() {
        let remote = crate::util::config::RemoteSettings {
            goal_skeptic_models: vec![planner_pair(), strategist_pair()],
            ..Default::default()
        };
        let r = cfg_with_goal_config_and_remote(GoalConfig::default(), remote)
            .resolve_goal_skeptic_models(false);
        assert_eq!(
            r.value,
            vec![
                GoalRoleModelChoice::Explicit(planner_pair()),
                GoalRoleModelChoice::Explicit(strategist_pair()),
            ]
        );
        assert_eq!(r.source, ConfigSource::Remote);
    }
    #[test]
    fn resolve_goal_skeptic_models_config_pool_overrides_remote_pool() {
        let remote = crate::util::config::RemoteSettings {
            goal_skeptic_models: vec![strategist_pair(), strategist_pair()],
            ..Default::default()
        };
        let cfg = cfg_with_goal_config_and_remote(
            GoalConfig {
                skeptic_models: vec![planner_pair(), strategist_pair()],
                ..Default::default()
            },
            remote,
        );
        let r = cfg.resolve_goal_skeptic_models(false);
        assert_eq!(
            r.value,
            vec![
                GoalRoleModelChoice::Explicit(planner_pair()),
                GoalRoleModelChoice::Explicit(strategist_pair()),
            ]
        );
        assert_eq!(r.source, ConfigSource::Config);
    }
    #[test]
    fn resolve_goal_skeptic_models_no_pool_inherits() {
        let r = Config::default().resolve_goal_skeptic_models(false);
        assert!(r.value.is_empty());
        assert_eq!(r.source, ConfigSource::Default);
    }
    /// `[goal]` model pins parse from both the inline-table and `[[...]]` array forms.
    #[test]
    fn goal_model_pins_parse_from_toml() {
        let toml_str = r#"
[goal]
enabled = true
planner_model = { model = "grow-build", agent_type = "grow" }

[goal.strategist_model]
model = "grow-composer-2.5-fast"
agent_type = "cursor"

[[goal.skeptic_models]]
model = "grow-build"
agent_type = "grow"

[[goal.skeptic_models]]
model = "grow-composer-2.5-fast"
agent_type = "cursor"
"#;
        let raw: toml::Value = toml::from_str(toml_str).unwrap();
        let cfg = Config::new_from_toml_cfg(&raw).unwrap();
        assert_eq!(cfg.goal.planner_model.as_ref().unwrap().model, "grow-build");
        assert_eq!(
            cfg.goal.strategist_model.as_ref().unwrap().agent_type,
            "cursor"
        );
        assert_eq!(cfg.goal.skeptic_models.len(), 2);
        assert_eq!(cfg.goal.skeptic_models[0].model, "grow-build");
        assert_eq!(
            cfg.resolve_goal_planner_model(false).source,
            ConfigSource::Config
        );
    }
    /// A malformed pin must drop to `None`, not fail the whole parse (which
    /// would silently wipe every other setting).
    #[test]
    fn goal_model_pin_malformed_is_dropped_not_fatal() {
        let toml_str = r#"
[goal]
enabled = true
classifier_max_runs = 6
planner_model = { agent_type = "grow" }
"#;
        let raw: toml::Value = toml::from_str(toml_str).unwrap();
        let cfg = Config::new_from_toml_cfg(&raw)
            .expect("malformed planner_model must not fail the whole parse");
        assert!(cfg.goal.planner_model.is_none());
        assert_eq!(cfg.goal.classifier_max_runs, Some(6));
    }
    #[test]
    fn goal_skeptic_models_drop_malformed_entry_keep_rest() {
        let toml_str = r#"
[goal]
enabled = true

[[goal.skeptic_models]]
model = "grow-build"
agent_type = "grow"

[[goal.skeptic_models]]
agent_type = "cursor"

[[goal.skeptic_models]]
model = "grow-composer-2.5-fast"
agent_type = "cursor"
"#;
        let raw: toml::Value = toml::from_str(toml_str).unwrap();
        let cfg = Config::new_from_toml_cfg(&raw).unwrap();
        assert_eq!(cfg.goal.skeptic_models.len(), 2);
        assert_eq!(cfg.goal.skeptic_models[0].model, "grow-build");
        assert_eq!(cfg.goal.skeptic_models[1].model, "grow-composer-2.5-fast");
    }
    /// Acceptance test: a full managed-config `[goal]` block resolves end-to-end,
    /// every value sourced from config (not remote/default).
    #[test]
    #[serial]
    fn full_goal_managed_config_resolves_end_to_end() {
        clear_goal_envs();
        clear_goal_model_env();
        let raw: toml::Value = toml::from_str(
            r#"
[goal]
enabled = true
classifier_enabled = true
planner_enabled = true
verifier_count = 3
classifier_max_runs = 6
planner_model = { model = "grow-build", agent_type = "grow" }
strategist_model = { model = "grow-composer-2.5-fast", agent_type = "cursor" }

[[goal.skeptic_models]]
model = "grow-build"
agent_type = "grow"

[[goal.skeptic_models]]
model = "grow-composer-2.5-fast"
agent_type = "cursor"
"#,
        )
        .unwrap();
        let cfg = Config::new_from_toml_cfg(&raw).expect("[goal] config must parse");
        let grow_build = crate::util::config::GoalRoleModel {
            model: "grow-build".into(),
            agent_type: "grow".into(),
        };
        let composer = crate::util::config::GoalRoleModel {
            model: "grow-composer-2.5-fast".into(),
            agent_type: "cursor".into(),
        };
        let goal_enabled = cfg.resolve_goal().value;
        assert!(goal_enabled);
        assert!(cfg.resolve_goal_classifier_enabled(goal_enabled).value);
        assert!(cfg.resolve_goal_planner_enabled(goal_enabled).value);
        assert_eq!(cfg.resolve_goal_verifier_count().value, 3);
        assert_eq!(cfg.resolve_goal_classifier_max_runs().value, 6);
        let use_current = cfg.resolve_goal_use_current_model_only().value;
        assert!(!use_current);
        let planner = cfg.resolve_goal_planner_model(use_current);
        assert_eq!(
            planner.value,
            GoalRoleModelChoice::Explicit(grow_build.clone())
        );
        assert_eq!(planner.source, ConfigSource::Config);
        assert_eq!(
            cfg.resolve_goal_strategist_model(use_current).value,
            GoalRoleModelChoice::Explicit(composer.clone())
        );
        assert_eq!(
            cfg.resolve_goal_skeptic_models(use_current).value,
            vec![
                GoalRoleModelChoice::Explicit(grow_build),
                GoalRoleModelChoice::Explicit(composer),
            ]
        );
        clear_goal_envs();
        clear_goal_model_env();
    }
    /// Run the production scan (`deserialize_collecting_unrecognized`) on a
    /// TOML string, mirroring the [model] removal + default-merge in
    /// `new_from_toml_cfg`.
    fn unused_keys_from_toml(toml_str: &str) -> Vec<String> {
        let raw: toml::Value = toml::from_str(toml_str).unwrap();
        let raw_without_models = {
            let mut r = raw.clone();
            if let toml::Value::Table(ref mut t) = r {
                t.remove("model");
            }
            r
        };
        let mut base = toml::Value::try_from(Config::default()).unwrap();
        if let toml::Value::Table(ref mut t) = base {
            t.remove("model");
        }
        crate::config::deep_merge_toml(&mut base, &raw_without_models);
        let (_config, unused) =
            Config::deserialize_collecting_unrecognized(base, &raw_without_models)
                .expect("config should deserialize");
        unused
    }
    #[test]
    fn config_warns_on_section_typo() {
        let raw: toml::Value = toml::from_str(
            r#"
            [endpoint]
            deployment_key = "provider-token-test"
        "#,
        )
        .unwrap();
        let config = Config::new_from_toml_cfg(&raw).expect("should parse");
        assert!(config.endpoints.deployment_key.is_none());
        let unused = unused_keys_from_toml(
            r#"
            [endpoint]
            deployment_key = "provider-token-test"
        "#,
        );
        assert!(unused.iter().any(|k| k == "endpoint"), "got: {unused:?}");
    }
    #[test]
    fn known_non_serde_config_paths_are_not_reported_unused() {
        let unused = unused_keys_from_toml(
            r#"
            [features]
            remote_fetch = false
            not_a_real_feature = true
            [slash_command_tags]
            workflows = "new"
        "#,
        );
        assert!(
            unused.iter().any(|k| k == "features.remote_fetch"),
            "removed remote_fetch must be reported as an unknown key: {unused:?}"
        );
        assert!(
            !unused.iter().any(|k| k == "slash_command_tags"),
            "slash_command_tags is a real table: {unused:?}"
        );
        assert!(
            unused.iter().any(|k| k == "features.not_a_real_feature"),
            "real typos still surface: {unused:?}"
        );
    }
    #[test]
    fn config_warns_on_field_typos() {
        let unused = unused_keys_from_toml(
            r#"
            [endpoints]
            deplomyent_key = "test"
            [ui]
            yoloo = true
            [features]
            telmetry = true
        "#,
        );
        assert!(
            unused.iter().any(|k| k == "endpoints.deplomyent_key"),
            "got: {unused:?}"
        );
        assert!(unused.iter().any(|k| k == "ui.yoloo"), "got: {unused:?}");
        assert!(
            unused.iter().any(|k| k == "features.telmetry"),
            "got: {unused:?}"
        );
    }
    #[test]
    fn config_accepts_compact_permission_section() {
        let unused = unused_keys_from_toml(
            r#"
            [permission]
            allow = ["Read(//tmp/**)"]
            deny = ["Bash(rm *)"]
            ask = ["WebFetch"]
        "#,
        );
        assert!(
            unused.is_empty(),
            "false positive on [permission] keys: {unused:?}"
        );
    }
    /// `prompt_policy` is not consumed from any TOML permission section (the
    /// verbose loader keeps only `rules`; prompt policy comes from .claude
    /// settings `defaultMode`), so it must warn rather than be a silent no-op.
    #[test]
    fn permission_prompt_policy_warns_as_unconsumed() {
        let unused = unused_keys_from_toml(
            r#"
            [permission]
            deny = ["Bash(rm *)"]
            prompt_policy = "deny"
        "#,
        );
        assert_eq!(
            unused,
            vec!["permission.prompt_policy".to_string()],
            "an unconsumed key in a security section must be flagged"
        );
    }
    /// A typo'd `[permission]` sub-key must still warn — silently dropping a
    /// misspelled security rule would leave the user believing it's in force.
    #[test]
    fn permission_unknown_subkey_still_warns() {
        let unused = unused_keys_from_toml(
            r#"
            [permission]
            denny = ["Bash(rm *)"]
            ask = ["WebFetch"]
        "#,
        );
        assert_eq!(
            unused,
            vec!["permission.denny".to_string()],
            "exactly the typo'd sub-key must be flagged"
        );
    }
    /// Permission *values* are opaque: a malformed `[[permission.rules]]`
    /// entry neither warns nor fails Config load — the out-of-band loaders
    /// parse it tolerantly and warn per item.
    #[test]
    fn malformed_permission_rules_do_not_fail_config_load() {
        let toml_str = r#"
            [[permission.rules]]
            pattern = 5
        "#;
        let raw: toml::Value = toml::from_str(toml_str).unwrap();
        Config::new_from_toml_cfg(&raw)
            .expect("malformed rule values are the permission loaders' concern");
        let unused = unused_keys_from_toml(toml_str);
        assert!(unused.is_empty(), "got: {unused:?}");
    }
    /// A non-table `[permission]` value still fails Config load (pre-existing
    /// behavior): a fundamentally broken security section should be loud.
    #[test]
    fn non_table_permission_value_fails_config_load() {
        let raw: toml::Value = toml::from_str(r#"permission = "foo""#).unwrap();
        assert!(
            Config::new_from_toml_cfg(&raw).is_err(),
            "non-table [permission] must fail loudly"
        );
    }
    /// Wrong-typed values for the opaque passthrough keys must neither warn
    /// nor fail config load — an admin typo in a managed layer must not brick
    /// startup fleet-wide; the out-of-band consumers degrade gracefully.
    #[test]
    fn wrong_typed_passthrough_value_neither_warns_nor_fails() {
        let toml_str = r#"
            [marketplace]
            default_skills_installs_purged = "yes"
        "#;
        let unused = unused_keys_from_toml(toml_str);
        assert!(unused.is_empty(), "got: {unused:?}");
        let raw: toml::Value = toml::from_str(toml_str).unwrap();
        Config::new_from_toml_cfg(&raw)
            .expect("wrong-typed passthrough values must not fail config load");
    }
    /// Exempting `[permission]` and friends must not swallow warnings for
    /// genuinely unknown keys.
    #[test]
    fn unknown_key_still_warns_next_to_exempt_sections() {
        let unused = unused_keys_from_toml(
            r#"
            [permission]
            deny = ["Bash(rm *)"]
            [marketplace]
            default_skills_installs_purged = true
            [ui]
            yollo = true
        "#,
        );
        assert_eq!(
            unused,
            vec!["ui.yollo".to_string()],
            "exactly the typo'd key must be flagged"
        );
    }
    fn empty_config() -> toml::Value {
        toml::Value::Table(toml::map::Map::new())
    }
    fn clear_runtime_env_vars() {
        unsafe {
            std::env::remove_var("GROW_SUBAGENTS");
            std::env::remove_var("GROW_RESPECT_GITIGNORE");
            std::env::remove_var("GROW_SESSION_SUMMARY_MODEL");
            std::env::remove_var("GROW_CURSOR_SKILLS_ENABLED");
            std::env::remove_var("GROW_CURSOR_RULES_ENABLED");
            std::env::remove_var("GROW_CURSOR_AGENTS_ENABLED");
            std::env::remove_var("GROW_CLAUDE_SKILLS_ENABLED");
            std::env::remove_var("GROW_CLAUDE_RULES_ENABLED");
            std::env::remove_var("GROW_CLAUDE_AGENTS_ENABLED");
        }
    }
    fn clear_managed_mcp_env_vars() {
        unsafe {
            std::env::remove_var("GROW_MANAGED_MCPS_ENABLED");
            std::env::remove_var("GROW_MANAGED_MCP_GATEWAY_TOOLS_ENABLED");
        }
    }
    fn isolate_compat_env() -> Vec<EnvGuard> {
        COMPAT_CELLS
            .into_iter()
            .map(|cell| EnvGuard::unset(cell.env_var()))
            .collect()
    }
    fn parse_compat(source: &str) -> CompatConfigToml {
        let raw: toml::Value = toml::from_str(source).unwrap();
        raw.get("compat").unwrap().clone().try_into().unwrap()
    }
    fn remote_settings_with(
        key: CompatRemoteKey,
        value: bool,
    ) -> crate::util::config::RemoteSettings {
        let mut remote = crate::util::config::RemoteSettings::default();
        match key {
            CompatRemoteKey::CursorSkills => remote.cursor_skills_enabled = Some(value),
            CompatRemoteKey::CursorRules => remote.cursor_rules_enabled = Some(value),
            CompatRemoteKey::CursorAgents => remote.cursor_agents_enabled = Some(value),
            CompatRemoteKey::CursorMcps => remote.cursor_mcps_enabled = Some(value),
            CompatRemoteKey::CursorHooks => remote.cursor_hooks_enabled = Some(value),
            CompatRemoteKey::ClaudeSkills => remote.claude_skills_enabled = Some(value),
            CompatRemoteKey::ClaudeRules => remote.claude_rules_enabled = Some(value),
            CompatRemoteKey::ClaudeAgents => remote.claude_agents_enabled = Some(value),
            CompatRemoteKey::ClaudeMcps => remote.claude_mcps_enabled = Some(value),
            CompatRemoteKey::ClaudeHooks => remote.claude_hooks_enabled = Some(value),
        }
        remote
    }
    #[test]
    #[serial]
    fn resolve_compat_defaults_match_registry() {
        let _env = isolate_compat_env();
        assert_eq!(
            resolve_compat_config(&CompatConfigToml::default(), None),
            CompatConfig::default()
        );
    }
    #[test]
    fn compat_config_cell_is_tolerant_and_fail_closed_per_cell() {
        let raw: toml::Value = toml::from_str(
            r#"
[compat.cursor]
skills = false
rules = "malformed"
[compat.claude]
hooks = true
"#,
        )
        .unwrap();
        let cell = |vendor, surface| {
            COMPAT_CELLS
                .into_iter()
                .find(|cell| cell.vendor() == vendor && cell.surface() == surface)
                .unwrap()
        };
        assert_eq!(
            compat_config_cell(Ok(&raw), cell(CompatVendor::Cursor, CompatSurface::Skills)),
            Ok(Some(false))
        );
        assert_eq!(
            compat_config_cell(Ok(&raw), cell(CompatVendor::Cursor, CompatSurface::Rules)),
            Err(CompatConfigCellError::Malformed)
        );
        assert_eq!(
            compat_config_cell(Ok(&raw), cell(CompatVendor::Claude, CompatSurface::Hooks)),
            Ok(Some(true))
        );
        assert_eq!(
            compat_config_cell(Err(()), cell(CompatVendor::Claude, CompatSurface::Hooks)),
            Err(CompatConfigCellError::Unavailable)
        );
    }
    #[test]
    #[serial]
    fn remote_keys_are_one_hot_and_false_overrides_default() {
        let _env = isolate_compat_env();
        for key in COMPAT_CELLS
            .into_iter()
            .filter_map(|cell| cell.remote_key())
        {
            let remote = remote_settings_with(key, false);
            for cell in COMPAT_CELLS {
                assert_eq!(
                    remote_compat_value(Some(&remote), cell.remote_key()),
                    (cell.remote_key() == Some(key)).then_some(false),
                    "{key:?} mapped to {}.{}",
                    cell.vendor().as_str(),
                    cell.surface().as_str()
                );
            }
        }
        let remote = remote_settings_with(CompatRemoteKey::CursorSkills, false);
        assert!(CompatConfig::default().cursor.skills);
        assert!(
            !resolve_compat_config(&CompatConfigToml::default(), Some(&remote))
                .cursor
                .skills
        );
    }
    #[test]
    #[serial]
    fn resolve_runtime_fields_headless_defaults() {
        clear_runtime_env_vars();
        clear_managed_mcp_env_vars();
        let raw = empty_config();
        let mut cfg = Config::new_from_toml_cfg(&raw).unwrap();
        cfg.resolve_runtime_fields(&RuntimeResolutionContext {
            raw_config: &raw,
            remote_settings: None,
            is_headless: true,
            cli_subagents: None,
            cli_session_summary_model: None,
            cli_experimental_memory: false,
            cli_no_memory: false,
            todo_gate: false,
            laziness_debug_log: None,
        });
        assert!(
            !cfg.managed_mcps_enabled,
            "headless should default managed_mcps to false"
        );
        assert!(!cfg.managed_mcp_gateway_tools_enabled);
    }
    #[test]
    #[serial]
    fn resolve_runtime_fields_managed_gateway_tools_from_remote() {
        clear_runtime_env_vars();
        clear_managed_mcp_env_vars();
        let raw = empty_config();
        let remote = crate::util::config::RemoteSettings {
            managed_mcp_gateway_tools_enabled: Some(true),
            ..Default::default()
        };
        let mut cfg = Config::new_from_toml_cfg(&raw).unwrap();
        cfg.resolve_runtime_fields(&RuntimeResolutionContext {
            raw_config: &raw,
            remote_settings: Some(&remote),
            is_headless: false,
            cli_subagents: None,
            cli_session_summary_model: None,
            cli_experimental_memory: false,
            cli_no_memory: false,
            todo_gate: false,
            laziness_debug_log: None,
        });
        assert!(cfg.managed_mcp_gateway_tools_enabled);
    }
    #[test]
    #[serial]
    fn resolve_runtime_fields_subagents_from_config() {
        clear_runtime_env_vars();
        let raw: toml::Value = toml::from_str("[subagents]\nenabled = true").unwrap();
        let mut cfg = Config::new_from_toml_cfg(&raw).unwrap();
        cfg.resolve_runtime_fields(&RuntimeResolutionContext {
            raw_config: &raw,
            remote_settings: None,
            is_headless: false,
            cli_subagents: None,
            cli_session_summary_model: None,
            cli_experimental_memory: false,
            cli_no_memory: false,
            todo_gate: false,
            laziness_debug_log: None,
        });
        assert!(cfg.subagents_enabled);
    }
    #[test]
    #[serial]
    fn resolve_runtime_fields_cli_subagents_override() {
        clear_runtime_env_vars();
        let raw = empty_config();
        let mut cfg = Config::new_from_toml_cfg(&raw).unwrap();
        cfg.resolve_runtime_fields(&RuntimeResolutionContext {
            raw_config: &raw,
            remote_settings: None,
            is_headless: false,
            cli_subagents: Some(true),
            cli_session_summary_model: None,
            cli_experimental_memory: false,
            cli_no_memory: false,
            todo_gate: false,
            laziness_debug_log: None,
        });
        assert!(cfg.subagents_enabled);
    }
    #[test]
    #[serial]
    fn resolve_runtime_fields_gitignore_from_env() {
        clear_runtime_env_vars();
        unsafe { std::env::set_var("GROW_RESPECT_GITIGNORE", "0") };
        let raw = empty_config();
        let mut cfg = Config::new_from_toml_cfg(&raw).unwrap();
        cfg.resolve_runtime_fields(&RuntimeResolutionContext {
            raw_config: &raw,
            remote_settings: None,
            is_headless: false,
            cli_subagents: None,
            cli_session_summary_model: None,
            cli_experimental_memory: false,
            cli_no_memory: false,
            todo_gate: false,
            laziness_debug_log: None,
        });
        assert!(!cfg.respect_gitignore);
        clear_runtime_env_vars();
    }
    #[test]
    #[serial]
    fn resolve_runtime_fields_aux_model_override_from_cli() {
        clear_runtime_env_vars();
        let raw = empty_config();
        let mut cfg = Config::new_from_toml_cfg(&raw).unwrap();
        cfg.resolve_runtime_fields(&RuntimeResolutionContext {
            raw_config: &raw,
            remote_settings: None,
            is_headless: false,
            cli_subagents: None,
            cli_session_summary_model: Some("custom-ss"),
            cli_experimental_memory: false,
            cli_no_memory: false,
            todo_gate: false,
            laziness_debug_log: None,
        });
        assert_eq!(cfg.session_summary_model, Some("custom-ss".to_owned()));
    }
    #[test]
    #[serial]
    fn resolve_runtime_fields_path_hints_from_remote() {
        clear_runtime_env_vars();
        let raw = empty_config();
        let remote = crate::util::config::RemoteSettings {
            path_not_found_hints: Some(true),
            ..Default::default()
        };
        let mut cfg = Config::new_from_toml_cfg(&raw).unwrap();
        cfg.resolve_runtime_fields(&RuntimeResolutionContext {
            raw_config: &raw,
            remote_settings: Some(&remote),
            is_headless: false,
            cli_subagents: None,
            cli_session_summary_model: None,
            cli_experimental_memory: false,
            cli_no_memory: false,
            todo_gate: false,
            laziness_debug_log: None,
        });
        assert!(cfg.path_not_found_hints);
    }
    #[test]
    #[serial]
    fn resolve_runtime_fields_idempotent() {
        clear_runtime_env_vars();
        let raw: toml::Value = toml::from_str("[subagents]\nenabled = true").unwrap();
        let mut cfg = Config::new_from_toml_cfg(&raw).unwrap();
        let ctx = RuntimeResolutionContext {
            raw_config: &raw,
            remote_settings: None,
            is_headless: false,
            cli_subagents: None,
            cli_session_summary_model: None,
            cli_experimental_memory: false,
            cli_no_memory: false,
            todo_gate: false,
            laziness_debug_log: None,
        };
        cfg.resolve_runtime_fields(&ctx);
        let first_subagents = cfg.subagents_enabled;
        let first_gitignore = cfg.respect_gitignore;
        let first_mcps = cfg.managed_mcps_enabled;
        cfg.resolve_runtime_fields(&ctx);
        assert_eq!(cfg.subagents_enabled, first_subagents);
        assert_eq!(cfg.respect_gitignore, first_gitignore);
        assert_eq!(cfg.managed_mcps_enabled, first_mcps);
    }

    #[test]
    fn version_overrides_apply_into_typed_config() {
        let mut value: toml::Value = toml::from_str(
            r#"
[models]
default = "grow-build"

[[version_overrides]]
minimum_version = "1.8.0"
[version_overrides.models]
default = "grow-4.5"
"#,
        )
        .unwrap();
        let v = semver::Version::parse("1.8.0").unwrap();
        config::apply_version_overrides(&mut value, &v).unwrap();
        let cfg = Config::new_from_toml_cfg(&value).unwrap();
        assert_eq!(cfg.models.default.as_deref(), Some("grow-4.5"));
    }
    /// Build a minimal `ModelEntry` for testing resolve_model_list.
    fn prefetch_model_entry(
        slug: &str,
        context_window: u64,
        api_backend: ApiBackend,
    ) -> ModelEntry {
        ModelEntry {
            info: ModelInfo {
                user_selectable: true,
                id: None,
                model: slug.to_owned(),
                base_url: "https://test.example.com/v1".to_owned(),
                name: Some(slug.to_owned()),
                description: None,
                output_limit: None,
                temperature: None,
                top_p: None,
                api_backend,
                auth_scheme: Default::default(),
                extra_headers: IndexMap::new(),
                query_params: IndexMap::new(),
                env_http_headers: IndexMap::new(),
                context_window: NonZeroU64::new(context_window).unwrap(),
                use_concise: false,
                agent_type: default_agent_type(),
                inference_idle_timeout_secs: None,
                max_retries: None,
                hidden: false,
                reasoning_effort: None,
                supports_reasoning_effort: false,
                reasoning_efforts: Vec::new(),
                compactions_remaining: None,
                compaction_at_tokens: None,
                show_model_fingerprint: false,
                stream_tool_calls: None,
                laziness_detector: LazinessDetectorPerModelConfig::default(),
                auto_compact_threshold_percent: None,
                system_prompt_label: None,
            },
            api_key: None,
            env_key: None,
            auth_provider: None,
        }
    }
    #[test]
    fn per_model_value_overrides_global_model_default() {
        let mut cfg = Config::default();
        cfg.models.max_retries = Some(9);
        cfg.models.output_limit = Some(8192);
        cfg.config_models.insert(
            "remote-only-model".to_owned(),
            ConfigModelOverride {
                max_retries: Some(2),
                output_limit: Some(16_384),
                ..Default::default()
            },
        );
        let resolved = resolve_model_list(&cfg);
        let model = resolved
            .get("remote-only-model")
            .expect("model should exist");
        assert_eq!(
            model.info.max_retries,
            Some(2),
            "per-model value must win over the [models] default"
        );
        assert_eq!(
            model.info.output_limit,
            Some(16_384),
            "per-model output_limit must override the [models] default"
        );
    }
    #[test]
    fn resolve_model_list_empty_config_yields_empty_catalog() {
        let cfg = Config::default();
        let resolved = resolve_model_list(&cfg);
        assert!(resolved.is_empty());
    }
    #[test]
    #[serial]
    fn mcp_liveness_watchers_default_is_true() {
        unsafe { std::env::remove_var("GROW_MCP_LIVENESS_WATCHERS") };
        let r = resolve_mcp_liveness_watchers(None, None, None, None, None);
        assert!(r.value, "default-on by spec");
        assert_eq!(r.source, ConfigSource::Default);
    }
    #[test]
    #[serial]
    fn mcp_liveness_watchers_requirement_wins_over_everything() {
        unsafe { std::env::set_var("GROW_MCP_LIVENESS_WATCHERS", "true") };
        let r = resolve_mcp_liveness_watchers(
            Some(false),
            Some(true),
            Some(true),
            Some(true),
            Some(true),
        );
        unsafe { std::env::remove_var("GROW_MCP_LIVENESS_WATCHERS") };
        assert!(!r.value, "requirement overrides every other layer");
        assert_eq!(r.source, ConfigSource::Requirement);
    }
    #[test]
    #[serial]
    fn mcp_liveness_watchers_cli_wins_over_env_and_below() {
        unsafe { std::env::set_var("GROW_MCP_LIVENESS_WATCHERS", "true") };
        let r =
            resolve_mcp_liveness_watchers(None, Some(false), Some(true), Some(true), Some(true));
        unsafe { std::env::remove_var("GROW_MCP_LIVENESS_WATCHERS") };
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::Cli);
    }
    #[test]
    #[serial]
    fn mcp_liveness_watchers_env_wins_over_config_and_below() {
        unsafe { std::env::set_var("GROW_MCP_LIVENESS_WATCHERS", "false") };
        let r = resolve_mcp_liveness_watchers(None, None, Some(true), Some(true), Some(true));
        unsafe { std::env::remove_var("GROW_MCP_LIVENESS_WATCHERS") };
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::Env);
    }
    #[test]
    #[serial]
    fn mcp_liveness_watchers_config_wins_over_managed_and_feature_flag() {
        unsafe { std::env::remove_var("GROW_MCP_LIVENESS_WATCHERS") };
        let r = resolve_mcp_liveness_watchers(None, None, Some(false), Some(true), Some(true));
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::Config);
    }
    #[test]
    #[serial]
    fn mcp_liveness_watchers_managed_wins_over_feature_flag() {
        unsafe { std::env::remove_var("GROW_MCP_LIVENESS_WATCHERS") };
        let r = resolve_mcp_liveness_watchers(None, None, None, Some(false), Some(true));
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::ManagedConfig);
    }
    #[test]
    #[serial]
    fn mcp_liveness_watchers_feature_flag_used_when_no_higher_layer() {
        unsafe { std::env::remove_var("GROW_MCP_LIVENESS_WATCHERS") };
        let r = resolve_mcp_liveness_watchers(None, None, None, None, Some(false));
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::Remote);
    }
    #[test]
    #[serial]
    fn mcp_auto_restart_default_is_true() {
        unsafe { std::env::remove_var("GROW_MCP_AUTO_RESTART") };
        let r = resolve_mcp_auto_restart(None, None, None, None, None);
        assert!(r.value, "recovery is on by default");
        assert_eq!(r.source, ConfigSource::Default);
    }
    #[test]
    #[serial]
    fn mcp_auto_restart_requirement_wins_over_everything() {
        unsafe { std::env::set_var("GROW_MCP_AUTO_RESTART", "false") };
        let r = resolve_mcp_auto_restart(
            Some(true),
            Some(false),
            Some(false),
            Some(false),
            Some(false),
        );
        unsafe { std::env::remove_var("GROW_MCP_AUTO_RESTART") };
        assert!(r.value);
        assert_eq!(r.source, ConfigSource::Requirement);
    }
    #[test]
    #[serial]
    fn mcp_auto_restart_env_wins_over_config_and_below() {
        unsafe { std::env::set_var("GROW_MCP_AUTO_RESTART", "true") };
        let r = resolve_mcp_auto_restart(None, None, Some(false), Some(false), Some(false));
        unsafe { std::env::remove_var("GROW_MCP_AUTO_RESTART") };
        assert!(r.value);
        assert_eq!(r.source, ConfigSource::Env);
    }
    #[test]
    #[serial]
    fn mcp_push_server_status_default_is_true() {
        unsafe { std::env::remove_var("GROW_MCP_PUSH_SERVER_STATUS") };
        let r = resolve_mcp_push_server_status(None, None, None, None, None);
        assert!(r.value, "default-on by spec");
        assert_eq!(r.source, ConfigSource::Default);
    }
    #[test]
    #[serial]
    fn mcp_push_server_status_requirement_wins_over_everything() {
        unsafe { std::env::set_var("GROW_MCP_PUSH_SERVER_STATUS", "true") };
        let r = resolve_mcp_push_server_status(
            Some(false),
            Some(true),
            Some(true),
            Some(true),
            Some(true),
        );
        unsafe { std::env::remove_var("GROW_MCP_PUSH_SERVER_STATUS") };
        assert!(!r.value, "requirement overrides every other layer");
        assert_eq!(r.source, ConfigSource::Requirement);
    }
    #[test]
    #[serial]
    fn mcp_push_server_status_cli_wins_over_env_and_below() {
        unsafe { std::env::set_var("GROW_MCP_PUSH_SERVER_STATUS", "true") };
        let r =
            resolve_mcp_push_server_status(None, Some(false), Some(true), Some(true), Some(true));
        unsafe { std::env::remove_var("GROW_MCP_PUSH_SERVER_STATUS") };
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::Cli);
    }
    #[test]
    #[serial]
    fn mcp_push_server_status_env_wins_over_config_and_below() {
        unsafe { std::env::set_var("GROW_MCP_PUSH_SERVER_STATUS", "false") };
        let r = resolve_mcp_push_server_status(None, None, Some(true), Some(true), Some(true));
        unsafe { std::env::remove_var("GROW_MCP_PUSH_SERVER_STATUS") };
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::Env);
    }
    #[test]
    #[serial]
    fn mcp_push_server_status_config_wins_over_managed_and_feature_flag() {
        unsafe { std::env::remove_var("GROW_MCP_PUSH_SERVER_STATUS") };
        let r = resolve_mcp_push_server_status(None, None, Some(false), Some(true), Some(true));
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::Config);
    }
    #[test]
    #[serial]
    fn mcp_push_server_status_managed_wins_over_feature_flag() {
        unsafe { std::env::remove_var("GROW_MCP_PUSH_SERVER_STATUS") };
        let r = resolve_mcp_push_server_status(None, None, None, Some(false), Some(true));
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::ManagedConfig);
    }
    #[test]
    #[serial]
    fn mcp_push_server_status_feature_flag_used_when_no_higher_layer() {
        unsafe { std::env::remove_var("GROW_MCP_PUSH_SERVER_STATUS") };
        let r = resolve_mcp_push_server_status(None, None, None, None, Some(false));
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::Remote);
    }
    #[test]
    #[serial]
    fn mcp_recursive_config_watch_default_is_true() {
        unsafe { std::env::remove_var("GROW_MCP_RECURSIVE_CONFIG_WATCH") };
        let r = resolve_mcp_recursive_config_watch(None, None, None, None, None);
        assert!(r.value, "default-on by spec");
        assert_eq!(r.source, ConfigSource::Default);
    }
    #[test]
    #[serial]
    fn mcp_recursive_config_watch_requirement_wins_over_everything() {
        unsafe { std::env::set_var("GROW_MCP_RECURSIVE_CONFIG_WATCH", "true") };
        let r = resolve_mcp_recursive_config_watch(
            Some(false),
            Some(true),
            Some(true),
            Some(true),
            Some(true),
        );
        unsafe { std::env::remove_var("GROW_MCP_RECURSIVE_CONFIG_WATCH") };
        assert!(!r.value, "requirement overrides every other layer");
        assert_eq!(r.source, ConfigSource::Requirement);
    }
    #[test]
    #[serial]
    fn mcp_recursive_config_watch_cli_wins_over_env_and_below() {
        unsafe { std::env::set_var("GROW_MCP_RECURSIVE_CONFIG_WATCH", "true") };
        let r = resolve_mcp_recursive_config_watch(
            None,
            Some(false),
            Some(true),
            Some(true),
            Some(true),
        );
        unsafe { std::env::remove_var("GROW_MCP_RECURSIVE_CONFIG_WATCH") };
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::Cli);
    }
    #[test]
    #[serial]
    fn mcp_recursive_config_watch_env_wins_over_config_and_below() {
        unsafe { std::env::set_var("GROW_MCP_RECURSIVE_CONFIG_WATCH", "false") };
        let r = resolve_mcp_recursive_config_watch(None, None, Some(true), Some(true), Some(true));
        unsafe { std::env::remove_var("GROW_MCP_RECURSIVE_CONFIG_WATCH") };
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::Env);
    }
    #[test]
    #[serial]
    fn mcp_recursive_config_watch_config_wins_over_managed_and_feature_flag() {
        unsafe { std::env::remove_var("GROW_MCP_RECURSIVE_CONFIG_WATCH") };
        let r = resolve_mcp_recursive_config_watch(None, None, Some(false), Some(true), Some(true));
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::Config);
    }
    #[test]
    #[serial]
    fn mcp_recursive_config_watch_managed_wins_over_feature_flag() {
        unsafe { std::env::remove_var("GROW_MCP_RECURSIVE_CONFIG_WATCH") };
        let r = resolve_mcp_recursive_config_watch(None, None, None, Some(false), Some(true));
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::ManagedConfig);
    }
    #[test]
    #[serial]
    fn mcp_recursive_config_watch_feature_flag_used_when_no_higher_layer() {
        unsafe { std::env::remove_var("GROW_MCP_RECURSIVE_CONFIG_WATCH") };
        let r = resolve_mcp_recursive_config_watch(None, None, None, None, Some(false));
        assert!(!r.value);
        assert_eq!(r.source, ConfigSource::Remote);
    }
}
