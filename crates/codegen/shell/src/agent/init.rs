//! Agent bootstrap and lifecycle hooks.
//!
//! [`bootstrap`] runs the full init sequence (config resolution, process
//! singletons, model catalog) and returns a resolved config + `ModelsManager`.

use crate::agent::config::Config as AgentConfig;
use crate::agent::models::ModelsManager;

/// Resolve config, init process singletons, build the model catalog.
///
/// The `ModelsManager` is `Clone + Send`, so callers that need a handle
/// for the config watcher can clone it before passing it to
/// `MvpAgent::with_models`.
pub fn bootstrap(cfg: &AgentConfig) -> Result<(AgentConfig, ModelsManager), String> {
    let cfg = resolve_config(cfg);
    cfg.validate_model_filters()?;
    init_process(&cfg);
    let models_manager = ModelsManager::from_config(&cfg)?;

    Ok((cfg, models_manager))
}

/// Print a `bootstrap`/`MvpAgent::new` config error and exit (process boundary).
///
/// Restores native stderr before reporting the error. No-op when stderr was never redirected.
pub(crate) fn exit_on_config_error<T>(e: String) -> T {
    tty_utils::restore_native_stderr();
    eprintln!("\nConfiguration error:\n\n    {e}\n");
    std::process::exit(1);
}

/// Config transform for values that are supplied by the active agent backend.
fn resolve_config(cfg: &AgentConfig) -> AgentConfig {
    let mut cfg = cfg.clone();

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

        let grow_home = crate::util::grow_home::grow_home();
        crate::builtin::extract_builtin_files(&grow_home);
    });
}
