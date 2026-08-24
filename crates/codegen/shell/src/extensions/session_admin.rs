//! Session-administration extension handlers.
//!
//! Methods grouped here are operational/admin endpoints that mutate
//! persistent or shared agent state but are not part of the per-turn prompt
//! lifecycle:
//!
//! - `grow/session/rename`                  rename a local session
//! - `grow/session/delete`                  delete a local session
//! - `grow/session/update_mcp_servers`      mid-session MCP server swap
//! - `grow/session/fork`                    fork a session into a new one
//! - `grow/internal/reload_mcp_catalog`     config hot-reload, explicitly scoped
//! - `grow/internal/reload_skills`          skills file watcher fan-out
//! - `grow/internal/reload_models`          model list hot-reload from config.toml
//! - `grow/internal/reload_announcements`   local announcement hot-reload
//! - `grow/plugins/reload`                  rebuild shared plugin registry
//! - `grow/commands/list`                   list slash commands
//! - `grow/commands/execute`                execute a slash command out of band

use std::path::Path;
use std::sync::Arc;

use agent_client_protocol as acp;
use agent_client_protocol::Client as _;
use serde::Deserialize;

use super::{ExtResult, parse_params, to_raw_response};
use crate::agent::MvpAgent;
use crate::session::persistence::list_summaries;
use crate::session::storage::jsonl::JsonlStorageAdapter;
use crate::session::{ExtMethodResult, SessionCommand};

#[tracing::instrument(skip_all, fields(method = %args.method))]
pub async fn handle(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    match args.method.as_ref() {
        "grow/session/rename" => handle_session_rename(agent, args).await,
        "grow/session/delete" => handle_session_delete(agent, args).await,
        "grow/session/update_mcp_servers" => handle_update_mcp_servers(agent, args).await,
        "grow/session/fork" => handle_session_fork(agent, args).await,
        "grow/internal/reload_mcp_catalog" => handle_reload_mcp_catalog(agent, args).await,
        "grow/internal/reload_skills" => handle_reload_skills(agent),
        "grow/internal/reload_workflows" => handle_reload_workflows(agent),
        "grow/internal/reload_models" => handle_reload_models(agent).await,
        "grow/internal/reload_announcements" => handle_reload_announcements(agent, args),
        "grow/plugins/reload" => handle_plugins_reload(agent).await,
        "grow/commands/list" => handle_commands_list(agent, args).await,
        "grow/commands/execute" => handle_command_execute(agent, args).await,
        "grow/queue/prompt_status" => handle_prompt_status(agent, args).await,
        _ => Err(acp::Error::method_not_found()),
    }
}

async fn handle_prompt_status(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct PromptStatusRequest {
        session_id: String,
        prompt_id: String,
    }

    let request: PromptStatusRequest = parse_params(args)?;
    let session_id = acp::SessionId::new(Arc::from(request.session_id.as_str()));
    let Some(handle) = agent.session_handle_waiting_for_load(&session_id).await else {
        return Err(acp::Error::invalid_request()
            .data(format!("unknown session id: {}", request.session_id)));
    };
    let (respond_to, response) = tokio::sync::oneshot::channel();
    handle
        .cmd_tx
        .send(SessionCommand::QueryPromptStatus {
            prompt_id: request.prompt_id,
            respond_to,
        })
        .map_err(|_| acp::Error::internal_error().data("session actor unavailable"))?;
    let status = response
        .await
        .map_err(|_| acp::Error::internal_error().data("session actor dropped prompt status"))?;
    to_raw_response(&status)
}

