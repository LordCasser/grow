//! Shared hook source path discovery.

use std::path::{Path, PathBuf};

use config::resolve_global_hook_sources;
use hooks::discovery::HookSource;
use hooks::error::HookError;

/// Owned paths for hook sources. Callers borrow via `as_sources()`.
pub struct HookSourcePaths {
    pub global: Vec<PathBuf>,
    pub project: Vec<PathBuf>,
}

impl HookSourcePaths {
    /// Borrow as `HookSource` refs. Project sources are excluded when untrusted.
    pub fn as_sources(&self, include_project: bool) -> (Vec<HookSource<'_>>, Vec<HookSource<'_>>) {
        let global = self.global.iter().map(|p| path_to_source(p)).collect();
        let project = if include_project {
            self.project.iter().map(|p| path_to_source(p)).collect()
        } else {
            vec![]
        };
        (global, project)
    }
}

fn path_to_source(p: &Path) -> HookSource<'_> {
    if p.is_dir() {
        HookSource::Directory(p)
    } else {
        HookSource::HookFile(p)
    }
}

/// Global + project Grow hook source paths. Registry files and foreign vendor
/// settings are never discovery sources.
pub fn discover_hook_source_paths(git_root: Option<&Path>) -> HookSourcePaths {
    let grow = config::user_grow_home();

    // Soft hooks-paths I/O keeps fixed slots; hard resolve omits Grow globals.
    let mut global: Vec<PathBuf> =
        match resolve_global_hook_sources(grow.as_deref(), /* reject_symlinks */ false) {
            Ok(resolved) => {
                if let Some(e) = &resolved.configured_error {
                    tracing::warn!(
                        error = %e,
                        "hooks-paths unreadable; retaining fixed Grow hook discovery sources only"
                    );
                }
                resolved
                    .discovery_sources()
                    .map(|s| s.path.clone())
                    .collect()
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "global hook source resolve hard-failed; omitting Grow global sources"
                );
                Vec::new()
            }
        };

    let mut project = Vec::new();
    if let Some(root) = git_root {
        project.push(root.join(".grow").join("hooks"));
    }

    HookSourcePaths { global, project }
}

/// Single load entry point: build canonical Grow sources, gate project sources on
/// trust, then load. Every session-startup and mid-session reload site routes
/// through here so the source policy stays in one place.
pub fn discover_hooks(
    git_root: Option<&Path>,
    trusted: bool,
) -> (::hooks::discovery::HookRegistry, Vec<HookError>) {
    // Read fresh each call (not cached): a mid-session `/hooks` reload must see an
    // updated `config.toml`. This is lighter than
    // `ConfigLayers::load` (only the small user config file, with no campaigns
    // or version overrides).
    let config_layers = config::hook_config_layers();
    assemble_hooks(&config_layers, git_root, trusted)
}

/// Pure, injectable core: combine config-layer hooks with file-source hooks and
/// dedup once. Config-layer specs are placed first so that, under the first-wins
/// dedup in [`::hooks::discovery::registry_from_specs_deduped`], a config
/// hook wins over a byte-identical file hook. `config_layers` is a parameter (not
/// read here) so tests can drive it with hand-built layers.
pub fn assemble_hooks(
    config_layers: &[config::HookConfigLayer],
    git_root: Option<&Path>,
    trusted: bool,
) -> (::hooks::discovery::HookRegistry, Vec<HookError>) {
    let (mut specs, mut errors) = ::hooks::config::parse_hooks_from_config_layers(config_layers);

    let source_paths = discover_hook_source_paths(git_root);
    let (global_sources, project_sources) = source_paths.as_sources(trusted);
    let (file_specs, file_errors) =
        ::hooks::discovery::collect_specs_from_sources(&global_sources, &project_sources);
    specs.extend(file_specs);
    errors.extend(file_errors);

    (
        ::hooks::discovery::registry_from_specs_deduped(specs),
        errors,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_hook_sources_require_a_fresh_trust_verdict() {
        let paths = HookSourcePaths {
            global: vec![PathBuf::from("/global/hooks")],
            project: vec![PathBuf::from("/project/.grow/hooks")],
        };

        let (global, project) = paths.as_sources(false);
        assert_eq!(global.len(), 1);
        assert!(project.is_empty());

        let (_, project) = paths.as_sources(true);
        assert_eq!(project.len(), 1);
    }
}
