#![cfg_attr(rustfmt, rustfmt::skip)]
//! Async effect execution.
//!
//! This module takes [`Effect`] values produced by [`super::dispatch`] and
//! spawns them as async tasks on a [`JoinSet`].  When tasks complete,
//! the event loop converts their output into [`TaskResult`] and feeds it
//! back through dispatch.
mod helpers;
use super::super::actions;
use super::super::session_title_resolve::worktree_resume_failure_message;
#[allow(unused_imports)]
use super::{dispatch};
use crate::app::session as agent;
pub(super) use helpers::parse_session_load_foreground;
pub(crate) use helpers::{
    EffectMeta, SessionFlags, persist_permission_mode_and_notify, persist_setting,
    sanitize_user_error,
};
use helpers::*;
use std::path::{Path, PathBuf};
use agent_client_protocol as acp;
use tokio::task::JoinSet;
use tokio::io::AsyncBufReadExt as _;
use acp_transport::{AcpAgentTx, acp_send};
use actions::{ClipboardPasteTarget, Effect, ProbedAttachment, SubagentKillOutcome, TaskResult};
#[cfg(test)]
use actions::PermissionModePersist;
#[cfg(test)]
use agent::AgentId;
use crate::unified_log as ulog;
use shell::sampling::error::http_status_from_error;
use shell::session::{ExtMethodResult, SessionInfoResponse};

fn control_rpc_outcome(status: Option<&str>) -> actions::ControlRpcOutcome {
    if status == Some("superseded") {
        actions::ControlRpcOutcome::Superseded
    } else {
        actions::ControlRpcOutcome::AuthoritativeUpdatePending
    }
}

fn control_request_failure(error: acp::Error) -> actions::ControlRequestFailure {
    actions::ControlRequestFailure {
        terminal_published: shell::session::control_terminal_was_published(&error),
        message: sanitize_user_error(&error.to_string()),
    }
}

fn ext_control_rpc_outcome(response: &acp::ExtResponse) -> actions::ControlRpcOutcome {
    let status = serde_json::from_str::<serde_json::Value>(response.0.get())
        .ok()
        .and_then(|value| {
            value
                .pointer("/result/status")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        });
    control_rpc_outcome(status.as_deref())
}