fn handle_reload_announcements(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    struct ReloadAnnouncements {
        announcements: Vec<announcements::Announcement>,
    }

    let request: ReloadAnnouncements = parse_params(args)?;
    let count = request.announcements.len();
    agent.cfg.borrow_mut().announcements = request.announcements;
    agent.emit_announcements();
    ExtMethodResult::success(serde_json::json!({ "announcements": count }))
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

// session/rename

/// Handles renaming a session.
async fn handle_session_rename(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RenameRequest {
        session_id: String,
        title: String,
        #[serde(default)]
        cwd: Option<String>,
    }

    let mut req: RenameRequest = parse_params(args)?;
    // Timeline title validation is repeated at this protocol boundary so the
    // caller receives a precise request error before any storage work begins.
    req.title = req.title.trim().to_string();
    if req.title.is_empty() {
        return Err(acp::Error::invalid_request().data("title must not be blank"));
    }
    if req.title.chars().count() > 160 {
        return Err(acp::Error::invalid_request().data("title must be at most 160 characters"));
    }

    let session_id = acp::SessionId::new(Arc::from(req.session_id.as_str()));

    // Find the session info, scoping to cwd if provided
    let summaries = list_summaries(req.cwd.as_deref())
        .await
        .map_err(|e| acp::Error::internal_error().data(format!("failed to list sessions: {e}")))?;

    let summary = summaries
        .iter()
        .find(|s| s.info.id == session_id)
        .ok_or_else(|| {
            acp::Error::invalid_request().data(format!("session not found: {}", req.session_id))
        })?;

    let info = summary.info.clone();

    let live_handle = agent.session_handle_waiting_for_load(&session_id).await;
    let event = if let Some(handle) = live_handle {
        let (respond_to, response) = tokio::sync::oneshot::channel();
        handle
            .cmd_tx
            .send(crate::session::commands::SessionCommand::SetSessionTitle {
                title: req.title.clone(),
                respond_to,
            })
            .map_err(|_| acp::Error::internal_error().data("session actor unavailable"))?;
        response
            .await
            .map_err(|_| {
                acp::Error::internal_error().data("session actor dropped title acknowledgement")
            })?
            .map_err(|error| {
                acp::Error::internal_error()
                    .data(format!("failed to commit session/title event: {error}"))
            })?
    } else {
        let storage = JsonlStorageAdapter::default();
        let event = storage
            .append_session_title_durable(&info, req.title.clone())
            .await
            .map_err(|error| {
                acp::Error::internal_error()
                    .data(format!("failed to commit session/title event: {error}"))
            })?;
        event
    };

    // Update session search index with new title
    crate::session::storage::search::notify_session_updated(&info.id.to_string(), &info.cwd);

    tracing::info!(session_id = %req.session_id, title = %req.title, "Session renamed");

    to_raw_response(&serde_json::json!({
        "success": true,
        "title": req.title,
        "eventSeq": event.seq.get(),
    }))
}

// session/delete

/// Delete a session from history.
async fn handle_session_delete(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct DeleteRequest {
        session_id: String,
        #[serde(default)]
        cwd: Option<String>,
    }

    let req: DeleteRequest = parse_params(args)?;

    let session_id = acp::SessionId::new(Arc::from(req.session_id.as_str()));

    // Tear down any live actor first (cancel turn/subagents/bg tasks,
    // process-scope kill, flush). Then wipe history so shutdown cannot
    // rewrite the session directory after delete.
    if agent.sessions.borrow().contains_key(&session_id) {
        agent.teardown_live_session_before_delete(&session_id).await;
    }

    // Local delete: disk + FTS eviction.
    // Mirrored by the `grow sessions delete <id>` CLI path.
    crate::session::persistence::delete_session_history(&req.session_id, req.cwd.as_deref())
        .await
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))?;

    tracing::info!(session_id = %req.session_id, "Session deleted");

    to_raw_response(&serde_json::json!({ "success": true }))
}

// session/update_mcp_servers

