pub mod reloader;
pub mod watcher;
use crate::bundle;
pub use config_types::{
    MemoryDreamConfig, MemoryEmbeddingConfig, MemoryFlushConfig, MemoryGcConfig, MemoryIndexConfig,
    MemoryInitialInjectionConfig, MemorySearchConfig, MemorySessionConfig, MemoryWatcherConfig,
    MmrConfig, TemporalDecayConfig,
};
use serde::Deserialize;
/// Full configuration for the memory system.
///
/// Parsed from the `[memory]` section of `$GROW_HOME/config.toml`. Disabled by
/// default; enabled via
/// `--experimental-memory` CLI flag or `GROW_MEMORY=1` env var.
/// Force-disabled via `GROW_MEMORY=0` (overrides TOML and remote settings).
///
/// All sub-configs are pre-populated with production-ready defaults.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    /// Whether memory is enabled for this session.
    pub enabled: bool,
    /// Index / chunking settings.
    pub index: MemoryIndexConfig,
    /// Embedding provider settings.
    pub embedding: MemoryEmbeddingConfig,
    /// Hybrid search scoring settings.
    pub search: MemorySearchConfig,
    /// First-turn memory injection behavior.
    pub initial_injection: MemoryInitialInjectionConfig,
    /// Session lifecycle settings.
    pub session: MemorySessionConfig,
    /// File watcher settings for detecting external memory edits.
    pub watcher: MemoryWatcherConfig,
    /// Garbage collection settings for orphaned workspace directories.
    pub gc: MemoryGcConfig,
    /// autoDream consolidation settings.
    pub dream: MemoryDreamConfig,
    /// Pre-compaction memory flush settings.
    ///
    /// **Note:** Configured under `[compaction.memory_flush]` in config.toml,
    /// not under `[memory]`. Flush is a compaction behavior.
    #[serde(skip)]
    pub flush: MemoryFlushConfig,
    /// Per-agent memory root override (e.g. `~/.grow/agent-memory/<name>/`).
    #[serde(skip)]
    pub root_dir_override: Option<std::path::PathBuf>,
    /// When true, the root is already project-scoped so MemoryStorage should
    /// skip the workspace hash subdirectory (use `new_flat` instead of `new`).
    #[serde(skip)]
    pub flat_memory_root: bool,
}
impl MemoryConfig {
    /// Resolve the final memory config from all sources (in priority order):
    /// 1. CLI flag `--no-memory` (absolute highest — always disables, overrides all)
    /// 2. CLI flag `--experimental-memory` (enables, but overridden by --no-memory)
    /// 3. `GROW_MEMORY` env var: `1`/`true` enables, `0`/`false` force-disables
    /// 4. Config file `[memory]` / `[compaction]` sections
    /// 5. Remote settings from `/v1/settings`
    ///
    /// Remote settings only override fields when the corresponding local
    /// config section is absent. Section-level granularity: if `[memory.search]`
    /// exists in TOML, all search fields come from TOML; if absent, remote
    /// search settings apply.
    pub fn resolve(
        experimental_memory: bool,
        no_memory: bool,
        config: &toml::Value,
        remote: Option<&crate::util::config::RemoteSettings>,
    ) -> Self {
        let mut result: Self = config
            .get("memory")
            .and_then(|v| v.clone().try_into().ok())
            .unwrap_or_default();
        if let Some(compaction) = config.get("compaction") {
            if let Some(flush) = compaction.get("memory_flush")
                && let Ok(f) = flush.clone().try_into()
            {
                result.flush = f;
            }
        }
        if let Some(remote) = remote {
            let has_local_search = config.get("memory").and_then(|m| m.get("search")).is_some();
            if !has_local_search {
                if let Some(v) = remote.memory_search_max_results {
                    result.search.max_results = v as usize;
                }
                if let Some(v) = remote.memory_search_min_score {
                    result.search.min_score = v;
                }
                if let Some(v) = remote.memory_temporal_decay_enabled {
                    result.search.temporal_decay.enabled = v;
                }
                if let Some(v) = remote.memory_temporal_decay_half_life_days {
                    result.search.temporal_decay.half_life_days = v;
                }
                if let Some(v) = remote.memory_mmr_enabled {
                    result.search.mmr.enabled = v;
                }
                if let Some(v) = remote.memory_mmr_lambda {
                    result.search.mmr.lambda = v.clamp(0.0, 1.0);
                }
            }
            let has_local_initial_injection = config
                .get("memory")
                .and_then(|m| m.get("initial_injection"))
                .is_some();
            if !has_local_initial_injection {
                if let Some(v) = remote.memory_initial_injection_enabled {
                    result.initial_injection.enabled = v;
                }
                if let Some(v) = remote.memory_initial_injection_min_score {
                    result.initial_injection.min_score = Some(v);
                }
            }
            let has_local_embedding = config
                .get("memory")
                .and_then(|m| m.get("embedding"))
                .is_some();
            if !has_local_embedding {
                if let Some(ref v) = remote.memory_embedding_model {
                    result.embedding.model = Some(v.clone());
                }
                if let Some(v) = remote.memory_embedding_dimensions {
                    result.embedding.dimensions = v as usize;
                }
            }
            let has_local_flush = config
                .get("compaction")
                .and_then(|c| c.get("memory_flush"))
                .is_some();
            if !has_local_flush {
                if let Some(v) = remote.flush_enabled {
                    result.flush.enabled = v;
                }
                if let Some(v) = remote.flush_soft_threshold_tokens {
                    result.flush.soft_threshold_tokens = v;
                }
                if let Some(v) = remote.flush_idle_timeout_secs {
                    result.flush.idle_timeout_secs = Some(v);
                }
                if let Some(v) = remote.flush_semantic_dedup_threshold {
                    result.flush.semantic_dedup_threshold = Some(v.clamp(0.0, 1.0));
                }
            }
            let has_local_watcher = config
                .get("memory")
                .and_then(|m| m.get("watcher"))
                .is_some();
            if !has_local_watcher && let Some(v) = remote.memory_watcher_enabled {
                result.watcher.enabled = v;
            }
            let has_local_dream = config.get("memory").and_then(|m| m.get("dream")).is_some();
            if !has_local_dream {
                if let Some(v) = remote.dream_enabled {
                    result.dream.enabled = v;
                }
                if let Some(v) = remote.dream_min_hours {
                    result.dream.min_hours = v;
                }
                if let Some(v) = remote.dream_min_sessions {
                    result.dream.min_sessions = v;
                }
                if let Some(v) = remote.dream_check_interval_secs {
                    result.dream.check_interval_secs = Some(v);
                }
            }
        }
        let resolved = crate::agent::config::resolve_enabled(
            if experimental_memory {
                Some(true)
            } else {
                None
            },
            "GROW_MEMORY",
            result.enabled,
            config.get("memory").is_some(),
            remote.and_then(|r| r.memory_enabled),
            false,
        );
        result.enabled = resolved.value;
        if no_memory {
            result.enabled = false;
        }
        result
    }
}
/// Configuration for subagent (task tool) support.
///
/// Parsed from the `[subagents]` section of `$GROW_HOME/config.toml`. Enabled
/// by default; can be disabled via
/// `GROW_SUBAGENTS=0` env var or `[subagents] enabled = false`
/// in config.toml.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentClassifierInput {
    /// Include the primary Agent's trusted task context in the ephemeral
    /// judgment branch. This is the safer default for rare fence escalation.
    #[default]
    Context,
    /// Send only the structured proposed action and classifier policy. This
    /// reduces tokens when deployments intentionally do not need task intent.
    RequestOnly,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SubagentsConfig {
    /// Whether subagent support is enabled.
    pub enabled: bool,
    /// Raw `[subagents] max_depth` (i64 so out-of-range parses; clamped ≥1 at resolve).
    #[serde(default)]
    pub max_depth: Option<i64>,
    /// Permission route for all subagent capability requests and tool calls.
    /// It is always independent of the primary session's live mode.
    #[serde(default)]
    pub permission_mode: workspace::permission::types::RequestPermissionMode,
    /// Input scope for child Auto escalation judgments.
    #[serde(default)]
    pub classifier_input: SubagentClassifierInput,
    /// Per-subagent model ID overrides.
    /// Keys are agent names, values are model IDs that must exist in the
    /// available models registry. Parsed from `[subagents.models]` in config.toml.
    ///
    /// ```toml
    /// [subagents.models]
    /// explore = "grow-3-fast"
    /// plan = "grow-3"
    /// ```
    #[serde(default)]
    pub models: std::collections::HashMap<String, String>,
    /// Per-subagent enable/disable toggles.
    /// Keys are agent names, values are booleans.
    /// Omitted agents default to enabled (`true`).
    ///
    /// ```toml
    /// [subagents.toggle]
    /// explore = true
    /// plan = false
    /// ```
    #[serde(default)]
    pub toggle: std::collections::HashMap<String, bool>,
}
impl SubagentsConfig {
    /// Check if a subagent is enabled.
    /// Returns `true` if the agent is not in the toggle map (default enabled).
    pub fn is_subagent_enabled(&self, name: &str) -> bool {
        self.toggle.get(name).copied().unwrap_or(true)
    }
    pub const ENV_MAX_DEPTH: &'static str = "GROW_SUBAGENTS_MAX_DEPTH";
    pub const DEFAULT_MAX_DEPTH: u32 = 1;
    /// Clamp to `1..=u32::MAX`. Values below 1 (including 0 / negatives) warn
    /// and become 1 so nesting is never accidentally disabled.
    pub fn clamp_max_depth(raw: i64, source: &str) -> u32 {
        if raw < i64::from(Self::DEFAULT_MAX_DEPTH) {
            tracing::warn!(
                source,
                value = raw,
                "subagents max_depth < 1; clamping to 1"
            );
            Self::DEFAULT_MAX_DEPTH
        } else if raw > i64::from(u32::MAX) {
            tracing::warn!(
                source,
                value = raw,
                "subagents max_depth exceeds u32::MAX; clamping"
            );
            u32::MAX
        } else {
            raw as u32
        }
    }
    /// Precedence: env > TOML > remote > [`Self::DEFAULT_MAX_DEPTH`].
    ///
    /// Depth 0 is the top-level session; a child is parent+1. Spawn is rejected
    /// when `depth >= max`. So `max = 1` allows only top-level spawns; nested
    /// spawns from a first-level subagent need `max >= 2`.
    pub fn resolve_max_depth(env: Option<&str>, config: Option<i64>, remote: Option<u32>) -> u32 {
        if let Some(raw) = env {
            match raw.trim().parse::<i64>() {
                Ok(v) => return Self::clamp_max_depth(v, "env"),
                Err(_) => {
                    tracing::warn!(
                        value = %raw,
                        "invalid GROW_SUBAGENTS_MAX_DEPTH (expected integer); ignoring"
                    );
                }
            }
        }
        if let Some(v) = config {
            return Self::clamp_max_depth(v, "config");
        }
        if let Some(v) = remote {
            return Self::clamp_max_depth(i64::from(v), "remote");
        }
        Self::DEFAULT_MAX_DEPTH
    }
    /// Resolve the final subagents config from all sources (in priority order):
    /// 1. CLI flag `--subagents` (absolute highest — always enables)
    /// 2. `GROW_SUBAGENTS` env var: `1`/`true` enables, `0`/`false` force-disables
    /// 3. Config file `[subagents]` section
    /// 4. Default (enabled)
    ///
    /// `enabled` is deliberately not remotely gated — only explicit local
    /// intent (CLI flag, `GROW_SUBAGENTS`, `[subagents] enabled`) changes
    /// the default.
    ///
    /// Project files are excluded from this trust-independent base; Task
    /// boundaries overlay them using the parent cwd's authoritative trust verdict.
    pub fn resolve(cli_flag: bool, config: &toml::Value) -> Self {
        let mut result: Self = config
            .get("subagents")
            .and_then(|v| v.clone().try_into().ok())
            .unwrap_or_default();
        let resolved = crate::agent::config::resolve_enabled(
            if cli_flag { Some(true) } else { None },
            "GROW_SUBAGENTS",
            result.enabled,
            config.get("subagents").is_some(),
            None,
            true,
        );
        result.enabled = resolved.value;
        result
    }
}
/// Auxiliary model overrides under `[models]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModelOverrideConfig {
    /// `None` = current model.
    pub session_title: Option<String>,
    /// Optional fallback used only after the active runtime is proven to reject
    /// image input. The active runtime always receives the first image attempt.
    pub image_description: Option<String>,
    /// Next-prompt suggestion model pin. Unlike the other overrides this does
    /// NOT fill a compiled default — see [`PromptSuggestModelPin`].
    #[serde(skip)]
    pub prompt_suggestion: PromptSuggestModelPin,
}
impl Default for ModelOverrideConfig {
    fn default() -> Self {
        Self {
            session_title: None,
            image_description: None,
            prompt_suggestion: PromptSuggestModelPin::Unpinned,
        }
    }
}
/// Resolved model pin for the next-prompt suggestion call (tab-autocomplete
/// ghost text), `env > config.toml > remote` — see
/// [`ModelOverrideConfig::resolve`].
///
/// Unlike the other auxiliary overrides this does not collapse to a plain
/// model string: the consumer (`handle_suggest_prompt`) must distinguish
/// an explicit pin from "unpinned" (where the client hint may apply), and
/// whether the pin came from the env
/// escape hatch. Every effective model except an env pin is catalog-guarded —
/// when the model is not in the shell's catalog the per-turn suggestion request is
/// skipped entirely rather than fired doomed. The env pin is deliberately
/// exempt so `GROW_PROMPT_SUGGESTIONS_MODEL` keeps working for models a
/// catalog does not list (mirrors the pager, which forwards the env value
/// without checking its catalog).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PromptSuggestModelPin {
    /// `GROW_PROMPT_SUGGESTIONS_MODEL` — used verbatim, bypasses the
    /// catalog guard.
    Env(String),
    /// `[models] prompt_suggestion` in config.toml, or the remote
    /// `prompt_suggestion_model` (remote settings) — catalog-guarded.
    Pinned(String),
    /// No explicit pin: the client hint may apply and remains catalog-guarded.
    #[default]
    Unpinned,
}
/// Drop whitespace-only auxiliary model overrides (treat like unset).
fn non_empty_model_override(value: Option<&str>) -> Option<String> {
    value.and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}
