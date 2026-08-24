use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tools::implementations::grow_build::task::backend::{ChannelBackend, SubagentBackend};
use tools::implementations::grow_build::task::types::{
    ModelOverrideProvenance, SubagentCancelRequest, SubagentCancelTarget, SubagentEvent,
    SubagentOwner, SubagentRequest, SubagentRuntimeOverrides,
};
use workflow::{AgentOpts, AgentResult, BudgetState, HostError, WorkflowHostRequest};

use super::notify::WorkflowNotifySender;
use super::schema_contract::{
    SCHEMA_CONTRACT_RETRIES, compile_contract_schema, contract_prompt, validate_contract_output,
};
use super::tracker::WorkflowTracker;

pub(crate) const WORKFLOW_MAX_AGENT_RUNS: u32 =
    (workflow::MAX_AGENT_BUDGET as u32) * (SCHEMA_CONTRACT_RETRIES + 1);
pub(crate) const WORKFLOW_MAX_SCRIPT_DIAGNOSTICS_EVENTS: u32 = 64;
pub(crate) const WORKFLOW_MAX_SCRATCH_FILES: usize = 64;
pub(crate) const WORKFLOW_MAX_SCRATCH_FILE_BYTES: usize = 10 * 1024 * 1024;
pub(crate) const WORKFLOW_MAX_SCRATCH_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const WORKFLOW_MAX_AGENT_PROMPT_BYTES: usize = 1024 * 1024;
const WORKFLOW_MAX_TEMPLATE_OUTPUT_BYTES: usize = 1024 * 1024;
const WORKFLOW_MAX_PHASE_BYTES: usize = 256;
const WORKFLOW_MAX_LOG_BYTES: usize = 4 * 1024;
const WORKFLOW_CHILD_DRAIN_TIMEOUT: Duration = Duration::from_secs(20);
const WORKFLOW_MAX_SCRATCH_NAME_BYTES: usize = 255;
const SCRATCH_ARTIFACT_ROOT: &str = "scratch";

pub(crate) type DiagnosticHook = Arc<dyn Fn(&str, &serde_json::Value, bool) + Send + Sync>;

pub(crate) struct WorkflowHostParams {
    pub run_id: String,
    pub cwd: PathBuf,
    pub scratch_directory: Arc<crate::session::storage::ContainedDirectory>,
    pub tracker: Arc<parking_lot::Mutex<WorkflowTracker>>,
    pub store: super::store::WorkflowRunStore,
    pub notify: WorkflowNotifySender,
    pub subagent_event_tx:
        mpsc::UnboundedSender<tools::implementations::grow_build::task::types::SubagentEvent>,
    pub parent_session_id: String,
    pub parent_timeline: Option<chat_state::ChatStateHandle>,
    pub allow_fork_context: bool,
    pub templates: std::collections::HashMap<String, String>,
    pub diagnostics: DiagnosticHook,
    pub cancel: CancellationToken,
    pub max_concurrency: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostDrainOutcome {
    Drained,
    TimedOut,
}

pub(crate) fn spawn_workflow_host_service(
    params: WorkflowHostParams,
    mut rx: mpsc::UnboundedReceiver<WorkflowHostRequest>,
) -> (
    tokio::task::JoinHandle<()>,
    oneshot::Receiver<HostDrainOutcome>,
) {
    let (drained_tx, drained_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let max_concurrency = params.max_concurrency as usize;
        let service = Arc::new(HostService {
            active_agents: AtomicU32::new(0),
            agent_runs: AtomicU32::new(0),
            script_diagnostics_events: AtomicU32::new(0),
            scratch_io: tokio::sync::Mutex::new(()),
            agent_slots: tokio::sync::Semaphore::new(max_concurrency),
            params,
        });
        loop {
            let req = tokio::select! {
                req = rx.recv() => match req {
                    Some(req) => req,
                    None => break,
                },
                _ = service.params.cancel.cancelled() => {
                    while let Ok(req) = rx.try_recv() {
                        reply_cancelled(req);
                    }
                    break;
                }
            };
            service.clone().dispatch(req).await;
        }
        let drain_outcome = service.cancel_and_drain_children().await;
        let _ = drained_tx.send(drain_outcome);
    });
    (handle, drained_rx)
}

fn reply_cancelled(req: WorkflowHostRequest) {
    use WorkflowHostRequest as R;
    match req {
        R::ReserveAgentCalls { reply, .. } | R::ReleaseAgentCalls { reply, .. } => {
            let _ = reply.send(Err(HostError::Cancelled));
        }
        R::SpawnAgent { reply, .. } => {
            let _ = reply.send(Err(HostError::Cancelled));
        }
        R::BudgetQuery { reply } => {
            let _ = reply.send(Err(HostError::Cancelled));
        }
        R::RenderTemplate { reply, .. }
        | R::WriteScratchFile { reply, .. }
        | R::ReadScratchFile { reply, .. }
        | R::GitDiffSince { reply, .. } => {
            let _ = reply.send(Err(HostError::Cancelled));
        }
        R::Phase { .. } | R::Log { .. } | R::Diagnostic { .. } => {}
    }
}

