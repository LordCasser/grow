//! Marketplace plugin discovery.
//!
//! Discovery is driven exclusively by `.grow-plugin/marketplace.json`; the
//! index is the marketplace identity and allowlist.

use std::path::Path;

use crate::catalog;
use crate::index;
use crate::types::{MarketplaceEntry, MarketplaceScan};
use extension_types::{ComponentItem, PluginComponents};

/// Scan a marketplace directory for plugins, reporting whether a
/// `plugin-index.json` component catalog was loaded.
///
pub fn scan_marketplace(root: &Path) -> Result<MarketplaceScan, String> {
    let idx = index::load_index(root)?;
    tracing::debug!(
        "using marketplace index: {} ({} plugins)",
        idx.name,
        idx.plugins.len()
    );
    let plugin_catalog = catalog::load_catalog(root);
    let mut plugins = Vec::new();
    for entry in &idx.plugins {
        // URL-sourced entries: build entry from index metadata only
        // (the actual repo is cloned at install time, not scan time).
        if let Some((url, git_ref)) = entry.remote_url() {
            let discovered = MarketplaceEntry {
                name: entry.name.clone(),
                version: entry.version.clone(),
                description: entry.description.clone(),
                category: entry.category.clone(),
                author: entry.author.as_ref().map(|a| a.name.clone()),
                tags: entry.tags.clone(),
                keywords: entry.keywords.clone(),
                domains: entry.domains.clone(),
                homepage: entry.homepage.clone(),
                relative_path: entry.name.clone(),
                remote_url: Some(url.to_string()),
                remote_ref: git_ref.map(|s| s.to_string()),
                remote_sha: entry.remote_sha().map(|s| s.to_string()),
                remote_subdir: entry.remote_subdir().map(|s| s.to_string()),
                components: entry.remote_sha().and_then(|sha| {
                    plugin_catalog
                        .as_ref()
                        .and_then(|c| c.components_for(&entry.name, Some(sha)).cloned())
                }),
            };
            plugins.push(discovered);
            continue;
        }

        let rel_path = match entry.resolved_marketplace_path() {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    "marketplace index entry '{}' has invalid source path: {}",
                    entry.name,
                    e
                );
                continue;
            }
        };
        let plugin_dir = match rel_path.join_under(root) {
            Ok(path) => path,
            Err(e) => {
                tracing::warn!(
                    "marketplace index entry '{}' source path escapes marketplace root: {}",
                    entry.name,
                    e
                );
                continue;
            }
        };
        if !plugin_dir.is_dir() {
            tracing::warn!(
                "marketplace index entry '{}' points to non-existent dir: {}",
                entry.name,
                plugin_dir.display()
            );
            continue;
        }
        let Some(mut discovered) = scan_single_plugin(&plugin_dir, rel_path.as_str()) else {
            tracing::warn!(
                plugin = entry.name,
                path = %plugin_dir.display(),
                "marketplace plugin has no valid plugin.json"
            );
            continue;
        };
        // Enrich from index metadata.
        if discovered.description.is_none() {
            discovered.description = entry.description.clone();
        }
        discovered.category = entry.category.clone();
        discovered.tags = entry.tags.clone();
        discovered.keywords = entry.keywords.clone();
        discovered.domains = entry.domains.clone();
        discovered.homepage = entry.homepage.clone();
        if discovered.author.is_none() {
            discovered.author = entry.author.as_ref().map(|a| a.name.clone());
        }
        if let Some(components) = plugin_catalog
            .as_ref()
            .and_then(|c| c.components_for(&entry.name, None).cloned())
        {
            discovered.components = Some(components);
        }
        plugins.push(discovered);
    }
    Ok(MarketplaceScan {
        entries: plugins,
        catalog_loaded: plugin_catalog.is_some(),
    })
}

/// Scan a single plugin directory for metadata and components.
fn scan_single_plugin(plugin_dir: &Path, relative_path: &str) -> Option<MarketplaceEntry> {
    let manifest = agent::plugins::manifest::load_manifest(plugin_dir).ok()?;
    let name = manifest.name.clone();
    let version = manifest.version.clone();
    let description = manifest.description.clone();
    let author = manifest.author.as_ref().and_then(|a| a.name.clone());

    let components = scan_components(plugin_dir, &manifest);

    Some(MarketplaceEntry {
        name,
        version,
        description,
        category: None,
        author,
        tags: Vec::new(),
        keywords: Vec::new(),
        domains: Vec::new(),
        homepage: None,
        relative_path: relative_path.to_string(),
        remote_url: None,
        remote_ref: None,
        remote_sha: None,
        remote_subdir: None,
        components: Some(components),
    })
}

fn scan_components(
    plugin_dir: &Path,
    manifest: &agent::plugins::manifest::PluginManifest,
) -> PluginComponents {
    let skill_dirs = manifest.skill_dirs(plugin_dir);
    let command_dirs = manifest.command_dirs(plugin_dir);
    let agent_dirs = manifest.agent_dirs(plugin_dir);

    let mut components = PluginComponents {
        skills: agent::plugins::registry::skill_md_paths(&skill_dirs)
            .into_iter()
            .filter_map(|path| component_name_from_parent(&path))
            .map(|name| ComponentItem::new(name, None))
            .collect(),
        commands: md_component_items(&command_dirs),
        agents: md_component_items(&agent_dirs),
        ..PluginComponents::default()
    };

    let hooks_path = manifest.hooks_path(plugin_dir);
    if let Some(value) = hooks_path.as_deref().and_then(read_json) {
        append_hook_items(&value, &mut components.hooks);
    }
    if let Some(value) = manifest.inline_hooks() {
        append_hook_items(value, &mut components.hooks);
    }

    let mcp_path = manifest.mcp_config_path(plugin_dir);
    if let Some(value) = mcp_path.as_deref().and_then(read_json) {
        append_object_keys(value.get("mcpServers"), &mut components.mcp_servers);
    }
    if let Some(value) = manifest.inline_mcp_servers() {
        let normalized = agent::plugins::manifest::normalize_inline_mcp_servers(value);
        append_object_keys(normalized.get("mcpServers"), &mut components.mcp_servers);
    }

    let lsp_path = manifest.lsp_config_path(plugin_dir);
    if let Some(value) = lsp_path.as_deref().and_then(read_json) {
        append_object_keys(Some(&value), &mut components.lsp_servers);
    }
    if let Some(value) = manifest.inline_lsp_servers() {
        append_object_keys(Some(value), &mut components.lsp_servers);
    }

    dedupe_components(&mut components);
    components
}

