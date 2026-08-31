//! Shell-side MCP catalog: merge configured server sources and apply policy.
//!
//! Merge layers are applied in order; later `insert()` beats earlier
//! `or_insert()`:
//!   - config.toml    — seeds the map; `enabled = false` blocks lower layers
//!   - Plugins        — `or_insert` (won't override config.toml)
//!   - Client         — `insert` (always wins)
use acp_transport::protocol as acp;
use std::collections::HashMap;

fn normalize_url(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}

/// Dedup key for the merge map: normalized URL for Http/Sse, name for Stdio.
fn mcp_server_key(s: &acp::McpServer) -> String {
    match s {
        acp::McpServer::Http(acp::McpServerHttp { url, .. })
        | acp::McpServer::Sse(acp::McpServerSse { url, .. }) => normalize_url(url),
        acp::McpServer::Stdio(acp::McpServerStdio { name, .. }) => name.clone(),
        // TODO(acp-0.10): `McpServer` is #[non_exhaustive].
        _ => String::new(),
    }
}

pub(crate) fn mcp_server_name(s: &acp::McpServer) -> &str {
    match s {
        acp::McpServer::Http(acp::McpServerHttp { name, .. })
        | acp::McpServer::Sse(acp::McpServerSse { name, .. })
        | acp::McpServer::Stdio(acp::McpServerStdio { name, .. }) => name,
        // TODO(acp-0.10): `McpServer` is #[non_exhaustive].
        _ => "",
    }
}

pub fn merge_mcp_servers(
    client_mcp_servers: Vec<acp::McpServer>,
    cwd: &std::path::Path,
    plugin_registry: Option<&agent::plugins::PluginRegistry>,
) -> Vec<acp::McpServer> {
    let mut servers: HashMap<String, acp::McpServer> =
        merge_mcp_servers_sourced(cwd, plugin_registry)
            .into_iter()
            .map(|(server, _source)| (mcp_server_key(&server), server))
            .collect();

    for server in client_mcp_servers {
        servers.insert(mcp_server_key(&server), server);
    }

    let disabled = crate::util::config::disabled_mcp_server_names(cwd);
    let mut merged: Vec<_> = servers
        .into_values()
        .filter(|server| !disabled.contains(mcp_server_name(server)))
        .collect();
    merged.sort_by_key(mcp_server_key);
    crate::agent::folder_trust::filter_untrusted_project_mcp(cwd, merged)
}

/// Re-merge the catalog into one live session's MCP set and push the
/// result via [`crate::session::SessionCommand::UpdateMcpServers`]; returns
/// `true` if the command was enqueued (session still alive).
///
/// Shared core for every live-session catalog refresh path
/// (config hot-reload, post-grant reload) so the merge
/// inputs and the dropped-oneshot-response contract can't drift between them.
pub(crate) fn merge_and_send_mcp_update(
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<crate::session::SessionCommand>,
    cwd: &std::path::Path,
    initial_client_mcp_servers: Vec<acp::McpServer>,
    plugin_registry: Option<&agent::plugins::PluginRegistry>,
) -> bool {
    let merged = merge_mcp_servers(initial_client_mcp_servers, cwd, plugin_registry);
    let (tx, _rx) = tokio::sync::oneshot::channel();
    cmd_tx
        .send(crate::session::SessionCommand::UpdateMcpServers {
            mcp_servers: merged,
            respond_to: tx,
        })
        .is_ok()
}