struct HostService {
    active_agents: AtomicU32,
    agent_runs: AtomicU32,
    script_diagnostics_events: AtomicU32,
    scratch_io: tokio::sync::Mutex<()>,
    agent_slots: tokio::sync::Semaphore,
    params: WorkflowHostParams,
}

struct FinishOnce<'a> {
    host: &'a HostService,
    agent_id: String,
    finished: bool,
}

impl FinishOnce<'_> {
    fn finish(&mut self, state: &str, total_tokens: u64, total_duration: u64) {
        debug_assert!(!self.finished, "agent roster row finished twice");
        if std::mem::replace(&mut self.finished, true) {
            return;
        }
        self.host.params.tracker.lock().agent_finished(
            &self.host.params.run_id,
            &self.agent_id,
            state,
            total_tokens,
            total_duration,
        );
    }

    fn rebind(&mut self, new_agent_id: &str) {
        self.host.params.tracker.lock().rebind_agent_id(
            &self.host.params.run_id,
            &self.agent_id,
            new_agent_id,
        );
        self.agent_id = new_agent_id.to_string();
    }
}

impl HostService {
    async fn dispatch(self: Arc<Self>, req: WorkflowHostRequest) {
        match req {
            WorkflowHostRequest::ReserveAgentCalls { count, reply } => {
                let reserved = self
                    .params
                    .tracker
                    .lock()
                    .reserve_agents(&self.params.run_id, count);
                let result = match reserved {
                    Ok(state) => {
                        if let Err(error) = self.params.store.persist_ack(&state).await {
                            self.params
                                .tracker
                                .lock()
                                .release_agents(&self.params.run_id, count);
                            Err(HostError::Failed(format!(
                                "workflow agent-budget reservation could not be persisted: {error}"
                            )))
                        } else {
                            Ok(())
                        }
                    }
                    Err((requested, maximum)) => {
                        Err(HostError::AgentCallQuotaExceeded { requested, maximum })
                    }
                };
                let _ = reply.send(result);
            }
            WorkflowHostRequest::ReleaseAgentCalls { count, reply } => {
                let released = self
                    .params
                    .tracker
                    .lock()
                    .release_agents(&self.params.run_id, count);
                let result = match released {
                    Some(state) => {
                        if let Err(error) = self.params.store.persist_ack(&state).await {
                            tracing::warn!(
                                run_id = %self.params.run_id,
                                %error,
                                "workflow agent-budget release not persisted; keeping in-memory release"
                            );
                        }
                        Ok(())
                    }
                    None => Err(HostError::Failed(format!(
                        "workflow run not found for agent-budget release: {}",
                        self.params.run_id
                    ))),
                };
                let _ = reply.send(result);
            }
            WorkflowHostRequest::SpawnAgent {
                operation_id,
                opts,
                reply,
            } => {
                let svc = self.clone();
                tokio::spawn(async move {
                    let result = svc.spawn_agent(operation_id, opts).await;
                    let _ = reply.send(result);
                });
            }
            WorkflowHostRequest::Phase { title, replayed } => {
                if title.len() > WORKFLOW_MAX_PHASE_BYTES {
                    tracing::warn!(run_id = %self.params.run_id, "workflow phase title dropped: too large");
                    return;
                }
                let state = self
                    .params
                    .tracker
                    .lock()
                    .set_phase(&self.params.run_id, &title);
                if let Some(state) = state {
                    let elapsed = self.elapsed_ms();
                    let agents = self.active_agents.load(Ordering::Relaxed);
                    if replayed {
                        self.params.notify.emit_ephemeral(&state, elapsed, agents);
                    } else {
                        self.params.notify.emit(&state, elapsed, agents);
                    }
                }
            }
            WorkflowHostRequest::Log { message, replayed } => {
                if message.len() > WORKFLOW_MAX_LOG_BYTES {
                    tracing::warn!(run_id = %self.params.run_id, "workflow log dropped: too large");
                    return;
                }
                if !replayed {
                    tracing::info!(run_id = %self.params.run_id, "workflow log: {message}");
                }
            }
            WorkflowHostRequest::Diagnostic {
                name,
                fields,
                replayed,
            } => {
                if !replayed
                    && self
                        .script_diagnostics_events
                        .fetch_add(1, Ordering::Relaxed)
                        < WORKFLOW_MAX_SCRIPT_DIAGNOSTICS_EVENTS
                {
                    let host_fields = serde_json::json!({
                        "run_id": &self.params.run_id,
                        "script_event_name_bytes": name.len().min(64 * 1024),
                        "script_event_field_count": fields.as_object().map_or(0, serde_json::Map::len),
                    });
                    (self.params.diagnostics)(
                        "workflow_script_event_suppressed",
                        &host_fields,
                        false,
                    );
                }
            }
            WorkflowHostRequest::BudgetQuery { reply } => {
                let _ = reply.send(Ok(self.budget_state()));
            }
            WorkflowHostRequest::RenderTemplate { name, vars, reply } => {
                let _ = reply.send(self.render_template(&name, &vars));
            }
            WorkflowHostRequest::WriteScratchFile {
                name,
                content,
                reply,
            } => {
                let svc = self.clone();
                tokio::spawn(async move {
                    let _ = reply.send(svc.write_scratch_file(&name, &content).await);
                });
            }
            WorkflowHostRequest::ReadScratchFile { name, reply } => {
                let svc = self.clone();
                tokio::spawn(async move {
                    let _ = reply.send(svc.read_scratch_file(&name).await);
                });
            }
            WorkflowHostRequest::GitDiffSince { commit, reply } => {
                let svc = self.clone();
                tokio::spawn(async move {
                    let _ = reply.send(svc.git_diff_since(&commit).await);
                });
            }
        }
    }

