//! Agent bootstrap and lifecycle hooks.
//!
//! [`bootstrap`] runs the full init sequence (config resolution, process
//! singletons, model catalog) and returns a resolved config + `ModelsManager`.

use crate::agent::config::{self, Config as AgentConfig};
use crate::agent::models::ModelsManager;

/// Resolve config, init process singletons, build the model catalog.
///
/// The `ModelsManager` is `Clone + Send`, so callers that need a handle
/// for the config watcher can clone it before passing it to
/// `MvpAgent::with_models`.
pub fn bootstrap(cfg: &AgentConfig) -> Result<(AgentConfig, ModelsManager), String> {
    crate::managed_config::managed_policy_gate()?;
    let cfg = resolve_config(cfg);
    cfg.validate_model_filters()?;
    init_process(&cfg);
    let models_manager = ModelsManager::from_config(&cfg)?;

    Ok((cfg, models_manager))
}

/// Print a `bootstrap`/`MvpAgent::new` config error and exit (process boundary).
///
/// Restores native stderr first: a managed-policy refusal on the ACP/server path reaches here
/// while fd 2 may still point at the `/dev/null` the TUI's `redirect_native_stderr()` set, which
/// would swallow the message. No-op when stderr was never redirected (headless).
pub(crate) fn exit_on_config_error<T>(e: String) -> T {
    tty_utils::restore_native_stderr();
    eprintln!("\nConfiguration error:\n\n    {e}\n");
    std::process::exit(1);
}

/// Config transform: apply local managed settings and requirements.
fn resolve_config(cfg: &AgentConfig) -> AgentConfig {
    let mut cfg = cfg.clone();

    if let Ok(layers) = crate::config::ConfigLayers::load()
        && layers.has_managed()
    {
        let origins = crate::config::config_origins(&layers);
        let managed_keys: Vec<&str> = origins
            .iter()
            .filter(|(_, s)| matches!(s, config::ConfigSource::ManagedConfig))
            .map(|(k, _)| k.as_str())
            .collect();
        if !managed_keys.is_empty() {
            tracing::info!(keys = ?managed_keys, "managed_config.toml fields");
        }
    }

    // Unit tests must not inherit machine-global managed settings or
    // requirements (for example a developer's pinned default model).
    let (managed_enforced, requirements_enforced) = if cfg!(test) {
        (Vec::new(), Vec::new())
    } else {
        (
            crate::config::apply_managed_settings_features(&mut cfg),
            crate::config::apply_requirements(&mut cfg),
        )
    };

    for e in managed_enforced.iter().chain(&requirements_enforced) {
        tracing::info!(field = %e.path, value = %e.value, source = %e.source, "policy override");
    }

    if !cfg!(test) {
        crate::util::config::sync_campaign_fields(&mut cfg);
    }

    if let Some(rs) = cfg.remote_settings.as_ref()
        && let Some(v) = rs.path_not_found_hints
    {
        cfg.path_not_found_hints = v;
    }

    cfg
}

/// Initialize process-level singletons. `Once`-guarded: only the first call
/// takes effect.
fn init_process(cfg: &AgentConfig) {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // Every agent mode passes through here, so local diagnostic records
        // carry the version stamp and resource ceilings in effect.
        ::diagnostics::unified_log::set_version(version::VERSION);
        crate::util::limits::log_effective_limits();

        if !cfg!(test) {
            // Clear a logged-out team's files before the background sync runs.
            crate::managed_config::clear_orphan();
            crate::managed_config::spawn_sync(tokio_util::sync::CancellationToken::new());
        }

        let grow_home = crate::util::grow_home::grow_home();
        crate::builtin::extract_builtin_files(&grow_home);

        crate::extensions::marketplace::purge_default_skills_installs(&grow_home);
    });
}