pub(crate) fn execute(
    effect: Effect,
    tasks: &mut JoinSet<TaskResult>,
    acp_tx: &AcpAgentTx,
    cwd: &Path,
    session_flags: &SessionFlags,
) -> (bool, EffectMeta) {
    let meta = EffectMeta;
    match effect {
        Effect::RegisterActiveSession { session_id, cwd } => {
            crate::app::signal_handler::set_current_session_id(Some(session_id.clone()));
            if let Err(e) = shell::active_sessions::register(shell::active_sessions::ActiveSession {
                session_id,
                pid: std::process::id(),
                cwd,
                opened_at: chrono::Utc::now(),
            }) {
                tracing::warn!(?e, "Failed to register active session");
            }
        }
        Effect::UnregisterActiveSession { session_id } => {
            crate::app::signal_handler::set_current_session_id(None);
            unregister_active_session_best_effort(&session_id);
        }
        Effect::Quit => {
            ulog::info("pager quit", None, None);
            return (true, meta);
        }
        Effect::SetWorkingDir { path } => {
            if let Err(e) = std::env::set_current_dir(&path) {
                tracing::warn!(error = %e, "project picker: failed to set_current_dir");
            }
        }
        Effect::FetchProjectPickerRecents { agent_id, picker_id } => {
            tasks.spawn(async move {
                let dirs = crate::project_picker::sources::collect_recent_dirs(10).await;
                TaskResult::ProjectPickerRecentsLoaded {
                    agent_id,
                    picker_id,
                    dirs,
                }
            });
        }
        Effect::FetchDashboardLocationCandidates {
            base_cwd,
            picker_id,
        } => {
            tasks.spawn(async move {
                let (dirs, worktrees) = tokio::join!(
                    crate::project_picker::sources::collect_recent_dirs(10),
                    tokio::task::spawn_blocking(crate::git_info::worktree_label_index),
                );
                TaskResult::DashboardLocationCandidatesLoaded {
                    base_cwd,
                    picker_id,
                    dirs,
                    worktrees: worktrees.unwrap_or_default(),
                }
            });
        }
        Effect::LaunchTrajectory {
            agent_id,
            session_id,
        } => {
            let cwd = cwd.to_path_buf();
            let (runtime_tx, runtime_rx) = tokio::sync::oneshot::channel::<String>();
            let runtime_agent_id = agent_id;
            tasks.spawn(async move {
                match runtime_rx.await {
                    Ok(message) => TaskResult::TrajectoryRuntimeEnded {
                        agent_id: runtime_agent_id,
                        message: sanitize_user_error(&message),
                    },
                    Err(_) => TaskResult::CancelComplete,
                }
            });
            tasks.spawn(async move {
                let result = async {
                    let executable = std::env::current_exe().map_err(|error| {
                        format!("couldn't locate the Grow executable: {error}")
                    })?;
                    let mut command = tokio::process::Command::new(executable);
                    command
                        .arg("trajectory")
                        .arg(session_id.0.as_ref())
                        .arg("--no-open")
                        .current_dir(cwd)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::piped());
                    command.kill_on_drop(true);
                    tty_utils::detach_command(&mut command);
                    let mut child = command.spawn().map_err(|error| {
                        format!("couldn't start Trajectory debugger: {error}")
                    })?;
                    let stderr = child
                        .stderr
                        .take()
                        .ok_or_else(|| "Trajectory debugger has no readiness channel".to_string())?;
                    let mut lines = tokio::io::BufReader::new(stderr).lines();
                    let (readiness_tx, readiness_rx) = tokio::sync::oneshot::channel();
                    let supervisor = tokio::spawn(async move {
                        let mut readiness_tx = Some(readiness_tx);
                        let mut runtime_tx = Some(runtime_tx);
                        let mut diagnostics = Vec::new();
                        loop {
                            match lines.next_line().await {
                                Ok(Some(line)) => {
                                    if let Some(url) = trajectory_ready_url(&line)
                                        && let Some(tx) = readiness_tx.take()
                                    {
                                        diagnostics.clear();
                                        let _ = tx.send(Ok(url.to_owned()));
                                    } else {
                                        if readiness_tx.is_none() {
                                            tracing::warn!(diagnostic = %line, "Trajectory runtime diagnostic");
                                        }
                                        if diagnostics.len() < 8 {
                                            diagnostics.push(line);
                                        }
                                    }
                                }
                                Ok(None) => {
                                    let status = child.wait().await.map_err(|error| {
                                        format!("couldn't inspect Trajectory process: {error}")
                                    });
                                    if let Some(tx) = readiness_tx.take() {
                                        let detail = if diagnostics.is_empty() {
                                            String::new()
                                        } else {
                                            format!(": {}", diagnostics.join(" | "))
                                        };
                                        let message = match status {
                                            Ok(status) => format!(
                                                "Trajectory debugger exited before it was ready ({status}){detail}"
                                            ),
                                            Err(error) => error,
                                        };
                                        let _ = tx.send(Err(message));
                                    } else if let Some(tx) = runtime_tx.take() {
                                        let detail = if diagnostics.is_empty() {
                                            String::new()
                                        } else {
                                            format!(": {}", diagnostics.join(" | "))
                                        };
                                        let message = match status {
                                            Ok(status) => format!(
                                                "Trajectory debugger stopped ({status}){detail}"
                                            ),
                                            Err(error) => error,
                                        };
                                        let _ = tx.send(message);
                                    }
                                    break;
                                }
                                Err(error) => {
                                    if let Some(tx) = readiness_tx.take() {
                                        let _ = tx.send(Err(format!(
                                            "couldn't read Trajectory startup output: {error}"
                                        )));
                                    } else if let Some(tx) = runtime_tx.take() {
                                        let _ = tx.send(format!(
                                            "Trajectory debugger output failed after launch: {error}"
                                        ));
                                    }
                                    break;
                                }
                            }
                        }
                        Ok::<(), String>(())
                    });
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(10),
                        readiness_rx,
                    )
                    .await
                    {
                        Ok(Ok(result)) => result,
                        Ok(Err(_)) => {
                            let _ = supervisor.await;
                            Err("Trajectory debugger readiness channel closed unexpectedly".into())
                        }
                        Err(_) => {
                            supervisor.abort();
                            let _ = supervisor.await;
                            Err("Trajectory debugger did not become ready within 10 seconds".into())
                        }
                    }
                }
                .await
                .map_err(|error| sanitize_user_error(&error));
                TaskResult::TrajectoryLaunched { agent_id, result }
            });
        }
        Effect::CreateSession {
            agent_id,
            cwd: session_cwd,
            model_id,
            preferred_session_id,
        } => {
            let tx = acp_tx.clone();
            let mcp_servers = shell::util::config::load_mcp_servers(&session_cwd);
            let mcp_count = mcp_servers.len();
            #[allow(unused_mut)]
            let mut meta = session_flags.to_meta();
            if let Some(ref mid) = model_id {
                meta.get_or_insert_with(acp::Meta::new)
                    .insert("modelId".into(), serde_json::json!(mid.0));
            }
            if let Some(ref sid) = preferred_session_id {
                meta.get_or_insert_with(acp::Meta::new)
                    .insert("sessionId".into(), serde_json::json!(sid));
            }
            let preferred_for_preflight = preferred_session_id.clone();
            tasks
                .spawn(async move {
                    if let Some(ref sid) = preferred_for_preflight {
                        let session_cwd_str = session_cwd.to_string_lossy();
                        if let Err(e) = crate::app::session_startup::ensure_session_id_available(
                            sid,
                            &session_cwd_str,
                        ) {
                            return TaskResult::SessionFailed {
                                agent_id,
                                error: sanitize_user_error(&e.to_string()),
                            };
                        }
                    }
                    ulog::info(
                        "session.create.start",
                        None,
                        Some(serde_json::json!({"mcp_server_count": mcp_count})),
                    );
                    let create_start = std::time::Instant::now();
                    let result = acp_send(
                            acp::NewSessionRequest::new(session_cwd.clone())
                                .mcp_servers(mcp_servers)
                                .meta(meta),
                            &tx,
                        )
                        .await;
                    let create_elapsed_ms = create_start.elapsed().as_millis() as u64;
                    match result {
                        Ok(resp) => {
                            ulog::info(
                                "session.create.done",
                                Some(&resp.session_id.0),
                                Some(
                                    serde_json::json!({
                                "elapsed_ms": create_elapsed_ms,
                                "mcp_server_count": mcp_count,
                            }),
                                ),
                            );
                            TaskResult::SessionCreated {
                                agent_id,
                                session_id: resp.session_id,
                                models: resp.models,
                            }
                        }
                        Err(e) => {
                            let error = e.to_string();
                            ulog::error(
                                "session.create.failed",
                                None,
                                Some(
                                    serde_json::json!({
                                "elapsed_ms": create_elapsed_ms,
                                "error": &error,
                            }),
                                ),
                            );
                            TaskResult::SessionFailed {
                                agent_id,
                                error: sanitize_user_error(&error),
                            }
                        }
                    }
                });
        }
        Effect::CreateWorktreeSession {
            agent_id,
            load_session_id,
            label,
            git_ref,
            model_id,
            preferred_session_id,
        } => {
            let tx = acp_tx.clone();
            let cwd = cwd.to_path_buf();
            let mut meta = session_flags.to_meta();
            if let Some(ref mid) = model_id {
                meta.get_or_insert_with(acp::Meta::new)
                    .insert("modelId".into(), serde_json::json!(mid.0));
            }
            if load_session_id.is_none() && let Some(ref sid) = preferred_session_id {
                meta.get_or_insert_with(acp::Meta::new)
                    .insert("sessionId".into(), serde_json::json!(sid));
            }
            let restore_code = session_flags.restore_code;
            let resume_local_miss = session_flags.resume_local_miss.clone();
            tracing::info!(
                ?restore_code,
                ?load_session_id,
                ?git_ref,
                "CreateWorktreeSession: restore_code, load_session_id, git_ref"
            );
            tasks
                .spawn(async move {
                    if let Some(sid) = load_session_id {
                        let local_miss = resume_local_miss
                            .as_deref()
                            .filter(|t| *t == sid);
                        let resume_started = std::time::Instant::now();
                        let wt_type = shell::util::config::worktree_type();
                        let copy_mode = if git_ref.is_some() {
                            "clean"
                        } else {
                            "dirty"
                        };
                        let mut payload = serde_json::json!({
                        "sessionId": sid,
                        "sourceCwd": cwd.to_string_lossy(),
                        "copyMode": copy_mode,
                        "worktreeType": wt_type,
                    });
                        if let Some(rc) = restore_code {
                            payload["restoreCode"] = serde_json::Value::Bool(rc);
                        }
                        if let Some(ref r) = git_ref {
                            payload["gitRef"] = serde_json::Value::String(r.clone());
                        }
                        let ext_req = acp::ExtRequest::new(
                            "grow/git/worktree/resume_session",
                            serde_json::value::to_raw_value(&payload)
                                .expect("serialize resume params")
                                .into(),
                        );
                        let ext_resp = match acp_send(ext_req, &tx).await {
                            Ok(resp) => {
                                tracing::info!(
                                session_id = %sid,
                                elapsed_ms = resume_started.elapsed().as_millis() as u64,
                                "worktree resume_session: ACP call completed"
                            );
                                resp
                            }
                            Err(e) => {
                                tracing::warn!(
                                session_id = %sid,
                                elapsed_ms = resume_started.elapsed().as_millis() as u64,
                                error = %e,
                                "worktree resume_session: ACP call failed"
                            );
                                return TaskResult::WorktreeSessionFailed {
                                    agent_id,
                                    error: worktree_resume_failure_message(
                                        local_miss,
                                        &sanitize_user_error(&e.to_string()),
                                    ),
                                };
                            }
                        };
                        let resp_value: serde_json::Value = match serde_json::from_str(
                            ext_resp.0.get(),
                        ) {
                            Ok(v) => v,
                            Err(e) => {
                                return TaskResult::WorktreeSessionFailed {
                                    agent_id,
                                    error: worktree_resume_failure_message(
                                        local_miss,
                                        &sanitize_user_error(&e.to_string()),
                                    ),
                                };
                            }
                        };
                        if let Some(err) = resp_value
                            .get("error")
                            .filter(|v| !v.is_null())
                        {
                            let msg = err
                                .as_str()
                                .map(String::from)
                                .unwrap_or_else(|| err.to_string());
                            return TaskResult::WorktreeSessionFailed {
                                agent_id,
                                error: worktree_resume_failure_message(
                                    local_miss,
                                    &sanitize_user_error(&msg),
                                ),
                            };
                        }
                        let result_obj = resp_value.get("result").unwrap_or(&resp_value);
                        let new_session_id = result_obj
                            .get("sessionId")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&sid);
                        let wt_path = result_obj
                            .get("worktreePath")
                            .and_then(|v| v.as_str())
                            .map(PathBuf::from)
                            .unwrap_or_else(|| cwd.clone());
                        let eff_cwd = result_obj
                            .get("effectiveCwd")
                            .and_then(|v| v.as_str())
                            .map(PathBuf::from)
                            .unwrap_or_else(|| wt_path.clone());
                        let (code_restored, restore_summary, restore_degree) = parse_worktree_restore_payload(
                            result_obj,
                        );
                        return TaskResult::WorktreeForked {
                            agent_id,
                            session_id: acp::SessionId::new(new_session_id),
                            worktree_path: wt_path,
                            session_cwd: eff_cwd,
                            code_restored,
                            restore_summary,
                            restore_degree,
                        };
                    }
                    let worktree_id = preferred_session_id
                        .clone()
                        .unwrap_or_else(|| {
                            format!("pager-{}", &uuid::Uuid::new_v4().simple().to_string()[..12])
                        });
                    let copy_mode = if git_ref.is_some() { "clean" } else { "dirty" };
                    let mut params = serde_json::json!({
                    "sourceWorktreePath": cwd.to_string_lossy(),
                    "newSessionId": worktree_id,
                    "copyMode": copy_mode,
                });
                    if let Some(ref lbl) = label {
                        params["label"] = serde_json::Value::String(lbl.clone());
                    }
                    if let Some(ref r) = git_ref {
                        params["gitRef"] = serde_json::Value::String(r.clone());
                    }
                    let ext_req = acp::ExtRequest::new(
                        "grow/git/worktree/create_from_worktree_sync",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize worktree params")
                            .into(),
                    );
                    let ext_resp = match acp_send(ext_req, &tx).await {
                        Ok(resp) => resp,
                        Err(e) => {
                            return TaskResult::WorktreeSessionFailed {
                                agent_id,
                                error: sanitize_user_error(
                                    &format!("couldn't create worktree: {e}"),
                                ),
                            };
                        }
                    };
                    let resp_value: serde_json::Value = match serde_json::from_str(
                        ext_resp.0.get(),
                    ) {
                        Ok(v) => v,
                        Err(e) => {
                            return TaskResult::WorktreeSessionFailed {
                                agent_id,
                                error: sanitize_user_error(
                                    &format!("couldn't create worktree: {e}"),
                                ),
                            };
                        }
                    };
                    if let Some(err) = resp_value.get("error") {
                        let msg = err
                            .as_str()
                            .map(String::from)
                            .unwrap_or_else(|| err.to_string());
                        return TaskResult::WorktreeSessionFailed {
                            agent_id,
                            error: sanitize_user_error(
                                &format!("couldn't create worktree: {msg}"),
                            ),
                        };
                    }
                    let result_obj = resp_value.get("result").unwrap_or(&resp_value);
                    let worktree_root = match result_obj
                        .get("worktreePath")
                        .and_then(|v| v.as_str())
                    {
                        Some(p) => PathBuf::from(p),
                        None => {
                            return TaskResult::WorktreeSessionFailed {
                                agent_id,
                                error: sanitize_user_error(
                                    "couldn't create worktree: response missing worktreePath",
                                ),
                            };
                        }
                    };
                    let session_cwd = if let Some(git_root) = result_obj
                        .get("sourceGitRoot")
                        .and_then(|v| v.as_str())
                    {
                        let cwd_str = cwd.to_string_lossy();
                        if let Some(relative) = cwd_str.strip_prefix(git_root) {
                            let relative = relative.trim_start_matches('/');
                            if relative.is_empty() {
                                worktree_root.clone()
                            } else {
                                worktree_root.join(relative)
                            }
                        } else {
                            worktree_root.clone()
                        }
                    } else {
                        worktree_root.clone()
                    };
                    if let Some(ref sid) = preferred_session_id {
                        let session_cwd_str = session_cwd.to_string_lossy();
                        if let Err(e) = crate::app::session_startup::ensure_session_id_available(
                            sid,
                            &session_cwd_str,
                        ) {
                            return TaskResult::WorktreeSessionFailed {
                                agent_id,
                                error: sanitize_user_error(&e.to_string()),
                            };
                        }
                    }
                    let mcp_servers = shell::util::config::load_mcp_servers(&session_cwd);
                    let result = acp_send(
                            acp::NewSessionRequest::new(session_cwd.clone())
                                .mcp_servers(mcp_servers)
                                .meta(meta),
                            &tx,
                        )
                        .await;
                    match result {
                        Ok(resp) => {
                            TaskResult::WorktreeSessionCreated {
                                agent_id,
                                session_id: resp.session_id,
                                worktree_path: worktree_root,
                                session_cwd,
                                models: resp.models,
                            }
                        }
                        Err(e) => {
                            TaskResult::WorktreeSessionFailed {
                                agent_id,
                                error: sanitize_user_error(
                                    &format!(
                            "couldn't create session in worktree: {e}"
                        ),
                                ),
                            }
                        }
                    }
                });
        }
        Effect::LoadSession { agent_id, session_id, session_cwd } => {
            let tx = acp_tx.clone();
            let mut meta = session_flags.to_meta();
            if let Some(true) = session_flags.restore_code {
                meta.get_or_insert_with(acp::Meta::new)
                    .insert("grow/restore_code".into(), serde_json::Value::Bool(true));
            }
            let cwd = session_cwd.unwrap_or_else(|| cwd.to_path_buf());
            let mcp_started = std::time::Instant::now();
            let mcp_servers = shell::util::config::load_mcp_servers(&cwd);
            tracing::info!(
                elapsed_ms = mcp_started.elapsed().as_millis() as u64,
                server_count = mcp_servers.len(),
                "load_session: mcp server discovery"
            );
            let acp_session_id = acp::SessionId::new(session_id);
            tasks
                .spawn(async move {
                    ulog::info("session.load.start", Some(&acp_session_id.0), None);
                    let load_started = std::time::Instant::now();
                    let result = acp_send(
                            acp::LoadSessionRequest::new(
                                    acp_session_id.clone(),
                                    cwd.clone(),
                                )
                                .mcp_servers(mcp_servers.clone())
                                .meta(meta.clone()),
                            &tx,
                        )
                        .await;
                    let load_elapsed_ms = load_started.elapsed().as_millis() as u64;
                    tracing::info!(
                    session_id = %acp_session_id.0,
                    elapsed_ms = load_elapsed_ms,
                    ok = result.is_ok(),
                    "load_session: acp load_session completed"
                );
                    match result {
                        Ok(resp) => {
                            ulog::info(
                                "session.load.done",
                                Some(&acp_session_id.0),
                                Some(serde_json::json!({"elapsed_ms": load_elapsed_ms})),
                            );
                            let (code_restored, restore_summary, restore_degree) = parse_session_load_restore_meta(
                                resp.meta.as_ref(),
                            );
                            let foreground = parse_session_load_foreground(resp.meta.as_ref());
                            TaskResult::SessionLoaded {
                                agent_id,
                                session_id: acp_session_id,
                                models: resp.models,
                                code_restored,
                                restore_summary,
                                restore_degree,
                                foreground,
                            }
                        }
                        Err(e) => {
                            let error = e.to_string();
                            ulog::error(
                                "session.load.failed",
                                Some(&acp_session_id.0),
                                Some(
                                    serde_json::json!({"elapsed_ms": load_elapsed_ms, "error": &error}),
                                ),
                            );
                            TaskResult::SessionLoadFailed {
                                agent_id,
                                session_id: acp_session_id,
                                error: sanitize_user_error(&error),
                            }
                        }
                    }
                });
        }
        Effect::FetchSessionList { query, seq, .. } => {
            let tx = acp_tx.clone();
            let cwd = cwd.to_path_buf();
            tasks
                .spawn(async move {
                    let mut params = serde_json::json!({
                    "cwd": cwd.to_string_lossy(),
                    "limit": 30,
                });
                    if let Some(q) = &query {
                        params["query"] = serde_json::Value::String(q.clone());
                    } else {
                        params["allowRelax"] = serde_json::Value::Bool(true);
                    }
                    let request = acp::ExtRequest::new(
                        "grow/session/list",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize session list params")
                            .into(),
                    );
                    let result = acp_send(request, &tx).await;
                    match result {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            if let Some(err) = wrapper.get("error") {
                                return TaskResult::SessionListFailed {
                                    error: err.as_str().unwrap_or("unknown error").to_string(),
                                    seq,
                                    query,
                                };
                            }
                            let payload = wrapper.get("result").unwrap_or(&wrapper);
                            let sessions = parse_session_picker_entries(payload);
                            let scope = parse_session_list_scope(payload);
                            TaskResult::SessionListLoaded {
                                sessions,
                                scope,
                                seq,
                                query,
                            }
                        }
                        Err(e) => {
                            TaskResult::SessionListFailed {
                                error: sanitize_user_error(&format!("{e}")),
                                seq,
                                query,
                            }
                        }
                    }
                });
        }
        Effect::DebounceSessionSearch { query, seq } => {
            tasks
                .spawn(async move {
                    tokio::time::sleep(
                            std::time::Duration::from_millis(SESSION_SEARCH_DEBOUNCE_MS),
                        )
                        .await;
                    TaskResult::SessionSearchDebounceExpired {
                        query,
                        seq,
                    }
                });
        }
        Effect::FetchRoster => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let request = acp::ExtRequest::new(
                        "grow/sessions/list",
                        serde_json::value::to_raw_value(&serde_json::json!({}))
                            .expect("serialize roster list params")
                            .into(),
                    );
                    match acp_send(request, &tx).await {
                        Ok(resp) => {
                            let parsed = crate::app::roster::parse_roster_list_response(
                                resp.0.get(),
                            );
                            match parsed {
                                Some(r) => {
                                    TaskResult::RosterLoaded {
                                        sessions: r.sessions,
                                    }
                                }
                                None => {
                                    tracing::warn!("failed to parse grow/sessions/list response");
                                    TaskResult::RosterFailed {
                                        error: "parse error".to_string(),
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            TaskResult::RosterFailed {
                                error: sanitize_user_error(&format!("{e}")),
                            }
                        }
                    }
                });
        }
        Effect::FetchDashboardSessions => {
            let tx = acp_tx.clone();
            let cwd = cwd.to_path_buf();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "cwd": cwd.to_string_lossy(),
                    "limit": 30,
                });
                    let request = acp::ExtRequest::new(
                        "grow/session/list",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize session list params")
                            .into(),
                    );
                    match acp_send(request, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            if wrapper.get("error").is_some() {
                                return TaskResult::DashboardSessionsLoaded {
                                    sessions: vec![],
                                };
                            }
                            let payload = wrapper.get("result").unwrap_or(&wrapper);
                            let sessions = parse_session_picker_entries(payload)
                                .iter()
                                .map(session_picker_entry_to_roster)
                                .collect();
                            TaskResult::DashboardSessionsLoaded {
                                sessions,
                            }
                        }
                        Err(_) => {
                            TaskResult::DashboardSessionsLoaded {
                                sessions: vec![],
                            }
                        }
                    }
                });
        }
       Effect::LoadCardDetail { session_id, cwd, generation } => {
            tasks
                .spawn(async move {
                    use crate::app::root::CardDetail;
                    let result_session_id = session_id.clone();
                    let detail = tokio::task::spawn_blocking(move || {
                            let info = shell::session::info::Info {
                                id: acp::SessionId::new(session_id),
                                cwd,
                            };
                            let first_prompt_preview = extract_first_user_prompt(&info)
                                .unwrap_or_default();
                            let (turn_count, tool_call_count) = count_timeline_stats(&info);
                            CardDetail {
                                turn_count,
                                tool_call_count,
                                first_prompt_preview,
                            }
                        })
                        .await
                        .unwrap_or(CardDetail {
                            turn_count: 0,
                            tool_call_count: 0,
                            first_prompt_preview: String::new(),
                        });
                    TaskResult::CardDetailLoaded {
                        session_id: result_session_id,
                        generation,
                        detail,
                    }
                });
        }
        Effect::SendPrompt {
            agent_id,
            session_id,
            text,
            prompt_id,
            skill_token_ranges,
        } => {
            let tx = acp_tx.clone();
            let screen_mode = session_flags.screen_mode_label;
            tasks
                .spawn(async move {
                    ulog::info(
                        "prompt.acp_send.start",
                        Some(&session_id.0),
                        Some(
                            serde_json::json!({
                        "kind": "text",
                        "len": text.len(),
                        "prompt_id": prompt_id,
                    }),
                        ),
                    );
                    let send_start = std::time::Instant::now();
                    let prompt = vec![plain_prompt_content_block(text, &skill_token_ranges)];
                    let req = acp::PromptRequest::new(session_id.clone(), prompt)
                        .meta(
                            prompt_request_meta(&prompt_id, screen_mode)
                                .as_object()
                                .cloned(),
                        );
                    let result = acp_send(req, &tx).await;
                    let send_elapsed_ms = send_start.elapsed().as_millis() as u64;
                    ulog::info(
                        "prompt.acp_send.done",
                        Some(&session_id.0),
                        Some(
                            serde_json::json!({
                        "kind": "text",
                        "elapsed_ms": send_elapsed_ms,
                        "ok": result.is_ok(),
                        "prompt_id": prompt_id,
                    }),
                        ),
                    );
                    log_prompt_result(&session_id, &result);
                    let http_status = result
                        .as_ref()
                        .err()
                        .and_then(http_status_from_error);
                    TaskResult::PromptResponse {
                        agent_id,
                        result: result
                            .map_err(|e| format_acp_error(&e)),
                        http_status,
                        prompt_id: Some(prompt_id),
                    }
                });
        }
        Effect::SendPromptBlocks { agent_id, session_id, blocks, prompt_id } => {
            let tx = acp_tx.clone();
            let screen_mode = session_flags.screen_mode_label;
            tasks
                .spawn(async move {
                    ulog::info(
                        "prompt.acp_send.start",
                        Some(&session_id.0),
                        Some(
                            serde_json::json!({
                        "kind": "blocks",
                        "block_count": blocks.len(),
                        "prompt_id": prompt_id,
                    }),
                        ),
                    );
                    let send_start = std::time::Instant::now();
                    let meta = prompt_request_meta(&prompt_id, screen_mode);
                    let req = acp::PromptRequest::new(session_id.clone(), blocks)
                        .meta(meta.as_object().cloned());
                    let result = acp_send(req, &tx).await;
                    let send_elapsed_ms = send_start.elapsed().as_millis() as u64;
                    ulog::info(
                        "prompt.acp_send.done",
                        Some(&session_id.0),
                        Some(
                            serde_json::json!({
                        "kind": "blocks",
                        "elapsed_ms": send_elapsed_ms,
                        "ok": result.is_ok(),
                        "prompt_id": prompt_id,
                    }),
                        ),
                    );
                    log_prompt_result(&session_id, &result);
                    let http_status = result
                        .as_ref()
                        .err()
                        .and_then(http_status_from_error);
                    TaskResult::PromptResponse {
                        agent_id,
                        result: result
                            .map_err(|e| format_acp_error(&e)),
                        http_status,
                        prompt_id: Some(prompt_id),
                    }
                });
        }
        Effect::SendBashCommand { agent_id, session_id, command, prompt_id } => {
            let tx = acp_tx.clone();
            let screen_mode = session_flags.screen_mode_label;
            tasks
                .spawn(async move {
                    use shell::extensions::prompt_meta::PromptBlockMeta;
                    ulog::info(
                        "prompt.acp_send.start",
                        Some(&session_id.0),
                        Some(
                            serde_json::json!({
                        "kind": "bash",
                        "len": command.len(),
                        "prompt_id": prompt_id,
                    }),
                        ),
                    );
                    let send_start = std::time::Instant::now();
                    let meta = PromptBlockMeta::bash(&command);
                    let prompt = vec![acp::ContentBlock::Text(
                    acp::TextContent::new(command).meta(
                        serde_json::to_value(&meta)
                            .expect("PromptBlockMeta serializes")
                            .as_object()
                            .cloned(),
                    ),
                )];
                    let req = acp::PromptRequest::new(session_id.clone(), prompt)
                        .meta(
                            prompt_request_meta(&prompt_id, screen_mode)
                                .as_object()
                                .cloned(),
                        );
                    let result = acp_send(req, &tx).await;
                    let send_elapsed_ms = send_start.elapsed().as_millis() as u64;
                    ulog::info(
                        "prompt.acp_send.done",
                        Some(&session_id.0),
                        Some(
                            serde_json::json!({
                        "kind": "bash",
                        "elapsed_ms": send_elapsed_ms,
                        "ok": result.is_ok(),
                        "prompt_id": prompt_id,
                    }),
                        ),
                    );
                    log_prompt_result(&session_id, &result);
                    let http_status = result
                        .as_ref()
                        .err()
                        .and_then(http_status_from_error);
                    TaskResult::PromptResponse {
                        agent_id,
                        result: result.map_err(|e| e.to_string()),
                        http_status,
                        prompt_id: Some(prompt_id),
                    }
                });
        }
        Effect::CancelTurn {
            session_id,
            cancel_subagents,
            pause_goal,
            trigger,
            rewind_if_pristine,
        } => {
            let tx = acp_tx.clone();
            let trigger_str = trigger.map(|t| t.as_wire_str());
            tasks
                .spawn(async move {
                    ulog::info(
                        "cancel.acp_send.start",
                        Some(&session_id.0),
                        Some(
                            serde_json::json!({
                        "cancel_subagents": cancel_subagents,
                        "pause_goal": pause_goal,
                        "trigger": trigger_str,
                        "rewind_if_pristine": rewind_if_pristine,
                    }),
                        ),
                    );
                    let send_start = std::time::Instant::now();
                    let mut meta = serde_json::json!({ "cancelSubagents": cancel_subagents });
                    if pause_goal {
                        meta["pauseGoal"] = true.into();
                    }
                    if let Some(t) = trigger_str {
                        meta["cancelTrigger"] = t.into();
                    }
                    if rewind_if_pristine {
                        meta["rewindIfPristine"] = true.into();
                    }
                    let req = acp::CancelNotification::new(session_id.clone())
                        .meta(meta.as_object().cloned());
                    let result = acp_send(req, &tx).await;
                    ulog::info(
                        "cancel.acp_send.done",
                        Some(&session_id.0),
                        Some(
                            serde_json::json!({
                        "ok": result.is_ok(),
                        "elapsed_ms": send_start.elapsed().as_millis() as u64,
                    }),
                        ),
                    );
                    if let Err(e) = result {
                        tracing::warn!("Failed to send cancel notification: {e}");
                    }
                    TaskResult::CancelComplete
                });
        }
        Effect::TogglePlanMode { session_id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "sessionId": session_id.0.to_string(),
                });
                    let notification = acp::ExtNotification::new(
                        "grow/toggle_plan_mode",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize toggle_plan_mode params")
                            .into(),
                    );
                    if let Err(e) = acp_send(notification, &tx).await {
                        tracing::warn!("Failed to send toggle_plan_mode notification: {e}");
                    }
                    TaskResult::CancelComplete
                });
        }
        Effect::QueueRemove { session_id, id, expected_version } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "sessionId": session_id.0.to_string(),
                    "id": id,
                    "expectedVersion": expected_version,
                });
                    let notification = acp::ExtNotification::new(
                        "grow/queue/remove",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize queue/remove params")
                            .into(),
                    );
                    if let Err(e) = acp_send(notification, &tx).await {
                        tracing::warn!("Failed to send queue/remove notification: {e}");
                    }
                    TaskResult::CancelComplete
                });
        }
        Effect::QueueReorder { session_id, ordered_ids } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "sessionId": session_id.0.to_string(),
                    "orderedIds": ordered_ids,
                });
                    let notification = acp::ExtNotification::new(
                        "grow/queue/reorder",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize queue/reorder params")
                            .into(),
                    );
                    if let Err(e) = acp_send(notification, &tx).await {
                        tracing::warn!("Failed to send queue/reorder notification: {e}");
                    }
                    TaskResult::CancelComplete
                });
        }
        Effect::QueueClear { session_id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "sessionId": session_id.0.to_string(),
                });
                    let notification = acp::ExtNotification::new(
                        "grow/queue/clear",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize queue/clear params")
                            .into(),
                    );
                    if let Err(e) = acp_send(notification, &tx).await {
                        tracing::warn!("Failed to send queue/clear notification: {e}");
                    }
                    TaskResult::CancelComplete
                });
        }
        Effect::QueueEdit { session_id, id, new_text } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "sessionId": session_id.0.to_string(),
                    "id": id,
                    "newText": new_text,
                });
                    let notification = acp::ExtNotification::new(
                        "grow/queue/edit",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize queue/edit params")
                            .into(),
                    );
                    if let Err(e) = acp_send(notification, &tx).await {
                        tracing::warn!("Failed to send queue/edit notification: {e}");
                    }
                    TaskResult::CancelComplete
                });
        }
        Effect::QueueHoldEdit { session_id, id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "sessionId": session_id.0.to_string(),
                    "id": id,
                });
                    let notification = acp::ExtNotification::new(
                        "grow/queue/hold_edit",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize queue/hold_edit params")
                            .into(),
                    );
                    if let Err(e) = acp_send(notification, &tx).await {
                        tracing::warn!("Failed to send queue/hold_edit notification: {e}");
                    }
                    TaskResult::CancelComplete
                });
        }
        Effect::QueueReleaseEdit { session_id, id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "sessionId": session_id.0.to_string(),
                    "id": id,
                });
                    let notification = acp::ExtNotification::new(
                        "grow/queue/release_edit",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize queue/release_edit params")
                            .into(),
                    );
                    if let Err(e) = acp_send(notification, &tx).await {
                        tracing::warn!("Failed to send queue/release_edit notification: {e}");
                    }
                    TaskResult::CancelComplete
                });
        }
        Effect::QueueInterject {
            session_id,
            expected_turn_id,
            id,
            expected_version,
            new_text,
        } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let mut params = serde_json::json!({
                    "sessionId": session_id.0.to_string(),
                    "expectedTurnId": expected_turn_id,
                    "id": id,
                    "expectedVersion": expected_version,
                });
                    if let Some(new_text) = new_text {
                        params["newText"] = serde_json::Value::String(new_text);
                    }
                    let notification = acp::ExtNotification::new(
                        "grow/queue/interject",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize queue/interject params")
                            .into(),
                    );
                    if let Err(e) = acp_send(notification, &tx).await {
                        tracing::warn!("Failed to send queue/interject notification: {e}");
                    }
                    TaskResult::CancelComplete
                });
        }
        Effect::SwitchBehavior {
            agent_id,
            session_id,
            control_token,
            mode,
        } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let mut meta = acp::Meta::new();
                    control_token.shell_intent().insert_meta(&mut meta);
                    let req = acp::SetSessionModeRequest::new(
                        session_id.clone(),
                        acp::SessionModeId::new(mode.as_id()),
                    )
                    .meta(meta);
                    let result = acp_send(req, &tx)
                        .await
                        .map(|response| {
                            control_rpc_outcome(
                                response
                                    .meta
                                    .as_ref()
                                    .and_then(|meta| meta.get("status"))
                                    .and_then(serde_json::Value::as_str),
                            )
                        })
                        .map_err(control_request_failure);
                    TaskResult::SwitchBehaviorComplete {
                        agent_id,
                        session_id,
                        control_token,
                        mode,
                        result,
                    }
                });
        }
        Effect::Compact {
            agent_id,
            session_id,
            user_context,
            track_foreground,
        } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                        "sessionId": session_id.0.to_string(),
                        "userContext": user_context,
                    });
                    let req = acp::ExtRequest::new(
                        "grow/compact_conversation",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize compact params")
                            .into(),
                    );
                    let result = match acp_send(req, &tx).await {
                        Ok(response) => serde_json::from_str::<serde_json::Value>(response.0.get())
                            .map_err(|error| sanitize_user_error(&format!(
                                "invalid compact response: {error}"
                            )))
                            .and_then(|value| {
                                serde_json::from_value(
                                    value.get("status").cloned().unwrap_or_default(),
                                )
                                .map_err(|error| sanitize_user_error(&format!(
                                    "invalid compact status: {error}"
                                )))
                            }),
                        Err(error) => Err(sanitize_user_error(&error.to_string())),
                    };
                    TaskResult::CompactComplete {
                        agent_id,
                        track_foreground,
                        result,
                    }
                });
        }
        Effect::FetchPromptHistory { agent_id, cwd, session_id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "cwd": cwd.to_string_lossy(),
                    "session_id": session_id,
                });
                    let req = acp::ExtRequest::new(
                        "grow/prompt_history",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize prompt_history params")
                            .into(),
                    );
                    match acp_send(req, &tx).await {
                        Ok(resp) => {
                            let resp_value: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let prompts = resp_value
                                .get("result")
                                .and_then(|r| r.get("prompts"))
                                .or_else(|| resp_value.get("prompts"))
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr
                                        .iter()
                                        .filter_map(|v| v.as_str().map(String::from))
                                        .collect::<Vec<String>>()
                                })
                                .unwrap_or_default();
                            TaskResult::PromptHistoryLoaded {
                                agent_id,
                                prompts,
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Failed to fetch prompt history: {e}");
                            TaskResult::PromptHistoryLoaded {
                                agent_id,
                                prompts: Vec::new(),
                            }
                        }
                    }
                });
        }
        Effect::KillBgTask { session_id, task_id } => {
            let tx = acp_tx.clone();
            let sid = session_id.0.to_string();
            tasks
                .spawn(async move {
                    let params = shell::extensions::task::KillTaskRequest {
                        session_id: sid.clone(),
                        task_id: task_id.clone(),
                    };
                    let req = acp::ExtRequest::new(
                        "grow/task/kill",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize kill params")
                            .into(),
                    );
                    match acp_send(req, &tx).await {
                        Ok(resp) => {
                            let outcome = parse_kill_outcome(resp.0.get());
                            TaskResult::BgTaskKilled {
                                session_id: sid,
                                task_id,
                                outcome,
                            }
                        }
                        Err(e) => {
                            TaskResult::BgTaskKillFailed {
                                session_id: sid,
                                task_id,
                                error: sanitize_user_error(&e.to_string()),
                            }
                        }
                    }
                });
        }
        Effect::KillSubagent { session_id, subagent_id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "sessionId": session_id.0.to_string(),
                    "subagentId": &subagent_id,
                });
                    let req = acp::ExtRequest::new(
                        "grow/subagent/cancel",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize cancel params")
                            .into(),
                    );
                    let outcome = match acp_send(req, &tx).await {
                        Ok(resp) => parse_subagent_kill_outcome(resp.0.get()),
                        Err(e) => {
                            tracing::warn!("Failed to cancel subagent: {e}");
                            SubagentKillOutcome::RpcFailed
                        }
                    };
                    TaskResult::KillSubagentComplete {
                        session_id,
                        subagent_id,
                        outcome,
                    }
                });
        }
        Effect::DeleteScheduledTask { session_id, task_id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "sessionId": session_id.0.to_string(),
                    "taskId": task_id,
                });
                    let req = acp::ExtRequest::new(
                        "grow/scheduler/delete",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize scheduler delete params")
                            .into(),
                    );
                    if let Err(e) = acp_send(req, &tx).await {
                        tracing::warn!(task_id, "Failed to delete scheduled task: {e}");
                    }
                    TaskResult::CancelComplete
                });
        }
        Effect::DemoteToBackground { session_id, tool_call_id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "sessionId": session_id.0.to_string(),
                    "terminalId": tool_call_id,
                });
                    let req = acp::ExtRequest::new(
                        "grow/terminal/background",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize background params")
                            .into(),
                    );
                    if let Err(e) = acp_send(req, &tx).await {
                        tracing::warn!("Failed to send background request: {e}");
                    }
                    TaskResult::CancelComplete
                });
        }
        Effect::SwitchModel {
            agent_id,
            session_id,
            control_token,
            model_id,
            effort,
            effort_patch,
        } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let mut meta = acp::Meta::new();
                    if let Some(eff) = effort {
                            use shell::sampling::types::{
                                REASONING_EFFORT_META_KEY, reasoning_effort_meta_value,
                            };
                            meta.insert(
                                REASONING_EFFORT_META_KEY.to_string(),
                                reasoning_effort_meta_value(eff),
                            );
                    }
                    if effort_patch {
                        meta.insert(
                            shell::session::EFFORT_PATCH_META_KEY.to_string(),
                            serde_json::Value::Bool(true),
                        );
                    }
                    control_token.shell_intent().insert_meta(&mut meta);
                    let req = acp::SetSessionModelRequest::new(
                            session_id.clone(),
                            model_id.clone(),
                        )
                        .meta(meta);
                    let result = acp_send(req, &tx)
                        .await
                        .map(|response| {
                            control_rpc_outcome(
                                response
                                    .meta
                                    .as_ref()
                                    .and_then(|meta| meta.get("status"))
                                    .and_then(serde_json::Value::as_str),
                            )
                        })
                        .map_err(control_request_failure);
                    TaskResult::SwitchModelComplete {
                        agent_id,
                        session_id,
                        control_token,
                        model_id,
                        effort,
                        result,
                    }
                });
        }
        Effect::SwitchAgent {
            agent_id,
            session_id,
            control_token,
            agent_name,
        } => {
            let tx = acp_tx.clone();
            tasks.spawn(async move {
                #[derive(serde::Serialize)]
                #[serde(rename_all = "camelCase")]
                struct SetAgentRequest<'a> {
                    session_id: &'a str,
                    agent_name: &'a str,
                    intent: shell::session::ControlIntent,
                }
                let request = acp::ExtRequest::new(
                    "grow/session/set_agent",
                    serde_json::value::to_raw_value(&SetAgentRequest {
                        session_id: session_id.0.as_ref(),
                        agent_name: &agent_name,
                        intent: control_token.shell_intent(),
                    })
                    .expect("serialize set-agent params")
                    .into(),
                );
                let result = acp_send(request, &tx)
                    .await
                    .map(|response| ext_control_rpc_outcome(&response))
                    .map_err(control_request_failure);
                TaskResult::SwitchAgentComplete {
                    agent_id,
                    session_id,
                    control_token,
                    agent_name,
                    result,
                }
            });
        }
        Effect::ProbeClipboardAttachment { ctx, change_count } => {
            tasks
                .spawn(async move {
                    let probe_target = ctx.target.clone();
                    let probe_text = ctx.source.text().map(str::to_owned);
                    let probe_bracketed = ctx.source.is_bracketed();
                    let probe = tokio::task::spawn_blocking(move || {
                        if change_count.is_some()
                            && crate::clipboard::clipboard_change_count() != change_count
                        {
                            return (ProbedAttachment::ProbeDropped, None);
                        }
                        if probe_bracketed
                            && crate::terminal::terminal_context()
                                .brand
                                .delivers_ime_as_bracketed_paste()
                        {
                            match crate::clipboard::bracketed_payload_came_from_clipboard_result(
                                probe_text.as_deref().unwrap_or(""),
                            ) {
                                Ok(true) => {}
                                Ok(false) => return (ProbedAttachment::ProbeDropped, None),
                                Err(_) => return (ProbedAttachment::ProbeFailed, None),
                            }
                        }
                        let (image_data, file_urls) = match crate::clipboard::system_clipboard_probe_attachments(
                            probe_text.as_deref(),
                        ) {
                            Ok(probe) => probe,
                            Err(_) => return (ProbedAttachment::ProbeFailed, None),
                        };
                        let image = match image_data {
                            Some(data) => {
                                let mut pasted = crate::prompt_images::from_clipboard_data(
                                    &data,
                                );
                                pasted.prepare_preview_blocking();
                                match &probe_target {
                                    ClipboardPasteTarget::AgentPrompt {
                                        images_dir: Some(dir),
                                        ..
                                    } => {
                                        match crate::prompt_images::persist_to_session(
                                            &mut pasted,
                                            dir,
                                        ) {
                                            Ok(()) => ProbedAttachment::Image(pasted),
                                            Err(e) => ProbedAttachment::PersistFailed(e.to_string()),
                                        }
                                    }
                                    ClipboardPasteTarget::AgentPrompt {
                                        images_dir: None,
                                        ..
                                    } => ProbedAttachment::Image(pasted),
                                    ClipboardPasteTarget::DashboardDispatch
                                    | ClipboardPasteTarget::DashboardPeek { .. } => {
                                        ProbedAttachment::Image(pasted)
                                    }
                                }
                            }
                            None => ProbedAttachment::NoRaster,
                        };
                        (image, file_urls)
                    });
                    let (image, file_urls) = match tokio::time::timeout(
                            std::time::Duration::from_secs(CLIPBOARD_PROBE_TIMEOUT_SECS),
                            probe,
                        )
                        .await
                    {
                        Ok(Ok(pair)) => pair,
                        Ok(Err(e)) => {
                            tracing::warn!(error = %e, "clipboard attachment probe task failed");
                            (ProbedAttachment::ProbeFailed, None)
                        }
                        Err(_elapsed) => {
                            tracing::warn!("clipboard attachment probe timed out");
                            (ProbedAttachment::ProbeFailed, None)
                        }
                    };
                    TaskResult::ClipboardAttachmentProbed {
                        ctx,
                        image,
                        file_urls,
                    }
                });
        }
        Effect::PreparePromptImagePreview { preparation } => {
            tasks
                .spawn(async move {
                    let preview = preparation.preview();
                    if tokio::task::spawn_blocking(move || preparation.run())
                        .await
                        .is_err()
                    {
                        preview.mark_failed();
                    }
                    TaskResult::PromptImagePreviewPrepared
                });
        }
        Effect::LoadImageViewer {
            agent_id,
            child_session_id,
            owner_id,
            path,
        } => {
            tasks.spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    crate::prompt_images::load_image_data(&path)
                })
                .await
                .unwrap_or(crate::prompt_images::ImageLoadResult::Failed);
                TaskResult::ImageViewerLoaded {
                    agent_id,
                    child_session_id,
                    owner_id,
                    result,
                }
            });
        }
        Effect::PlanDoctorFix { target, report, terminal, request } => {
            tasks
                .spawn(async move {
                    let result = tokio::task::spawn_blocking(move || match request {
                            crate::slash::command::DoctorRequest::ListFixes => {
                                Ok(
                                    actions::DoctorPlanningOutcome::Listing(
                                        crate::diagnostics::format_applicable_automatic_fixes(
                                            &report,
                                            &terminal,
                                        ),
                                    ),
                                )
                            }
                            crate::slash::command::DoctorRequest::Fix(id) => {
                                match crate::diagnostics::select_fix_plan(
                                    id,
                                    &report,
                                    &terminal,
                                ) {
                                    Ok(Some(plan)) => {
                                        Ok(actions::DoctorPlanningOutcome::Plan(Box::new(plan)))
                                    }
                                    Ok(None) => {
                                        Ok(
                                            actions::DoctorPlanningOutcome::RunLocally(
                                                crate::diagnostics::human_fix_command(id)
                                                    .unwrap_or_else(|| id.to_string()),
                                            ),
                                        )
                                    }
                                    Err(error) => Err(error.to_string()),
                                }
                            }
                            crate::slash::command::DoctorRequest::Report => {
                                unreachable!("report does not enter the planning effect")
                            }
                        })
                        .await
                        .map_err(|error| format!("Could not prepare the fix: {error}"))
                        .and_then(|result| result);
                    TaskResult::DoctorFixPlanned {
                        target,
                        result,
                    }
                });
        }
        Effect::ApplyDoctorFix { target, plan } => {
            tasks
                .spawn(async move {
                    let result = tokio::task::spawn_blocking(move || crate::diagnostics::apply_fix(
                            *plan,
                        ))
                        .await
                        .map_err(|error| format!("Could not apply the fix: {error}"))
                        .and_then(|result| result.map_err(|error| error.to_string()));
                    TaskResult::DoctorFixApplied {
                        target,
                        result,
                    }
                });
        }
        Effect::FetchChangelog => {
            tasks
                .spawn(async move {
                    let changelog = tokio::task::spawn_blocking(|| {
                            shell::util::changelog::ChangelogManager::new()
                                .fetch()
                        })
                        .await
                        .unwrap_or_else(|e| {
                            tracing::warn!(error = %e, "changelog fetch task failed");
                            shell::util::changelog::Changelog {
                                markdown: None,
                                entries: None,
                            }
                        });
                    TaskResult::ChangelogFetched {
                        markdown: changelog.markdown,
                        entries: changelog.entries.unwrap_or_default(),
                    }
                });
        }
        Effect::PersistAnnouncementsHidden { hidden_ids } => {
            tasks
                .spawn(async move {
                    announcements::write_hidden_announcement_ids(&hidden_ids)
                        .await;
                    TaskResult::AnnouncementsHiddenPersisted {
                        result: Ok(()),
                    }
                });
        }
        Effect::PersistMemoryFullscreen { fullscreen } => {
            persist_hint(
                tasks,
                "memory_modal_fullscreen",
                fullscreen,
                "memory fullscreen",
            );
        }
        Effect::PersistProjectPickerDisabled { disabled } => {
            persist_hint(
                tasks,
                "project_picker_disabled",
                disabled,
                "project picker opt-out",
            );
        }
        Effect::PersistDashboard(persisted) => {
            tasks
                .spawn(async move {
                    let result = tokio::task::spawn_blocking(move || {
                            if let Err(e) = crate::views::dashboard::state::write_persisted(
                                &persisted,
                            ) {
                                tracing::warn!(error = %e, "failed to persist dashboard config");
                            }
                        })
                        .await;
                    if let Err(e) = result {
                        tracing::warn!(error = %e, "failed to persist dashboard: join error");
                    }
                    TaskResult::CancelComplete
                });
        }
        Effect::PersistWorktreeMode { mode, config_key } => {
            debug_assert!(
                config_key == "fork_worktree_mode" || config_key == "new_session_worktree_mode",
                "unexpected worktree config_key: {config_key}"
            );
            persist_hint(tasks, config_key, mode.as_config_str(), "worktree mode");
        }
        Effect::PersistPreferredModel { model_id, reasoning_effort } => {
            let model_id_str = model_id.0.to_string();
            tasks
                .spawn(async move {
                    let result = shell::util::config::persist_models_default(
                            Some(model_id_str),
                            reasoning_effort,
                        )
                        .await
                        .map_err(|e| e.to_string());
                    if let Err(ref e) = result {
                        tracing::warn!("failed to save default model preference: {e}");
                    }
                    TaskResult::PreferredModelPersisted {
                        result,
                    }
                });
        }
        Effect::PersistPermissionMode { canonical, session_id, persist } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(
                    persist_permission_mode_and_notify(
                        canonical,
                        session_id,
                        persist,
                        tx,
                    ),
                );
        }
        Effect::NotifySessionPermissionMode {
            canonical,
            session_id,
        } => {
            let tx = acp_tx.clone();
            tasks.spawn(async move {
                let params = serde_json::json!({
                    "sessionId": session_id,
                    "permissionMode": canonical,
                });
                let notification = acp::ExtNotification::new(
                    "grow/permission_mode_changed",
                    serde_json::value::to_raw_value(&params)
                        .expect("serialize session permission mode")
                        .into(),
                );
                if let Err(error) = acp_send(notification, &tx).await {
                    tracing::warn!("Failed to set session permission mode: {error}");
                }
                TaskResult::CancelComplete
            });
        }
        Effect::PersistSetting { key, value, rollback_value } => {
            tasks
                .spawn(async move {
                    match persist_setting(key, value.clone()).await {
                        Ok(()) => {
                            TaskResult::SettingPersisted {
                                key,
                                value,
                            }
                        }
                        Err(error) => {
                            TaskResult::SettingPersistFailed {
                                key,
                                rollback_value,
                                error,
                            }
                        }
                    }
                });
        }
        Effect::FetchMcpsList { agent_id, session_id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let req = mcp_list_request(&session_id);
                    let result = match acp_send(req, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let inner = wrapper.get("result").unwrap_or(&wrapper);
                            serde_json::from_value::<
                                crate::views::mcps_modal::McpsListResponse,
                            >(inner.clone())
                                .map(crate::views::mcps_modal::convert_list_response)
                                .map_err(|_| "couldn't load server list".to_string())
                        }
                        Err(e) => {
                            Err(
                                sanitize_user_error(
                                    &format!(
                        "couldn't load server list: {e}"
                    ),
                                ),
                            )
                        }
                    };
                    TaskResult::McpsListLoaded {
                        agent_id,
                        result,
                    }
                });
        }
        Effect::McpSetupSubmit { agent_id, session_id, server_name, values } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "sessionId": session_id.0.to_string(),
                    "serverName": server_name,
                    "values": values,
                });
                    let req = acp::ExtRequest::new(
                        "grow/mcp/setup",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize mcp/setup params")
                            .into(),
                    );
                    let result = match acp_send(req, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let result_obj = wrapper.get("result");
                            if result_obj
                                .and_then(|r| r.get("ok"))
                                .and_then(|ok| ok.as_bool())
                                .unwrap_or(false)
                            {
                                Ok(())
                            } else {
                                let detail = result_obj
                                    .and_then(|r| r.get("error"))
                                    .and_then(|e| e.as_str())
                                    .map(|s| s.to_string())
                                    .unwrap_or_else(|| "setup failed".to_string());
                                Err(detail)
                            }
                        }
                        Err(e) => Err(sanitize_user_error(&format!("setup failed: {e}"))),
                    };
                    TaskResult::McpSetupSubmitDone {
                        agent_id,
                        server_name,
                        result,
                    }
                });
        }
        Effect::FetchHooksList { agent_id, session_id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "sessionId": session_id.0.to_string(),
                });
                    let req = acp::ExtRequest::new(
                        "grow/hooks/list",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize hooks/list params")
                            .into(),
                    );
                    let result = match acp_send(req, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let inner = wrapper.get("result").unwrap_or(&wrapper);
                            serde_json::from_value::<
                                extension_types::HooksListResponse,
                            >(inner.clone())
                                .map_err(|_| "couldn't load hooks".to_string())
                        }
                        Err(e) => {
                            Err(
                                sanitize_user_error(&format!("couldn't load hooks: {e}")),
                            )
                        }
                    };
                    TaskResult::HooksListLoaded {
                        agent_id,
                        result,
                    }
                });
        }
        Effect::FetchPluginsList { agent_id, session_id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "sessionId": session_id.0.to_string(),
                });
                    let req = acp::ExtRequest::new(
                        "grow/plugins/list",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize plugins/list params")
                            .into(),
                    );
                    let result = match acp_send(req, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let inner = wrapper.get("result").unwrap_or(&wrapper);
                            serde_json::from_value::<
                                extension_types::PluginsListResponse,
                            >(inner.clone())
                                .map_err(|_| "couldn't load plugins".to_string())
                        }
                        Err(e) => {
                            Err(
                                sanitize_user_error(&format!("couldn't load plugins: {e}")),
                            )
                        }
                    };
                    TaskResult::PluginsListLoaded {
                        agent_id,
                        result,
                    }
                });
        }
        Effect::HooksAction { agent_id, session_id, action } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let req_body = extension_types::HooksActionRequest {
                        session_id: session_id.0.to_string(),
                        action,
                    };
                    let req = acp::ExtRequest::new(
                        "grow/hooks/action",
                        serde_json::value::to_raw_value(&req_body)
                            .expect("serialize hooks/action params")
                            .into(),
                    );
                    let result = match acp_send(req, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let inner = wrapper.get("result").unwrap_or(&wrapper);
                            serde_json::from_value::<
                                extension_types::ActionOutcome,
                            >(inner.clone())
                                .map_err(|_| "couldn't complete hooks action".to_string())
                        }
                        Err(e) => {
                            Err(
                                sanitize_user_error(
                                    &format!(
                        "couldn't complete hooks action: {e}"
                    ),
                                ),
                            )
                        }
                    };
                    TaskResult::HooksActionResult {
                        agent_id,
                        result,
                    }
                });
        }
        Effect::PluginsAction { agent_id, session_id, action } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let req_body = extension_types::PluginsActionRequest {
                        session_id: session_id.0.to_string(),
                        action,
                    };
                    let req = acp::ExtRequest::new(
                        "grow/plugins/action",
                        serde_json::value::to_raw_value(&req_body)
                            .expect("serialize plugins/action params")
                            .into(),
                    );
                    let result = match acp_send(req, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let inner = wrapper.get("result").unwrap_or(&wrapper);
                            serde_json::from_value::<
                                extension_types::ActionOutcome,
                            >(inner.clone())
                                .map_err(|_| "couldn't complete plugins action".to_string())
                        }
                        Err(e) => {
                            Err(
                                sanitize_user_error(
                                    &format!(
                        "couldn't complete plugins action: {e}"
                    ),
                                ),
                            )
                        }
                    };
                    TaskResult::PluginsActionResult {
                        agent_id,
                        result,
                    }
                });
        }
        Effect::FetchMarketplaceList { agent_id, session_id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "sessionId": session_id.0.to_string(),
                });
                    let req = acp::ExtRequest::new(
                        "grow/marketplace/list",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize marketplace/list params")
                            .into(),
                    );
                    let result = match acp_send(req, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let inner = wrapper.get("result").unwrap_or(&wrapper);
                            serde_json::from_value::<
                                extension_types::MarketplaceListResponse,
                            >(inner.clone())
                                .map_err(|_| "couldn't load marketplace".to_string())
                        }
                        Err(e) => {
                            Err(
                                sanitize_user_error(
                                    &format!(
                        "couldn't load marketplace: {e}"
                    ),
                                ),
                            )
                        }
                    };
                    TaskResult::MarketplaceListLoaded {
                        agent_id,
                        result,
                    }
                });
        }
        Effect::FetchPluginCtaCatalog { agent_id, session_id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "sessionId": session_id.0.to_string(),
                });
                    let req = acp::ExtRequest::new(
                        "grow/marketplace/list",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize marketplace/list params")
                            .into(),
                    );
                    let result = match acp_send(req, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let inner = wrapper.get("result").unwrap_or(&wrapper);
                            serde_json::from_value::<
                                extension_types::MarketplaceListResponse,
                            >(inner.clone())
                                .map_err(|_| "couldn't load marketplace".to_string())
                        }
                        Err(e) => {
                            Err(
                                sanitize_user_error(
                                    &format!(
                        "couldn't load marketplace: {e}"
                    ),
                                ),
                            )
                        }
                    };
                    TaskResult::PluginCtaCatalogLoaded {
                        agent_id,
                        result,
                    }
                });
        }
        Effect::FetchSkillsList { agent_id, session_id: _ } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "cwd": "."
                });
                    let req = acp::ExtRequest::new(
                        "grow/skills/list",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize skills/list params")
                            .into(),
                    );
                    let result = match acp_send(req, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let inner = wrapper.get("result").unwrap_or(&wrapper);
                            serde_json::from_value::<
                                Vec<
                                    tools::implementations::skills::types::SkillInfo,
                                >,
                            >(inner.get("skills").cloned().unwrap_or_default())
                                .map_err(|_| "couldn't load skills".to_string())
                        }
                        Err(e) => {
                            Err(
                                sanitize_user_error(&format!("couldn't load skills: {e}")),
                            )
                        }
                    };
                    TaskResult::SkillsListLoaded {
                        agent_id,
                        result,
                    }
                });
        }
        Effect::FetchWorkflowsList { agent_id, session_id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "sessionId": session_id
                });
                    let req = acp::ExtRequest::new(
                        "grow/workflows/list",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize workflows/list params")
                            .into(),
                    );
                    let result = match acp_send(req, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let inner = wrapper.get("result").unwrap_or(&wrapper);
                            serde_json::from_value::<
                                Vec<crate::views::extensions_modal::WorkflowInfo>,
                            >(inner.get("workflows").cloned().unwrap_or_default())
                                .map_err(|_| "couldn't load workflows".to_string())
                        }
                        Err(e) => {
                            Err(
                                sanitize_user_error(
                                    &format!(
                        "couldn't load workflows: {e}"
                    ),
                                ),
                            )
                        }
                    };
                    TaskResult::WorkflowsListLoaded {
                        agent_id,
                        session_id,
                        result,
                    }
                });
        }
        Effect::ToggleSkill { agent_id, session_id: _, skill_name, enabled } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "name": skill_name,
                    "enabled": enabled,
                    "cwd": ".",
                });
                    let req = acp::ExtRequest::new(
                        "grow/skills/toggle",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize skills/toggle params")
                            .into(),
                    );
                    let result = match acp_send(req, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let inner = wrapper.get("result").unwrap_or(&wrapper);
                            let parsed = serde_json::from_value::<
                                Vec<
                                    tools::implementations::skills::types::SkillInfo,
                                >,
                            >(inner.get("skills").cloned().unwrap_or_default())
                                .map_err(|_| "couldn't toggle skill".to_string());
                            if parsed.is_ok() {
                                let refresh = acp::ExtRequest::new(
                                    "grow/skills/refresh-baseline",
                                    serde_json::value::to_raw_value(&serde_json::json!({}))
                                        .expect("serialize empty params")
                                        .into(),
                                );
                                let _ = acp_send(refresh, &tx).await;
                            }
                            parsed
                        }
                        Err(e) => {
                            Err(
                                sanitize_user_error(&format!("couldn't toggle skill: {e}")),
                            )
                        }
                    };
                    TaskResult::SkillsToggleDone {
                        agent_id,
                        result,
                    }
                });
        }
        Effect::CheckMarketplaceUpdates { agent_id, session_id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "sessionId": session_id.0.to_string(),
                });
                    let list_req = acp::ExtRequest::new(
                        "grow/marketplace/list",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize marketplace/list params")
                            .into(),
                    );
                    let outdated = match acp_send(list_req, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let inner = wrapper.get("result").unwrap_or(&wrapper);
                            serde_json::from_value::<
                                extension_types::MarketplaceListResponse,
                            >(inner.clone())
                                .ok()
                                .map(|r| {
                                    r
                                        .sources
                                        .iter()
                                        .flat_map(|s| {
                                            let source_url = s.source_url_or_path.clone();
                                            s.plugins
                                                .iter()
                                                .filter_map(move |p| {
                                                    if p.install_status == "update_available" {
                                                        Some((
                                                            p.name.clone(),
                                                            p.installed_version.clone().unwrap_or_else(|| "?".into()),
                                                            p.version.clone().unwrap_or_else(|| "?".into()),
                                                            source_url.clone(),
                                                            p.relative_path.clone(),
                                                        ))
                                                    } else {
                                                        None
                                                    }
                                                })
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default()
                        }
                        Err(_) => Vec::new(),
                    };
                    if outdated.is_empty() {
                        return TaskResult::MarketplaceUpdatesAvailable {
                            agent_id,
                            updates: Vec::new(),
                        };
                    }
                    let mut succeeded = Vec::new();
                    for (name, old, new, source_url, rel_path) in outdated {
                        let action = extension_types::MarketplaceAction::Update {
                            source_url_or_path: source_url.clone(),
                            plugin_relative_path: rel_path.clone(),
                        };
                        let req_body = extension_types::MarketplaceActionRequest {
                            session_id: session_id.0.to_string(),
                            action,
                        };
                        let update_req = acp::ExtRequest::new(
                            "grow/marketplace/action",
                            serde_json::value::to_raw_value(&req_body)
                                .expect("serialize marketplace/action params")
                                .into(),
                        );
                        let update_succeeded = match acp_send(update_req, &tx).await {
                            Ok(resp) => {
                                let wrapper: serde_json::Value = serde_json::from_str(
                                        resp.0.get(),
                                    )
                                    .unwrap_or_default();
                                let inner = wrapper.get("result").unwrap_or(&wrapper);
                                serde_json::from_value::<
                                    extension_types::ActionOutcome,
                                >(inner.clone())
                                    .is_ok_and(|outcome| marketplace_outcome_succeeded(
                                        &outcome,
                                    ))
                            }
                            Err(_) => false,
                        };
                        if update_succeeded {
                            succeeded.push((name, old, new));
                        }
                    }
                    if !succeeded.is_empty() {
                        let notify_params = serde_json::json!({
                        "sessionId": session_id.0.to_string(),
                        "updates": succeeded,
                    });
                        let notify_req = acp::ExtRequest::new(
                            "grow/plugins/notify-updates",
                            serde_json::value::to_raw_value(&notify_params)
                                .expect("serialize notify-updates params")
                                .into(),
                        );
                        let _ = acp_send(notify_req, &tx).await;
                    }
                    TaskResult::MarketplaceUpdatesAvailable {
                        agent_id,
                        updates: succeeded,
                    }
                });
        }
        Effect::MarketplaceAction { agent_id, session_id, action } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let req_body = extension_types::MarketplaceActionRequest {
                        session_id: session_id.0.to_string(),
                        action,
                    };
                    let req = acp::ExtRequest::new(
                        "grow/marketplace/action",
                        serde_json::value::to_raw_value(&req_body)
                            .expect("serialize marketplace/action params")
                            .into(),
                    );
                    let result = match acp_send(req, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let inner = wrapper.get("result").unwrap_or(&wrapper);
                            serde_json::from_value::<
                                extension_types::ActionOutcome,
                            >(inner.clone())
                                .map_err(|e| {
                                    tracing::debug!("failed to parse marketplace action response: {e}");
                                    "couldn't complete marketplace action".to_string()
                                })
                        }
                        Err(e) => {
                            Err(
                                sanitize_user_error(
                                    &format!(
                        "couldn't complete marketplace action: {e}"
                    ),
                                ),
                            )
                        }
                    };
                    TaskResult::MarketplaceActionResult {
                        agent_id,
                        result,
                    }
                });
        }
        Effect::InstallPluginFromCta {
            agent_id,
            session_id,
            source_url_or_path,
            plugin_relative_path,
        } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let plugin_name = plugin_relative_path
                        .rsplit('/')
                        .next()
                        .unwrap_or(plugin_relative_path.as_str())
                        .to_string();
                    let action = extension_types::MarketplaceAction::Install {
                        source_url_or_path,
                        plugin_relative_path,
                    };
                    let req_body = extension_types::MarketplaceActionRequest {
                        session_id: session_id.0.to_string(),
                        action,
                    };
                    let req = acp::ExtRequest::new(
                        "grow/marketplace/action",
                        serde_json::value::to_raw_value(&req_body)
                            .expect("serialize marketplace/action params")
                            .into(),
                    );
                    let result = match acp_send(req, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let inner = wrapper.get("result").unwrap_or(&wrapper);
                            serde_json::from_value::<
                                extension_types::ActionOutcome,
                            >(inner.clone())
                                .map_err(|e| {
                                    tracing::debug!("failed to parse marketplace action response: {e}");
                                    "couldn't complete marketplace action".to_string()
                                })
                        }
                        Err(e) => {
                            Err(
                                sanitize_user_error(
                                    &format!(
                        "couldn't complete marketplace action: {e}"
                    ),
                                ),
                            )
                        }
                    };
                    TaskResult::CtaPluginInstallDone {
                        agent_id,
                        plugin_name,
                        result,
                    }
                });
        }
        Effect::ReloadPluginsForCta { agent_id, session_id, plugin_name } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let req_body = extension_types::PluginsActionRequest {
                        session_id: session_id.0.to_string(),
                        action: extension_types::PluginsAction::Reload,
                    };
                    let req = acp::ExtRequest::new(
                        "grow/plugins/action",
                        serde_json::value::to_raw_value(&req_body)
                            .expect("serialize plugins/action params")
                            .into(),
                    );
                    let result = match acp_send(req, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let inner = wrapper.get("result").unwrap_or(&wrapper);
                            serde_json::from_value::<
                                extension_types::ActionOutcome,
                            >(inner.clone())
                                .map_err(|_| "couldn't complete plugins action".to_string())
                        }
                        Err(e) => {
                            Err(
                                sanitize_user_error(
                                    &format!(
                        "couldn't complete plugins action: {e}"
                    ),
                                ),
                            )
                        }
                    };
                    TaskResult::CtaPluginReloadDone {
                        agent_id,
                        plugin_name,
                        result,
                    }
                });
        }
        Effect::FetchPluginCtaMcps { agent_id, session_id, plugin_name } => {
            let tx = acp_tx.clone();
            tasks.spawn(fetch_plugin_cta_mcps(agent_id, session_id, plugin_name, tx));
        }
        Effect::RetryPluginCtaMcps { agent_id, session_id, plugin_name } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    tokio::time::sleep(
                            std::time::Duration::from_millis(CTA_MCP_RETRY_DELAY_MS),
                        )
                        .await;
                    fetch_plugin_cta_mcps(agent_id, session_id, plugin_name, tx).await
                });
        }
        Effect::DismissCtaInstalled { agent_id, plugin_name } => {
            tasks
                .spawn(async move {
                    tokio::time::sleep(
                            std::time::Duration::from_millis(CTA_INSTALLED_DISMISS_MS),
                        )
                        .await;
                    TaskResult::CtaInstalledDismissTimeout {
                        agent_id,
                        plugin_name,
                    }
                });
        }
        Effect::UpsertMcpServer { agent_id, session_id, name, config } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    #[derive(serde::Serialize)]
                    struct McpUpsertRequest {
                        session_id: String,
                        server_name: String,
                        #[serde(flatten)]
                        config: shell::util::config::McpServerConfig,
                    }
                    let req_body = McpUpsertRequest {
                        session_id: session_id.0.to_string(),
                        server_name: name,
                        config: *config,
                    };
                    let req = acp::ExtRequest::new(
                        "grow/mcp/upsert",
                        serde_json::value::to_raw_value(&req_body)
                            .expect("serialize mcp/upsert params")
                            .into(),
                    );
                    let result = match acp_send(req, &tx).await {
                        Ok(_) => Ok(()),
                        Err(e) => {
                            Err(
                                sanitize_user_error(
                                    &format!(
                        "couldn't save server config: {e}"
                    ),
                                ),
                            )
                        }
                    };
                    TaskResult::McpToggleDone {
                        agent_id,
                        result,
                    }
                });
        }
        Effect::DeleteMcpServer { agent_id, session_id, server_name } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    #[derive(serde::Serialize)]
                    struct McpDeleteRequest {
                        session_id: String,
                        server_name: String,
                    }
                    let req_body = McpDeleteRequest {
                        session_id: session_id.0.to_string(),
                        server_name,
                    };
                    let req = acp::ExtRequest::new(
                        "grow/mcp/delete",
                        serde_json::value::to_raw_value(&req_body)
                            .expect("serialize mcp/delete params")
                            .into(),
                    );
                    let result = match acp_send(req, &tx).await {
                        Ok(_) => Ok(()),
                        Err(e) => {
                            Err(
                                sanitize_user_error(&format!("couldn't delete server: {e}")),
                            )
                        }
                    };
                    TaskResult::McpToggleDone {
                        agent_id,
                        result,
                    }
                });
        }
        Effect::ToggleMcpServer { agent_id, session_id, server_name, enabled } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "session_id": session_id.0.to_string(),
                    "server_name": server_name,
                    "enabled": enabled,
                });
                    let req = acp::ExtRequest::new(
                        "grow/mcp/toggle",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize mcp/toggle params")
                            .into(),
                    );
                    let result = match acp_send(req, &tx).await {
                        Ok(_) => Ok(()),
                        Err(e) => Err(format_acp_error(&e)),
                    };
                    TaskResult::McpToggleDone {
                        agent_id,
                        result,
                    }
                });
        }
        Effect::ToggleMcpTool {
            agent_id,
            session_id,
            server_name,
            tool_name,
            enabled,
        } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "session_id": session_id.0.to_string(),
                    "server_name": server_name,
                    "tool_name": tool_name,
                    "enabled": enabled,
                });
                    let req = acp::ExtRequest::new(
                        "grow/mcp/toggle_tool",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize mcp/toggle_tool params")
                            .into(),
                    );
                    let result = match acp_send(req, &tx).await {
                        Ok(_) => Ok(()),
                        Err(e) => {
                            Err(
                                sanitize_user_error(&format!("couldn't toggle tool: {e}")),
                            )
                        }
                    };
                    TaskResult::McpToggleDone {
                        agent_id,
                        result,
                    }
                });
        }
        Effect::FetchSessionAgentName { agent_id, session_id, revision } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    match fetch_session_info(&session_id, &tx).await {
                        Ok(info) => {
                            TaskResult::SessionAgentNameResolved {
                                agent_id,
                                session_id: session_id.clone(),
                                revision,
                                agent_name: info.data.agent_name,
                            }
                        }
                        Err(e) => {
                            tracing::debug!("session agent name fetch failed: {e}");
                            TaskResult::SessionAgentNameResolved {
                                agent_id,
                                session_id,
                                revision,
                                agent_name: None,
                            }
                        }
                    }
                });
        }
        Effect::ShowSessionInfo { agent_id, session_id, revision, show_resolved_model, nonce } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    match fetch_session_info(&session_id, &tx).await {
                        Ok(info) => {
                            let title = lookup_session_title(&session_id).await;
                            let text = format_session_info(
                                &info,
                                title.as_deref(),
                                show_resolved_model,
                            );
                            TaskResult::SessionInfoComplete {
                                agent_id,
                                session_id: session_id.clone(),
                                revision,
                                info: Box::new(info),
                                text,
                                title,
                                show_resolved_model,
                                nonce,
                            }
                        }
                        Err(error) => {
                            TaskResult::SessionInfoFailed {
                                agent_id,
                                error,
                                nonce,
                            }
                        }
                    }
                });
        }
        Effect::RenameSession { agent_id, session_id, title, cwd } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    #[derive(serde::Serialize)]
                    #[serde(rename_all = "camelCase")]
                    struct RenameRequest {
                        session_id: String,
                        title: String,
                        cwd: String,
                    }
                    let request = acp::ExtRequest::new(
                        "grow/session/rename",
                        serde_json::value::to_raw_value(
                                &RenameRequest {
                                    session_id: session_id.0.to_string(),
                                    title: title.clone(),
                                    cwd: cwd.to_string_lossy().to_string(),
                                },
                            )
                            .expect("serialize rename params")
                            .into(),
                    );
                    match acp_send(request, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            if let Some(err) = wrapper
                                .get("error")
                                .filter(|v| !v.is_null())
                            {
                                let msg = err
                                    .as_str()
                                    .map(String::from)
                                    .unwrap_or_else(|| err.to_string());
                                return TaskResult::RenameSessionFailed {
                                    agent_id,
                                    error: msg,
                                };
                            }
                            TaskResult::RenameSessionComplete {
                                agent_id,
                                title,
                            }
                        }
                        Err(e) => {
                            TaskResult::RenameSessionFailed {
                                agent_id,
                                error: sanitize_user_error(
                                    &format!("couldn't rename session: {e}"),
                                ),
                            }
                        }
                    }
                });
        }
        Effect::DeleteSession { session_id, cwd, after } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    #[derive(serde::Serialize)]
                    #[serde(rename_all = "camelCase")]
                    struct DeleteRequest {
                        session_id: String,
                        cwd: String,
                    }
                    let request = acp::ExtRequest::new(
                        "grow/session/delete",
                        serde_json::value::to_raw_value(
                                &DeleteRequest {
                                    session_id: session_id.clone(),
                                    cwd,
                                },
                            )
                            .expect("serialize delete params")
                            .into(),
                    );
                    match acp_send(request, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            if let Some(err) = wrapper
                                .get("error")
                                .filter(|v| !v.is_null())
                            {
                                let msg = err
                                    .as_str()
                                    .map(String::from)
                                    .unwrap_or_else(|| err.to_string());
                                return TaskResult::DeleteSessionFailed {
                                    session_id,
                                    error: msg,
                                };
                            }
                            TaskResult::DeleteSessionComplete {
                                session_id,
                                after,
                            }
                        }
                        Err(e) => {
                            TaskResult::DeleteSessionFailed {
                                session_id,
                                error: sanitize_user_error(
                                    &format!("couldn't delete session: {e}"),
                                ),
                            }
                        }
                    }
                });
        }
        Effect::ShowContextInfo { agent_id, session_id, nonce } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    match fetch_session_info(&session_id, &tx).await {
                        Ok(info) => {
                            TaskResult::ContextInfoComplete {
                                agent_id,
                                info: Box::new(info),
                                nonce,
                            }
                        }
                        Err(error) => {
                            TaskResult::ContextInfoFailed {
                                agent_id,
                                error,
                                nonce,
                            }
                        }
                    }
                });
        }
        Effect::FetchSessionUsage { agent_id, session_id, nonce } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    match fetch_session_usage(&session_id, &tx).await {
                        Ok(usage) => {
                            TaskResult::SessionUsageComplete {
                                agent_id,
                                session_id,
                                usage: Box::new(usage),
                                nonce,
                            }
                        }
                        Err(error) => {
                            TaskResult::SessionUsageFailed {
                                agent_id,
                                session_id,
                                error,
                                nonce,
                            }
                        }
                    }
                });
        }
        Effect::RewriteMemoryNote {
            agent_id,
            session_id,
            raw_text,
            context_summary,
            nonce,
        } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let request = acp::ExtRequest::new(
                        "grow/memory/rewrite",
                        serde_json::value::to_raw_value(
                                &serde_json::json!({
                        "sessionId": session_id.0.to_string(),
                        "rawText": raw_text,
                        "contextSummary": context_summary,
                    }),
                            )
                            .expect("serialize memory/rewrite params")
                            .into(),
                    );
                    match acp_send(request, &tx).await {
                        Ok(resp) => {
                            let parsed: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let text = parsed
                                .get("result")
                                .and_then(|r| r.get("rewritten"))
                                .or_else(|| parsed.get("rewritten"))
                                .and_then(|v| v.as_str())
                                .map(String::from)
                                .unwrap_or(raw_text);
                            TaskResult::MemoryNoteRewritten {
                                agent_id,
                                result: Ok(text),
                                nonce,
                            }
                        }
                        Err(_) => {
                            TaskResult::MemoryNoteRewritten {
                                agent_id,
                                result: Ok(raw_text),
                                nonce,
                            }
                        }
                    }
                });
        }
        Effect::SaveMemoryNote { agent_id, text, cwd } => {
            tasks
                .spawn(async move {
                    let result = tokio::task::spawn_blocking(move || {
                            let storage = memory::MemoryStorage::new(&cwd, None);
                            storage
                                .append_to_memory(memory::MemoryScope::Global, &text)
                        })
                        .await
                        .map_err(|e| format!("task join error: {e}"))
                        .and_then(|r| r.map_err(|e| format!("{e}")));
                    TaskResult::MemoryNoteSaved {
                        agent_id,
                        result,
                    }
                });
        }
        Effect::SendBtw { agent_id, session_id, question, minimal_request_id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let request = acp::ExtRequest::new(
                        "grow/btw",
                        serde_json::value::to_raw_value(
                                &serde_json::json!({
                        "sessionId": session_id.0.to_string(),
                        "question": question,
                    }),
                            )
                            .expect("serialize btw params")
                            .into(),
                    );
                    match acp_send(request, &tx).await {
                        Ok(resp) => {
                            let parsed: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let answer = parsed
                                .get("result")
                                .and_then(|r| r.get("answer"))
                                .and_then(|a| a.as_str())
                                .unwrap_or("No response")
                                .to_string();
                            TaskResult::BtwResponse {
                                agent_id,
                                result: Ok(answer),
                                minimal_request_id,
                            }
                        }
                        Err(e) => {
                            TaskResult::BtwResponse {
                                agent_id,
                                result: Err(
                                    sanitize_user_error(&format!("side question failed: {e}")),
                                ),
                                minimal_request_id,
                            }
                        }
                    }
                });
        }
        Effect::SendRecap { session_id, auto } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let request = acp::ExtRequest::new(
                        "grow/recap",
                        serde_json::value::to_raw_value(
                                &serde_json::json!({
                        "sessionId": session_id.0.to_string(),
                        "auto": auto,
                    }),
                            )
                            .expect("serialize recap params")
                            .into(),
                    );
                    match acp_send(request, &tx).await {
                        Ok(_) => {
                            TaskResult::RecapRequested {
                                session_id,
                                auto,
                                error: None,
                            }
                        }
                        Err(e) => {
                            TaskResult::RecapRequested {
                                session_id,
                                auto,
                                error: Some(format!("recap request failed: {e}")),
                            }
                        }
                    }
                });
        }
        Effect::SendInterject {
            agent_id,
            session_id,
            expected_turn_id,
            text,
            interjection_id,
            blocks,
        } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = build_interject_params(
                        &session_id,
                        &expected_turn_id,
                        &text,
                        &interjection_id,
                        blocks.as_deref(),
                    );
                    let request = acp::ExtRequest::new(
                        "grow/steer",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize interject params")
                            .into(),
                    );
                    match acp_send(request, &tx).await {
                        Ok(_) => {
                            TaskResult::InterjectQueued {
                                agent_id,
                            }
                        }
                        Err(e) => {
                            TaskResult::InterjectFailed {
                                agent_id,
                                error: sanitize_user_error(
                                    &format!("couldn't send interjection: {e}"),
                                ),
                                text,
                                blocks,
                            }
                        }
                    }
                });
        }
        Effect::ExecuteSlashCommand {
            agent_id,
            session_id,
            request,
        } => {
            let tx = acp_tx.clone();
            tasks.spawn(async move {
                let params = shell::extensions::session_admin::ExecuteCommandRequest {
                    session_id: session_id.0.to_string(),
                    command: request.command.clone(),
                    description: request.description.clone(),
                    invocation_id: request.invocation_id.clone(),
                };
                let ext_request = acp::ExtRequest::new(
                    "grow/commands/execute",
                    serde_json::value::to_raw_value(&params)
                        .expect("serialize commands/execute params")
                        .into(),
                );
                let error = acp_send(ext_request, &tx).await.err().map(|error| {
                    sanitize_user_error(&format!("couldn't execute command: {error}"))
                });
                TaskResult::SlashCommandExecuted {
                    agent_id,
                    session_id,
                    request,
                    error,
                }
            });
        }
        Effect::QueryPromptStatus {
            agent_id,
            session_id,
            prompt_id,
        } => {
            let tx = acp_tx.clone();
            tasks.spawn(async move {
                let params = serde_json::json!({
                    "sessionId": session_id.0.to_string(),
                    "promptId": prompt_id,
                });
                let request = acp::ExtRequest::new(
                    "grow/queue/prompt_status",
                    serde_json::value::to_raw_value(&params)
                        .expect("serialize prompt status params")
                        .into(),
                );
                let status = match acp_send(request, &tx).await {
                    Ok(response) => serde_json::from_str(response.0.get()).map_err(|error| {
                        sanitize_user_error(&format!("invalid prompt status response: {error}"))
                    }),
                    Err(error) => Err(sanitize_user_error(&format!(
                        "couldn't reconcile prompt status: {error}"
                    ))),
                };
                TaskResult::PromptStatusResolved {
                    agent_id,
                    prompt_id,
                    status,
                }
            });
        }
        Effect::FetchCatalogEntry { kind, name } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({ "kind": kind, "name": name });
                    let request = acp::ExtRequest::new(
                        "grow/bundle/entry/get",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize bundle/entry/get params")
                            .into(),
                    );
                    match acp_send(request, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            if let Some(err) = wrapper.get("error") {
                                let msg = err
                                    .as_str()
                                    .map(String::from)
                                    .unwrap_or_else(|| "unknown error".to_string());
                                return TaskResult::CatalogEntryFailed {
                                    error: msg,
                                };
                            }
                            let inner = wrapper.get("result").unwrap_or(&wrapper);
                            match serde_json::from_value::<
                                super::super::bundle::EntryGetResult,
                            >(inner.clone()) {
                                Ok(r) => {
                                    TaskResult::CatalogEntryReady {
                                        kind: r.kind,
                                        name: r.name,
                                        content: r.content,
                                    }
                                }
                                Err(e) => {
                                    tracing::debug!("failed to parse catalog entry response: {e}");
                                    TaskResult::CatalogEntryFailed {
                                        error: "couldn't load entry".to_string(),
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            TaskResult::CatalogEntryFailed {
                                error: sanitize_user_error(
                                    &format!("couldn't load entry: {e}"),
                                ),
                            }
                        }
                    }
                });
        }
        Effect::FetchBundleStatus => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let request = acp::ExtRequest::new(
                        "grow/bundle/status",
                        serde_json::value::to_raw_value(&serde_json::json!({}))
                            .expect("serialize bundle/status params")
                            .into(),
                    );
                    match acp_send(request, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            if let Some(err) = wrapper.get("error") {
                                let msg = err
                                    .as_str()
                                    .map(String::from)
                                    .unwrap_or_else(|| "unknown error".to_string());
                                return TaskResult::BundleStatusFailed {
                                    error: msg,
                                };
                            }
                            let inner = wrapper.get("result").unwrap_or(&wrapper);
                            match serde_json::from_value::<
                                super::super::bundle::BundleStatusResult,
                            >(inner.clone()) {
                                Ok(r) => {
                                    TaskResult::BundleStatusReady {
                                        has_cache: r.has_cache,
                                        version: r.version,
                                        agents: r.agents,
                                        skills: r.skills,
                                    }
                                }
                                Err(_) => {
                                    TaskResult::BundleStatusFailed {
                                        error: "couldn't fetch bundle status".to_string(),
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            TaskResult::BundleStatusFailed {
                                error: sanitize_user_error(
                                    &format!("couldn't fetch bundle status: {e}"),
                                ),
                            }
                        }
                    }
                });
        }
        Effect::RefreshAvailableCommands { agent_id, session_id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({ "sessionId": session_id });
                    let req = acp::ExtRequest::new(
                        "grow/commands/list",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize commands/list params")
                            .into(),
                    );
                    match acp_send(req, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let inner = wrapper.get("result").unwrap_or(&wrapper);
                            let commands: Vec<acp::AvailableCommand> = inner
                                .get("commands")
                                .and_then(|v| serde_json::from_value(v.clone()).ok())
                                .unwrap_or_default();
                            TaskResult::AvailableCommandsRefreshed {
                                agent_id,
                                commands,
                            }
                        }
                        Err(e) => {
                            tracing::warn!("commands/list refresh failed: {e}");
                            TaskResult::AvailableCommandsRefreshed {
                                agent_id,
                                commands: vec![],
                            }
                        }
                    }
                });
        }
        Effect::FetchRewindPoints { agent_id, session_id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let request = acp::ExtRequest::new(
                        "grow/rewind/points",
                        serde_json::value::to_raw_value(
                                &serde_json::json!({
                        "sessionId": session_id.0.to_string()
                    }),
                            )
                            .expect("serialize rewind/points params")
                            .into(),
                    );
                    match acp_send(request, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            if let Some(err) = wrapper
                                .get("error")
                                .filter(|v| !v.is_null())
                            {
                                return TaskResult::RewindPointsFailed {
                                    agent_id,
                                    error: err.as_str().unwrap_or("unknown error").to_string(),
                                };
                            }
                            let result_val = wrapper
                                .get("result")
                                .cloned()
                                .unwrap_or(wrapper.clone());
                            match serde_json::from_value::<
                                crate::views::rewind::RewindPointsResponse,
                            >(result_val) {
                                Ok(r) => {
                                    TaskResult::RewindPointsLoaded {
                                        agent_id,
                                        points: r.rewind_points,
                                    }
                                }
                                Err(e) => {
                                    TaskResult::RewindPointsFailed {
                                        agent_id,
                                        error: format!("invalid response: {e}"),
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            TaskResult::RewindPointsFailed {
                                agent_id,
                                error: sanitize_user_error(&e.to_string()),
                            }
                        }
                    }
                });
        }
        Effect::RewindPreview { agent_id, session_id, target_prompt_index, mode } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let request = acp::ExtRequest::new(
                        "grow/rewind/execute",
                        serde_json::value::to_raw_value(
                                &serde_json::json!({
                        "sessionId": session_id.0.to_string(),
                        "targetPromptIndex": target_prompt_index,
                        "force": false,
                        "mode": mode.wire_value(),
                    }),
                            )
                            .expect("serialize rewind/execute preview params")
                            .into(),
                    );
                    match acp_send(request, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let result_val = wrapper
                                .get("result")
                                .cloned()
                                .unwrap_or(wrapper.clone());
                            match serde_json::from_value::<
                                crate::views::rewind::RewindResponse,
                            >(result_val) {
                                Ok(r) => {
                                    TaskResult::RewindPreviewComplete {
                                        agent_id,
                                        response: r,
                                        target_prompt_index,
                                        mode,
                                    }
                                }
                                Err(e) => {
                                    TaskResult::RewindPreviewFailed {
                                        agent_id,
                                        error: format!("invalid response: {e}"),
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            TaskResult::RewindPreviewFailed {
                                agent_id,
                                error: sanitize_user_error(&e.to_string()),
                            }
                        }
                    }
                });
        }
        Effect::RewindExecute { agent_id, session_id, target_prompt_index, mode } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let request = acp::ExtRequest::new(
                        "grow/rewind/execute",
                        serde_json::value::to_raw_value(
                                &serde_json::json!({
                        "sessionId": session_id.0.to_string(),
                        "targetPromptIndex": target_prompt_index,
                        "force": true,
                        "mode": mode.wire_value(),
                    }),
                            )
                            .expect("serialize rewind/execute params")
                            .into(),
                    );
                    match acp_send(request, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let result_val = wrapper
                                .get("result")
                                .cloned()
                                .unwrap_or(wrapper.clone());
                            match serde_json::from_value::<
                                crate::views::rewind::RewindResponse,
                            >(result_val) {
                                Ok(r) => {
                                    TaskResult::RewindExecuteComplete {
                                        agent_id,
                                        response: r,
                                    }
                                }
                                Err(e) => {
                                    TaskResult::RewindExecuteFailed {
                                        agent_id,
                                        error: format!("invalid response: {e}"),
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            TaskResult::RewindExecuteFailed {
                                agent_id,
                                error: sanitize_user_error(&e.to_string()),
                            }
                        }
                    }
                });
        }
        Effect::DeepSearchSessions { query, seq } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let deadline = tokio::time::Instant::now()
                        + std::time::Duration::from_secs(30);
                    let retry_interval = std::time::Duration::from_secs(3);
                    let mut results = Vec::new();
                    loop {
                        let params = serde_json::json!({
                        "query": query,
                        "limit": 20,
                        "includeContent": true,
                    });
                        let request = acp::ExtRequest::new(
                            "grow/session/search",
                            serde_json::value::to_raw_value(&params)
                                .expect("serialize deep search params")
                                .into(),
                        );
                        let remaining = deadline
                            .saturating_duration_since(tokio::time::Instant::now());
                        if remaining.is_zero() {
                            break;
                        }
                        let result = tokio::time::timeout(
                                remaining,
                                acp_send(request, &tx),
                            )
                            .await;
                        match result {
                            Ok(Ok(resp)) => {
                                let wrapper: serde_json::Value = serde_json::from_str(
                                        resp.0.get(),
                                    )
                                    .unwrap_or_default();
                                let payload = wrapper.get("result").unwrap_or(&wrapper);
                                if let Some(hits) = payload.get("results") {
                                    results = serde_json::from_value::<
                                        Vec<
                                            shell::extensions::session_search::SearchSessionHit,
                                        >,
                                    >(hits.clone())
                                        .unwrap_or_default();
                                }
                                let bootstrapping = payload
                                    .get("bootstrapping")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                if !bootstrapping {
                                    break;
                                }
                            }
                            Ok(Err(e)) => {
                                tracing::warn!("deep search failed: {e}");
                                break;
                            }
                            Err(_) => {
                                tracing::warn!("deep search timed out");
                                break;
                            }
                        }
                        tokio::time::sleep(retry_interval).await;
                    }
                    TaskResult::DeepSearchResults {
                        results,
                        seq,
                    }
                });
        }
        Effect::ForkSession {
            agent_id,
            parent_session_id,
            parent_cwd,
            parent_is_worktree,
            new_session_id,
        } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let sid_str = parent_session_id.0.to_string();
                    let parent_cwd_str = parent_cwd.to_string_lossy().into_owned();
                    if let Some(ref nid) = new_session_id
                        && let Err(e) = crate::app::session_startup::ensure_session_id_available(
                            nid,
                            &parent_cwd_str,
                        )
                    {
                        return TaskResult::ForkSessionFailed {
                            agent_id,
                            error: sanitize_user_error(&e.to_string()),
                        };
                    }
                    let payload = crate::app::session_startup::fork_session_params(
                        &sid_str,
                        &parent_cwd,
                        new_session_id.as_deref(),
                        parent_is_worktree,
                    );
                    let req = acp::ExtRequest::new(
                        "grow/session/fork",
                        serde_json::value::to_raw_value(&payload)
                            .expect("serialize fork params")
                            .into(),
                    );
                    match acp_send(req, &tx).await {
                        Ok(resp) => {
                            if let Some(err) = crate::app::session_startup::fork_response_error(
                                resp.0.get(),
                            ) {
                                let msg = err.trim_matches('"').to_string();
                                return TaskResult::ForkSessionFailed {
                                    agent_id,
                                    error: sanitize_user_error(&format!("fork failed: {msg}")),
                                };
                            }
                            match crate::app::session_startup::fork_response_new_session_id(
                                resp.0.get(),
                            ) {
                                Some(sid) => {
                                    TaskResult::ForkSessionReady {
                                        agent_id,
                                        new_session_id: acp::SessionId::new(sid),
                                        cwd: parent_cwd,
                                    }
                                }
                                None => {
                                    TaskResult::ForkSessionFailed {
                                        agent_id,
                                        error: "fork response missing newSessionId".into(),
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            TaskResult::ForkSessionFailed {
                                agent_id,
                                error: sanitize_user_error(&format!("fork failed: {e}")),
                            }
                        }
                    }
                });
        }
        Effect::HydrateSessionTitleFromDisk { agent_id, session_id, cwd } => {
            tasks
                .spawn(async move {
                    let info = shell::session::info::Info {
                        id: session_id,
                        cwd: cwd.to_string_lossy().to_string(),
                    };
                    let session_dir = shell::session::persistence::session_dir(&info);
                    let title = tokio::task::spawn_blocking(move || -> Option<
                            (String, bool),
                        > {
                            let summary = shell::session::persistence::read_summary_in_session_dir(
                                &session_dir,
                            )
                            .ok()?;
                            let manual = summary.manual_title_opt();
                            let is_manual = manual.is_some();
                            let title = manual.or_else(|| summary.display_title_opt())?;
                            Some((title, is_manual))
                        })
                        .await
                        .ok()
                        .flatten();
                    TaskResult::SessionTitleFromDisk {
                        agent_id,
                        title,
                    }
                });
        }
        Effect::DebounceSuggestions { agent_id, generation } => {
            tasks
                .spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    TaskResult::SuggestionDebounceExpired {
                        agent_id,
                        generation,
                    }
                });
        }
        Effect::DebouncePluginCta { agent_id, generation } => {
            tasks
                .spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    TaskResult::PluginCtaDebounceExpired {
                        agent_id,
                        generation,
                    }
                });
        }
        Effect::FetchShellSuggestions {
            agent_id,
            text,
            cursor,
            cwd,
            generation,
            limit,
            include_ai,
            ai_model,
            session_id,
            token_only,
        } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    // By reference: echoed back below as `request_text`.
                    "text": &text,
                    "cursor": cursor,
                    "cwd": cwd,
                    "includeAi": include_ai,
                    "aiModel": ai_model,
                    "sessionId": session_id,
                    "limit": limit,
                    "generation": generation,
                    "tokenOnly": token_only,
                });
                    let req = acp::ExtRequest::new(
                        "grow/suggest",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize suggest params")
                            .into(),
                    );
                    match acp_send(req, &tx).await {
                        Ok(resp) => {
                            let wrapper: serde_json::Value = serde_json::from_str(
                                    resp.0.get(),
                                )
                                .unwrap_or_default();
                            let parsed = crate::views::suggestion_controller::SuggestResponseParsed::from_json(
                                &wrapper,
                            );
                            match parsed {
                                Some(response) => {
                                    TaskResult::ShellSuggestionsLoaded {
                                        agent_id,
                                        response,
                                        request_text: text,
                                        request_cursor: cursor,
                                    }
                                }
                                None => TaskResult::CancelComplete,
                            }
                        }
                        Err(_) => TaskResult::CancelComplete,
                    }
                });
        }
        Effect::FetchPromptSuggestion { agent_id, generation, model, session_id } => {
            let tx = acp_tx.clone();
            tasks
                .spawn(async move {
                    let params = serde_json::json!({
                    "generation": generation,
                    "model": model,
                    "sessionId": session_id,
                });
                    let req = acp::ExtRequest::new(
                        "grow/suggestPrompt",
                        serde_json::value::to_raw_value(&params)
                            .expect("serialize suggestPrompt params")
                            .into(),
                    );
                    let suggestion = match acp_send(req, &tx).await {
                        Ok(resp) => {
                            serde_json::from_str::<serde_json::Value>(resp.0.get())
                                .ok()
                                .as_ref()
                                .map(|v| v.get("result").unwrap_or(v))
                                .and_then(|r| r.get("suggestion"))
                                .and_then(|s| s.as_str())
                                .map(str::to_owned)
                        }
                        Err(_) => None,
                    };
                    TaskResult::PromptSuggestionLoaded {
                        agent_id,
                        suggestion,
                        generation,
                    }
                });
        }
    }
    (false, meta)
}

