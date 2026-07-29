//! Agent bootstrap and lifecycle hooks.
//!
//! [`bootstrap`] runs the full init sequence (config resolution, process
//! singletons, model catalog) and returns a resolved config + `ModelsManager`.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::agent::config::{self, Config as AgentConfig, ModelEntry};
use crate::agent::models::ModelsManager;
use crate::auth::AuthManager;
use crate::config::StorageMode;

/// Resolve config, init process singletons, build the model catalog.
///
/// The `ModelsManager` is `Clone + Send`, so callers that need a handle
/// for the config watcher can clone it before passing it to
/// `MvpAgent::with_models`.
pub fn bootstrap(
    cfg: &AgentConfig,
    auth_manager: &Arc<AuthManager>,
    prefetched: Option<IndexMap<String, ModelEntry>>,
) -> Result<(AgentConfig, ModelsManager), String> {
    // Remote kill-switch before the gate (settings-only prefetch — no managed-config
    // sync, so a live server cannot heal a tampered policy before fail-closed).
    let mut cfg = cfg.clone();
    ensure_remote_settings_side_effects(&mut cfg, false);
    crate::managed_config::managed_policy_gate()?;
    let cfg = resolve_config(&cfg, auth_manager);
    cfg.validate_model_filters()?;
    init_process(&cfg, auth_manager);
    let models_manager = ModelsManager::from_config(&cfg, prefetched, auth_manager.clone())?;

    // Refresh on every auth refresh — the FSEvents watcher can silently die after
    // macOS sleep, stranding the catalog on bundled defaults.
    models_manager.start_auth_refresh_watcher(auth_manager.refresh_notifier());

    Ok((cfg, models_manager))
}

/// Print a `bootstrap`/`MvpAgent::new` config error and exit (process boundary).
///
/// Restores native stderr first: a managed-policy refusal on the ACP/server path reaches here
/// while fd 2 may still point at the `/dev/null` the TUI's `redirect_native_stderr()` set, which
/// would swallow the message. No-op when stderr was never redirected (headless).
pub(crate) fn exit_on_config_error<T>(e: String) -> T {
    xai_tty_utils::restore_native_stderr();
    eprintln!("\nConfiguration error:\n\n    {e}\n");
    std::process::exit(1);
}

/// Fill `remote_settings` if absent and apply process-global remote side effects
/// (signature kill-switch and caches). Safe to call more than once.
///
/// `sync_managed`: when true, missing-settings fallback may also refresh
/// managed-config. Must be false before the managed-policy gate.
fn ensure_remote_settings_side_effects(cfg: &mut AgentConfig, sync_managed: bool) {
    // Fallback: if the client didn't pre-supply remote settings, fetch them
    // now so remote-settings-gated features work regardless of which client
    // spawned us. Clients that already call `start_early_prefetch()` and
    // thread the result into `cfg.remote_settings` skip this entirely.
    if cfg.remote_settings.is_none() {
        let handle = if sync_managed {
            crate::agent::models::start_early_prefetch(Some(cfg.auth.clone()))
        } else {
            crate::agent::models::start_early_prefetch_settings_only(Some(cfg.auth.clone()))
        };
        if let Some(handle) = handle {
            match handle.join() {
                Ok(result) => {
                    cfg.remote_settings = result.settings;
                    crate::util::config::set_remote_campaigns_from_settings(
                        cfg.remote_settings.as_ref(),
                    );
                    tracing::info!("remote_settings fetched as shell-level fallback");
                }
                Err(_) => {
                    tracing::warn!("remote_settings fallback prefetch thread panicked");
                }
            }
        }
    }
    crate::agent::config::apply_remote_settings_side_effects(cfg.remote_settings.as_ref());
}

/// Config transform: apply managed settings, fetch remote settings,
/// resolve storage mode.
fn resolve_config(cfg: &AgentConfig, auth_manager: &AuthManager) -> AgentConfig {
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

    // Idempotent: bootstrap may already have fetched + applied side effects for the gate.
    // Full prefetch (with managed-config sync when stale) is allowed after the gate.
    ensure_remote_settings_side_effects(&mut cfg, true);
    if !cfg!(test) {
        crate::util::config::sync_campaign_fields(&mut cfg);
    }

    // env var > remote settings > Local. Skip remote settings for Generic (grow -p, subagents).
    let has_service_auth = auth_manager.current().is_some_and(|a| a.is_service_auth());
    if cfg.storage_mode == StorageMode::Local
        && cfg.mode != crate::agent::config::AgentMode::Generic
    {
        cfg.storage_mode =
            StorageMode::from_remote_gated(cfg.remote_settings.as_ref(), has_service_auth);
    }
    // A CLI/env-set Writeback still requires service.example.com auth.
    if cfg.storage_mode == StorageMode::Writeback && !has_service_auth {
        tracing::info!("Writeback is disabled: requires auth with service.example.com");
        cfg.storage_mode = StorageMode::Local;
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
fn init_process(cfg: &AgentConfig, _auth_manager: &AuthManager) {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // Every agent mode passes through here, so local diagnostic records
        // carry the version stamp and resource ceilings in effect.
        grow_diagnostics::unified_log::set_version(grow_version::VERSION);
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