    fn budget_state(&self) -> BudgetState {
        let tracker = self.params.tracker.lock();
        let run = tracker.get(&self.params.run_id);
        let total = run.as_ref().and_then(|r| r.agent_budget);
        let spent = run.as_ref().map_or(0, |r| r.agents_used);
        BudgetState {
            total,
            spent,
            reserved: 0,
            remaining: total.map(|total| total.saturating_sub(spent)),
        }
    }

    fn elapsed_ms(&self) -> u64 {
        self.params.tracker.lock().elapsed_ms(&self.params.run_id)
    }

    async fn spawn_agent(
        &self,
        operation_id: String,
        mut opts: AgentOpts,
    ) -> Result<AgentResult, HostError> {
        if self.params.cancel.is_cancelled() {
            return Err(HostError::Cancelled);
        }
        if opts.prompt.len() > WORKFLOW_MAX_AGENT_PROMPT_BYTES {
            return Err(HostError::Failed(format!(
                "agent prompt exceeds {WORKFLOW_MAX_AGENT_PROMPT_BYTES} bytes"
            )));
        }
        if opts.fork_context && !self.params.allow_fork_context {
            return Err(HostError::Unsupported(
                "fork_context is restricted to built-in workflows".into(),
            ));
        }
        if opts.resume_from.is_some() {
            opts.fork_context = false;
        }

        if opts
            .label
            .as_ref()
            .is_some_and(|label| label.len() > WORKFLOW_MAX_PHASE_BYTES)
            || opts
                .phase
                .as_ref()
                .is_some_and(|phase| phase.len() > WORKFLOW_MAX_PHASE_BYTES)
        {
            return Err(HostError::Failed(
                "agent label and phase must each be at most 256 bytes".into(),
            ));
        }

        let id = operation_id;
        let explicit_label = opts.label.clone();
        let capability_mode = match opts.capability_mode.as_deref() {
            None => None,
            Some(s) => Some(
                serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|_| {
                    HostError::Failed(format!(
                        "invalid capability_mode '{s}' (expected read-only, read-write, \
                             execute, or all)"
                    ))
                })?,
            ),
        };
        let isolation = opts
            .isolation_worktree
            .then_some(tool_types::SubagentIsolationMode::Worktree);
        let subagent_type = opts
            .agent_type
            .clone()
            .unwrap_or_else(|| "general-purpose".to_string());

        let schema_validator = match &opts.output_schema {
            None => None,
            Some(schema) => Some(compile_contract_schema(schema).map_err(HostError::Failed)?),
        };
        let prompt = match &opts.output_schema {
            None => opts.prompt.clone(),
            Some(schema) => contract_prompt(&opts.prompt, schema),
        };

        let description = self.params.tracker.lock().agent_started(
            &self.params.run_id,
            crate::session::workflow::tracker::WorkflowAgentRow {
                agent_id: id.clone(),
                label: explicit_label.unwrap_or_default(),
                phase: opts.phase.clone(),
                model: opts.model.clone(),
                state: "queued".to_string(),
                tokens_used: 0,
                duration_ms: 0,
            },
        );
        let mut row = FinishOnce {
            host: self,
            agent_id: id.clone(),
            finished: false,
        };
        // Persist and surface the queued row before waiting for a permit. This
        // makes max_concurrency observable and keeps cancellation of queued
        // work from looking like an agent that never existed.
        self.tick();
        let _slot = tokio::select! {
            permit = self.agent_slots.acquire() => permit.map_err(|_| {
                HostError::Failed("workflow concurrency gate closed".into())
            })?,
            _ = self.params.cancel.cancelled() => {
                row.finish("cancelled", 0, 0);
                return Err(HostError::Cancelled);
            }
        };
        self.params
            .tracker
            .lock()
            .agent_running(&self.params.run_id, &id);
        self.tick();
        let cancel_token = CancellationToken::new();