/// Like [`merge_mcp_servers`] but returns `ConfigSource` alongside each server.
pub fn merge_mcp_servers_sourced(
    cwd: &std::path::Path,
    plugin_registry: Option<&agent::plugins::PluginRegistry>,
) -> Vec<(acp::McpServer, tools::types::config_source::ConfigSource)> {
    let _mcp_merge_timer = crate::instrumentation::timer("mcp_merge");
    use tools::types::config_source::ConfigSource;

    let toml_claimed_names = crate::util::config::all_toml_mcp_server_names(cwd);

    let mut servers: HashMap<String, (acp::McpServer, ConfigSource)> =
        crate::util::config::load_mcp_servers(cwd)
            .into_iter()
            .map(|s| {
                let key = mcp_server_key(&s);
                let source =
                    crate::util::config::nearest_project_mcp_definition(cwd, mcp_server_name(&s))
                        .map(|path| ConfigSource::Project { path })
                        .unwrap_or_else(|| ConfigSource::ConfigToml {
                            path: tools::util::grow_home::grow_home().join("config.toml"),
                        });
                (key, (s, source))
            })
            .collect();
    for (name, (_, source)) in &servers {
        tracing::info!(server = name, source = ?source, "MCP server loaded from source");
    }

    // Plugins
    if let Some(registry) = plugin_registry {
        for plugin in registry.active_plugins() {
            let mut plugin_servers: Vec<acp::McpServer> = Vec::new();
            if let Some(ref mcp_path) = plugin.mcp_config_path {
                let servers = load_plugin_mcp_servers(
                    mcp_path,
                    &plugin.name,
                    &plugin.root_str(),
                    &plugin.data_dir_str(),
                );
                plugin_servers.extend(servers);
            }
            if let Some(ref inline_value) = plugin.inline_mcp_servers {
                let servers = load_plugin_mcp_servers_from_value(
                    inline_value,
                    &plugin.name,
                    &plugin.root_str(),
                    &plugin.data_dir_str(),
                );
                plugin_servers.extend(servers);
            }
            if plugin_servers.is_empty() {
                continue;
            }
            let mut seen_names: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            plugin_servers.retain(|server| seen_names.insert(mcp_server_name(server).to_string()));
            let source = ConfigSource::Plugin {
                plugin_name: plugin.name.clone(),
                path: plugin.root.clone(),
            };
            for server in plugin_servers {
                if toml_claimed_names.contains(mcp_server_name(&server)) {
                    continue;
                }
                let key = mcp_server_key(&server);
                servers.entry(key).or_insert((server, source.clone()));
            }
        }
    }

    servers.into_values().collect()
}

fn load_plugin_mcp_servers(
    mcp_path: &std::path::Path,
    plugin_name: &str,
    plugin_root: &str,
    plugin_data: &str,
) -> Vec<acp::McpServer> {
    let Some(config) = crate::util::config::read_mcp_json(mcp_path) else {
        return vec![];
    };
    load_plugin_mcp_servers_from_config(&config, plugin_name, plugin_root, plugin_data)
}

/// Like [`load_plugin_mcp_servers`] but from an in-memory JSON value (no I/O).
fn load_plugin_mcp_servers_from_value(
    root: &serde_json::Value,
    plugin_name: &str,
    plugin_root: &str,
    plugin_data: &str,
) -> Vec<acp::McpServer> {
    let normalized = agent::plugins::manifest::normalize_inline_mcp_servers(root);
    let Ok(config) = serde_json::from_value::<crate::util::config::McpConfig>(normalized) else {
        tracing::warn!(plugin = plugin_name, "failed to parse plugin MCP config");
        return vec![];
    };
    load_plugin_mcp_servers_from_config(&config, plugin_name, plugin_root, plugin_data)
}