async fn handle_update_mcp_servers(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Params {
        session_id: acp::SessionId,
        mcp_servers: Vec<acp::McpServer>,
    }

    let params: Params = parse_params(args)?;

    let (handle, cwd) = {
        let sessions = agent.sessions.borrow();
        let h = sessions
            .get(&params.session_id)
            .cloned()
            .ok_or_else(|| acp::Error::invalid_params().data("unknown session id"))?;
        let cwd = std::path::PathBuf::from(&h.info.cwd);
        (h, cwd)
    };

    let merged = crate::session::mcp_catalog::merge_mcp_servers(
        params.mcp_servers.clone(),
        &cwd,
        agent.plugin_registry_handle().snapshot().as_deref(),
    );

    let (tx, rx) = tokio::sync::oneshot::channel();
    handle
        .cmd_tx
        .send(SessionCommand::UpdateMcpServers {
            mcp_servers: merged,
            respond_to: tx,
        })
        .map_err(|_| acp::Error::internal_error().data("session closed"))?;

    // Wait for the session actor to finish MCP re-initialization.
    rx.await
        .map_err(|_| acp::Error::internal_error().data("session closed"))?
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))?;

    // Persist the new client set on the handle so config hot-reloads
    // (`reload_mcp_catalog`) re-merges from
    // the client's latest intent rather than the `session/new` snapshot —
    // otherwise a reload would resurrect servers the client just removed
    // (or drop ones it just added).
    if let Some(h) = agent.sessions.borrow_mut().get_mut(&params.session_id) {
        h.initial_client_mcp_servers = params.mcp_servers;
    }

    ExtMethodResult::success(serde_json::json!({ "ok": true }))
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

// internal/reload_skills