fn component_name_from_parent(path: &Path) -> Option<String> {
    path.parent()?.file_name()?.to_str().map(ToOwned::to_owned)
}

fn md_component_items(dirs: &[std::path::PathBuf]) -> Vec<ComponentItem> {
    dirs.iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "md"))
        .filter_map(|entry| {
            entry
                .path()
                .file_stem()
                .and_then(|name| name.to_str())
                .map(|name| ComponentItem::new(name, None))
        })
        .collect()
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
}

fn append_object_keys(value: Option<&serde_json::Value>, target: &mut Vec<ComponentItem>) {
    let Some(object) = value.and_then(serde_json::Value::as_object) else {
        return;
    };
    target.extend(
        object
            .keys()
            .map(|name| ComponentItem::new(name.clone(), None)),
    );
}

fn append_hook_items(value: &serde_json::Value, target: &mut Vec<ComponentItem>) {
    let Some(events) = value.get("hooks").and_then(serde_json::Value::as_object) else {
        return;
    };
    for (event, groups) in events {
        let Some(groups) = groups.as_array() else {
            continue;
        };
        for group in groups {
            let matcher = group
                .get("matcher")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned);
            let handler_count = group
                .get("hooks")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len);
            target.extend(
                std::iter::repeat_with(|| ComponentItem::new(event.clone(), matcher.clone()))
                    .take(handler_count),
            );
        }
    }
}

fn dedupe_components(components: &mut PluginComponents) {
    for items in [
        &mut components.skills,
        &mut components.commands,
        &mut components.agents,
        &mut components.mcp_servers,
        &mut components.lsp_servers,
    ] {
        let mut seen = std::collections::HashSet::new();
        items.retain(|item| seen.insert((item.name.clone(), item.description.clone())));
        items.sort_by(|left, right| {
            (&left.name, &left.description).cmp(&(&right.name, &right.description))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_index(root: &Path, plugins: &str) {
        let directory = root.join(".grow-plugin");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("marketplace.json"),
            format!(r#"{{"version":1,"name":"test","plugins":[{plugins}]}}"#),
        )
        .unwrap();
    }

    fn write_plugin(root: &Path, name: &str) {
        let plugin = root.join("plugins").join(name);
        std::fs::create_dir_all(plugin.join("skills").join("review")).unwrap();
        std::fs::write(
            plugin.join("plugin.json"),
            format!(r#"{{"name":"{name}","version":"1.0.0"}}"#),
        )
        .unwrap();
        std::fs::write(plugin.join("skills/review/SKILL.md"), "# Review").unwrap();
    }

    #[test]
    fn canonical_index_drives_local_discovery() {
        let root = tempfile::tempdir().unwrap();
        write_plugin(root.path(), "review-tools");
        write_index(
            root.path(),
            r#"{"name":"review-tools","description":"Review","source":{"type":"local","path":"plugins/review-tools"}}"#,
        );

        let scan = scan_marketplace(root.path()).unwrap();
        assert_eq!(scan.entries.len(), 1);
        let plugin = &scan.entries[0];
        assert_eq!(plugin.name, "review-tools");
        assert_eq!(plugin.description.as_deref(), Some("Review"));
        assert_eq!(plugin.components.as_ref().unwrap().skills.len(), 1);
    }

    #[test]
    fn git_entry_uses_canonical_source_shape() {
        let root = tempfile::tempdir().unwrap();
        write_index(
            root.path(),
            r#"{"name":"remote","source":{"type":"git","url":"https://example.com/plugin.git","ref":"main","sha":"abc","path":"plugin"}}"#,
        );

        let plugin = scan_marketplace(root.path()).unwrap().entries.remove(0);
        assert_eq!(
            plugin.remote_url.as_deref(),
            Some("https://example.com/plugin.git")
        );
        assert_eq!(plugin.remote_ref.as_deref(), Some("main"));
        assert_eq!(plugin.remote_sha.as_deref(), Some("abc"));
        assert_eq!(plugin.remote_subdir.as_deref(), Some("plugin"));
    }

    #[test]
    fn missing_or_invalid_index_is_an_error() {
        let missing = tempfile::tempdir().unwrap();
        write_plugin(missing.path(), "unindexed");
        assert!(scan_marketplace(missing.path()).is_err());

        let invalid = tempfile::tempdir().unwrap();
        let directory = invalid.path().join(".grow-plugin");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("marketplace.json"), "not json").unwrap();
        assert!(scan_marketplace(invalid.path()).is_err());
    }

    #[test]
    fn indexed_directory_without_manifest_is_not_a_plugin() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("plugins/unmanifested/skills")).unwrap();
        write_index(
            root.path(),
            r#"{"name":"unmanifested","source":{"type":"local","path":"plugins/unmanifested"}}"#,
        );
        assert!(scan_marketplace(root.path()).unwrap().entries.is_empty());
    }
}
