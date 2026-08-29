//! Grow configuration loaded from `$GROW_HOME/config.toml`.
//!
//! Trusted project `.grow/config.toml` files are discovered and overlaid by
//! workspace-aware consumers because their merge semantics are subsystem-specific.

pub mod campaigns;
pub mod config_override;
pub mod fs_atomic;
pub mod global_hook_sources;
mod loader;
pub mod managed_text;
mod paths;
pub mod shell;
pub mod version_overrides;

// Only the cross-crate campaign surface is re-exported at the root; the rest stays
// reachable via the `pub mod` paths for in-crate use without widening the API.
pub use campaigns::{
    CampaignEntry, CampaignOverrides, filter_active_campaigns, ids_touching_paths,
};
pub use global_hook_sources::{
    GlobalHookSource, GlobalHookSourceError, GlobalHookSourceKind, ResolvedGlobalHookSources,
    ensure_grow_hook_slots, existing_ancestor_chain, is_direct_hook_json_name,
    list_direct_hook_json_files, missing_configured_sources, path_has_symlink_component,
    resolve_global_hook_sources, unique_ancestors_rootward,
};

#[cfg(unix)]
pub use global_hook_sources::{
    validate_direct_hook_json_file, validated_hook_json_files_for_sources,
};
pub use loader::{
    CampaignsState, ConfigLayers, HookConfigLayer, HookProvenance, USER_CONFIG_FILENAME,
    apply_version_overrides_with_registered, campaigns_application_disabled, campaigns_state_path,
    deep_merge_toml, expand_env_vars_in_string, expand_env_vars_in_toml, hook_config_layers,
    hook_config_layers_at, load_config_file, load_dismissed_ids_from_home,
    load_effective_config_disk_only, load_from_disk, load_toml_file, toml_error_detail,
};
pub use paths::{
    decode_cwd_from_dirname, default_grow_home, encode_cwd_dirname, grow_application,
    grow_application_in, grow_home, sessions_cwd_dir, user_grow_home,
};
pub use version_overrides::{VersionOverrideError, apply_version_overrides};

/// Parse an env var as a boolean. `None` if unset or unrecognized.
pub fn env_bool(name: &str) -> Option<bool> {
    let value = std::env::var(name).ok()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "" => None,
        "1" | "true" | "yes" | "on" | "enabled" => Some(true),
        "0" | "false" | "no" | "off" | "disabled" => Some(false),
        _ => None,
    }
}
