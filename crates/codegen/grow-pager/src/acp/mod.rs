//! ACP (Agent Communication Protocol) connection management.
//!
//! Handles spawning the agent process, initializing the protocol,
//! authenticating, and providing the channel for communication.

pub mod leader_bridge;
pub mod meta;
pub mod model_state;
pub mod spawn;
pub mod tracker;

use anyhow::Result;
use tokio_util::sync::CancellationToken;

use crate::client_identity::{HEADLESS_CLIENT_TYPE, PAGER_CLIENT_TYPE, PAGER_CLIENT_VERSION};
use agent_client_protocol as acp;
use grow_shell::agent::config::Config as AgentConfig;
use grow_shell::sampling::types::ReasoningEffort;
use xai_acp_lib::{AcpAgentTx, AcpClientRx, acp_send};

pub use model_state::ModelState;

/// Construct a `METHOD_NOT_FOUND` error for `WaitForTerminalExit`.
///
/// Both the interactive pager and headless mode reject this ACP method
/// (the adapter falls back to polling). Centralised here so the error
/// code and message format stay in sync.
pub(crate) fn wait_for_exit_not_supported(context: &str) -> acp::Error {
    acp::Error::new(
        acp::ErrorCode::MethodNotFound.into(),
        format!("{context} does not handle WaitForTerminalExit"),
    )
}

/// Result of connecting to an agent.
pub struct AcpConnection {
    /// Send requests to the agent.
    pub tx: AcpAgentTx,
    /// Receive notifications from the agent.
    pub rx: AcpClientRx,
    /// Available models and current selection.
    pub models: ModelState,
    /// Whether the agent is a grow-shell instance.
    pub is_grow_shell: bool,
    /// Cancellation token to stop the agent.
    pub cancel: CancellationToken,
    /// In-process agent worker thread (`connect` only). Join after cancel so
    /// session actors can flush SessionEnd hooks. `None` in leader mode.
    pub agent_thread: Option<std::thread::JoinHandle<anyhow::Result<()>>>,
    /// ACP-advertised slash commands parsed from `InitializeResponse.meta.availableCommands`.
    /// Seeded into every new `AgentSession` so autocomplete has shell builtins
    /// and skills immediately, before any `AvailableCommandsUpdate` arrives.
    pub available_commands: Vec<acp::AvailableCommand>,
    // NOTE: Startup announcements from InitializeResponse.meta are not yet supported.
    // Requires shell to include announcements in initialize metadata.
    // When available, add field: startup_announcements: Option<Vec<grow_announcements::Announcement>>
    /// Leader connection status. `Some` only when connected via leader.
    pub leader_status_rx: Option<tokio::sync::watch::Receiver<leader_bridge::ConnectionStatus>>,
    /// Whether cancel-rewind is enabled (resolved by shell from config layers).
    pub cancel_rewind_enabled: bool,
    /// Whether the session-recap feature is rolled out for this connection,
    /// resolved by the shell (remote settings / config / env; default OFF) and
    /// advertised in `InitializeResponse.meta.sessionRecap`. The client gates
    /// its automatic away-recap poll and the manual `/recap` on this so a
    /// disabled feature produces zero `grow/recap` traffic. Defaults to `false`
    /// when absent (e.g. an older shell that predates the feature).
    pub session_recap_available: bool,
}

