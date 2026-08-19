//! MCP server data types, status enum, response conversion, and section
//! presentation helpers.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpSectionId {
    Plugin(String),
    Local,
}

impl PartialOrd for McpSectionId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for McpSectionId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self, other) {
            (Self::Plugin(a), Self::Plugin(b)) => a.cmp(b),
            (Self::Plugin(_), Self::Local) => Ordering::Less,
            (Self::Local, Self::Plugin(_)) => Ordering::Greater,
            (Self::Local, Self::Local) => Ordering::Equal,
        }
    }
}

/// Collapse/expand key for a section header row in the MCP servers tab.
pub fn section_key(section: &McpSectionId) -> String {
    match section {
        McpSectionId::Plugin(name) => format!("mcp-section:plugin:{name}"),
        McpSectionId::Local => "mcp-section:local".into(),
    }
}

/// Display label for a managed-service section header.
pub fn section_label(section: &McpSectionId, count: usize) -> String {
    match section {
        McpSectionId::Plugin(name) => format!("Plugin: {name} ({count})"),
        McpSectionId::Local => format!("Local ({count})"),
    }
}

pub fn section_description_lines(section: &McpSectionId) -> Vec<String> {
    match section {
        McpSectionId::Plugin(_) | McpSectionId::Local => Vec::new(),
    }
}

/// Classify a server into a UI section.
pub fn section_for(server: &McpServerInfo) -> McpSectionId {
    if let Some(ref name) = server.plugin_name {
        McpSectionId::Plugin(name.clone())
    } else {
        McpSectionId::Local
    }
}

/// Whether the user may delete this server from local config.
pub fn is_removable(server: &McpServerInfo) -> bool {
    let _ = server;
    true
}