        let spawn_once =
            |child_id: String, prompt: String, resume_from: Option<String>, fork_context: bool| {
                SubagentRequest {
                    id: child_id,
                    prompt,
                    description: description.clone(),
                    subagent_type: subagent_type.clone(),
                    parent_session_id: self.params.parent_session_id.clone(),
                    parent_prompt_id: None,
                    resume_from,
                    cwd: None,
                    runtime_overrides: SubagentRuntimeOverrides {
                        model: opts.model.clone(),
                        output_token_budget: None,
                        model_override_provenance: ModelOverrideProvenance::Tool,
                        capability_mode,
                        isolation,
                        output_schema: None,
                        ..Default::default()
                    },
                    run_in_background: false,
                    surface_completion: false,
                    await_to_completion: true,
                    fork_context,
                    owner: SubagentOwner::workflow(&self.params.run_id),
                    goal_context: None,
                    cancel_token: cancel_token.clone(),
                }
            };

        let mut attempts: u32 = 0;
        let mut total_tokens: u64 = 0;
        let mut total_duration: u64 = 0;
        let mut resume_child: Option<String> = opts.resume_from.clone();
        let mut next_prompt = prompt;
        let mut fork_context = opts.fork_context;

        let (result, output) = loop {
            attempts += 1;
            let run = self.agent_runs.fetch_add(1, Ordering::Relaxed);
            if run >= WORKFLOW_MAX_AGENT_RUNS {
                row.finish("failed", total_tokens, total_duration);
                return Err(HostError::Failed(format!(
                    "workflow agent-run quota exceeded (maximum {WORKFLOW_MAX_AGENT_RUNS})"
                )));
            }
            self.tick();
            let child_id = if attempts == 1 {
                id.clone()
            } else {
                let retry_id = deterministic_retry_id(&id, attempts);
                row.rebind(&retry_id);
                retry_id
            };
            let request = spawn_once(
                child_id.clone(),
                next_prompt.clone(),
                resume_child,
                fork_context,
            );

            let backend = ChannelBackend::for_session(
                self.params.subagent_event_tx.clone(),
                self.params.parent_session_id.clone(),
            );
            let durable = if let Some(parent_timeline) = self.params.parent_timeline.as_ref() {
                match crate::agent::subagent::durable_child_operation(
                    &self.params.parent_session_id,
                    &child_id,
                    parent_timeline,
                )
                .await
                {
                    Ok(durable) => durable,
                    Err(error) => {
                        row.finish("failed", total_tokens, total_duration);
                        return Err(HostError::Failed(error));
                    }
                }
            } else {
                crate::agent::subagent::DurableChildOperation::Missing
            };
            let result = match durable {
                crate::agent::subagent::DurableChildOperation::Completed(result) => result,
                crate::agent::subagent::DurableChildOperation::Open => {
                    row.finish("failed", total_tokens, total_duration);
                    return Err(HostError::Failed(format!(
                        "workflow operation {child_id} has an open canonical child; reconcile it before resume"
                    )));
                }
                crate::agent::subagent::DurableChildOperation::Missing => {
                    self.active_agents.fetch_add(1, Ordering::Relaxed);
                    self.tick();
                    let result_fut = backend.spawn(request);
                    tokio::pin!(result_fut);
                    let result = tokio::select! {
                        result = &mut result_fut => result,
                        _ = self.params.cancel.cancelled() => {
                            cancel_token.cancel();
                            self.active_agents.fetch_sub(1, Ordering::Relaxed);
                            row.finish("cancelled", total_tokens, total_duration);
                            return Err(HostError::Cancelled);
                        }
                    };
                    self.active_agents.fetch_sub(1, Ordering::Relaxed);
                    match result {
                        Ok(result) => result,
                        Err(error) => {
                            row.finish("failed", total_tokens, total_duration);
                            return Err(HostError::Failed(format!(
                                "subagent coordinator failed before completion: {error}"
                            )));
                        }
                    }
                }
            };
            total_tokens = total_tokens.saturating_add(result.total_tokens_used);
            total_duration += result.duration_ms;
            if self.params.cancel.is_cancelled() {
                row.finish("cancelled", total_tokens, total_duration);
                return Err(HostError::Cancelled);
            }

            if result.backgrounded {
                row.finish("failed", total_tokens, total_duration);
                self.tick();
                return Err(HostError::Failed(format!(
                    "subagent {child_id} was auto-backgrounded by the await budget; its result \
                     is not available to this run (engine bug — workflow spawns must await to \
                     completion)"
                )));
            }

            let Some(validator) = schema_validator.as_ref() else {
                let output = if result.success {
                    serde_json::Value::String(result.output.to_string())
                } else {
                    serde_json::Value::String(
                        result
                            .error
                            .clone()
                            .unwrap_or_else(|| result.output.to_string()),
                    )
                };
                break (result, output);
            };
            if !result.success {
                let output = serde_json::Value::String(
                    result
                        .error
                        .clone()
                        .unwrap_or_else(|| result.output.to_string()),
                );
                break (result, output);
            }

            match validate_contract_output(validator, &result.output) {
                Ok(value) => break (result, value),
                Err(err) if attempts <= SCHEMA_CONTRACT_RETRIES => {
                    resume_child = Some(result.child_session_id.clone());
                    fork_context = false;
                    next_prompt = format!(
                        "Your final message did not satisfy the output contract: {err}\n\
                         Reply with a single ```json fenced block containing one JSON \
                         value conforming to the schema from <output-contract>, and \
                         nothing else."
                    );
                    continue;
                }
                Err(err) => {
                    let mut result = result;
                    result.success = false;
                    let output = serde_json::Value::String(format!(
                        "structured output validation failed: {err}"
                    ));
                    break (result, output);
                }
            }
        };