/// CLI flags that affect agent configuration, threaded from PagerArgs.
#[derive(Debug, Clone, Default)]
pub struct ConnectFlags {
    pub subagents: bool,
    pub experimental_memory: bool,
    pub no_memory: bool,
    /// Session-scoped `--todo-gate` override. Forces
    /// `ReminderPolicy.todo_gate.enabled = true` for this session.
    pub todo_gate: bool,
    /// Session-scoped `--laziness-debug-log <path>` override. When set,
    /// the Layer-3 classifier fires after every turn regardless of the
    /// per-model enable gate, and the full outcome is appended to the
    /// given JSONL file. Observation-only (no nudges). Prototype/eval
    /// use only; not persisted to config.toml.
    pub laziness_debug_log: Option<std::path::PathBuf>,
    /// Client identifier for ACP Initialize metadata.
    pub client_identifier: Option<String>,
    /// Hunk tracker mode for ACP Initialize capabilities.
    pub hunk_tracker_mode: Option<String>,
    /// Terminal capability in ACP Initialize.
    pub terminal: bool,
    /// Filesystem read capability in ACP Initialize.
    pub fs_read: bool,
    /// Filesystem write capability in ACP Initialize.
    pub fs_write: bool,
    /// Installer field for config.toml.
    pub installer: Option<String>,
    /// Remote settings from early prefetch (used for memory config resolution).
    pub remote_settings: Option<grow_shell::util::config::RemoteSettings>,
    /// Override the entire system prompt.
    pub system_prompt_override: Option<String>,
    /// Extra rules appended to the system prompt (from `--rules`).
    pub rules: Option<String>,
    /// Override reasoning effort for all models.
    pub reasoning_effort_override: Option<ReasoningEffort>,
    /// CLI permission rules from --allow / --deny flags.
    /// Not supported in leader mode (agent config is set at leader startup).
    pub permission_rules: Vec<grow_workspace::permission::types::PermissionRule>,
    /// Seed agent sessions with always-approve (YOLO) permission mode.
    pub default_yolo_mode: bool,
    /// Seed agent sessions with auto (classifier) permission mode.
    /// Ignored when `default_yolo_mode` is true.
    pub default_auto_mode: bool,
}

/// Connect to an agent: spawn, initialize, authenticate.
///
/// This is the main entry point for establishing an ACP connection.
/// After this returns, the agent is ready to create sessions and receive prompts.
pub async fn connect(cancel: &CancellationToken, flags: ConnectFlags) -> Result<AcpConnection> {
    // Load agent config from disk
    let raw_config = grow_shell::config::load_effective_config()
        .map_err(|e| anyhow::anyhow!("Failed to load config: {}", e))?;
    let mut agent_config = AgentConfig::new_from_toml_cfg(&raw_config)
        .map_err(|e| anyhow::anyhow!("Failed to create agent config: {}", e))?;

    agent_config.resolve_runtime_fields(&grow_shell::agent::config::RuntimeResolutionContext {
        raw_config: &raw_config,
        remote_settings: flags.remote_settings.as_ref(),
        is_headless: false,
        cli_subagents: Some(flags.subagents),
        cli_session_summary_model: None,
        cli_experimental_memory: flags.experimental_memory,
        cli_no_memory: flags.no_memory,
        todo_gate: flags.todo_gate,
        laziness_debug_log: flags.laziness_debug_log.as_deref(),
    });

    // Permission mode seeds for every session this agent creates (CLI / config).
    agent_config.default_yolo_mode = flags.default_yolo_mode;
    agent_config.default_auto_mode = flags.default_auto_mode && !flags.default_yolo_mode;

    if let Some(effort) = flags.reasoning_effort_override {
        agent_config.reasoning_effort_override = Some(effort);
    }
    if !flags.permission_rules.is_empty() {
        agent_config.cli_agent_overrides.permission_rules = flags.permission_rules.clone();
    }

    apply_config_writes(&flags);

    // Spawn the agent
    let memory_config = agent_config.memory_config.clone();
    let spawned = spawn::spawn_grow_shell(agent_config, cancel, memory_config).await?;
    let (tx, rx) = (spawned.channel.tx, spawned.channel.rx);

    // Initialize
    let (
        models,
        is_grow_shell,
        auth_methods,
        default_auth_method_id,
        available_commands,
        cancel_rewind_enabled,
        session_recap_available,
    ) = initialize(&tx, &flags).await?;

    authenticate(&tx, &auth_methods, default_auth_method_id.as_ref()).await?;

    Ok(AcpConnection {
        tx,
        rx,
        models,
        is_grow_shell,
        cancel: spawned.cancel,
        agent_thread: Some(spawned.thread_handle),
        available_commands,
        leader_status_rx: None,
        cancel_rewind_enabled,
        session_recap_available,
    })
}