/// Reload skills for ALL active sessions. Called by the skills file watcher
fn handle_reload_skills(agent: &MvpAgent) -> ExtResult {
    let reloaded = agent.reload_skills_all_sessions();
    ExtMethodResult::success(serde_json::json!({ "reloaded": reloaded }))
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

fn handle_reload_workflows(agent: &MvpAgent) -> ExtResult {
    let reloaded = agent.advertise_commands_all_sessions();
    ExtMethodResult::success(serde_json::json!({ "reloaded": reloaded }))
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

// internal/reload_mcp_catalog

/// Reload the canonical MCP catalog. `project_root = None` targets every
/// active session; `Some(root)` targets sessions at or beneath that root.
async fn handle_reload_mcp_catalog(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Params {
        project_root: serde_json::Value,
    }

    let params: Params = parse_params(args)?;
    let project_root = match params.project_root {
        serde_json::Value::Null => None,
        serde_json::Value::String(root) => Some(std::path::PathBuf::from(root)),
        _ => {
            return Err(acp::Error::invalid_params().data("projectRoot must be a string or null"));
        }
    };

    // Collect (session_id, cwd) pairs once so we don't hold the
    // `sessions` RefCell borrow across `.await` points.
    let session_ids: Vec<(acp::SessionId, std::path::PathBuf)> = agent
        .sessions
        .borrow()
        .iter()
        .map(|(sid, h)| (sid.clone(), std::path::PathBuf::from(&h.info.cwd)))
        .filter(|(_, cwd)| {
            project_root
                .as_deref()
                .is_none_or(|root| cwd_matches(cwd, root))
        })
        .collect();

    if session_ids.is_empty() {
        return ExtMethodResult::success(serde_json::json!({ "updated": 0 }))
            .to_ext_response()
            .map_err(|e| acp::Error::internal_error().data(e.to_string()));
    }

    let mut updated = 0u32;
    for (session_id, cwd) in &session_ids {
        let Some(handle) = agent.sessions.borrow().get(session_id).cloned() else {
            continue;
        };
        if crate::session::mcp_catalog::merge_and_send_mcp_update(
            &handle.cmd_tx,
            cwd,
            handle.initial_client_mcp_servers.clone(),
            agent.plugin_registry_handle().snapshot().as_deref(),
        ) {
            updated += 1;
        }
    }

    tracing::info!(
        updated,
        total = session_ids.len(),
        project_root = ?project_root,
        "reloaded MCP catalog for matching sessions"
    );
    ExtMethodResult::success(serde_json::json!({ "updated": updated }))
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

/// Returns `true` iff `session_cwd` equals `target_cwd` or sits
/// beneath it (so a `<repo>/` edit reloads `<repo>/subdir/` sessions
/// too).
///
/// This uses `Path::starts_with`, which is
/// **component-aware** — `/repo-test` does NOT match `/repo` even
/// though the byte prefix matches. That is the desired behavior
/// (component-aware avoids the `/foo-bar` ⊂ `/foo` foot-gun). Paths
/// come from `SessionInfo::cwd` (always absolute) and the watcher's
/// emitted path (also absolute), so no canonicalization is needed
/// here. The `==` short-circuit is redundant (`Path::starts_with` is
/// reflexive) but kept for an explicit zero-allocation fast path.
fn cwd_matches(session_cwd: &std::path::Path, target_cwd: &std::path::Path) -> bool {
    session_cwd == target_cwd || session_cwd.starts_with(target_cwd)
}

// internal/reload_models

/// Re-resolve the agent model list from config.toml. Called by the config
/// hot-reload watcher when `[provider.*]` or `[models]` changes.
///
/// Re-reads config from disk, re-runs the same resolution logic as
/// `new_with_models()` for user TOML config entries, and swaps the model list
/// in-place. Prefetched (API) and default models are NOT re-fetched -- only
/// BYOK entries from config are updated.
async fn handle_reload_models(agent: &MvpAgent) -> ExtResult {
    let disk_config = crate::config::load_effective_config()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))?;

    let toml_config = crate::agent::config::Config::new_from_toml_cfg(&disk_config)
        .map_err(|e| acp::Error::internal_error().data(e))?;

    // Build the complete candidate off to the side. Runtime-only fields
    // (remote settings, endpoints, CLI flags) survive, while no live reader is
    // changed until the catalog validates as one atomic snapshot.
    let mut merged_config = agent.cfg.borrow().clone();
    let overrides = crate::config::ModelOverrideConfig::resolve(
        merged_config.session_title_model_override.as_deref(),
        &disk_config,
        merged_config.remote_settings.as_ref(),
    );
    merged_config.models = toml_config.models;
    merged_config.config_models = toml_config.config_models;
    merged_config.auth_providers = toml_config.auth_providers;
    merged_config.config_warnings = toml_config.config_warnings;
    merged_config.session_title_model = overrides.session_title;
    merged_config.image_description_model = overrides.image_description;
    merged_config.prompt_suggest_model_pin = overrides.prompt_suggestion;
    crate::util::config::sync_campaign_fields(&mut merged_config);

    agent
        .models_manager
        .apply_config(merged_config.clone())
        .map_err(|error| acp::Error::invalid_request().data(error))?;
    *agent.cfg.borrow_mut() = merged_config.clone();

    // Existing sessions adopt the same validated provider snapshot at their
    // actor mailbox boundary. A removed selection falls back to the newly
    // resolved default; explicit per-session reasoning effort remains intact.
    let fallback_model = agent.models_manager.current_model_id();
    let live_catalog = agent.models_manager.models();
    let (reloads, mut reload_error) = {
        let mut sessions = agent.sessions.borrow_mut();
        let mut reloads = Vec::with_capacity(sessions.len());
        let mut reload_error = None;
        for (session_id, handle) in sessions.iter_mut() {
            let selected = if agent
                .models_manager
                .model_in_catalog(handle.model_id.0.as_ref())
            {
                handle.model_id.clone()
            } else {
                fallback_model.clone()
            };
            let Some(mut sampling_config) = agent
                .models_manager
                .sampling_config_for_model(selected.0.as_ref())
            else {
                continue;
            };
            let inference_idle_timeout = {
                let per_model = crate::agent::config::find_model_by_catalog_id(
                    &live_catalog,
                    selected.0.as_ref(),
                )
                .and_then(|entry| entry.info.inference_idle_timeout_secs);
                let remote = merged_config
                    .remote_settings
                    .as_ref()
                    .and_then(|settings| settings.inference_idle_timeout_secs);
                std::time::Duration::from_secs(per_model.or(remote).unwrap_or(600).max(10))
            };
            let max_retries = sampler::resolve_max_retries(sampling_config.max_retries);
            let auto_compact_threshold_percent =
                crate::util::config::resolve_auto_compact_threshold_percent(
                    &merged_config,
                    selected.0.as_ref(),
                    crate::agent::config::find_model_by_catalog_id(
                        &live_catalog,
                        selected.0.as_ref(),
                    )
                    .map(|entry| &entry.info),
                );
            if handle.reasoning_effort.is_some() {
                sampling_config.reasoning_effort = handle.reasoning_effort;
            }
            let (responds_to, response) = tokio::sync::oneshot::channel();
            let command = crate::session::commands::SessionCommand::ReloadModelConfig {
                model_id: selected.clone(),
                sampling_config,
                image_description_model: merged_config.image_description_model.clone(),
                inference_idle_timeout,
                max_retries,
                auto_compact_threshold_percent,
                responds_to,
            };
            if handle.cmd_tx.send(command).is_err() {
                reload_error.get_or_insert_with(|| {
                    acp::Error::internal_error().data(format!(
                        "session {} rejected the catalog reload command",
                        session_id.0
                    ))
                });
            } else {
                reloads.push((session_id.clone(), selected, response));
            }
        }
        (reloads, reload_error)
    };
    for (session_id, selected, response) in reloads {
        let result = response.await.unwrap_or_else(|_| {
            Err(acp::Error::internal_error().data(format!(
                "session {} dropped the catalog reload acknowledgement",
                session_id.0
            )))
        });
        match result {
            Ok(()) => {
                if let Some(handle) = agent.sessions.borrow_mut().get_mut(&session_id) {
                    handle.model_id = selected;
                }
            }
            Err(error) => {
                reload_error.get_or_insert(error);
            }
        }
    }
    if let Some(error) = reload_error {
        return Err(error);
    }
    let count = agent.models_manager.models().len();
    tracing::info!(count, "model list reloaded from config.toml");
    ExtMethodResult::success(serde_json::json!({ "models": count }))
        .to_ext_response()
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))
}