impl ModelOverrideConfig {
    /// CLI flag > env var > config.toml > remote settings. An absent auxiliary
    /// image-description model means a proven text-only runtime permanently
    /// removes image groups instead of converting them to descriptions.
    /// `prompt_suggestion` resolves to a [`PromptSuggestModelPin`] instead of
    /// a model string (no CLI flag; the default and the catalog guard live at
    /// the consumer, `handle_suggest_prompt`).
    pub fn resolve(
        cli_session_title_model: Option<&str>,
        config: &toml::Value,
        remote: Option<&crate::util::config::RemoteSettings>,
    ) -> Self {
        let models_table = config.get("models");
        let parsed_models: crate::agent::config::ModelsConfig = models_table
            .and_then(|v| v.clone().try_into().ok())
            .unwrap_or_default();
        let mut result = Self {
            session_title: non_empty_model_override(parsed_models.session_title.as_deref()),
            image_description: non_empty_model_override(parsed_models.image_description.as_deref()),
            prompt_suggestion: non_empty_model_override(parsed_models.prompt_suggestion.as_deref())
                .map(PromptSuggestModelPin::Pinned)
                .unwrap_or_default(),
        };
        let has_local_title = models_table.and_then(|m| m.get("session_title")).is_some();
        let has_local_id = models_table
            .and_then(|m| m.get("image_description"))
            .is_some();
        if let Some(remote) = remote {
            if !has_local_title {
                result.session_title =
                    non_empty_model_override(remote.session_title_model.as_deref());
            }
            if !has_local_id {
                result.image_description =
                    non_empty_model_override(remote.image_description_model.as_deref());
            }
            if result.prompt_suggestion == PromptSuggestModelPin::Unpinned
                && let Some(v) = non_empty_model_override(remote.prompt_suggestion_model.as_deref())
            {
                result.prompt_suggestion = PromptSuggestModelPin::Pinned(v);
            }
        }
        if let Ok(v) = std::env::var("GROW_SESSION_TITLE_MODEL") {
            result.session_title = non_empty_model_override(Some(v.as_str()));
        }
        if let Ok(v) = std::env::var("GROW_IMAGE_DESCRIPTION_MODEL") {
            result.image_description = non_empty_model_override(Some(v.as_str()));
        }
        if let Ok(v) = std::env::var("GROW_PROMPT_SUGGESTIONS_MODEL")
            && let Some(v) = non_empty_model_override(Some(v.as_str()))
        {
            result.prompt_suggestion = PromptSuggestModelPin::Env(v);
        }
        if let Some(v) = cli_session_title_model {
            result.session_title = non_empty_model_override(Some(v));
        }
        result
    }
}
/// Tool behavior configuration (`[tools]` in config.toml).
///
/// Controls cross-cutting tool behavior such as `.gitignore` filtering.
///
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ToolsConfig {
    /// When `true`, all tools (including `read_file`) filter gitignored
    /// files. When `false` (default), each tool picks its own default.
    pub respect_gitignore: bool,
}
impl ToolsConfig {
    /// Resolve the final tools config, in priority order:
    /// 1. Env var `GROW_RESPECT_GITIGNORE` (`0`/`false` off,
    ///    `1`/`true` on).
    /// 2. `[tools]` block from the merged effective config.
    /// 3. Default (`false`).
    ///
    pub fn resolve(config: &toml::Value) -> Self {
        let tools = config.get("tools");
        let mut result = Self {
            respect_gitignore: tools
                .and_then(|t| t.get("respect_gitignore"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        };
        match std::env::var("GROW_RESPECT_GITIGNORE").as_deref() {
            Ok("0") | Ok("false") => {
                result.respect_gitignore = false;
            }
            Ok("1") | Ok("true") => {
                result.respect_gitignore = true;
            }
            _ => {}
        }
        result
    }
}
pub use config::ConfigLayers;
pub use config::{load_config_file, load_from_disk, load_toml_file, user_grow_home};
/// Map of "dotted.path" to which config file the value came from.
pub fn config_origins(
    layers: &ConfigLayers,
) -> std::collections::HashMap<String, crate::agent::config::ConfigSource> {
    use crate::agent::config::ConfigSource;
    let mut origins = std::collections::HashMap::new();
    walk_toml(
        &layers.user,
        &mut vec![],
        ConfigSource::Config,
        &mut origins,
    );
    origins
}
fn walk_toml(
    value: &toml::Value,
    path: &mut Vec<String>,
    source: crate::agent::config::ConfigSource,
    origins: &mut std::collections::HashMap<String, crate::agent::config::ConfigSource>,
) {
    match value {
        toml::Value::Table(table) => {
            for (k, v) in table {
                path.push(k.clone());
                walk_toml(v, path, source, origins);
                path.pop();
            }
        }
        _ => {
            origins.insert(path.join("."), source);
        }
    }
}
/// The `[skills]` table from an effective config, shared by the reload
/// dispatch and `grow inspect`.
pub(crate) use crate::config::reloader::parse_skills_config;
/// Effective config: the user layer plus local campaign overlay.
pub use crate::util::config::load_effective_config;
/// Effective config with disk campaigns only — for one-shot entrypoints that
/// never load runtime settings.
pub use crate::util::config::load_effective_config_disk_only;
/// Resolve sandbox profile and apply OS-level enforcement. Called once at startup.
///
/// `cli_profile` is the resumed/forced base profile (a resumed session's saved
/// profile, or an explicit `--sandbox`); it wins over a fresh env/config read.
pub fn apply_sandbox(
    sandbox_config: Option<&crate::agent::config::SandboxSettingsConfig>,
    cli_profile: Option<&str>,
    cwd: Option<&std::path::Path>,
) {
    let owned;
    let config = match sandbox_config {
        Some(c) => c,
        None => {
            owned = crate::agent::config::SandboxSettingsConfig::from_effective_config();
            &owned
        }
    };
    let resolved = config.resolve_profile(cli_profile);
    sandbox::set_auto_allow_bash(config.resolve_auto_allow_bash().value);
    let sandbox_profile: sandbox::ProfileName = resolved.value.parse().unwrap_or_else(|e| {
        eprintln!("warning: {e}, defaulting to no sandbox");
        sandbox::ProfileName::Off
    });
    sandbox::set_configured_profile(&resolved.value);
    let workspace = cwd
        .and_then(|p| dunce::canonicalize(p).ok())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    #[cfg(target_os = "linux")]
    let requires_read_deny = sandbox::requires_read_deny(&sandbox_profile, &workspace);
    #[cfg(target_os = "linux")]
    let requires_hook_write_deny = sandbox::requires_hook_write_deny(&sandbox_profile, &workspace);
    #[cfg(target_os = "linux")]
    let requires_bwrap = requires_read_deny || requires_hook_write_deny;
    #[cfg(target_os = "linux")]
    {
        let refuse_unprotected = |detail: &str| {
            eprintln!(
                "error: this sandbox could not enforce its mount-namespace deny set \
                 on Linux (bubblewrap missing/unusable, or a deny glob exceeded its \
                 expansion limit — see any message above). Install bubblewrap with \
                 `apt install -y bubblewrap` if needed. Refusing to start with denied \
                 paths unprotected.{detail}"
            );
        };
        match sandbox::bwrap_reexec_for_profile(&sandbox_profile, &workspace) {
            Some(mut cmd) => {
                use std::os::unix::process::CommandExt;
                let err = cmd.exec();
                if requires_bwrap {
                    refuse_unprotected(&format!(" (bwrap exec failed: {err})"));
                    std::process::exit(1);
                }
                eprintln!(
                    "WARNING: bwrap exec failed: {err}. \
                     Falling back to Landlock sandbox. \
                     Install bubblewrap: apt install -y bubblewrap"
                );
            }
            None if requires_bwrap && sandbox::is_inside_bwrap() => {
                if requires_hook_write_deny
                    && let Err(e) = sandbox::verify_hook_write_deny_enforced()
                {
                    eprintln!(
                        "error: sandbox reports bwrap but required hook write-deny \
                         mounts are missing or writable ({e}); refusing to start \
                         (possible __GROW_INSIDE_BWRAP spoof)"
                    );
                    std::process::exit(1);
                }
            }
            None if requires_bwrap => {
                refuse_unprotected("");
                std::process::exit(1);
            }
            None => {}
        }
    }
    if sandbox_profile != sandbox::ProfileName::Off {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let requires_protection = {
            let is_custom = matches!(sandbox_profile, sandbox::ProfileName::Custom(_));
            let needs_hooks = sandbox::requires_hook_write_deny(&sandbox_profile, &workspace);
            is_custom || needs_hooks
        };
        let mut sandbox = sandbox::SandboxManager::new(sandbox_profile, &workspace);
        if let Err(e) = sandbox.apply(&workspace) {
            eprintln!("warning: sandbox could not be applied: {e}");
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            #[cfg(target_os = "macos")]
            let unappliable = requires_protection && !sandbox.is_applied();
            #[cfg(target_os = "linux")]
            let unappliable =
                requires_protection && !sandbox.is_applied() && !sandbox::is_inside_bwrap();
            if unappliable {
                eprintln!(
                    "error: could not apply the '{}' sandbox profile (including \
                     direct global-hook write protection); refusing to start.",
                    sandbox.profile()
                );
                std::process::exit(1);
            }
            #[cfg(target_os = "linux")]
            if requires_hook_write_deny
                && sandbox::is_inside_bwrap()
                && let Err(e) = sandbox::verify_hook_write_deny_enforced()
            {
                eprintln!(
                    "error: required hook write-deny mounts not verified after apply ({e}); \
                     refusing to start"
                );
                std::process::exit(1);
            }
        }
        sandbox.install();
    }
}
/// Load `<cwd>/.grow/config.toml` (with this layer's `[[version_overrides]]`
/// applied). Empty table if the file is missing.
pub fn load_project_config(cwd: &std::path::Path) -> std::io::Result<toml::Value> {
    load_config_file(&cwd.join(".grow").join("config.toml"))
}
pub use workspace::project_config::find_project_configs;
/// Resolve the effective `[plugins]` config for a working directory the same
/// way a session does at reload time: global/user config
/// ([`load_effective_config`]) plus every ancestor project `.grow/config.toml`
/// ([`find_project_configs`], extending `paths` and `disabled`) plus the
/// imported `enabledPlugins` merge.
///
/// Shared by `reload_plugins_impl`, `grow/commands/list`, and the agent's
/// eager plugin-registry fan-out so all three discover the same plugins for a
/// given cwd. Centralizing it prevents the paths/disabled/discovered-command
/// drift those callers would otherwise accumulate.
pub fn resolve_effective_plugins_config(
    cwd: &std::path::Path,
) -> crate::agent::config::PluginsConfig {
    let extract = |toml_val: &toml::Value| -> Option<crate::agent::config::PluginsConfig> {
        toml_val
            .get("plugins")
            .and_then(|v| v.clone().try_into().ok())
    };
    let mut plugins_cfg = load_effective_config()
        .ok()
        .and_then(|t| extract(&t))
        .unwrap_or_default();
    let project_trusted = crate::agent::folder_trust::project_scope_allowed(cwd);
    for config_path in find_project_configs(cwd) {
        if let Ok(toml_val) = load_config_file(&config_path)
            && let Some(proj) = extract(&toml_val)
        {
            if project_trusted {
                plugins_cfg.paths.extend(proj.paths);
            }
            plugins_cfg.disabled.extend(proj.disabled);
        }
    }
    plugins_cfg
}
pub use config::{deep_merge_toml, expand_env_vars_in_string, expand_env_vars_in_toml};
/// Add a plugin path to `[plugins].paths` in `~/.grow/config.toml`.
///
/// Creates the `[plugins]` section and `paths` array if they don't exist.
/// Deduplicates: if the path is already present, this is a no-op.
pub fn add_plugin_path(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = crate::util::grow_home::grow_home().join("config.toml");
    let content = std::fs::read_to_string(&config_path).unwrap_or_default();
    let mut config: toml::Value = if content.is_empty() {
        toml::Value::Table(toml::map::Map::new())
    } else {
        toml::from_str(&content).map_err(|e| format!("failed to parse config.toml: {e}"))?
    };
    let table = config
        .as_table_mut()
        .ok_or("config.toml root is not a table")?;
    if !table.contains_key("plugins") {
        table.insert(
            "plugins".to_string(),
            toml::Value::Table(toml::map::Map::new()),
        );
    }
    let plugins = table
        .get_mut("plugins")
        .and_then(|v| v.as_table_mut())
        .ok_or("[plugins] is not a table")?;
    if !plugins.contains_key("paths") {
        plugins.insert("paths".to_string(), toml::Value::Array(vec![]));
    }
    let paths = plugins
        .get_mut("paths")
        .and_then(|v| v.as_array_mut())
        .ok_or("[plugins].paths is not an array")?;
    let already_present = paths.iter().any(|v| v.as_str().is_some_and(|s| s == path));
    if !already_present {
        paths.push(toml::Value::String(path.to_string()));
    }
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&config_path, toml::to_string_pretty(&config)?)?;
    Ok(())
}
/// Remove a plugin path from `[plugins].paths` in `~/.grow/config.toml`.
///
/// If the path is not found, this is a no-op (returns Ok).
pub fn remove_plugin_path(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = crate::util::grow_home::grow_home().join("config.toml");
    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    let mut config: toml::Value =
        toml::from_str(&content).map_err(|e| format!("failed to parse config.toml: {e}"))?;
    if let Some(plugins) = config
        .as_table_mut()
        .and_then(|t| t.get_mut("plugins"))
        .and_then(|v| v.as_table_mut())
        && let Some(paths) = plugins.get_mut("paths").and_then(|v| v.as_array_mut())
    {
        paths.retain(|v| v.as_str().is_none_or(|s| s != path));
    }
    std::fs::write(&config_path, toml::to_string_pretty(&config)?)?;
    Ok(())
}
/// Add a plugin to `[plugins].disabled` in `~/.grow/config.toml`.
///
/// Creates the `[plugins]` section and `disabled` array if they don't exist.
/// Deduplicates: if already present, this is a no-op.
pub fn add_disabled_plugin(plugin_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = crate::util::grow_home::grow_home().join("config.toml");
    let content = std::fs::read_to_string(&config_path).unwrap_or_default();
    let mut config: toml::Value = if content.is_empty() {
        toml::Value::Table(toml::map::Map::new())
    } else {
        toml::from_str(&content).map_err(|e| format!("failed to parse config.toml: {e}"))?
    };
    let table = config
        .as_table_mut()
        .ok_or("config.toml root is not a table")?;
    if !table.contains_key("plugins") {
        table.insert(
            "plugins".to_string(),
            toml::Value::Table(toml::map::Map::new()),
        );
    }
    let plugins = table
        .get_mut("plugins")
        .and_then(|v| v.as_table_mut())
        .ok_or("[plugins] is not a table")?;
    if !plugins.contains_key("disabled") {
        plugins.insert("disabled".to_string(), toml::Value::Array(vec![]));
    }
    let disabled = plugins
        .get_mut("disabled")
        .and_then(|v| v.as_array_mut())
        .ok_or("[plugins].disabled is not an array")?;
    let already = disabled
        .iter()
        .any(|v| v.as_str().is_some_and(|s| s == plugin_id));
    if !already {
        disabled.push(toml::Value::String(plugin_id.to_string()));
    }
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&config_path, toml::to_string_pretty(&config)?)?;
    Ok(())
}
/// Remove a plugin from `[plugins].disabled` in `~/.grow/config.toml`.
///
/// If the plugin is not in the disabled list, this is a no-op.
pub fn remove_disabled_plugin(plugin_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = crate::util::grow_home::grow_home().join("config.toml");
    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    let mut config: toml::Value =
        toml::from_str(&content).map_err(|e| format!("failed to parse config.toml: {e}"))?;
    if let Some(plugins) = config
        .as_table_mut()
        .and_then(|t| t.get_mut("plugins"))
        .and_then(|v| v.as_table_mut())
        && let Some(disabled) = plugins.get_mut("disabled").and_then(|v| v.as_array_mut())
    {
        disabled.retain(|v| v.as_str().is_none_or(|s| s != plugin_id));
    }
    std::fs::write(&config_path, toml::to_string_pretty(&config)?)?;
    Ok(())
}
/// Add a plugin to `[plugin_cta].dismissed` in `~/.grow/config.toml`.
///
/// Creates the `[plugin_cta]` section and `dismissed` array if they don't exist.
/// Deduplicates: if already present, this is a no-op.
pub fn add_dismissed_plugin_cta(plugin_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = crate::util::grow_home::grow_home().join("config.toml");
    add_dismissed_plugin_cta_to_file(plugin_id, &config_path)
}
/// Add a dismissed plugin CTA to a specific config file (path-parameterized for tests).
#[doc(hidden)]
pub fn add_dismissed_plugin_cta_to_file(
    plugin_id: &str,
    config_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(config_path).unwrap_or_default();
    let mut config: toml::Value = if content.is_empty() {
        toml::Value::Table(toml::map::Map::new())
    } else {
        toml::from_str(&content).map_err(|e| format!("failed to parse config.toml: {e}"))?
    };
    let table = config
        .as_table_mut()
        .ok_or("config.toml root is not a table")?;
    if !table.contains_key("plugin_cta") {
        table.insert(
            "plugin_cta".to_string(),
            toml::Value::Table(toml::map::Map::new()),
        );
    }
    let plugin_cta = table
        .get_mut("plugin_cta")
        .and_then(|v| v.as_table_mut())
        .ok_or("[plugin_cta] is not a table")?;
    if !plugin_cta.contains_key("dismissed") {
        plugin_cta.insert("dismissed".to_string(), toml::Value::Array(vec![]));
    }
    let dismissed = plugin_cta
        .get_mut("dismissed")
        .and_then(|v| v.as_array_mut())
        .ok_or("[plugin_cta].dismissed is not an array")?;
    let already = dismissed
        .iter()
        .any(|v| v.as_str().is_some_and(|s| s == plugin_id));
    if !already {
        dismissed.push(toml::Value::String(plugin_id.to_string()));
    }
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(config_path, toml::to_string_pretty(&config)?)?;
    Ok(())
}
/// All plugin ids listed in `[plugin_cta].dismissed` in `~/.grow/config.toml`.
///
/// Read once (e.g. on catalog load) and cached so the matched-debounce recompute
/// doesn't parse the config from disk on the UI thread.
pub fn dismissed_plugin_ctas() -> std::collections::HashSet<String> {
    let config_path = crate::util::grow_home::grow_home().join("config.toml");
    dismissed_plugin_ctas_in_file(&config_path)
}
/// Read the dismissed plugin CTA set from a specific config file (for tests).
#[doc(hidden)]
pub fn dismissed_plugin_ctas_in_file(
    config_path: &std::path::Path,
) -> std::collections::HashSet<String> {
    let Ok(content) = std::fs::read_to_string(config_path) else {
        return std::collections::HashSet::new();
    };
    let Ok(config) = toml::from_str::<toml::Value>(&content) else {
        return std::collections::HashSet::new();
    };
    config
        .as_table()
        .and_then(|t| t.get("plugin_cta"))
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("dismissed"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}
/// Validate that a hook path is safe to add to `~/.grow/hooks-paths`.
///
/// CWE-427: Only paths under `~/.grow/` are allowed to prevent
/// arbitrary hook path injection that bypasses the project trust gate.
/// Paths are canonicalized (resolving symlinks and `..`) before checking.
pub fn validate_hooks_path(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let candidate = std::path::Path::new(path);
    if !candidate.is_absolute() {
        return Err("Hook path must be absolute.".into());
    }
    let grow_home = crate::util::grow_home::grow_home();
    let canonical = dunce::canonicalize(candidate)
        .or_else(|_| {
            let mut base = candidate.to_path_buf();
            let mut tail = Vec::new();
            while !base.exists() {
                if let Some(file_name) = base.file_name() {
                    tail.push(file_name.to_os_string());
                    base.pop();
                } else {
                    break;
                }
            }
            let mut resolved = dunce::canonicalize(&base)?;
            for component in tail.into_iter().rev() {
                resolved.push(component);
            }
            Ok(resolved)
        })
        .map_err(|e: std::io::Error| format!("Cannot resolve hook path: {e}"))?;
    let canonical_home = dunce::canonicalize(&grow_home).unwrap_or_else(|_| grow_home.clone());
    if !canonical.starts_with(&canonical_home) {
        return Err(format!(
            "Hook path must be under ~/.grow/ ({}). Got: {}",
            canonical_home.display(),
            canonical.display()
        )
        .into());
    }
    Ok(())
}
/// Post-install steps for a newly installed plugin repo.
///
/// Auto-enables all plugins in the repo so they are active after the next reload.
/// Returns `(plugin_names, warnings)` for status messaging.
pub fn post_install_plugin(repo_key: &str) -> (Vec<String>, Vec<String>) {
    let registry = agent::plugins::InstallRegistry::load();
    let Some(repo) = registry.get_repo(repo_key) else {
        return (
            vec![],
            vec![format!("repo not found in registry: {repo_key}")],
        );
    };
    let names: Vec<String> = repo.plugins.keys().cloned().collect();
    let mut warnings = Vec::new();
    for name in &names {
        if let Err(e) = add_enabled_plugin(name) {
            warnings.push(format!("auto-enable {name}: {e}"));
        }
    }
    (names, warnings)
}
/// Add a plugin to `[plugins].enabled` in `~/.grow/config.toml`.
///
/// Used for project-scope plugins that are disabled by default.
/// Deduplicates: if already present, this is a no-op.
pub fn add_enabled_plugin(plugin_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = crate::util::grow_home::grow_home().join("config.toml");
    let content = std::fs::read_to_string(&config_path).unwrap_or_default();
    let mut config: toml::Value = if content.is_empty() {
        toml::Value::Table(toml::map::Map::new())
    } else {
        toml::from_str(&content).map_err(|e| format!("failed to parse config.toml: {e}"))?
    };
    let table = config
        .as_table_mut()
        .ok_or("config.toml root is not a table")?;
    if !table.contains_key("plugins") {
        table.insert(
            "plugins".to_string(),
            toml::Value::Table(toml::map::Map::new()),
        );
    }
    let plugins = table
        .get_mut("plugins")
        .and_then(|v| v.as_table_mut())
        .ok_or("[plugins] is not a table")?;
    if !plugins.contains_key("enabled") {
        plugins.insert("enabled".to_string(), toml::Value::Array(Vec::new()));
    }
    let enabled = plugins
        .get_mut("enabled")
        .and_then(|v| v.as_array_mut())
        .ok_or("[plugins].enabled is not an array")?;
    let already = enabled
        .iter()
        .any(|v| v.as_str().is_some_and(|s| s == plugin_id));
    if !already {
        enabled.push(toml::Value::String(plugin_id.to_string()));
    }
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&config_path, toml::to_string_pretty(&config)?)?;
    Ok(())
}
/// Remove a plugin from `[plugins].enabled` in `~/.grow/config.toml`.
pub fn remove_enabled_plugin(plugin_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = crate::util::grow_home::grow_home().join("config.toml");
    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    let mut config: toml::Value =
        toml::from_str(&content).map_err(|e| format!("failed to parse config.toml: {e}"))?;
    if let Some(plugins) = config
        .as_table_mut()
        .and_then(|t| t.get_mut("plugins"))
        .and_then(|v| v.as_table_mut())
        && let Some(enabled) = plugins.get_mut("enabled").and_then(|v| v.as_array_mut())
    {
        enabled.retain(|v| v.as_str().is_none_or(|s| s != plugin_id));
    }
    std::fs::write(&config_path, toml::to_string_pretty(&config)?)?;
    Ok(())
}
/// Add a hook path to `~/.grow/hooks-paths` (one path per line).
///
/// If the path is already present (exact string match), this is a no-op.
/// CWE-427: The path is validated to be under `~/.grow/` before writing.
pub fn add_hooks_path(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    validate_hooks_path(path)?;
    add_hooks_path_to_file(
        path,
        &crate::util::grow_home::grow_home().join("hooks-paths"),
    )
}
/// Add a hook path to a specific file (for tests).
pub fn add_hooks_path_to_file(
    path: &str,
    paths_file: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = paths_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let existing = std::fs::read_to_string(paths_file).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == path) {
        return Ok(());
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths_file)?;
    writeln!(file, "{}", path)?;
    Ok(())
}
/// Remove a hook path from `~/.grow/hooks-paths`.
///
/// If the path is not found (exact string match), this is a no-op.
/// Matches the same exact-string behavior as `add_hooks_path`.
pub fn remove_hooks_path(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    remove_hooks_path_from_file(
        path,
        &crate::util::grow_home::grow_home().join("hooks-paths"),
    )
}
/// Remove a hook path from a specific file (for tests).
pub fn remove_hooks_path_from_file(
    path: &str,
    paths_file: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = match std::fs::read_to_string(paths_file) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    let mut found = false;
    let new_lines: Vec<&str> = content
        .lines()
        .filter(|l| {
            if l.trim() == path {
                found = true;
                false
            } else {
                true
            }
        })
        .collect();
    if !found {
        return Ok(());
    }
    if let Some(parent) = paths_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        paths_file,
        new_lines.join("\n") + (if new_lines.is_empty() { "" } else { "\n" }),
    )?;
    Ok(())
}
#[cfg(test)]
mod tests;