fn parse_plugin_name(source_label: &str) -> Option<String> {
    let rest = source_label.strip_prefix("plugin:")?.trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpsListResponse {
    pub servers: Vec<McpsServerEntry>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpsServerEntry {
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub source_label: Option<String>,
    #[serde(default, rename = "type")]
    pub config_type: Option<String>,
    #[serde(default)]
    pub setup: Option<McpSetupConfig>,
    #[serde(default)]
    pub setup_values: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    pub session: Option<McpsServerSession>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpsServerSession {
    pub enabled: bool,
    pub status: Option<String>,
    #[serde(default)]
    pub tools: Vec<serde_json::Value>,
    #[serde(default)]
    pub setup_required: bool,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct McpSetupConfig {
    #[serde(default)]
    pub fields: Vec<McpSetupField>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct McpSetupField {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub options: Vec<McpSetupOption>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct McpSetupOption {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct McpToolDetail {
    pub name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct McpServerInfo {
    pub name: String,
    pub display_name: Option<String>,
    pub status: McpServerDisplayStatus,
    pub tool_count: usize,
    pub setup_required: bool,
    pub setup: Option<McpSetupConfig>,
    pub setup_values: std::collections::HashMap<String, String>,
    /// Detailed tool list for expanded view.
    pub tools: Vec<McpToolDetail>,
    /// Whether the server is enabled in config.
    pub enabled: bool,
    /// Display label from `source_label` (e.g. `"plugin: foo"`).
    pub source: String,
    /// Plugin name parsed from `source_label` (`"plugin: …"`).
    pub plugin_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum McpServerDisplayStatus {
    Ready,
    SetupRequired,
    Unavailable,
    Initializing,
}

impl McpServerDisplayStatus {
    /// Theme-aware status color for badge rendering.
    pub(crate) fn theme_color(&self, theme: &crate::theme::Theme) -> ratatui::style::Color {
        match self {
            Self::Ready => theme.accent_success,
            Self::SetupRequired => theme.warning,
            Self::Unavailable => theme.accent_error,
            Self::Initializing => theme.running,
        }
    }

    /// Short human label for the status.
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::SetupRequired => "setup required",
            Self::Unavailable => "unavailable",
            Self::Initializing => "initializing",
        }
    }
}

pub fn convert_list_response(resp: McpsListResponse) -> Vec<McpServerInfo> {
    let mut servers: Vec<McpServerInfo> = resp
        .servers
        .into_iter()
        .map(|entry| {
            let (status, tool_count, tools, enabled) = if let Some(session) = &entry.session {
                let enabled = session.enabled;
                if session.setup_required {
                    (McpServerDisplayStatus::SetupRequired, 0, vec![], enabled)
                } else if !enabled {
                    (McpServerDisplayStatus::Unavailable, 0, vec![], false)
                } else {
                    let st = match session.status.as_deref() {
                        Some("ready") => McpServerDisplayStatus::Ready,
                        Some("initializing") => McpServerDisplayStatus::Initializing,
                        _ => McpServerDisplayStatus::Unavailable,
                    };
                    let tools: Vec<McpToolDetail> = session
                        .tools
                        .iter()
                        .map(|t| McpToolDetail {
                            name: t
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            display_name: t
                                .get("displayName")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            description: t
                                .get("description")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            enabled: t.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
                        })
                        .collect();
                    let tc = tools.len();
                    (st, tc, tools, enabled)
                }
            } else {
                (McpServerDisplayStatus::Unavailable, 0, vec![], false)
            };
            let plugin_name = entry.source_label.as_deref().and_then(parse_plugin_name);
            let source = entry.source_label.unwrap_or_else(|| "local".to_string());
            let setup_required = entry
                .session
                .as_ref()
                .is_some_and(|session| session.setup_required)
                || matches!(status, McpServerDisplayStatus::SetupRequired);
            McpServerInfo {
                name: entry.name,
                display_name: entry.display_name,
                status,
                tool_count,
                setup_required,
                setup: entry.setup,
                setup_values: entry.setup_values.unwrap_or_default(),
                tools,
                enabled,
                source,
                plugin_name,
            }
        })
        .collect::<Vec<_>>();

    // Stable sort: plugins before local entries, then alphabetical by name.
    servers.sort_by(|a, b| {
        let source_rank = |s: &McpServerInfo| match section_for(s) {
            McpSectionId::Plugin(_) => 0,
            McpSectionId::Local => 1,
        };
        source_rank(a)
            .cmp(&source_rank(b))
            .then_with(|| {
                a.display_name
                    .as_deref()
                    .unwrap_or(&a.name)
                    .cmp(b.display_name.as_deref().unwrap_or(&b.name))
            })
            .then_with(|| a.name.cmp(&b.name))
    });

    servers
}

/// Patch a single server row in-place from an `grow/mcp/server_status`
/// push.
///
/// Finds the row by `name` and updates its `status` (and optionally its
/// `tools` list + `tool_count`). When the named server is not present
/// the call is a silent no-op — the pager may receive a status push
/// for a server it has not yet fetched (e.g. the modal was just opened
/// and the cached `mcp/list` response has not landed yet). The cheap
/// no-op keeps the push subscription side-effect-free in that case.
///
/// When duplicate names exist, only the first occurrence is mutated.
/// In practice `build_mcp_catalog` deduplicates by name before the
/// list reaches the pager, so this is dead-code in production.
///
/// Returns `true` when a row was actually mutated; the caller can use
/// this signal to decide whether a redraw is warranted.
pub fn patch_server_row(
    servers: &mut [McpServerInfo],
    name: &str,
    new_status: McpServerDisplayStatus,
    new_tools: Option<Vec<McpToolDetail>>,
) -> bool {
    let Some(row) = servers.iter_mut().find(|s| s.name == name) else {
        return false;
    };
    row.status = new_status;
    if let Some(tools) = new_tools {
        row.tool_count = tools.len();
        row.tools = tools;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_row(name: &str, status: McpServerDisplayStatus) -> McpServerInfo {
        McpServerInfo {
            name: name.to_string(),
            display_name: None,
            status,
            tool_count: 0,
            setup_required: false,
            setup: None,
            setup_values: std::collections::HashMap::new(),
            tools: Vec::new(),
            enabled: true,
            source: "local".to_string(),
            plugin_name: None,
        }
    }

    fn server_from_wire(name: &str, source_label: Option<&str>) -> McpServerInfo {
        convert_list_response(McpsListResponse {
            servers: vec![McpsServerEntry {
                name: name.to_string(),
                display_name: None,
                source_label: source_label.map(str::to_string),
                config_type: None,
                setup: None,
                setup_values: None,
                session: Some(McpsServerSession {
                    enabled: true,
                    status: Some("ready".into()),
                    tools: vec![],
                    setup_required: false,
                }),
            }],
        })
        .into_iter()
        .next()
        .unwrap()
    }

    #[test]
    fn section_description_lines_local_is_empty() {
        assert!(section_description_lines(&McpSectionId::Local).is_empty());
    }

    #[test]
    fn section_for_plugin_labeled_local_is_plugin_section() {
        let server = server_from_wire("my-mcp", Some("plugin: linter"));
        assert_eq!(
            section_for(&server),
            McpSectionId::Plugin("linter".to_string())
        );
    }

    #[test]
    fn is_removable_plugin_labeled_local_server() {
        let server = server_from_wire("my-mcp", Some("plugin: linter"));
        assert!(is_removable(&server));
    }

    #[test]
    fn convert_list_response_parses_plugin_name() {
        let server = server_from_wire("srv", Some("plugin: example"));
        assert_eq!(server.plugin_name.as_deref(), Some("example"));
        assert_eq!(server.source, "plugin: example");
    }

    #[test]
    fn convert_list_response_setup_required_takes_priority() {
        let servers = convert_list_response(McpsListResponse {
            servers: vec![McpsServerEntry {
                name: "acme".into(),
                display_name: None,
                source_label: Some("plugin: acme".into()),
                config_type: Some("http".into()),
                setup: Some(McpSetupConfig {
                    fields: vec![McpSetupField {
                        id: "site".into(),
                        label: "Site".into(),
                        field_type: "select".into(),
                        required: true,
                        default: Some("us1".into()),
                        options: vec![McpSetupOption {
                            label: "US1".into(),
                            value: "us1".into(),
                        }],
                    }],
                }),
                setup_values: None,
                session: Some(McpsServerSession {
                    enabled: true,
                    status: Some("setuprequired".into()),
                    tools: vec![],
                    setup_required: true,
                }),
            }],
        });
        assert_eq!(servers.len(), 1);
        assert!(servers[0].setup_required);
        assert_eq!(servers[0].status, McpServerDisplayStatus::SetupRequired);
        assert!(servers[0].setup.is_some());
    }

    #[test]
    fn patch_server_row_updates_existing() {
        let mut servers = vec![
            make_row("alpha", McpServerDisplayStatus::Initializing),
            make_row("beta", McpServerDisplayStatus::Initializing),
        ];
        let new_tools = vec![
            McpToolDetail {
                name: "t1".into(),
                display_name: None,
                description: None,
                enabled: true,
            },
            McpToolDetail {
                name: "t2".into(),
                display_name: None,
                description: Some("two".into()),
                enabled: true,
            },
        ];
        let mutated = patch_server_row(
            &mut servers,
            "beta",
            McpServerDisplayStatus::Ready,
            Some(new_tools),
        );
        assert!(mutated, "named row must be reported as mutated");
        assert_eq!(servers[0].status, McpServerDisplayStatus::Initializing);
        assert_eq!(servers[1].status, McpServerDisplayStatus::Ready);
        assert_eq!(servers[1].tool_count, 2);
        assert_eq!(servers[1].tools.len(), 2);
        assert_eq!(servers[1].tools[0].name, "t1");
    }

    #[test]
    fn patch_server_row_noop_when_absent() {
        let mut servers = vec![make_row("alpha", McpServerDisplayStatus::Ready)];
        let mutated = patch_server_row(
            &mut servers,
            "ghost",
            McpServerDisplayStatus::Unavailable,
            None,
        );
        assert!(!mutated, "missing-name push must be a silent no-op");
        // Existing row must be untouched.
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "alpha");
        assert_eq!(servers[0].status, McpServerDisplayStatus::Ready);
    }

    #[test]
    fn patch_server_row_status_only_keeps_tools() {
        let mut servers = vec![McpServerInfo {
            name: "alpha".into(),
            display_name: None,
            status: McpServerDisplayStatus::Ready,
            tool_count: 3,
            setup_required: false,
            setup: None,
            setup_values: std::collections::HashMap::new(),
            tools: vec![McpToolDetail {
                name: "existing".into(),
                display_name: None,
                description: None,
                enabled: true,
            }],
            enabled: true,
            source: "local".into(),
            plugin_name: None,
        }];
        let mutated = patch_server_row(
            &mut servers,
            "alpha",
            McpServerDisplayStatus::Unavailable,
            None,
        );
        assert!(mutated);
        assert_eq!(servers[0].status, McpServerDisplayStatus::Unavailable);
        // Tools left untouched when caller passes None.
        assert_eq!(servers[0].tool_count, 3);
        assert_eq!(servers[0].tools.len(), 1);
        assert_eq!(servers[0].tools[0].name, "existing");
    }
}