// plugins/reload

async fn handle_plugins_reload(agent: &MvpAgent) -> ExtResult {
    // Rebuild the shared registry so future/new sessions clone the latest.
    let session_cwd = agent
        .sessions
        .borrow()
        .values()
        .next()
        .map(|h| std::path::PathBuf::from(&h.info.cwd));
    let mut plugins = agent.cfg.borrow().plugins.clone();
    let disk_cfg = plugins.to_discovery_config();
    // Folder-trust gates repo-local project plugins (hooks/MCP). Resolve and
    // record the verdict for this cwd (honoring the real remote), then gate
    // plugins on it.
    let project_trusted = session_cwd.as_deref().is_some_and(|c| {
        let remote_settings = agent.cfg.borrow().remote_settings.clone();
        crate::agent::folder_trust::resolve_and_record(c, remote_settings.as_ref(), false)
    });
    // Explicit ACP `grow/plugins/reload`: force a full local-install re-copy.
    agent
        .plugin_registry_handle()
        .reload(session_cwd.as_deref(), &disk_cfg, project_trusted, true);

    // Eagerly fan out the new registry to every live session: each adopts a
    // cwd-correct snapshot (hooks + MCP + skills + client slash-command
    // catalog), the same refresh the originating session of a reload gets.
    agent.broadcast_plugin_registry_to_sessions(None);

    super::to_ext_response(Ok(serde_json::json!({"ok": true})))
}

// commands/list

async fn handle_command_execute(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ExecuteCommandRequest {
        session_id: String,
        command: String,
    }

    let request: ExecuteCommandRequest = parse_params(args)?;
    let session_id = acp::SessionId::new(Arc::from(request.session_id.as_str()));
    let Some(handle) = agent.session_handle_waiting_for_load(&session_id).await else {
        return Err(acp::Error::invalid_request()
            .data(format!("unknown session id: {}", request.session_id)));
    };
    handle
        .execute_slash_command(request.command)
        .await
        .map_err(|message| acp::Error::invalid_request().data(message))?;
    to_raw_response(&serde_json::json!({ "status": "executed" }))
}