/// Connect to a leader process and return an `AcpConnection`.
///
/// The leader provides the ACP transport via IPC (raw JSON strings over a
/// Unix socket). This function bridges that transport into the same typed
/// `(AcpAgentTx, AcpClientRx)` pair that `connect()` produces, then runs
/// the standard initialize + authenticate sequence.
pub async fn connect_via_leader(
    cancel: &CancellationToken,
    flags: ConnectFlags,
    raw_config: &toml::Value,
) -> Result<AcpConnection> {
    use grow_shell::leader::{
        ClientCapabilities, ClientMode, LeaderReconnector, ReconnectPolicy, connect_or_spawn,
    };

    // These flags are baked into the agent at startup.  In leader mode the
    // agent is already running, so per-client overrides cannot be applied.
    warn_unsupported_leader_flags(&flags);

    apply_config_writes(&flags);

    let mut agent_config = AgentConfig::new_from_toml_cfg(raw_config)
        .map_err(|e| anyhow::anyhow!("Failed to create agent config: {e}"))?;
    // resolve_diagnostics_mode reads remote_settings.
    agent_config.remote_settings = flags.remote_settings.clone();

    let client_type = flags
        .client_identifier
        .as_deref()
        .unwrap_or(HEADLESS_CLIENT_TYPE);
    let capabilities = ClientCapabilities {
        // Leader agent is pre-running; seed modes via capabilities → session meta.
        yolo_mode: flags.default_yolo_mode,
        auto_mode: flags.default_auto_mode && !flags.default_yolo_mode,
        default_model: agent_config.models.default.clone(),
        client_version: Some(PAGER_CLIENT_VERSION.to_string()),
        code_nav_enabled: false,
        terminal: flags.terminal,
        fs_read: flags.fs_read,
        fs_write: flags.fs_write,
    };

    let conn = connect_or_spawn(client_type, ClientMode::Stdio, capabilities.clone()).await?;

    let (status_tx, status_rx) = LeaderReconnector::status_channel();
    let reconnector =
        LeaderReconnector::new(client_type, ClientMode::Stdio, capabilities, status_tx);
    let bridge = leader_bridge::bridge_leader_connection(
        conn,
        cancel.clone(),
        Some(reconnector),
        ReconnectPolicy::unbounded(),
    )?;
    let (tx, rx) = (bridge.channel.tx, bridge.channel.rx);

    let (
        models,
        is_grow_shell,
        auth_methods,
        default_auth_method_id,
        available_commands,
        cancel_rewind_enabled,
        session_recap_available,
    ) = initialize(&tx, &flags).await?;

    authenticate(&tx, &auth_methods, default_auth_method_id.as_ref()).await?;

    Ok(AcpConnection {
        tx,
        rx,
        models,
        is_grow_shell,
        cancel: bridge.cancel,
        agent_thread: None,
        available_commands,
        leader_status_rx: Some(status_rx),
        cancel_rewind_enabled,
        session_recap_available,
    })
}

/// Warn about flags that only take effect in direct-spawn mode.
///
/// In leader mode the agent is already running; these per-agent settings
/// cannot be changed after the fact.
fn warn_unsupported_leader_flags(flags: &ConnectFlags) {
    // eprintln rather than tracing::warn because this runs before pager
    // TUI tracing is initialised — tracing output would be silently dropped.
    for flag in unsupported_leader_flags(flags) {
        eprintln!(
            "warning: {flag} has no effect in leader mode \
             (agent config is set at leader startup)"
        );
    }
}

fn unsupported_leader_flags(flags: &ConnectFlags) -> Vec<&'static str> {
    let mut out = Vec::new();
    if flags.experimental_memory {
        out.push("--experimental-memory");
    }
    if flags.no_memory {
        out.push("--no-memory");
    }
    if flags.subagents {
        out.push("--subagents");
    }
    if !flags.permission_rules.is_empty() {
        out.push("--allow/--deny permission rules");
    }
    out
}

/// Write config.toml fields based on CLI flags.
fn apply_config_writes(flags: &ConnectFlags) {
    // Use toml_edit to preserve existing config structure
    let config_path = grow_shell::util::grow_home::grow_home().join("config.toml");
    let content = std::fs::read_to_string(&config_path).unwrap_or_default();
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .unwrap_or_default();

    let mut changed = false;

    if let Some(ref installer) = flags.installer {
        let cli = doc
            .entry("cli")
            .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
        if let Some(tbl) = cli.as_table_mut() {
            tbl["installer"] = toml_edit::value(installer.as_str());
            changed = true;
        }
    }

    if changed {
        if let Some(parent) = config_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&config_path, doc.to_string()) {
            tracing::warn!(error = %e, "failed to write config.toml");
        }
    }
}