/// Fetch session info from ACP via `grow/session/info`.
async fn fetch_session_info(
    session_id: &acp::SessionId,
    tx: &AcpAgentTx,
) -> Result<SessionInfoResponse, String> {
    let request = acp::ExtRequest::new(
        "grow/session/info",
        serde_json::value::to_raw_value(
                &serde_json::json!({
            "sessionId": session_id.0.to_string()
        }),
            )
            .expect("serialize session/info params")
            .into(),
    );
    let resp = acp_send(request, tx)
        .await
        .map_err(|e| sanitize_user_error(&format!("couldn't fetch session info: {e}")))?;
    let envelope: ExtMethodResult<SessionInfoResponse> = serde_json::from_str(
            resp.0.get(),
        )
        .map_err(|e| {
            tracing::debug!("typed session info deser failed: {e}");
            "invalid session info response".to_string()
        })?;
    if let Some(err) = envelope.error {
        let msg = err.as_str().map(String::from).unwrap_or_else(|| err.to_string());
        return Err(msg);
    }
    envelope.result.ok_or_else(|| "session info response missing result".to_string())
}
/// `grow/session/usage` → [`PromptUsage`] (bare response, no envelope).
async fn fetch_session_usage(
    session_id: &acp::SessionId,
    tx: &AcpAgentTx,
) -> Result<shell::extensions::notification::PromptUsage, String> {
    let request = acp::ExtRequest::new(
        "grow/session/usage",
        serde_json::value::to_raw_value(
                &serde_json::json!({
            "sessionId": session_id.0.to_string()
        }),
            )
            .expect("serialize session/usage params")
            .into(),
    );
    let resp = acp_send(request, tx)
        .await
        .map_err(|e| {
            if i32::from(e.code) == i32::from(acp::Error::method_not_found().code) {
                "not supported by this agent version".to_string()
            } else {
                sanitize_user_error(&e.to_string())
            }
        })?;
    let parsed: shell::extensions::usage::SessionUsageResponse = serde_json::from_str(
            resp.0.get(),
        )
        .map_err(|e| {
            tracing::debug!("session usage deser failed: {e}");
            "invalid session usage response".to_string()
        })?;
    Ok(parsed.usage)
}
/// Look up the session title/summary from local persistence.
async fn lookup_session_title(session_id: &acp::SessionId) -> Option<String> {
    let summaries = shell::session::persistence::list_summaries(None)
        .await
        .ok()?;
    summaries
        .into_iter()
        .find(|s| s.info.id == *session_id)
        .and_then(|s| s.display_title_opt())
}
/// Format session info into a human-readable string.
///
/// Mirrors the TUI's `render_session_info` for pager display.
fn format_session_info(
    info: &SessionInfoResponse,
    title: Option<&str>,
    show_resolved_model: bool,
) -> String {
    let session_id = &info.session_id;
    let cwd = &info.cwd;
    let model = info.data.model.as_deref().unwrap_or("unknown");
    let model_display = shell::session::model_display_name(
        info.data.model_display_name.as_deref(),
        model,
        info.data.resolved_model_id.as_deref(),
        show_resolved_model,
    );
    let ctx = &info.data.context;
    let used = ctx.used;
    let total = ctx.total;
    let pct = ctx.usage_pct;
    let title_line = match title {
        Some(t) => format!("  Title: {t}\n"),
        None => String::new(),
    };
    let model_hash_line = if shell::session::should_show_model_fingerprint(
        info.data.show_model_fingerprint,
        model,
    ) {
        info.data
            .model_fingerprint
            .as_deref()
            .map(|fp| format!("\n  Model Hash: {fp}"))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let backend_line = info
        .data
        .api_backend
        .as_deref()
        .map(|b| format!("\n  API Backend: {b}"))
        .unwrap_or_default();
    let sandbox_line = sandbox::profile_name()
        .map(|profile| format!("\n  Sandbox: {profile}"))
        .unwrap_or_default();
    let turn_line = format!("\n  Turn: {}", info.data.turn_index);
    let version_display = version::display_version(
        update::channel_label(),
    );
    format!(
        "{title_line}  Shell version: {version_display}\n  Auth method: provider BYOK\n  Session ID: {session_id}\n  Working directory: {cwd}\n  Model: {model_display}{model_hash_line}{backend_line}{sandbox_line}{turn_line}\n  Context: {used} / {total} tokens ({pct}%)"
    )
}
/// Build the single text content block for a plain `Effect::SendPrompt`.
///
/// Non-empty `skill_token_ranges` are stamped into the block `_meta` as
/// `skillTokenRanges: [[start, end], …]` so session replay restyles the echo
/// exactly like the composer highlighted it at submit time. Contract: the
/// offsets index this block's `text`, which is displayed verbatim — this
/// producer never combines them with a `displayText` override, and the
/// tracker ignores them when one is present. Empty ranges keep `meta: None`
/// — the legacy wire shape stays byte-identical. Extracted from the spawn
/// for testability.
fn plain_prompt_content_block(
    text: String,
    skill_token_ranges: &[std::ops::Range<usize>],
) -> acp::ContentBlock {
    let meta = if skill_token_ranges.is_empty() {
        None
    } else {
        let ranges: Vec<serde_json::Value> = skill_token_ranges
            .iter()
            .map(|r| serde_json::json!([r.start, r.end]))
            .collect();
        let mut map = acp::Meta::new();
        map.insert(
            crate::acp::meta::user_prompt_meta::SKILL_TOKEN_RANGES.into(),
            serde_json::Value::Array(ranges),
        );
        Some(map)
    };
    acp::ContentBlock::Text(acp::TextContent::new(text).meta(meta))
}
/// Build the `PromptRequest._meta` payload: `promptId` for notification /
/// response correlation, plus `screenMode` (`fullscreen` | `inline` |
/// `minimal`; headless stamps `"headless"` in its own path) so the shell can
/// attribute `prompt_submitted` diagnostics to minimal vs. regular usage.
/// `screen_mode` is `None` only under `SessionFlags::default()` (tests); the
/// key is omitted then, keeping the legacy wire shape byte-identical.
/// Extracted from the spawns for testability.
fn prompt_request_meta(
    prompt_id: &str,
    screen_mode: Option<&'static str>,
) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert("promptId".into(), serde_json::Value::String(prompt_id.into()));
    if let Some(mode) = screen_mode {
        map.insert("screenMode".into(), serde_json::Value::String(mode.into()));
    }
    serde_json::Value::Object(map)
}
fn trajectory_ready_url(line: &str) -> Option<&str> {
    let (_, url) = line.strip_prefix("Trajectory for ")?.rsplit_once(": ")?;
    url.starts_with("http://").then_some(url)
}
/// Build the one current `grow/steer` shape. Text-only steering is represented
/// by a text content block, never a second top-level text channel.
fn build_interject_params(
    session_id: &acp::SessionId,
    expected_turn_id: &str,
    text: &str,
    interjection_id: &str,
    blocks: Option<&[acp::ContentBlock]>,
) -> serde_json::Value {
    let content = blocks.map_or_else(
        || vec![acp::ContentBlock::Text(acp::TextContent::new(text))],
        <[acp::ContentBlock]>::to_vec,
    );
    serde_json::json!({
        "sessionId": session_id.0.to_string(),
        "expectedTurnId": expected_turn_id,
        "interjectionId": interjection_id,
        "content": content,
    })
}
#[cfg(test)]
mod tests;