async fn handle_commands_list(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    let req: crate::session::slash_commands::ListCommandsRequest = parse_params(args)?;

    let availability = agent.command_availability();
    let skills_config = agent.cfg.borrow().skills.clone();

    // Chat product catalog never uses cwd plugin discovery / folder-trust.
    // Prefer kind=chat even when sessionId is set so the pull matches ACU.
    if req.kind.as_deref() == Some("chat") {
        let response = crate::session::slash_commands::list_commands(
            None,
            &skills_config,
            None,
            availability,
            false,
            Some("chat"),
        )
        .await?;
        return Ok(acp::ExtResponse::new(Arc::from(
            serde_json::value::to_raw_value(&response)?,
        )));
    }

    if let Some(session_id) = req.session_id.as_ref() {
        let Some(handle) = agent.session_handle_waiting_for_load(session_id).await else {
            return Err(
                acp::Error::invalid_request().data(format!("unknown session id: {}", session_id.0))
            );
        };
        let response = handle.list_available_commands().await;
        return Ok(acp::ExtResponse::new(Arc::from(
            serde_json::value::to_raw_value(&response)?,
        )));
    }

    // For a given cwd, compute the plugin registry the same way a session would
    // at spawn time (via build_for_cwd) and the same way reload_plugins_impl does
    // (ancestor project config walk). This is required so
    // that `grow/commands/list` (the pull used by embedding clients after session
    // start) returns plugin-provided slash commands for the target cwd.
    //
    // The shared snapshot is only populated at agent boot (using process CWD)
    // and by explicit reloads. In client<->container (and SSH) setups the agent's
    // launch CWD is unrelated to the user's chosen workspace dir, so relying on
    // snapshot() alone meant the post-start pull returned no project plugin
    // skills until the user manually reloaded.
    let plugin_reg = if let Some(cwd_str) = &req.cwd {
        let cwd = Path::new(cwd_str);

        // Folder-trust gates repo-local project plugins (hooks/MCP). Resolve and
        // record the verdict for this cwd (honoring the real remote) BEFORE the
        // plugins-config read below: that read gates its project-paths merge on
        // the recorded verdict, and a cold cwd (client-supplied, no session
        // resolve yet) must not first take the gate's remote-less backstop —
        // that would record a kill-switch-blind deny no later resolve can lift.
        let remote_settings = agent.cfg.borrow().remote_settings.clone();
        let project_trusted =
            crate::agent::folder_trust::resolve_and_record(cwd, remote_settings.as_ref(), false);

        // Effective [plugins] config (global + ancestor project configs),
        // shared with reload_plugins_impl and the eager
        // fan-out so the menu agrees with each session's registry for this cwd.
        let disk_cfg = crate::config::resolve_effective_plugins_config(cwd).to_discovery_config();

        // Fresh discovery for *this* cwd (includes .grow/plugins under it, plus
        // the cli --plugin-dir dirs). Does not mutate the shared snapshot.
        agent
            .plugin_registry_handle()
            .build_for_cwd(cwd, &disk_cfg, &[], project_trusted)
    } else {
        // No cwd: global/user skills only (pre-session case). Use the boot snapshot.
        agent.plugin_registry_handle().snapshot()
    };

    let response = crate::session::slash_commands::list_commands(
        req.cwd.as_deref(),
        &skills_config,
        plugin_reg.as_deref(),
        availability,
        false,
        req.kind.as_deref(),
    )
    .await?;
    Ok(acp::ExtResponse::new(Arc::from(
        serde_json::value::to_raw_value(&response)?,
    )))
}

// session/fork

async fn handle_session_fork(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    use crate::session::fork::{ForkSessionRequest, fork_session};

    let request: ForkSessionRequest = parse_params(args)?;

    let response = fork_session(request)
        .await
        .map_err(|e| acp::Error::internal_error().data(e.to_string()))?;

    to_raw_response(&response)
}