/// Build the per-session `_meta` for `InitializeRequest` (TUI and leader).
fn build_initialize_meta(flags: &ConnectFlags) -> serde_json::Value {
    let client_type = flags
        .client_identifier
        .as_deref()
        .unwrap_or(PAGER_CLIENT_TYPE);
    let mut meta = serde_json::json!({
        "clientType": client_type,
        "clientVersion": PAGER_CLIENT_VERSION,
    });
    if let Some(spo) = &flags.system_prompt_override {
        meta["systemPromptOverride"] = serde_json::Value::String(spo.clone());
    }
    if let Some(rules) = &flags.rules {
        meta["rules"] = serde_json::Value::String(rules.clone());
    }
    meta
}

/// Build `client_capabilities.meta`. The hunk-tracker mode is canonicalized at
/// this connect read so the agent runs exactly what the settings modal displays.
fn client_capabilities_meta(flags: &ConnectFlags) -> serde_json::Value {
    let hunk_mode =
        crate::settings::canonical_hunk_tracker_mode(flags.hunk_tracker_mode.as_deref());
    serde_json::json!({
        "grow/incrementalBashOutput": true,
        "grow/hunkTracker": { "mode": hunk_mode },
        "grow/bashOutputNoColor": true,
        "grow/gitHeadChanged": true,
    })
}

/// Parse `defaultAuthMethodId` from `InitializeResponse.meta`.
///
/// The agent is the source of truth for preferred-method selection (including
/// `[auth] preferred_method`); clients must not re-derive api_key vs session.
pub fn parse_default_auth_method_id(meta: Option<&acp::Meta>) -> Option<acp::AuthMethodId> {
    meta.and_then(|m| m.get("defaultAuthMethodId"))
        .and_then(|v| v.as_str())
        .map(|s| acp::AuthMethodId::new(s.to_owned()))
}