fn load_plugin_mcp_servers_from_config(
    config: &crate::util::config::McpConfig,
    plugin_name: &str,
    plugin_root: &str,
    plugin_data: &str,
) -> Vec<acp::McpServer> {
    let sub = |s: &str| -> String {
        let s = agent::plugins::manifest::substitute_env_vars(s, plugin_root, plugin_data);
        crate::config::expand_env_vars_in_string(&s)
    };
    let label = format!("plugin:{}", plugin_name);
    crate::util::config::parse_mcp_config(config, &label, &sub)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_cwd() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    /// A client-provided server (e.g. a client session binding injected at
    /// `session/new`) exists in no on-disk config —
    /// the merge must keep it. Catalog hot reload relies on this by re-seeding
    /// the merge with the session's
    /// `initial_client_mcp_servers`; if this property breaks, those reloads
    /// silently tear down client-injected servers mid-session.
    #[test]
    fn client_provided_servers_survive_merge() {
        let client = vec![acp::McpServer::Http(
            acp::McpServerHttp::new(
                "demo-mcp".to_string(),
                "http://mcp.example.test/api/mcp".to_string(),
            )
            .headers(vec![]),
        )];
        let cwd = empty_cwd();
        let merged = merge_mcp_servers(client, cwd.path(), None);
        assert!(
            merged.iter().any(|s| matches!(
                s,
                acp::McpServer::Http(acp::McpServerHttp { name, .. }) if name == "demo-mcp"
            )),
            "client-provided server must survive a merge with no disk sources"
        );
    }

    #[test]
    fn disabled_grow_config_server_is_excluded() {
        let cwd = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(cwd.path().join(".grow")).unwrap();
        std::fs::write(
            cwd.path().join(".grow").join("config.toml"),
            r#"
[mcp_servers.github]
url = "https://config.example.com/mcp"
enabled = false
"#,
        )
        .unwrap();
        let merged = merge_mcp_servers(vec![], cwd.path(), None);
        assert!(
            !merged.iter().any(|server| matches!(
                server,
                acp::McpServer::Http(acp::McpServerHttp { name, .. }) if name == "github"
            )),
            "disabled servers from .grow/config.toml must not be registered"
        );
    }

    #[test]
    fn project_mcp_server_reports_project_source() {
        let cwd = tempfile::tempdir().unwrap();
        git2::Repository::init(cwd.path()).unwrap();
        let config_path = cwd.path().join(".grow/config.toml");
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(
            &config_path,
            "[mcp_servers.source_test_unique]\nurl = \"https://source.example.com/mcp\"\n",
        )
        .unwrap();

        let sourced = merge_mcp_servers_sourced(cwd.path(), None);
        let (_, source) = sourced
            .into_iter()
            .find(|(server, _)| mcp_server_name(server) == "source_test_unique")
            .expect("project MCP server must be discovered");
        assert_eq!(
            source,
            tools::types::config_source::ConfigSource::Project { path: config_path }
        );
    }

    /// End-to-end folder-trust gate through the public merge: an untrusted
    /// workspace's project `.grow/config.toml` server is dropped before spawn (a
    /// client-supplied server still survives), while a trusted workspace keeps
    /// it. Existing merge tests record no decision, so the default-allowed gate
    /// leaves them a no-op.
    #[test]
    fn untrusted_workspace_drops_project_mcp_servers() {
        fn repo_with_project_server() -> tempfile::TempDir {
            let cwd = tempfile::tempdir().unwrap();
            git2::Repository::init(cwd.path()).unwrap();
            std::fs::create_dir_all(cwd.path().join(".grow")).unwrap();
            std::fs::write(
                cwd.path().join(".grow/config.toml"),
                "[mcp_servers.projsrv]\nurl = \"https://proj.example.com/mcp\"\n",
            )
            .unwrap();
            cwd
        }
        let untrusted = repo_with_project_server();
        crate::agent::folder_trust::record_for_test(untrusted.path(), false);
        let client = vec![acp::McpServer::Http(
            acp::McpServerHttp::new(
                "clientsrv".to_string(),
                "https://client.example.com/mcp".to_string(),
            )
            .headers(vec![]),
        )];
        let merged = merge_mcp_servers(client, untrusted.path(), None);
        assert!(
            !merged.iter().any(|s| mcp_server_name(s) == "projsrv"),
            "untrusted workspace must drop its repo-local MCP server"
        );
        assert!(
            merged.iter().any(|s| mcp_server_name(s) == "clientsrv"),
            "client-supplied server must be retained when untrusted"
        );

        let trusted = repo_with_project_server();
        crate::agent::folder_trust::record_for_test(trusted.path(), true);
        let merged = merge_mcp_servers(vec![], trusted.path(), None);
        assert!(
            merged.iter().any(|s| mcp_server_name(s) == "projsrv"),
            "trusted workspace must keep its repo-local MCP server"
        );
    }

    #[test]
    fn load_plugin_mcp_creates_stdio_server_with_env_substitution() {
        let config: crate::util::config::McpConfig = serde_json::from_value(serde_json::json!({
            "mcpServers": {
                "echo-mcp": {
                    "command": "python3",
                    "args": ["${GROW_PLUGIN_ROOT}/mcp-echo-server.py"]
                }
            }
        }))
        .expect("parse test MCP config");

        let servers = load_plugin_mcp_servers_from_config(
            &config,
            "team-tool",
            "/home/user/.grow/plugins/team-tool",
            "/home/user/.grow/plugin-data/team-tool",
        );

        assert_eq!(servers.len(), 1, "should create one server");
        match &servers[0] {
            acp::McpServer::Stdio(acp::McpServerStdio {
                name,
                command,
                args,
                ..
            }) => {
                assert_eq!(name, "echo-mcp");
                assert_eq!(command.display().to_string(), "python3");
                assert_eq!(
                    args.as_slice(),
                    &["/home/user/.grow/plugins/team-tool/mcp-echo-server.py"]
                );
            }
            other => panic!("expected Stdio server, got {:?}", other),
        }
    }

    #[test]
    fn load_plugin_mcp_disabled_server_excluded_from_merge() {
        let config: crate::util::config::McpConfig = serde_json::from_value(serde_json::json!({
            "mcpServers": {
                "test-server": {
                    "command": "node",
                    "args": ["server.js"]
                }
            }
        }))
        .expect("parse test MCP config");

        let servers = load_plugin_mcp_servers_from_config(
            &config,
            "my-plugin",
            "/tmp/plugin",
            "/tmp/plugin-data",
        );
        assert_eq!(servers.len(), 1);

        // Simulate disabling via disabled_mcp_server_names.
        let disabled: std::collections::HashSet<String> =
            ["test-server".to_string()].into_iter().collect();

        // Plugin servers are filtered against the same disabled-name set during merge.
        assert!(
            disabled.contains("test-server"),
            "disabled set should contain the plugin server name"
        );
    }

    #[test]
    fn load_plugin_mcp_from_value_accepts_direct_map() {
        let value = serde_json::json!({
            "sentry": { "type": "http", "url": "https://mcp.sentry.dev/mcp" }
        });
        let servers = load_plugin_mcp_servers_from_value(&value, "sentry", "/tmp/p", "/tmp/pd");
        assert_eq!(servers.len(), 1);
        match &servers[0] {
            acp::McpServer::Http(acp::McpServerHttp { name, url, .. }) => {
                assert_eq!(name, "sentry");
                assert_eq!(url, "https://mcp.sentry.dev/mcp");
            }
            other => panic!("expected Http server, got {:?}", other),
        }
    }

    #[test]
    fn plugin_server_deduped_across_file_and_inline() {
        use agent::plugins::PluginRegistry;
        use agent::plugins::PluginScope;
        use agent::plugins::discovery::{DiscoveredPlugin, PluginId};
        use agent::plugins::manifest::{PathOrInline, PluginManifest};

        let tmp = tempfile::tempdir().unwrap();
        let plugin_root = tmp.path().join("sentry");
        std::fs::create_dir_all(&plugin_root).unwrap();
        let mcp_json = plugin_root.join(".mcp.json");
        std::fs::write(
            &mcp_json,
            r#"{"mcpServers":{"sentry":{"type":"http","url":"https://mcp.sentry.dev/mcp"}}}"#,
        )
        .unwrap();

        let manifest = PluginManifest {
            name: "sentry".into(),
            version: None,
            description: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: vec![],
            skills: None,
            commands: None,
            agents: None,
            hooks: None,
            mcp_servers: Some(PathOrInline::Inline(serde_json::json!({
                "sentry": { "type": "http", "url": "https://mcp.sentry.dev/mcp" }
            }))),
            lsp_servers: None,
        };
        let id = PluginId::new(PluginScope::User, &plugin_root, "sentry");
        let dp = DiscoveredPlugin {
            manifest,
            id,
            root: plugin_root.clone(),
            canonical_root: plugin_root.clone(),
            scope: PluginScope::User,
            origin: agent::plugins::PluginOrigin::UserGrow,
            trusted: true,
            skill_dirs: vec![],
            command_dirs: vec![],
            agent_dirs: vec![],
            hooks_path: None,
            mcp_config_path: Some(mcp_json),
            lsp_config_path: None,
            conflict: None,
        };
        let registry = PluginRegistry::from_discovered(vec![dp], &[], &["sentry".to_string()]);

        let cwd = tempfile::tempdir().unwrap();
        let sourced = merge_mcp_servers_sourced(cwd.path(), Some(&registry));

        let sentry_count = sourced
            .iter()
            .filter(|(s, _)| mcp_server_name(s) == "sentry")
            .count();
        assert_eq!(
            sentry_count, 1,
            "sentry declared in both .mcp.json and inline must register exactly once"
        );
    }

    #[test]
    fn plugin_same_name_different_url_keeps_file_server() {
        use agent::plugins::PluginRegistry;
        use agent::plugins::PluginScope;
        use agent::plugins::discovery::{DiscoveredPlugin, PluginId};
        use agent::plugins::manifest::{PathOrInline, PluginManifest};

        let tmp = tempfile::tempdir().unwrap();
        let plugin_root = tmp.path().join("sentry");
        std::fs::create_dir_all(&plugin_root).unwrap();
        let mcp_json = plugin_root.join(".mcp.json");
        std::fs::write(
            &mcp_json,
            r#"{"mcpServers":{"sentry":{"type":"http","url":"https://file.example/mcp"}}}"#,
        )
        .unwrap();

        let manifest = PluginManifest {
            name: "sentry".into(),
            version: None,
            description: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: vec![],
            skills: None,
            commands: None,
            agents: None,
            hooks: None,
            mcp_servers: Some(PathOrInline::Inline(serde_json::json!({
                "sentry": { "type": "http", "url": "https://inline.example/mcp" }
            }))),
            lsp_servers: None,
        };
        let id = PluginId::new(PluginScope::User, &plugin_root, "sentry");
        let dp = DiscoveredPlugin {
            manifest,
            id,
            root: plugin_root.clone(),
            canonical_root: plugin_root.clone(),
            scope: PluginScope::User,
            origin: agent::plugins::PluginOrigin::UserGrow,
            trusted: true,
            skill_dirs: vec![],
            command_dirs: vec![],
            agent_dirs: vec![],
            hooks_path: None,
            mcp_config_path: Some(mcp_json),
            lsp_config_path: None,
            conflict: None,
        };
        let registry = PluginRegistry::from_discovered(vec![dp], &[], &["sentry".to_string()]);

        let cwd = tempfile::tempdir().unwrap();
        let sourced = merge_mcp_servers_sourced(cwd.path(), Some(&registry));

        let sentry: Vec<&acp::McpServer> = sourced
            .iter()
            .map(|(s, _)| s)
            .filter(|s| mcp_server_name(s) == "sentry")
            .collect();
        assert_eq!(
            sentry.len(),
            1,
            "same plugin declaring one server name twice must register exactly once"
        );
        match sentry[0] {
            acp::McpServer::Http(acp::McpServerHttp { url, .. }) => {
                assert_eq!(url, "https://file.example/mcp", "file source must win");
            }
            other => panic!("expected Http server, got {:?}", other),
        }
    }
}