        row.finish(
            if result.success { "done" } else { "failed" },
            total_tokens,
            total_duration,
        );
        self.tick();

        Ok(AgentResult {
            agent_id: result.subagent_id.clone(),
            success: result.success,
            output,
            cancelled: result.cancelled,
            tokens_used: total_tokens,
            duration_ms: total_duration,
        })
    }

    fn tick(&self) {
        let state = self.params.tracker.lock().get(&self.params.run_id);
        if let Some(state) = state {
            self.params.notify.emit_ephemeral(
                &state,
                self.elapsed_ms(),
                self.active_agents.load(Ordering::Relaxed),
            );
        }
    }

    async fn cancel_and_drain_children(&self) -> HostDrainOutcome {
        let (respond_to, response) = oneshot::channel();
        if self
            .params
            .subagent_event_tx
            .send(SubagentEvent::Cancel(SubagentCancelRequest {
                parent_session_id: Some(self.params.parent_session_id.clone()),
                target: SubagentCancelTarget::WorkflowRunId(self.params.run_id.clone()),
                respond_to,
            }))
            .is_err()
        {
            tracing::warn!(run_id = %self.params.run_id, "workflow child cancellation channel closed");
            return HostDrainOutcome::TimedOut;
        }
        if matches!(
            tokio::time::timeout(WORKFLOW_CHILD_DRAIN_TIMEOUT, response).await,
            Ok(Ok(_))
        ) {
            HostDrainOutcome::Drained
        } else {
            tracing::warn!(
                run_id = %self.params.run_id,
                timeout_ms = WORKFLOW_CHILD_DRAIN_TIMEOUT.as_millis() as u64,
                "workflow child cancel/drain timed out"
            );
            HostDrainOutcome::TimedOut
        }
    }

    fn render_template(&self, name: &str, vars: &serde_json::Value) -> Result<String, HostError> {
        let template = self
            .params
            .templates
            .get(name)
            .ok_or_else(|| HostError::Failed(format!("unknown template: {name}")))?;
        let mut out = template.clone();
        if out.len() > WORKFLOW_MAX_TEMPLATE_OUTPUT_BYTES {
            return Err(HostError::Failed(format!(
                "template exceeds {WORKFLOW_MAX_TEMPLATE_OUTPUT_BYTES} bytes"
            )));
        }
        if let Some(map) = vars.as_object() {
            for (key, value) in map {
                let value = match value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                out = out.replace(&format!("{{{key}}}"), &value);
                if out.len() > WORKFLOW_MAX_TEMPLATE_OUTPUT_BYTES {
                    return Err(HostError::Failed(format!(
                        "rendered template exceeds {WORKFLOW_MAX_TEMPLATE_OUTPUT_BYTES} bytes"
                    )));
                }
            }
        }
        Ok(out)
    }

    fn scratch_name(&self, name: &str) -> Result<(std::ffi::OsString, String), HostError> {
        if name.len() > WORKFLOW_MAX_SCRATCH_NAME_BYTES {
            return Err(HostError::Failed(format!(
                "scratch file name exceeds {WORKFLOW_MAX_SCRATCH_NAME_BYTES} bytes"
            )));
        }
        let candidate = Path::new(name);
        let mut components = candidate.components();
        match (components.next(), components.next()) {
            (Some(std::path::Component::Normal(part)), None) if !part.is_empty() => {}
            _ => {
                return Err(HostError::Failed(format!(
                    "scratch file name must be a single relative path component, got: {name}"
                )));
            }
        }
        Ok((
            std::ffi::OsString::from(name),
            format!("{SCRATCH_ARTIFACT_ROOT}/{name}"),
        ))
    }

    fn scratch_usage(&self, replacing: &std::ffi::OsStr) -> Result<(usize, u64), HostError> {
        let entries = self
            .params
            .scratch_directory
            .list_names()
            .map_err(|error| HostError::Failed(format!("scratch dir listing: {error}")))?;
        let mut files = 0usize;
        let mut bytes = 0u64;
        for name in entries {
            let file = self
                .params
                .scratch_directory
                .open_regular(&name, "Workflow scratch file")
                .map_err(|error| HostError::Failed(format!("scratch entry is invalid: {error}")))?;
            let meta = file
                .metadata()
                .map_err(|error| HostError::Failed(format!("scratch metadata: {error}")))?;
            if name != replacing {
                files = files.saturating_add(1);
                bytes = bytes.saturating_add(meta.len());
            }
        }
        Ok((files, bytes))
    }

    async fn write_scratch_file(&self, name: &str, content: &str) -> Result<String, HostError> {
        if content.len() > WORKFLOW_MAX_SCRATCH_FILE_BYTES {
            return Err(HostError::Failed(format!(
                "scratch file exceeds {WORKFLOW_MAX_SCRATCH_FILE_BYTES} byte limit"
            )));
        }
        let (name, artifact_path) = self.scratch_name(name)?;
        let _io = self.scratch_io.lock().await;

        let (other_files, other_bytes) = self.scratch_usage(&name)?;
        let target_exists = match self
            .params
            .scratch_directory
            .open_regular(&name, "Workflow scratch file")
        {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(HostError::Failed(format!("scratch target is invalid: {error}"))),
        };
        let resulting_files = other_files.saturating_add(usize::from(!target_exists));
        let resulting_bytes = other_bytes.saturating_add(content.len() as u64);
        if resulting_files > WORKFLOW_MAX_SCRATCH_FILES {
            return Err(HostError::Failed(format!(
                "scratch file quota exceeded (maximum {WORKFLOW_MAX_SCRATCH_FILES})"
            )));
        }
        if resulting_bytes > WORKFLOW_MAX_SCRATCH_TOTAL_BYTES {
            return Err(HostError::Failed(format!(
                "scratch byte quota exceeded (maximum {WORKFLOW_MAX_SCRATCH_TOTAL_BYTES})"
            )));
        }

        let scratch = self
            .params
            .scratch_directory
            .try_clone()
            .map_err(|error| HostError::Failed(format!("scratch capability clone: {error}")))?;
        let body = content.as_bytes().to_vec();
        tokio::task::spawn_blocking(move || {
            scratch
                .write_atomic(&name, &body, true, true)
                .map_err(|error| HostError::Failed(format!("scratch atomic write: {error}")))?;
            Ok::<(), HostError>(())
        })
        .await
        .map_err(|e| HostError::Failed(format!("scratch writer task failed: {e}")))??;
        Ok(artifact_path)
    }

    async fn read_scratch_file(&self, name: &str) -> Result<String, HostError> {
        let (name, _) = self.scratch_name(name)?;
        let _io = self.scratch_io.lock().await;
        let bytes = self
            .params
            .scratch_directory
            .read_bounded(
                &name,
                "Workflow scratch file",
                WORKFLOW_MAX_SCRATCH_FILE_BYTES as u64,
            )
            .map_err(|error| HostError::Failed(format!("scratch read: {error}")))?;
        String::from_utf8(bytes)
            .map_err(|e| HostError::Failed(format!("scratch file is not UTF-8: {e}")))
    }

    async fn git_diff_since(&self, commit: &str) -> Result<String, HostError> {
        const DIFF_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
        const DIFF_CAP_BYTES: usize = 256 * 1024;

        if commit.is_empty() || !commit.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(HostError::Failed(format!(
                "git_diff_since expects a commit hash, got: {commit}"
            )));
        }

        let mut cmd = tokio::process::Command::new("git");
        cmd.arg("diff")
            .arg(commit)
            .current_dir(&self.params.cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .envs(tty_utils::pager_env());
        tty_utils::detach_command(&mut cmd);

        let output = tokio::time::timeout(DIFF_TIMEOUT, cmd.output())
            .await
            .map_err(|_| HostError::Failed("git diff timed out".into()))?
            .map_err(|e| HostError::Failed(format!("git diff: {e}")))?;

        if !output.status.success() {
            return Err(HostError::Failed(format!(
                "git diff exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let mut text = String::from_utf8_lossy(&output.stdout).to_string();
        if text.len() > DIFF_CAP_BYTES {
            let mut end = DIFF_CAP_BYTES;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            text.push_str("\n… [diff truncated]");
        }
        Ok(text)
    }
}

fn deterministic_retry_id(operation_id: &str, attempt: u32) -> String {
    let digest = blake3::hash(format!("{operation_id}:retry:{attempt}").as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest.as_bytes()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::persistence::PersistenceMsg;
    use crate::session::workflow::notify::WorkflowNotifySender;
    use crate::session::workflow::store::WorkflowRunStore;
    use crate::session::workflow::tracker::WorkflowTracker;

    fn scratch_directory() -> Arc<crate::session::storage::ContainedDirectory> {
        let path = std::env::temp_dir().join(format!(
            "grow-workflow-host-test-{}",
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::create_dir(&path).unwrap();
        Arc::new(
            crate::session::storage::ContainedDirectory::open(
                &path,
                Path::new(""),
                "Workflow host test scratch",
                false,
            )
            .unwrap(),
        )
    }

    #[tokio::test]
    async fn max_concurrency_queues_agents_until_a_permit_is_released() {
        let run_id = "wf_concurrency".to_string();
        let mut tracker = WorkflowTracker::default();
        tracker.start_run(
            run_id.clone(),
            "demo".into(),
            "objective".into(),
            vec![],
            None,
            None,
        );
        let tracker = Arc::new(parking_lot::Mutex::new(tracker));
        let (persist_tx, _persist_rx) = mpsc::unbounded_channel::<PersistenceMsg>();
        let store = WorkflowRunStore::new(None, persist_tx.clone());
        let (gateway_tx, _gateway_rx) = mpsc::unbounded_channel();
        let notify = WorkflowNotifySender::new(
            agent_client_protocol::SessionId::new("test-session"),
            acp_transport::AcpAgentGatewaySender::new(gateway_tx),
            persist_tx,
            store.clone(),
        );
        let (subagent_tx, mut subagent_rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let params = WorkflowHostParams {
            run_id,
            cwd: std::env::temp_dir(),
            scratch_directory: scratch_directory(),
            tracker,
            store,
            notify,
            subagent_event_tx: subagent_tx,
            parent_session_id: "parent".into(),
            parent_timeline: None,
            allow_fork_context: false,
            templates: Default::default(),
            diagnostics: Arc::new(|_, _, _| {}),
            cancel: cancel.clone(),
            max_concurrency: 1,
        };
        let (host_tx, host_rx) = mpsc::unbounded_channel();
        let (handle, _drained) = spawn_workflow_host_service(params, host_rx);

        let (first_tx, first_rx) = oneshot::channel();
        let (second_tx, second_rx) = oneshot::channel();
        host_tx
            .send(WorkflowHostRequest::SpawnAgent {
                operation_id: uuid::Uuid::now_v7().to_string(),
                opts: AgentOpts {
                    prompt: "first".into(),
                    ..Default::default()
                },
                reply: first_tx,
            })
            .unwrap();
        host_tx
            .send(WorkflowHostRequest::SpawnAgent {
                operation_id: uuid::Uuid::now_v7().to_string(),
                opts: AgentOpts {
                    prompt: "second".into(),
                    ..Default::default()
                },
                reply: second_tx,
            })
            .unwrap();

        let first = tokio::time::timeout(Duration::from_secs(1), subagent_rx.recv())
            .await
            .expect("first agent should start")
            .expect("subagent channel should remain open");
        let SubagentEvent::Spawn(first) = first else {
            panic!("expected first spawn event")
        };
        assert_eq!(first.request.prompt, "first");
        assert!(
            tokio::time::timeout(Duration::from_millis(50), subagent_rx.recv())
                .await
                .is_err(),
            "second agent must remain queued while the only permit is held"
        );

        first
            .result_tx
            .send(
                tools::implementations::grow_build::task::types::SubagentResult {
                    success: true,
                    output: Arc::from("first done"),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(first_rx.await.unwrap().is_ok());

        let second = tokio::time::timeout(Duration::from_secs(1), subagent_rx.recv())
            .await
            .expect("second agent should start after permit release")
            .expect("subagent channel should remain open");
        let SubagentEvent::Spawn(second) = second else {
            panic!("expected second spawn event")
        };
        assert_eq!(second.request.prompt, "second");
        second
            .result_tx
            .send(
                tools::implementations::grow_build::task::types::SubagentResult {
                    success: true,
                    output: Arc::from("second done"),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(second_rx.await.unwrap().is_ok());

        drop(host_tx);
        cancel.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
    }

    #[tokio::test]
    async fn reserve_agent_calls_rolls_back_on_persist_failure() {
        let run_id = "wf_reserve_rollback".to_string();
        let mut tracker = WorkflowTracker::default();
        tracker.start_run(
            run_id.clone(),
            "demo".into(),
            "objective".into(),
            vec![],
            Some(1000),
            None,
        );
        tracker.reserve_agents(&run_id, 10).unwrap();
        assert_eq!(tracker.get(&run_id).unwrap().agents_used, 10);

        let tracker = Arc::new(parking_lot::Mutex::new(tracker));
        let (persist_tx, _persist_rx) = mpsc::unbounded_channel::<PersistenceMsg>();
        let store = WorkflowRunStore::new(None, persist_tx.clone());
        let (gateway_tx, _gateway_rx) = mpsc::unbounded_channel();
        let notify = WorkflowNotifySender::new(
            agent_client_protocol::SessionId::new("test-session"),
            acp_transport::AcpAgentGatewaySender::new(gateway_tx),
            persist_tx,
            store.clone(),
        );
        let (subagent_tx, _subagent_rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let params = WorkflowHostParams {
            run_id: run_id.clone(),
            cwd: std::env::temp_dir(),
            scratch_directory: scratch_directory(),
            tracker: tracker.clone(),
            store,
            notify,
            subagent_event_tx: subagent_tx,
            parent_session_id: "parent".into(),
            parent_timeline: None,
            allow_fork_context: false,
            templates: Default::default(),
            diagnostics: Arc::new(|_, _, _| {}),
            cancel: cancel.clone(),
            max_concurrency: 3,
        };

        let (host_tx, host_rx) = mpsc::unbounded_channel();
        let (handle, _drained) = spawn_workflow_host_service(params, host_rx);

        let (reply_tx, reply_rx) = oneshot::channel();
        host_tx
            .send(WorkflowHostRequest::ReserveAgentCalls {
                count: 40,
                reply: reply_tx,
            })
            .unwrap();

        let result = reply_rx.await.unwrap();
        assert!(
            matches!(
                result,
                Err(HostError::Failed(ref msg)) if msg.contains("could not be persisted")
            ),
            "expected persist failure, got {result:?}"
        );
        assert_eq!(
            tracker.lock().get(&run_id).unwrap().agents_used,
            10,
            "reserve must release_agents(count) when durable persistence fails"
        );

        drop(host_tx);
        cancel.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
    }

    #[tokio::test]
    async fn release_agent_calls_keeps_in_memory_release_on_persist_failure() {
        let run_id = "wf_release_persist".to_string();
        let mut tracker = WorkflowTracker::default();
        tracker.start_run(
            run_id.clone(),
            "demo".into(),
            "objective".into(),
            vec![],
            Some(1000),
            None,
        );
        tracker.reserve_agents(&run_id, 50).unwrap();
        assert_eq!(tracker.get(&run_id).unwrap().agents_used, 50);

        let tracker = Arc::new(parking_lot::Mutex::new(tracker));
        let (persist_tx, _persist_rx) = mpsc::unbounded_channel::<PersistenceMsg>();
        let store = WorkflowRunStore::new(None, persist_tx.clone());
        let (gateway_tx, _gateway_rx) = mpsc::unbounded_channel();
        let notify = WorkflowNotifySender::new(
            agent_client_protocol::SessionId::new("test-session"),
            acp_transport::AcpAgentGatewaySender::new(gateway_tx),
            persist_tx,
            store.clone(),
        );
        let (subagent_tx, _subagent_rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let params = WorkflowHostParams {
            run_id: run_id.clone(),
            cwd: std::env::temp_dir(),
            scratch_directory: scratch_directory(),
            tracker: tracker.clone(),
            store,
            notify,
            subagent_event_tx: subagent_tx,
            parent_session_id: "parent".into(),
            parent_timeline: None,
            allow_fork_context: false,
            templates: Default::default(),
            diagnostics: Arc::new(|_, _, _| {}),
            cancel: cancel.clone(),
            max_concurrency: 3,
        };

        let (host_tx, host_rx) = mpsc::unbounded_channel();
        let (handle, _drained) = spawn_workflow_host_service(params, host_rx);

        let (reply_tx, reply_rx) = oneshot::channel();
        host_tx
            .send(WorkflowHostRequest::ReleaseAgentCalls {
                count: 50,
                reply: reply_tx,
            })
            .unwrap();

        let result = reply_rx.await.unwrap();
        assert!(
            result.is_ok(),
            "release must succeed for accounting even if persist lags: {result:?}"
        );
        assert_eq!(
            tracker.lock().get(&run_id).unwrap().agents_used,
            0,
            "in-memory release must stick so resume does not double-charge"
        );

        drop(host_tx);
        cancel.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
    }
}