/// Send InitializeRequest and parse the response.
async fn initialize(
    tx: &AcpAgentTx,
    flags: &ConnectFlags,
) -> Result<(
    ModelState,
    bool,
    Vec<acp::AuthMethod>,
    Option<acp::AuthMethodId>,
    Vec<acp::AvailableCommand>,
    bool,
    bool,
)> {
    let req = acp::InitializeRequest::new(acp::ProtocolVersion::V1)
        .client_capabilities(
            acp::ClientCapabilities::new()
                .fs(acp::FileSystemCapabilities::new()
                    .read_text_file(flags.fs_read)
                    .write_text_file(flags.fs_write))
                .terminal(flags.terminal)
                .meta(client_capabilities_meta(flags).as_object().cloned()),
        )
        .meta(build_initialize_meta(flags).as_object().cloned());

    let resp: acp::InitializeResponse = acp_send(req, tx).await?;

    // Check if this is a grow-shell agent
    let is_grow_shell = resp
        .meta
        .as_ref()
        .and_then(|m| m.get("growShell"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Parse model state from response meta
    let models: ModelState = resp
        .meta
        .as_ref()
        .and_then(|m| m.get("modelState"))
        .and_then(|v| serde_json::from_value::<acp::SessionModelState>(v.clone()).ok())
        .into();

    // Parse available commands from response meta (shell builtins + skills).
    // These seed the slash command registry so autocomplete works immediately.
    let available_commands = parse_available_commands(resp.meta.as_ref());

    let cancel_rewind_enabled = resp
        .meta
        .as_ref()
        .and_then(|m| m.get("cancelRewind"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let session_recap_available = parse_session_recap_available(resp.meta.as_ref());
    let default_auth_method_id = parse_default_auth_method_id(resp.meta.as_ref());

    Ok((
        models,
        is_grow_shell,
        resp.auth_methods,
        default_auth_method_id,
        available_commands,
        cancel_rewind_enabled,
        session_recap_available,
    ))
}

/// Parse `availableCommands` from an `InitializeResponse.meta` value.
///
/// Extracted as a standalone function for testability (the full `initialize()`
/// function requires an ACP connection).
pub fn parse_available_commands(meta: Option<&acp::Meta>) -> Vec<acp::AvailableCommand> {
    meta.and_then(|m| m.get("availableCommands"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Parse `sessionRecap` from `InitializeResponse.meta` (shell rollout gate).
///
/// Default `false` when missing or non-bool so older agents and dark-launch
/// defaults produce zero automatic recap traffic.
pub fn parse_session_recap_available(meta: Option<&acp::Meta>) -> bool {
    meta.and_then(|m| m.get("sessionRecap"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Authenticate with the agent using the agent's chosen default method.
///
/// Prefer `defaultAuthMethodId` from initialize meta when present and listed.
/// The BYOK-only agent advertises one method; the metadata default remains the
/// source of truth for protocol peers that advertise more than one.
async fn authenticate(
    tx: &AcpAgentTx,
    auth_methods: &[acp::AuthMethod],
    default_auth_method_id: Option<&acp::AuthMethodId>,
) -> Result<()> {
    let method_id = select_eager_auth_method(auth_methods, default_auth_method_id)
        .ok_or_else(|| anyhow::anyhow!("No auth methods available"))?;
    crate::unified_log::info(
        "pager eager auth method selected",
        None,
        Some(serde_json::json!({
            "method_id": method_id.0.as_ref(),
            "from_default_auth_method_id": default_auth_method_id
                .is_some_and(|d| d.0.as_ref() == method_id.0.as_ref()),
            "methods_count": auth_methods.len(),
            "first_method": auth_methods.first().map(|m| m.id().0.as_ref()),
        })),
    );

    let _: acp::AuthenticateResponse =
        acp_send(acp::AuthenticateRequest::new(method_id), tx).await?;
    Ok(())
}

/// Pick the method id for eager authenticate.
///
/// 1. Agent's `defaultAuthMethodId` when present in the advertised list
/// 2. First advertised method
pub fn select_eager_auth_method(
    auth_methods: &[acp::AuthMethod],
    default_auth_method_id: Option<&acp::AuthMethodId>,
) -> Option<acp::AuthMethodId> {
    if let Some(default_id) = default_auth_method_id
        && auth_methods.iter().any(|m| m.id() == default_id)
    {
        return Some(default_id.clone());
    }
    auth_methods.first().map(|m| m.id().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_available_commands_from_meta() {
        let meta = serde_json::json!({
            "availableCommands": [
                {
                    "name": "compact",
                    "description": "Compact conversation history",
                    "input": { "hint": "<focus>" }
                },
                {
                    "name": "flush",
                    "description": "Flush memory"
                }
            ]
        });
        let cmds = parse_available_commands(meta.as_object());
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[0].name, "compact");
        assert_eq!(cmds[0].description, "Compact conversation history");
        assert!(cmds[0].input.is_some());
        assert_eq!(cmds[1].name, "flush");
        assert!(cmds[1].input.is_none());
    }

    #[test]
    fn parse_available_commands_missing_key_returns_empty() {
        let meta = serde_json::json!({ "growShell": true });
        let cmds = parse_available_commands(meta.as_object());
        assert!(cmds.is_empty());
    }

    #[test]
    fn parse_available_commands_none_meta_returns_empty() {
        let cmds = parse_available_commands(None);
        assert!(cmds.is_empty());
    }

    #[test]
    fn parse_available_commands_invalid_json_returns_empty() {
        let meta = serde_json::json!({
            "availableCommands": "not-an-array"
        });
        let cmds = parse_available_commands(meta.as_object());
        assert!(cmds.is_empty());
    }

    #[test]
    fn parse_session_recap_available_true() {
        let meta = serde_json::json!({ "sessionRecap": true });
        assert!(parse_session_recap_available(meta.as_object()));
    }

    #[test]
    fn parse_session_recap_available_false_explicit() {
        let meta = serde_json::json!({ "sessionRecap": false });
        assert!(!parse_session_recap_available(meta.as_object()));
    }

    #[test]
    fn parse_session_recap_available_defaults_off_when_missing() {
        let meta = serde_json::json!({ "growShell": true, "cancelRewind": true });
        assert!(!parse_session_recap_available(meta.as_object()));
        assert!(!parse_session_recap_available(None));
    }

    #[test]
    fn parse_session_recap_available_non_bool_defaults_off() {
        let meta = serde_json::json!({ "sessionRecap": "yes" });
        assert!(!parse_session_recap_available(meta.as_object()));
    }

    fn make_auth_method(id: &str) -> acp::AuthMethod {
        acp::AuthMethod::Agent(acp::AuthMethodAgent::new(
            acp::AuthMethodId::new(id),
            id.to_string(),
        ))
    }

    #[test]
    fn eager_auth_prefers_advertised_default_then_first() {
        let methods = vec![
            make_auth_method("provider.api_key"),
            make_auth_method("custom"),
        ];
        assert_eq!(
            select_eager_auth_method(&methods, Some(&acp::AuthMethodId::new("custom")),)
                .unwrap()
                .0
                .as_ref(),
            "custom"
        );
        assert_eq!(
            select_eager_auth_method(&methods, Some(&acp::AuthMethodId::new("missing")),)
                .unwrap()
                .0
                .as_ref(),
            "provider.api_key"
        );
    }

    // ── unsupported_leader_flags ──────────────────────────────────

    #[test]
    fn unsupported_leader_flags_empty_when_none_set() {
        let flags = ConnectFlags::default();
        assert!(unsupported_leader_flags(&flags).is_empty());
    }

    #[test]
    fn unsupported_leader_flags_detects_all() {
        let flags = ConnectFlags {
            experimental_memory: true,
            no_memory: true,
            subagents: true,
            ..Default::default()
        };
        let detected = unsupported_leader_flags(&flags);
        assert_eq!(detected.len(), 3);
        assert!(detected.contains(&"--experimental-memory"));
        assert!(detected.contains(&"--no-memory"));
        assert!(detected.contains(&"--subagents"));
    }

    #[test]
    fn unsupported_leader_flags_ignores_supported() {
        let flags = ConnectFlags {
            terminal: true,
            fs_read: true,
            fs_write: true,
            ..Default::default()
        };
        assert!(unsupported_leader_flags(&flags).is_empty());
    }

    #[test]
    fn build_initialize_meta_includes_rules_when_set() {
        let flags = ConnectFlags {
            rules: Some("Always reply in French.".into()),
            ..Default::default()
        };
        let meta = build_initialize_meta(&flags);
        assert_eq!(meta["rules"], "Always reply in French.");
    }

    #[test]
    fn build_initialize_meta_omits_rules_when_unset() {
        let flags = ConnectFlags::default();
        let meta = build_initialize_meta(&flags);
        assert!(
            meta.get("rules").is_none(),
            "rules key must be absent when --rules is not set; meta={meta:?}"
        );
    }

    #[test]
    fn build_initialize_meta_carries_system_prompt_override() {
        let flags = ConnectFlags {
            system_prompt_override: Some("YOU ARE A PIRATE.".into()),
            ..Default::default()
        };
        let meta = build_initialize_meta(&flags);
        assert_eq!(meta["systemPromptOverride"], "YOU ARE A PIRATE.");
    }

    #[test]
    fn build_initialize_meta_uses_custom_client_identifier_when_set() {
        let flags = ConnectFlags {
            client_identifier: Some("zed".into()),
            ..Default::default()
        };
        let meta = build_initialize_meta(&flags);
        assert_eq!(meta["clientType"], "zed");
    }

    #[test]
    fn client_capabilities_meta_defaults_absent_or_blank_mode_to_agent_only() {
        // Rows 1 & 2 of the truth table: nothing set, and a set-but-blank value,
        // both advertise the `agent_only` default (never `""` → AllDirty).
        let absent = client_capabilities_meta(&ConnectFlags::default());
        assert_eq!(absent["grow/hunkTracker"]["mode"], "agent_only");
        let blank = client_capabilities_meta(&ConnectFlags {
            hunk_tracker_mode: Some("   ".into()),
            ..Default::default()
        });
        assert_eq!(blank["grow/hunkTracker"]["mode"], "agent_only");
    }

    #[test]
    fn client_capabilities_meta_canonicalizes_off_and_mixed_case() {
        // Mixed-case / alias values are canonicalized so the agent runtime
        // matches the modal display.
        for raw in ["off", "OFF", "Disabled"] {
            let meta = client_capabilities_meta(&ConnectFlags {
                hunk_tracker_mode: Some(raw.into()),
                ..Default::default()
            });
            assert_eq!(meta["grow/hunkTracker"]["mode"], "off", "raw={raw}");
        }
    }
}
