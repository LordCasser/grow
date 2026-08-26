use super::*;

/// Scoped ACP control route for a live child actor. Dropping the child run
/// removes the route even on cancellation or an early error return.
struct ActiveChildRegistration {
    sessions: ActiveChildSessions,
    session_id: acp::SessionId,
}

impl ActiveChildRegistration {
    fn install(
        sessions: ActiveChildSessions,
        session_id: acp::SessionId,
        handle: SessionHandle,
    ) -> Self {
        sessions.borrow_mut().insert(session_id.clone(), handle);
        Self {
            sessions,
            session_id,
        }
    }
}

impl Drop for ActiveChildRegistration {
    fn drop(&mut self) {
        self.sessions.borrow_mut().remove(&self.session_id);
    }
}
use sampling_types::ReasoningEffort;
use tools::implementations::grow_build;

/// Filesystem work that may only begin after the parent Timeline owns the
/// child spawn intent. The target path is kept separately because it is part
/// of that immutable parent fact.
enum WorktreeMaterialization {
    Create { source: PathBuf },
    Rehydrate { snapshot_ref: String },
}

pub(super) fn canonical_total_tokens(totals: &chat_state::UsageTotals) -> u64 {
    totals.total_tokens()
}
pub(super) fn usage_is_incomplete(
    ledger_incomplete: bool,
    cancellation_may_hide_usage: bool,
    _known_total_tokens: u64,
    _has_usage_entries: bool,
) -> bool {
    ledger_incomplete || cancellation_may_hide_usage
}
pub(super) async fn record_subagent_usage(
    parent_cmd_tx: Option<&mpsc::UnboundedSender<SessionCommand>>,
    subagent_id: String,
    by_model: Option<Vec<(String, chat_state::UsageTotals)>>,
    parent_prompt_id: Option<String>,
    incomplete: bool,
) -> bool {
    match by_model {
        None => false,
        Some(by_model) if by_model.is_empty() && !incomplete => true,
        Some(by_model) => {
            let Some(cmd_tx) = parent_cmd_tx else {
                return false;
            };
            let (respond_to, ack) = oneshot::channel();
            if cmd_tx
                .send(SessionCommand::RecordSubagentUsage {
                    subagent_id,
                    by_model,
                    parent_prompt_id,
                    incomplete,
                    respond_to,
                })
                .is_err()
            {
                return false;
            }
            ack.await.is_ok()
        }
    }
}
pub(super) fn task_model_override_error(
    requested: Option<&str>,
    provenance: ModelOverrideProvenance,
    is_resume: bool,
    available: &indexmap::IndexMap<String, crate::agent::config::ModelEntry>,
    _is_session_auth: bool,
) -> Option<String> {
    if provenance != ModelOverrideProvenance::Tool || is_resume {
        return None;
    }
    let requested = requested?;
    crate::agent::models::task_model_error_for_catalog(requested, available)
}

fn validate_goal_context(request: &SubagentRequest) -> Result<(), String> {
    use tools::implementations::grow_build::task::types::SubagentOwner;
    match (&request.owner, &request.goal_context) {
        (
            SubagentOwner::Goal {
                goal_id,
                definition_revision,
            },
            Some(context),
        ) if context.view.goal_id == *goal_id
            && context.view.definition_revision == *definition_revision =>
        {
            Ok(())
        }
        (SubagentOwner::Goal { .. }, _) => Err(
            "Goal-owned subagent request has a missing or mismatched immutable context snapshot"
                .to_string(),
        ),
        (SubagentOwner::Task | SubagentOwner::Workflow { .. }, None) => Ok(()),
        (SubagentOwner::Task | SubagentOwner::Workflow { .. }, Some(_)) => {
            Err("Non-Goal subagent request cannot carry a Goal context snapshot".to_string())
        }
    }
}

fn resolve_workflow_sampler(
    request: &SubagentRequest,
    model_id: &str,
    ctx: &SubagentSpawnContext,
) -> Result<crate::agent::models::PublishedSessionRoute, String> {
    let run_id = request
        .owner
        .workflow_run_id()
        .ok_or_else(|| "Workflow sampler resolution requires a Workflow owner".to_owned())?;
    let tracker = ctx.workflow_tracker.as_ref().ok_or_else(|| {
        "Workflow sampler resolution requires the root Workflow tracker".to_owned()
    })?;
    let state = tracker
        .lock()
        .get(run_id)
        .ok_or_else(|| format!("Workflow Run '{run_id}' is no longer registered"))?;
    state
        .runtime_route
        .session_route_for(model_id, &ctx.models_manager, ctx.alpha_test_key.clone())
}

fn model_state_for_catalog(
    catalog: &indexmap::IndexMap<String, crate::agent::config::ModelEntry>,
    model_id: &acp::ModelId,
    reasoning_effort: Option<ReasoningEffort>,
) -> acp::SessionModelState {
    let selectable = crate::agent::models::task_selectable_catalog(catalog);
    let mut available_models: Vec<_> = crate::agent::config::to_acp_model_info(&selectable)
        .into_values()
        .collect();
    if let Some(info) = available_models
        .iter_mut()
        .find(|info| info.model_id == *model_id)
        && let Some(reasoning_effort) = reasoning_effort
        && info
            .meta
            .as_ref()
            .is_some_and(|meta| meta.contains_key(sampling_types::REASONING_EFFORTS_META_KEY))
    {
        let mut meta = info.meta.clone().unwrap_or_default();
        meta.insert(
            sampling_types::REASONING_EFFORT_META_KEY.to_owned(),
            sampling_types::reasoning_effort_meta_value(reasoning_effort),
        );
        info.meta = Some(meta);
    }
    acp::SessionModelState::new(model_id.clone(), available_models)
}

fn frozen_workflow_agent_definition(
    request: &SubagentRequest,
    ctx: &SubagentSpawnContext,
) -> Result<
    Option<(
        agent::config::AgentDefinition,
        Vec<tools::implementations::skills::types::SkillInfo>,
        Vec<String>,
    )>,
    String,
> {
    let Some(run_id) = request.owner.workflow_run_id() else {
        return Ok(None);
    };
    let tracker = ctx
        .workflow_tracker
        .as_ref()
        .ok_or_else(|| "Workflow Agent resolution requires the root Workflow tracker".to_owned())?;
    let tracker = tracker.lock();
    let route = &tracker
        .get(run_id)
        .ok_or_else(|| format!("Workflow Run '{run_id}' is no longer registered"))?
        .runtime_route;
    Ok(Some((
        route.agent_definition(&request.subagent_type)?,
        route.frozen_skills(),
        route.frozen_agent_names(),
    )))
}

async fn catch_up_child_catalog_generation(
    ctx: &SubagentSpawnContext,
    child_handle: &SessionHandle,
) -> Result<(), String> {
    if child_handle.workflow_run_id.is_some()
        || ctx.models_manager.catalog_revision() == ctx.catalog_revision
    {
        return Ok(());
    }
    let catalog = std::sync::Arc::new(ctx.models_manager.published_catalog());
    let revision = catalog.revision;
    let (responds_to, response) = tokio::sync::oneshot::channel();
    child_handle
        .cmd_tx
        .send(SessionCommand::ReloadModelConfig {
            catalog,
            responds_to,
        })
        .map_err(|_| format!("child actor closed before catalog revision {revision} catch-up"))?;
    response
        .await
        .map_err(|_| format!("child dropped catalog revision {revision} catch-up"))?
        .map_err(|error| format!("child rejected catalog revision {revision}: {error:?}"))
}

/// Runtime adapter for one shell child. Shared lifecycle state is owned by the
/// `tools` coordinator actor and reached only through `reporter`.
#[tracing::instrument(
    name = "subagent.handle_request",
    skip_all,
    fields(
        subagent_id = %request.id,
        parent_session_id = %ctx.parent_session_id,
        subagent_type = %request.subagent_type,
    )
)]
pub(crate) async fn run_shell_child(
    mut request: SubagentRequest,
    mut ctx: SubagentSpawnContext,
    cancel_token: CancellationToken,
    reporter: ChildReporter<ShellChildRuntime>,
    gateway: &GatewaySender,
) -> ChildRunOutput<ShellCompletionData> {
    let start = std::time::Instant::now();
    let mut completion_data = ShellCompletionData::from_context(&ctx);
    if request.owner.is_workflow() && cancel_token.is_cancelled() {
        return child_run_output(
            cancelled_result(&request, "Subagent was cancelled"),
            completion_data,
        );
    }
    if let Err(message) = validate_goal_context(&request) {
        return child_run_output(failure_result(&request, &message), completion_data);
    }
    let (definition, frozen_workflow_skills, frozen_subagent_names) =
        match frozen_workflow_agent_definition(&request, &ctx) {
            Ok(Some((definition, skills, names))) => (Some(definition), Some(skills), Some(names)),
            Ok(None) => (
                resolve_agent_definition(&request.subagent_type, &ctx),
                None,
                None,
            ),
            Err(message) => {
                return child_run_output(failure_result(&request, &message), completion_data);
            }
        };
    let workflow_agent_names = frozen_subagent_names.clone();
    let Some(mut definition) = definition else {
        let msg = format!("Unknown subagent type: {}", request.subagent_type);
        return child_run_output(failure_result(&request, &msg), completion_data);
    };
    match if request.owner.is_workflow() {
        SubagentValidateTypeOutcome::Ok
    } else {
        gate_subagent_type(&request.subagent_type, &ctx)
    } {
        SubagentValidateTypeOutcome::Disabled => {
            let msg = format!(
                "Subagent '{}' is not available to the current Agent or is disabled via [subagents.toggle]",
                request.subagent_type
            );
            return child_run_output(failure_result(&request, &msg), completion_data);
        }
        SubagentValidateTypeOutcome::Unknown { .. }
        | SubagentValidateTypeOutcome::ValidationUnavailable => {
            let msg = format!("Cannot validate subagent '{}'", request.subagent_type);
            return child_run_output(failure_result(&request, &msg), completion_data);
        }
        SubagentValidateTypeOutcome::Ok => {}
        _ => {
            let msg = format!("Cannot validate subagent '{}'", request.subagent_type);
            return child_run_output(failure_result(&request, &msg), completion_data);
        }
    }
    if !request.owner.is_workflow() {
        resolve_subagent_toolset(&request.subagent_type, &ctx, &mut definition);
    }
    let mut effective_runtime = crate::agent::subagent::resolution::resolve_runtime_config(
        &request.runtime_overrides,
        &definition,
    );
    let prompt = request.prompt.clone();
    // Resolve initial RWX before any worktree, MCP, or session side
    // effect. The Agent definition supplies the default, the Task call may
    // narrow or widen that request, and the immediate security parent's
    // immutable ceiling is the final upper bound. Incomparable read/write and
    // execute branches meet at read-only.
    let requested_capability_mode = effective_runtime.capability_mode;
    let initial_capability_mode = ctx
        .parent_capability_ceiling
        .as_ref()
        .map_or(requested_capability_mode, |ceiling| {
            ceiling.constrain_mode(requested_capability_mode)
        });
    if initial_capability_mode != requested_capability_mode {
        tracing::info!(
            requested = requested_capability_mode.as_str(),
            effective = initial_capability_mode.as_str(),
            "constrained nested subagent initial capability to the parent delegation ceiling"
        );
    }
    effective_runtime.capability_mode = initial_capability_mode;
    let resume_source = if let Some(resume_id) = request
        .resume_from
        .as_deref()
        .filter(|s| is_valid_resume_id(s))
    {
        match durable_resume_source_for(
            resume_id,
            &ctx.parent_session_id,
            &ctx.security_parent_session_id,
        ) {
            Some(info) => Some(info),
            None if reporter
                .source_is_active(resume_id, &ctx.security_parent_session_id)
                .await =>
            {
                let msg = format!(
                    "Cannot resume from subagent '{resume_id}': it is still running. \
                     Wait for it to complete before resuming."
                );
                return child_run_output(failure_result(&request, &msg), completion_data);
            }
            None => {
                let msg = format!(
                    "Cannot resume from subagent '{resume_id}': no completed canonical lifecycle \
                     was found."
                );
                return child_run_output(failure_result(&request, &msg), completion_data);
            }
        }
    } else {
        None
    };
    if let Some(ref source) = resume_source {
        if request.runtime_overrides.model.is_some() {
            tracing::debug!(
                subagent_id = %request.id,
                "Ignoring caller model override on resume; source model will be pinned"
            );
        }
        effective_runtime.model = None;
        if let Err(e) = crate::agent::subagent::resolution::validate_resume_identity(
            &request.subagent_type,
            &source.data,
        ) {
            return child_run_output(failure_result(&request, &e.to_string()), completion_data);
        }
        if source.worktree_path.is_none() {
            let confined = dunce::canonicalize(&source.child_cwd)
                .ok()
                .zip(dunce::canonicalize(&ctx.parent_cwd).ok())
                .is_some_and(|(child, parent)| child.is_dir() && child.starts_with(parent));
            if !confined {
                return child_run_output(
                    failure_result(
                        &request,
                        "Cannot resume a child whose cwd is outside the parent workspace",
                    ),
                    completion_data,
                );
            }
        }
    }
    if !request.owner.is_workflow() {
        if let Some(error) = task_model_override_error(
            request.runtime_overrides.model.as_deref(),
            request.runtime_overrides.model_override_provenance,
            resume_source.is_some(),
            &ctx.available_models,
            false,
        ) {
            return child_run_output(failure_result(&request, &error), completion_data);
        }
    }
    let (worktree_path, worktree_materialization) = if let Some(ref source) = resume_source {
        if effective_runtime.isolation != tool_types::SubagentIsolationMode::None
            && source.worktree_path.is_none()
        {
            return child_run_output(
                failure_result(
                    &request,
                    "Cannot resume with isolation: the source subagent has no worktree",
                ),
                completion_data,
            );
        }
        match source.worktree_path.as_deref() {
            None => (None, None),
            Some(dest) => {
                match resume_worktree_action(dest.is_dir(), source.snapshot_ref.as_deref()) {
                    ResumeWorktreeAction::Reuse => (Some(dest.to_path_buf()), None),
                    ResumeWorktreeAction::Rehydrate => {
                        let snapshot_ref = source.snapshot_ref.clone().unwrap_or_default();
                        (
                            Some(dest.to_path_buf()),
                            Some(WorktreeMaterialization::Rehydrate { snapshot_ref }),
                        )
                    }
                    ResumeWorktreeAction::Missing => {
                        return child_run_output(
                            failure_result(
                                &request,
                                &format!(
                                    "Cannot resume isolated subagent: worktree '{}' is missing and no snapshot exists",
                                    dest.display()
                                ),
                            ),
                            completion_data,
                        );
                    }
                }
            }
        }
    } else if effective_runtime.isolation != tool_types::SubagentIsolationMode::None {
        let source_cwd = parent_source_cwd(&ctx);
        let dest = match crate::session::worktree::worktree_base_dir_for_source(&source_cwd) {
            Ok(base) => base.join(format!("subagent-{}", request.id)),
            Err(e) => {
                tracing::warn!(
                    subagent_id = %request.id,
                    error = %e,
                    "Could not resolve worktree base dir, using temp dir for subagent worktree"
                );
                std::env::temp_dir()
                    .join("grow-subagent-worktrees")
                    .join(&request.id)
            }
        };
        (
            Some(dest),
            Some(WorktreeMaterialization::Create { source: source_cwd }),
        )
    } else {
        (None, None)
    };
    let worktree_freshly_created = matches!(
        &worktree_materialization,
        Some(WorktreeMaterialization::Create { .. })
    );
    if let Some(raw_cwd) = request.cwd.as_deref() {
        match sanitize_cwd_value(raw_cwd) {
            Some(cwd_path) => {
                if worktree_path.is_none() && resume_source.is_none() {
                    let requested = Path::new(&cwd_path);
                    let candidate = if requested.is_absolute() {
                        requested.to_path_buf()
                    } else {
                        ctx.parent_cwd.join(requested)
                    };
                    let canonical = match dunce::canonicalize(&candidate) {
                        Ok(path) if path.is_dir() => path,
                        _ => {
                            let msg = if candidate.exists() {
                                format!("cwd \"{cwd_path}\" exists but is not a directory")
                            } else {
                                format!("cwd \"{cwd_path}\" does not exist")
                            };
                            return child_run_output(
                                failure_result(&request, &msg),
                                completion_data,
                            );
                        }
                    };
                    let parent = match dunce::canonicalize(&ctx.parent_cwd) {
                        Ok(path) => path,
                        Err(error) => {
                            let msg = format!("cannot resolve parent workspace: {error}");
                            return child_run_output(
                                failure_result(&request, &msg),
                                completion_data,
                            );
                        }
                    };
                    if !canonical.starts_with(&parent) {
                        let msg = format!(
                            "cwd \"{}\" is outside the parent workspace \"{}\"; use isolation=\"worktree\" for an isolated child",
                            canonical.display(),
                            parent.display()
                        );
                        return child_run_output(failure_result(&request, &msg), completion_data);
                    }
                    request.cwd = Some(canonical.to_string_lossy().into_owned());
                } else {
                    request.cwd = Some(cwd_path);
                }
            }
            None => request.cwd = None,
        }
    }
    tracing::info!(
        subagent_id = %request.id,
        reasoning_effort = ?effective_runtime.reasoning_effort,
        capability_mode = ?effective_runtime.capability_mode,
        "Resolved subagent runtime configuration"
    );
    // Preserve the normalized, confinement-checked initial RWX on the
    // definition for session-state construction.
    definition.capability_mode = Some(effective_runtime.capability_mode);
    let child_depth = request
        .runtime_overrides
        .spawn_depth
        .unwrap_or(ctx.parent_depth + 1);
    let allow_nested_subagents = child_depth < ctx.subagents_max_depth;
    tracing::info!(
        subagent_id = %request.id,
        capability_mode = ?effective_runtime.capability_mode,
        visible_tools = definition.tool_config.tools.len(),
        "Configured subagent immutable initial RWX"
    );
    if !allow_nested_subagents {
        tracing::info!(
            subagent_id = %request.id,
            child_depth,
            "Marked task tool forbidden for child at max depth"
        );
    }
    // Ordinary Task forks pin the live parent model to preserve exact radix
    // reuse. A Workflow Run already owns a durable route snapshot; replacing
    // it with the parent's later model selection would make phases of one Run
    // nondeterministic. The forked conversation remains valid without cache
    // reuse, so Workflow ownership wins here.
    if request.fork_context && !request.owner.is_workflow() {
        effective_runtime.model = Some(ctx.model_id.0.to_string());
    }
    let workflow_model = resume_source
        .as_ref()
        .map(|source| source.model_id.as_str())
        .or(effective_runtime.model.as_deref());
    let resolved_model = if request.owner.is_workflow() {
        match workflow_model {
            Some(model_id) => resolve_workflow_sampler(&request, model_id, &ctx),
            None => Err("Workflow child request has no captured model route".to_owned()),
        }
    } else {
        resolve_effective_model_config(
            effective_runtime.model.as_deref(),
            &request.subagent_type,
            &ctx,
        )
        .await
        .map(|(sampling_config, model_id)| {
            let auto_compact_threshold_percent =
                ctx.resolve_auto_compact_threshold_percent(&model_id.0);
            let inference_idle_timeout = std::time::Duration::from_secs(
                ctx.resolve_inference_idle_timeout_secs(&model_id.0),
            );
            let max_retries = sampler::resolve_max_retries(sampling_config.max_retries);
            crate::agent::models::PublishedSessionRoute {
                model_id,
                sampling_config,
                image_description_model: ctx.image_description_model.clone(),
                inference_idle_timeout,
                max_retries,
                auto_compact_threshold_percent,
            }
        })
    };
    let mut effective_route = match resolved_model {
        Ok(resolved) => resolved,
        Err(error) => {
            return child_run_output(failure_result(&request, &error), completion_data);
        }
    };
    let mut effective_sampling_config = effective_route.sampling_config.clone();
    let mut effective_model_id = effective_route.model_id.clone();
    let subagent_max_turns = resolve_subagent_max_turns(definition.max_turns, ctx.parent_max_turns);
    // Workflow routes came from the immutable Run catalog; ordinary explicit
    // routes came from the live catalog. The remaining route is an
    // actor-committed parent snapshot. None may be reassembled from separately
    // observed catalog epochs.
    if let Some(ref source) = resume_source
        && effective_model_id.0.as_ref() != source.model_id.as_str()
    {
        let source_model = &source.model_id;
        if request.owner.is_workflow() {
            let resolved = match resolve_workflow_sampler(&request, source_model, &ctx) {
                Ok(resolved) => resolved,
                Err(message) => {
                    return child_run_output(failure_result(&request, &message), completion_data);
                }
            };
            effective_route = resolved;
            effective_sampling_config = effective_route.sampling_config.clone();
            effective_model_id = effective_route.model_id.clone();
        } else if let Some(resolved) = resolve_model_override_to_config(source_model, &ctx) {
            tracing::info!(
                subagent_id = %request.id,
                resolved_model = %effective_model_id.0,
                source_model = source_model,
                "Pinning resumed child to source model"
            );
            effective_sampling_config = resolved.0;
            effective_model_id = resolved.1;
            effective_route.model_id = effective_model_id.clone();
            effective_route.sampling_config = effective_sampling_config.clone();
            effective_route.auto_compact_threshold_percent =
                ctx.resolve_auto_compact_threshold_percent(&effective_model_id.0);
            effective_route.inference_idle_timeout = std::time::Duration::from_secs(
                ctx.resolve_inference_idle_timeout_secs(&effective_model_id.0),
            );
            effective_route.max_retries =
                sampler::resolve_max_retries(effective_sampling_config.max_retries);
        } else {
            let msg = format!(
                "Cannot resume from subagent '{}': source model '{source_model}' \
                 is no longer available in the model catalogue.",
                source.subagent_id,
            );
            return child_run_output(failure_result(&request, &msg), completion_data);
        }
    }
    if let Some(source) = resume_source.as_ref() {
        if !request.owner.is_workflow()
            && let Some(effort) = source.reasoning_effort
            && !ctx
                .models_manager
                .model_offers_reasoning_effort(effective_model_id.0.as_ref(), effort)
        {
            let message = format!(
                "Cannot resume from subagent '{}': source reasoning effort '{}' is no longer offered by model '{}'.",
                source.subagent_id, effort, effective_model_id.0,
            );
            return child_run_output(failure_result(&request, &message), completion_data);
        }
        effective_sampling_config.reasoning_effort = source.reasoning_effort;
    } else if let Some(policy) = effective_runtime.reasoning_effort.as_ref() {
        if let Some(raw) = policy.as_deref() {
            let effort = match raw.parse::<ReasoningEffort>() {
                Ok(effort) => effort,
                Err(error) => {
                    return child_run_output(
                        failure_result(
                            &request,
                            &format!("Invalid subagent reasoning effort '{raw}': {error}"),
                        ),
                        completion_data,
                    );
                }
            };
            if !request.owner.is_workflow()
                && !ctx
                    .models_manager
                    .model_offers_reasoning_effort(effective_model_id.0.as_ref(), effort)
            {
                return child_run_output(
                    failure_result(
                        &request,
                        &format!(
                            "Subagent reasoning effort '{effort}' is not offered by model '{}'.",
                            effective_model_id.0
                        ),
                    ),
                    completion_data,
                );
            }
            effective_sampling_config.reasoning_effort = Some(effort);
        } else {
            effective_sampling_config.reasoning_effort = None;
        }
    }
    let model_transport_key = sampling_types::model_image_input_key_from_parts(
        &effective_sampling_config.model,
        &effective_sampling_config.api_backend,
        &effective_sampling_config.base_url,
        &effective_sampling_config.query_params,
    );
    let expected_transport_key = resume_source
        .as_ref()
        .map(|source| &source.model_transport_key)
        .or(request.runtime_overrides.model_transport_key.as_ref());
    if let Some(expected) = expected_transport_key
        && expected != &model_transport_key
    {
        let origin = resume_source.as_ref().map_or_else(
            || "Workflow Run snapshot".to_owned(),
            |source| format!("resumed subagent '{}'", source.subagent_id),
        );
        let message = format!(
            "Cannot start subagent '{}': model transport for '{}' changed since the {origin} was recorded.",
            request.id, effective_model_id.0,
        );
        return child_run_output(failure_result(&request, &message), completion_data);
    }
    let subagent_id = request.id.clone();
    let child_session_id = acp::SessionId::new(subagent_id.clone());
    let override_cwd = select_override_cwd(
        resume_source.as_ref().map(|source| &source.data),
        request.cwd.as_deref(),
    );
    let effective_cwd = resolve_child_cwd(worktree_path.as_deref(), override_cwd, &ctx.parent_cwd)
        .to_string_lossy()
        .into_owned();
    let child_session_info = SessionInfo {
        id: child_session_id.clone(),
        cwd: effective_cwd,
    };
    let child_session_dir = session::persistence::session_dir(&child_session_info);
    let InitialContext {
        source: context_source,
        source_ref,
        prefix_len: mut inherited_prefix_len,
        conversation: mut forked_conversation,
        prompt_blobs: inherited_prompt_blobs,
        verbatim_fork: context_verbatim_fork,
    } = match bootstrap_initial_context(
        &request,
        resume_source.as_ref(),
        &ctx,
        effective_sampling_config.context_window,
    )
    .await
    {
        BootstrapInitialContext::Ready(ctx) => ctx,
        BootstrapInitialContext::Abort(msg) => {
            tracing::error!(
                subagent_id = %request.id,
                error = %msg,
                "Requested child lineage failed, aborting subagent spawn"
            );
            return child_run_output(failure_result(&request, &msg), completion_data);
        }
    };
    let verbatim_mirror_fork =
        context_source == InitialContextSource::Forked && context_verbatim_fork;
    let Some(child_system_head) = (agent::PromptContext {
        audience: agent::prompt::context::PromptAudience::Subagent,
        ..Default::default()
    })
    .render() else {
        return child_run_output(
            failure_result(&request, "failed to render the stable child System head"),
            completion_data,
        );
    };
    if let Err(message) = seed_child_system_head(
        &context_source,
        verbatim_mirror_fork,
        &mut forked_conversation,
        &mut inherited_prefix_len,
        &child_system_head,
    ) {
        return child_run_output(failure_result(&request, &message), completion_data);
    }
    let task_prompt_text = prompt.clone();
    let inherited_prefix_len = inherited_prefix_len.unwrap_or(0);
    let effective_source_str = match &context_source {
        InitialContextSource::New => "new",
        InitialContextSource::Forked => "forked",
        InitialContextSource::Resumed => "resumed",
    };
    let timeline_context_source = match &context_source {
        InitialContextSource::New => chat_state::SubagentContextSource::New,
        InitialContextSource::Forked => chat_state::SubagentContextSource::Forked,
        InitialContextSource::Resumed => chat_state::SubagentContextSource::Resumed,
    };
    let context_normalized = fork_context_normalized(&context_source, context_verbatim_fork);
    let capability_mode = Some(effective_runtime.capability_mode.as_str().to_owned());
    let permission_mode = serde_json::to_value(ctx.subagent_permission_mode)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned));
    let effective_permission_mode = ctx.permission_handle.as_ref().map(|permissions| {
        match permissions.effective_request_mode(Some(ctx.subagent_permission_mode)) {
            workspace::permission::types::EffectivePermissionMode::Ask => "ask",
            workspace::permission::types::EffectivePermissionMode::Auto => "auto",
            workspace::permission::types::EffectivePermissionMode::AlwaysApprove => {
                "always-approve"
            }
        }
        .to_owned()
    });
    let workflow_run_id = request.owner.workflow_run_id().map(str::to_string);
    let workflow_runtime_route = workflow_run_id.as_deref().and_then(|run_id| {
        ctx.workflow_tracker.as_ref().and_then(|tracker| {
            tracker
                .lock()
                .get(run_id)
                .map(|state| state.runtime_route.clone())
        })
    });
    if workflow_run_id.is_some() && workflow_runtime_route.is_none() {
        return child_run_output(
            failure_result(
                &request,
                "Workflow Run route disappeared before child spawn",
            ),
            completion_data,
        );
    }
    let child_model_state = match workflow_runtime_route.as_ref() {
        Some(route) => match route.model_state_for(
            effective_model_id.0.as_ref(),
            effective_sampling_config.reasoning_effort,
        ) {
            Ok(state) => state,
            Err(message) => {
                return child_run_output(failure_result(&request, &message), completion_data);
            }
        },
        None => model_state_for_catalog(
            &ctx.available_models,
            &effective_model_id,
            effective_sampling_config.reasoning_effort,
        ),
    };
    let effective_model_name = effective_model_id.0.to_string();
    let goal_id = request.owner.goal_id().map(str::to_string);
    let Some(parent_chat_state) = ctx.parent_chat_state.as_ref() else {
        return child_run_output(
            failure_result(
                &request,
                "Cannot persist subagent spawn: parent Timeline is unavailable",
            ),
            completion_data,
        );
    };
    let parent_spawn = match parent_chat_state
        .record_timeline_event_durably(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Spawned(chat_state::SubagentSpawnEvent {
                subagent_id: subagent_id.clone(),
                child_session_id: child_session_id.0.to_string(),
                security_parent_session_id: ctx.security_parent_session_id.clone(),
                subagent_type: request.subagent_type.clone(),
                description: request.description.clone(),
                prompt: request.prompt.clone(),
                context_source: timeline_context_source,
                source_ref: source_ref.clone(),
                context_normalized,
                resumed_from: request.resume_from.clone(),
                parent_prompt_id: request.parent_prompt_id.clone(),
                capability_mode: capability_mode.clone(),
                permission_mode: permission_mode.clone(),
                effective_permission_mode: effective_permission_mode.clone(),
                workflow_run_id: workflow_run_id.clone(),
                goal_id: goal_id.clone(),
                goal_definition_revision: request.owner.goal_definition_revision(),
                surface_completion: request.surface_completion,
                child_cwd: child_session_info.cwd.clone(),
                worktree_path: worktree_path
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned()),
                effective_model_id: effective_model_id.0.to_string(),
                model_transport_key: model_transport_key.clone(),
                reasoning_effort: effective_sampling_config.reasoning_effort,
            }),
        ))
        .await
    {
        Ok(event) => event,
        Err(error) => {
            let message = format!("Cannot persist subagent spawn in parent Timeline: {error}");
            return child_run_output(failure_result(&request, &message), completion_data);
        }
    };
    let subagent_seed = chat_state::SubagentSeedEvent {
        parent_timeline_id: ctx.parent_session_id.clone(),
        parent_spawn_seq: parent_spawn.seq.get(),
        subagent_id: subagent_id.clone(),
        security_parent_session_id: ctx.security_parent_session_id.clone(),
        context_source: timeline_context_source,
        source_ref,
        normalized: context_normalized,
    };

    // The parent spawn is the write-ahead intent. Only after it commits may
    // an isolated worktree be created or rehydrated. Any materialization
    // failure closes the parent lifecycle explicitly, leaving no orphan
    // filesystem mutation without a canonical owner.
    let materialization_error = match worktree_materialization {
        None => None,
        Some(WorktreeMaterialization::Rehydrate { snapshot_ref }) => {
            let target = worktree_path
                .as_deref()
                .expect("rehydration always has a planned target");
            let source_repo = resolve_subagent_source_repo(&ctx);
            match crate::session::worktree::rehydrate_subagent_worktree(
                target,
                &source_repo,
                &snapshot_ref,
                resume_source
                    .as_ref()
                    .map(|source| source.subagent_id.as_str()),
            )
            .await
            {
                Ok(path) if path == target => {
                    tracing::info!(
                        subagent_id = %request.id,
                        worktree_path = %path.display(),
                        snapshot_ref = %snapshot_ref,
                        "Rehydrated subagent worktree from snapshot for resume"
                    );
                    None
                }
                Ok(path) => Some(format!(
                    "Cannot resume isolated subagent: worktree materialized at '{}', expected '{}'",
                    path.display(),
                    target.display()
                )),
                Err(error) => Some(format!(
                    "Cannot resume isolated subagent: failed to rehydrate worktree: {error}"
                )),
            }
        }
        Some(WorktreeMaterialization::Create { source }) => {
            let target = worktree_path
                .as_deref()
                .expect("worktree creation always has a planned target")
                .to_path_buf();
            let target_for_task = target.clone();
            let subagent_id = request.id.clone();
            let creation_mode: fast_worktree::CreationMode = ctx.worktree_type.into();
            let btrfs_delegate = crate::session::worktree::btrfs_delegate_from_env();
            match tokio::task::spawn_blocking(move || {
                let mut builder = fast_worktree::WorktreeBuilder::new(&source, &target_for_task)
                    .working_tree_mode(fast_worktree::WorkingTreeMode::PreserveWorkingTree)
                    .creation_mode(creation_mode)
                    .worktree_kind(fast_worktree::WorktreeKind::Subagent)
                    .session_id(subagent_id);
                if let Some(delegate) = btrfs_delegate {
                    builder = builder.btrfs_delegate(delegate);
                }
                builder.create()
            })
            .await
            {
                Ok(Ok(report)) if report.worktree_path == target => {
                    tracing::info!(
                        subagent_id = %request.id,
                        worktree_path = %report.worktree_path.display(),
                        commit = %report.commit,
                        "Created isolated worktree for subagent"
                    );
                    None
                }
                Ok(Ok(report)) => Some(format!(
                    "Isolated subagent worktree materialized at '{}', expected '{}'",
                    report.worktree_path.display(),
                    target.display()
                )),
                Ok(Err(error)) => {
                    Some(format!("Cannot create isolated subagent worktree: {error}"))
                }
                Err(error) => Some(format!("Isolated subagent worktree task failed: {error}")),
            }
        }
    };
    if let Some(message) = materialization_error {
        let result = failure_result(&request, &message);
        if record_parent_subagent_end(parent_chat_state, &result, None, None)
            .await
            .is_ok()
        {
            completion_data.mark_terminal_committed();
            admit_completion_receipt_before_result(&request, &result, &mut completion_data).await;
        }
        return child_run_output(result, completion_data);
    }

    let (persistence, child_timeline_events, child_session_directory) =
        match session::persistence::new_child(
            &child_session_info,
            effective_model_id.clone(),
            session::persistence::SessionLineage {
                session_kind: match &context_source {
                    InitialContextSource::New => "subagent",
                    InitialContextSource::Forked => "subagent_fork",
                    InitialContextSource::Resumed => "subagent_resume",
                }
                .to_string(),
                context_source: effective_source_str.to_string(),
                parent_session_id: ctx.parent_session_id.clone(),
                parent_prompt_id: request.parent_prompt_id.clone(),
                subagent_seed,
            },
            forked_conversation.clone(),
            inherited_prompt_blobs,
        )
        .await
        {
            Ok(p) => p,
            Err(e) => {
                let msg = format!("Persistence error: {e}");
                let result = failure_result(&request, &msg);
                if record_parent_subagent_end(parent_chat_state, &result, None, None)
                    .await
                    .is_ok()
                {
                    completion_data.mark_terminal_committed();
                    admit_completion_receipt_before_result(&request, &result, &mut completion_data)
                        .await;
                }
                return child_run_output(result, completion_data);
            }
        };
    let child_cwd = resolve_child_cwd(worktree_path.as_deref(), override_cwd, &ctx.parent_cwd);
    let cwd_outside_parent = match (
        dunce::canonicalize(&child_cwd),
        dunce::canonicalize(&ctx.parent_cwd),
    ) {
        (Ok(child), Ok(parent)) => !child.starts_with(&parent),
        _ => child_cwd != ctx.parent_cwd,
    };
    let subagent_fs_watch = FsWatchCapabilities {
        hunk_tracking: ctx.hunk_tracking_enabled && cwd_outside_parent,
        ..FsWatchCapabilities::none()
    };
    let child_cwd_abs = paths::AbsPathBuf::new(child_cwd).unwrap_or_else(|_| {
        paths::AbsPathBuf::new(std::env::current_dir().unwrap_or_default())
            .expect("current_dir should be absolute")
    });
    let mut tool_ctx = ToolContext::with_preloaded_env(
        child_cwd_abs,
        Some(gateway.clone()),
        Some(child_session_id.clone()),
        ctx.fs.clone(),
        ctx.terminal.clone(),
        ctx.hunk_tracker_handle.clone(),
        (*ctx.session_env).clone(),
    )
    .with_hunk_tracking_enabled(ctx.hunk_tracking_enabled);
    tool_ctx.subagent_event_tx = Some(ctx.subagent_event_tx.clone());
    let task_output_budget = request
        .runtime_overrides
        .output_token_budget
        .map(crate::tools::tool_context::TaskOutputTokenBudget::limited);
    tool_ctx.task_output_token_budget = task_output_budget.clone();
    tool_ctx.sampler_retry_only_before_output = task_output_budget.is_some();
    tool_ctx.subagent_depth = child_depth;
    tool_ctx.lsp = ctx.lsp.clone();
    tool_ctx.process_scope = ctx.process_scope.clone();
    let tracker_model_id = effective_model_id.0.to_string();
    let credentials = chat_state::Credentials {
        api_key: effective_sampling_config.api_key.clone(),
        alpha_test_key: ctx.alpha_test_key.clone(),
    };
    ::diagnostics::unified_log::info(
        "subagent spawn credentials",
        None,
        Some(serde_json::json!({
            "subagent_id": &request.id,
            "subagent_type": &request.subagent_type,
            "effective_model": effective_model_id.0.as_ref(),
            "effective_model_raw": &effective_sampling_config.model,
            "base_url": &effective_sampling_config.base_url,
            "key_prefix": key_prefix(&effective_sampling_config.api_key),
            "auth_method_id": ctx.auth_method_id.0.as_ref(),
            "parent_model": ctx.model_id.0.as_ref(),
            "parent_key_prefix": key_prefix(&ctx.sampling_config.api_key),
            "context_window": effective_sampling_config.context_window,
        })),
    );
    // Freeze the author/policy-derived capability ceiling before agent memory
    // or any other session convenience injects concrete tools.
    if definition.authored_capability_tools.is_none() {
        definition.authored_capability_tools = Some(definition.tool_config.clone());
    }
    let agent_memory_scope = definition.memory;
    let agent_name_for_memory = definition.name.clone();
    if let Some(scope) = agent_memory_scope {
        let memory_tools: Vec<tools::registry::types::ToolConfig> = vec![
            (&grow_build::ReadFileTool).into(),
            (&grow_build::SearchReplaceTool).into(),
            (&grow_build::WriteTool).into(),
        ];
        for tc in memory_tools {
            if !definition.tool_config.tools.iter().any(|t| t.id == tc.id) {
                definition.tool_config.tools.push(tc);
            }
        }
        let resolved_mem = scope.resolve_dir(&agent_name_for_memory, &ctx.parent_cwd);
        let memory_dir = &resolved_mem.path;
        let memory_md = memory_dir.join("MEMORY.md");
        if memory_md.is_file()
            && let Ok(content) = std::fs::read_to_string(&memory_md)
        {
            const MAX_LINES: usize = 200;
            const MAX_BYTES: usize = 25 * 1024;
            let truncated: String = content
                .lines()
                .take(MAX_LINES)
                .collect::<Vec<_>>()
                .join("\n");
            let truncated = tools::util::truncate::truncate_str(&truncated, MAX_BYTES).to_string();
            if !truncated.is_empty() {
                let injection = format!(
                    "\n\n<agent-memory>\nMemory directory: {}\n\n{truncated}\n</agent-memory>",
                    memory_dir.display()
                );
                definition.prompt_body =
                    Some(definition.prompt_body.unwrap_or_default() + injection.as_str());
            }
        }
    }
    let is_plugin_agent = definition.plugin_name.is_some();
    if let Some(ref hooks_config) = definition.hooks {
        if is_plugin_agent {
            tracing::warn!(
                agent = %definition.name,
                plugin = ?definition.plugin_name,
                "ignoring hooks on plugin agent (not supported for security)"
            );
        } else if !crate::agent::folder_trust::agent_inline_hooks_allowed(definition.scope, || {
            crate::agent::folder_trust::project_scope_allowed(&ctx.parent_cwd)
        }) {
            tracing::warn!(
                agent = %definition.name,
                "ignoring hooks on untrusted project agent (folder not trusted; re-run with --trust)"
            );
        } else {
            let hooks_val = hooks_config.as_value();
            let (specs, errors) = ::hooks::config::parse_hooks_from_value_with_dir(
                &hooks_val,
                &format!("{}{}", ::hooks::config::AGENT_HOOK_PREFIX, definition.name),
                &ctx.parent_cwd,
            );
            for e in &errors {
                tracing::warn!(agent = %definition.name, error = ?e, "agent hook parse error");
            }
            if !specs.is_empty() {
                let specs: Vec<_> = specs
                    .into_iter()
                    .map(|mut s| {
                        if s.event == ::hooks::event::HookEventName::Stop {
                            s.event = ::hooks::event::HookEventName::SubagentStop;
                        }
                        s
                    })
                    .collect();
                let mut registry = ctx
                    .hook_registry
                    .as_ref()
                    .map(|r| (**r).clone())
                    .unwrap_or_default();
                registry.append_specs(specs);
                ctx.hook_registry = Some(std::sync::Arc::new(registry));
            }
        }
    }
    if !definition.mcp_servers.is_empty() {
        tracing::warn!(
            agent = %definition.name,
            plugin = ?definition.plugin_name,
            "ignoring child-owned mcpServers; subagents only inherit connected parent servers"
        );
    }
    let agent_mcp_servers: Vec<agent_client_protocol::McpServer> = Vec::new();
    let mut parent_mcp_pool =
        resolve_inherited_mcp_pool(ctx.parent_mcp_pool.take(), &definition.mcp_inheritance);
    if let (Some(pool), Some(ceiling)) = (
        parent_mcp_pool.as_mut(),
        ctx.parent_capability_ceiling.as_ref(),
    ) {
        let allowed = pool
            .eligibility()
            .current_clients()
            .into_iter()
            .filter_map(|(server, _, client_id)| {
                ceiling
                    .permits_mcp_binding(&server, client_id)
                    .then_some(server)
            })
            .collect::<Vec<_>>();
        pool.restrict_to_servers(allowed);
    }
    let mcp_inherited_count = parent_mcp_pool
        .as_ref()
        .map(|p| p.len() as u32)
        .unwrap_or(0);
    if mcp_inherited_count > 0 {
        tracing::info!(
            subagent_id = %request.id,
            mcp_count = mcp_inherited_count,
            "Subagent inherited MCP servers from parent pool"
        );
    }
    let inherit_skills = definition.inherit_skills;
    let definition_background = definition.background.unwrap_or(false);
    if frozen_workflow_skills.is_none() && inherit_skills && ctx.parent_skills.is_none() {
        let parent_cwd_str = ctx.parent_cwd.to_string_lossy().to_string();
        ctx.parent_skills = Some(
            agent::prompt::skills::list_skills_with_plugins(
                Some(&parent_cwd_str),
                &ctx.parent_skills_config,
                ctx.plugin_registry.as_deref(),
            )
            .await,
        );
    }
    let skills_inherited_count = if inherit_skills {
        ctx.parent_skills
            .as_ref()
            .map(|s| s.len() as u32)
            .unwrap_or(0)
    } else {
        0
    };
    if skills_inherited_count > 0 {
        tracing::info!(
            subagent_id = %request.id,
            skills_count = skills_inherited_count,
            "Subagent inherited skills from parent"
        );
    }
    let mcp_owned_count = 0;
    ::diagnostics::session_ctx::log_event(::diagnostics::events::SubagentLaunched {
        subagent_id: request.id.clone(),
        parent_session_id: request.parent_session_id.clone(),
        subagent_type: request.subagent_type.clone(),
        fork_context: matches!(context_source, InitialContextSource::Forked),
        resume_from: request.resume_from.clone(),
        isolated_worktree: worktree_path.is_some(),
        mcp_inherited_count,
        mcp_owned_count,
        skills_inherited_count,
    });
    let effective_inference_idle_timeout_secs = effective_sampling_config
        .idle_timeout_secs
        .unwrap_or(effective_route.inference_idle_timeout.as_secs());
    let _ = persistence
        .tx
        .send(crate::session::persistence::PersistenceMsg::CurrentModel {
            model_id: effective_model_id.clone(),
            agent_name: Some(definition.selector_identity()),
            reasoning_effort: Some(effective_sampling_config.reasoning_effort),
        });
    let recovery_persistence = persistence.clone();
    let recovery_timeline_events = child_timeline_events.clone();
    let spawn_result = session::spawn_session_on_thread(
        child_session_info,
        child_session_dir.clone(),
        gateway.clone(),
        effective_sampling_config,
        credentials,
        crate::agent::auth_method::new_shared_auth_method_id(Some(ctx.auth_method_id.clone())),
        tool_ctx,
        agent_mcp_servers,
        vec![],
        Default::default(),
        parent_mcp_pool,
        Vec::new(),
        true,
        None,
        persistence,
        None,
        crate::session::TimelineBootstrap::Existing(child_timeline_events),
        None,
        None,
        crate::session::StartupHints {
            inherited_prefix_len: Some(inherited_prefix_len),
            is_subagent: true,
            parent_session_id: Some(ctx.parent_session_id.clone()),
            subagent_type: Some(request.subagent_type.clone()),
            workflow_run_id: request.owner.workflow_run_id().map(str::to_owned),
            workflow_runtime_route,
            delegated_goal: request.owner.goal_id().is_some(),
            goal_usage_window: Some(ctx.goal_usage_window.clone()),
            subagent_permission_mode: Some(ctx.subagent_permission_mode),
            subagent_description: Some(request.description.clone()),
            preserve_inherited_system: verbatim_mirror_fork,
            ..Default::default()
        },
        workspace::permission::ClientType::Generic,
        ctx.permission_prompt_timeout,
        effective_route.auto_compact_threshold_percent,
        agent::DEFAULT_SYSTEM_PROMPT_LABEL.to_string(),
        ctx.resolve_compaction_verbatim_input(),
        ctx.resolve_compaction_pre_prune(),
        ctx.resolve_compaction_pre_prune_token_budget(),
        None,
        None,
        std::sync::Arc::new(parking_lot::Mutex::new(
            workspace::file_system::CodebaseIndexManager::new(),
        )),
        false,
        subagent_fs_watch,
        false,
        false,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
        definition,
        if inherit_skills {
            ctx.parent_skills_config.clone()
        } else {
            agent::prompt::skills::SkillsConfig::default()
        },
        frozen_workflow_skills
            .or_else(|| inherit_skills.then(|| ctx.parent_skills.take()).flatten()),
        false,
        None,
        None,
        None,
        0,
        Vec::new(),
        None,
        if verbatim_mirror_fork {
            None
        } else if let Some(scope) = agent_memory_scope {
            ctx.memory_config.as_ref().map(|mc| {
                let mut c = mc.clone();
                let resolved = scope.resolve_dir(&agent_name_for_memory, &ctx.parent_cwd);
                c.enabled = true;
                c.root_dir_override = Some(resolved.path);
                c.flat_memory_root = resolved.is_project_scoped;
                c
            })
        } else {
            ctx.memory_config.clone()
        },
        effective_model_id,
        ctx.permission_mode,
        None,
        effective_inference_idle_timeout_secs,
        Some(effective_route.max_retries),
        ctx.web_fetch_config.clone(),
        ctx.app_builder_deployer_config.clone(),
        ctx.write_file_enabled,
        ctx.goal_enabled,
        ctx.background_workflows_enabled && !request.owner.is_workflow(),
        true,
        ctx.subagents_max_depth,
        crate::config::SubagentClassifierInput::Context,
        ctx.ask_user_question_enabled,
        ctx.client_hooks.clone(),
        None,
        ctx.subagent_toggle.clone(),
        ctx.agent_config
            .as_ref()
            .map(|config| config.cli_agents.clone())
            .unwrap_or_default(),
        ctx.agent_config
            .as_ref()
            .map(|config| config.cli_agent_overrides.clone())
            .unwrap_or_default(),
        ctx.file_tool_overrides.clone(),
        frozen_subagent_names,
        agent::prompt::context::PromptAudience::Subagent,
        ctx.respect_gitignore,
        ctx.path_not_found_hints,
        ctx.resolve_tool_params_json(),
        ctx.plugin_registry.clone(),
        None,
        ctx.models_manager.clone(),
        ctx.permission_handle.clone(),
        None,
        effective_route.image_description_model.clone(),
        ctx.hook_registry.clone(),
        ctx.workspace_ops.clone(),
        vec![],
        std::mem::take(&mut ctx.remote_settings),
        std::mem::take(&mut ctx.laziness_debug_log),
        ctx.parent_terminal_backend.clone(),
        None,
        subagent_max_turns,
    )
    .await;
    let (mut child_handle, child_thread) = match spawn_result {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("Failed to spawn child session: {e}");
            let result = fail_subagent(
                &msg,
                &subagent_id,
                &child_session_id,
                start.elapsed().as_millis() as u64,
            );
            let result_ref = record_child_result_with_persistence(
                &recovery_persistence,
                recovery_timeline_events,
                &result,
            )
            .await
            .ok();
            if record_parent_subagent_end(parent_chat_state, &result, result_ref, None)
                .await
                .is_ok()
            {
                completion_data.mark_terminal_committed();
                admit_completion_receipt_before_result(&request, &result, &mut completion_data)
                    .await;
            }
            return child_run_output(result, completion_data);
        }
    };
    if request.owner.is_workflow()
        && let Some(tracker) = ctx.workflow_tracker.clone()
    {
        child_handle.workflow_tracker = tracker;
    }
    let _active_child_registration = ActiveChildRegistration::install(
        ctx.active_child_sessions.clone(),
        child_session_id.clone(),
        child_handle.clone(),
    );
    if let Err(error) = catch_up_child_catalog_generation(&ctx, &child_handle).await {
        let _ = child_handle.cmd_tx.send(SessionCommand::Shutdown);
        drop(child_thread);
        let message = format!("Failed to converge child model catalog: {error}");
        let result = fail_subagent(
            &message,
            &subagent_id,
            &child_session_id,
            start.elapsed().as_millis() as u64,
        );
        let result_ref = record_child_result_with_persistence(
            &recovery_persistence,
            recovery_timeline_events,
            &result,
        )
        .await
        .ok();
        if record_parent_subagent_end(parent_chat_state, &result, result_ref, None)
            .await
            .is_ok()
        {
            completion_data.mark_terminal_committed();
            admit_completion_receipt_before_result(&request, &result, &mut completion_data).await;
        }
        return child_run_output(result, completion_data);
    }
    let promoted = reporter
        .started(StartedChild {
            child_session_id: child_session_id.0.to_string(),
            resumed_from: request.resume_from.clone(),
            definition_background,
            control: ShellChildRuntime {
                child_handle: child_handle.clone(),
                _child_thread: child_thread,
            },
        })
        .await;
    if !promoted {
        if let Some(permission_handle) = &ctx.permission_handle {
            permission_handle.release_child(child_session_id.0.to_string());
        }
        ctx.workspace_ops
            .end_local_session(child_session_id.0.as_ref());
        let result = cancel_pending_shell_child(
            &subagent_id,
            &child_session_id,
            worktree_path.as_deref(),
            worktree_freshly_created,
            start.elapsed().as_millis() as u64,
        )
        .await;
        let result_ref = record_child_result(&child_handle.chat_state_handle, &result, None)
            .await
            .ok();
        let _ = child_handle.cmd_tx.send(SessionCommand::Shutdown);
        if record_parent_subagent_end(parent_chat_state, &result, result_ref, None)
            .await
            .is_ok()
        {
            completion_data.mark_terminal_committed();
            admit_completion_receipt_before_result(&request, &result, &mut completion_data).await;
        }
        return child_run_output(result, completion_data);
    }
    // Publish an interactive child only after its exact handle has been
    // registered, its model catalog has converged, and the runtime owner has
    // accepted it. Timeline admission remains the earlier durable write-ahead
    // fact, but Pager can now act on this projection immediately without an
    // `unknown session id` race.
    emit_subagent_notification(
        gateway,
        &ctx.parent_session_id,
        SessionUpdate::SubagentSpawned {
            subagent_id: subagent_id.clone(),
            child_session_id: child_session_id.0.to_string(),
            parent_session_id: ctx.parent_session_id.clone(),
            parent_prompt_id: request.parent_prompt_id.clone(),
            subagent_type: request.subagent_type.clone(),
            description: request.description.clone(),
            effective_context_source: Some(effective_source_str.to_string()),
            context_normalized,
            capability_mode,
            permission_mode,
            effective_permission_mode,
            model: Some(effective_model_name),
            model_state: Some(child_model_state),
            workflow_agent_names,
            resumed_from: request.resume_from.clone(),
            workflow_run_id,
            goal_id,
        },
        ctx.parent_cmd_tx.as_ref(),
    );
    completion_data.spawned_notification_emitted = true;
    spawn_progress_publisher(
        child_handle.signals_handle.clone(),
        gateway.clone(),
        ctx.parent_session_id.clone(),
        request.id.clone(),
        child_session_id.0.to_string(),
        start,
        cancel_token.clone(),
    );
    if let Some(snapshot) = request.goal_context.clone() {
        let _ = child_handle
            .cmd_tx
            .send(SessionCommand::SetGoalContextSnapshot { snapshot });
    }
    let (prompt_tx, prompt_rx) = oneshot::channel();
    let prompt_text = task_prompt_text;
    let child_prompt_id = uuid::Uuid::now_v7().to_string();
    let _ = child_handle.cmd_tx.send(SessionCommand::QueuePrompt {
        prompt_id: child_prompt_id.clone(),
        prompt_blocks: vec![acp::ContentBlock::Text(acp::TextContent::new(prompt_text))],
        origin: crate::session::PromptOrigin::User,
        turn_kind: crate::session::TurnKind::Internal,
        client_identifier: None,
        screen_mode: None,
        verbatim: true,
        json_schema: request.runtime_overrides.output_schema.clone(),
        respond_to: prompt_tx,
        persist_ack: None,
    });
    let wait_outcome = await_subagent_turn_or_cancellation(prompt_rx, cancel_token.clone()).await;
    let duration_ms = start.elapsed().as_millis() as u64;
    let mut cancellation_may_hide_usage = false;
    let mut result = match wait_outcome {
        SubagentWaitOutcome::Cancelled => {
            let (tool_calls, turns) = signals_snapshot_counts(&child_handle).await;
            cancellation_may_hide_usage = turns > 0 || tool_calls > 0;
            SubagentResult {
                success: false,
                cancelled: true,
                error: Some("Subagent was cancelled".to_string()),
                subagent_id: request.id.clone(),
                child_session_id: child_session_id.0.to_string(),
                tool_calls,
                turns,
                duration_ms,
                worktree_path: worktree_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string()),
                ..Default::default()
            }
        }
        SubagentWaitOutcome::TurnResult(turn_result) => {
            let was_cancelled = cancel_token.is_cancelled();
            let (tool_calls, turns) = match &*turn_result {
                Ok(Ok(crate::session::commands::PromptTurnOk {
                    turn_snapshot: Some(snapshot),
                    ..
                })) => (
                    snapshot.current.tool_call_count,
                    snapshot.current.turn_count,
                ),
                _ => signals_snapshot_counts(&child_handle).await,
            };
            let final_text = child_handle
                .chat_state_handle
                .get_last_assistant_text()
                .await
                .unwrap_or_default();
            let result_tokens = child_handle.chat_state_handle.get_projected_tokens().await;
            match *turn_result {
                Ok(Ok(crate::session::commands::PromptTurnOk {
                    completion_kind: PromptCompletionKind::Cancelled { category, context },
                    ..
                })) => {
                    cancellation_may_hide_usage = true;
                    let reason = cancellation_error_message(category, context.as_ref());
                    SubagentResult {
                        success: false,
                        cancelled: true,
                        error: Some(reason),
                        output: if final_text.is_empty() {
                            std::sync::Arc::from(format!(
                                "Subagent '{}' ({}) was cancelled. {} tool calls, {} turns.",
                                request.description, request.subagent_type, tool_calls, turns
                            ))
                        } else {
                            std::sync::Arc::from(final_text)
                        },
                        subagent_id: request.id.clone(),
                        child_session_id: child_session_id.0.to_string(),
                        tool_calls,
                        turns,
                        duration_ms,
                        tokens_used: result_tokens,
                        output_tokens_used: 0,
                        output_usage_incomplete: true,
                        total_tokens_used: 0,
                        worktree_path: worktree_path
                            .as_ref()
                            .map(|p| p.to_string_lossy().to_string()),
                        backgrounded: false,
                    }
                }
                Ok(Ok(crate::session::commands::PromptTurnOk {
                    completion_kind: PromptCompletionKind::MaxTurnsReached { limit },
                    ..
                })) => SubagentResult {
                    success: false,
                    cancelled: true,
                    error: Some(format!("max turns reached (limit: {limit})")),
                    output: if final_text.is_empty() {
                        std::sync::Arc::from(format!(
                            "Subagent '{}' ({}) hit max-turns limit ({limit}). {} tool calls, {} turns.",
                            request.description, request.subagent_type, tool_calls, turns
                        ))
                    } else {
                        std::sync::Arc::from(final_text)
                    },
                    subagent_id: request.id.clone(),
                    child_session_id: child_session_id.0.to_string(),
                    tool_calls,
                    turns,
                    duration_ms,
                    tokens_used: result_tokens,
                    output_tokens_used: 0,
                    output_usage_incomplete: true,
                    total_tokens_used: 0,
                    worktree_path: worktree_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string()),
                    backgrounded: false,
                },
                Ok(Ok(crate::session::commands::PromptTurnOk {
                    structured_output, ..
                })) => {
                    let wanted_schema = request.runtime_overrides.output_schema.is_some();
                    let (success, error, output) = match (wanted_schema, structured_output) {
                        (true, Some(Ok(value))) => {
                            (true, None, std::sync::Arc::from(value.to_string()))
                        }
                        (true, Some(Err(e))) => (
                            false,
                            Some(format!("structured output validation failed: {e}")),
                            std::sync::Arc::from(final_text),
                        ),
                        (true, None) => (
                            false,
                            Some("structured output requested but none produced".to_string()),
                            std::sync::Arc::from(final_text),
                        ),
                        (false, _) => (
                            true,
                            None,
                            if final_text.is_empty() {
                                std::sync::Arc::from(format!(
                                    "Subagent '{}' ({}) completed successfully. {} tool calls, {} turns.",
                                    request.description, request.subagent_type, tool_calls, turns
                                ))
                            } else {
                                std::sync::Arc::from(final_text)
                            },
                        ),
                    };
                    SubagentResult {
                        success,
                        error,
                        output,
                        subagent_id: request.id.clone(),
                        child_session_id: child_session_id.0.to_string(),
                        tool_calls,
                        turns,
                        duration_ms,
                        tokens_used: result_tokens,
                        output_tokens_used: 0,
                        output_usage_incomplete: true,
                        total_tokens_used: 0,
                        worktree_path: worktree_path
                            .as_ref()
                            .map(|p| p.to_string_lossy().to_string()),
                        ..Default::default()
                    }
                }
                Ok(Err(e)) => {
                    cancellation_may_hide_usage = was_cancelled;
                    SubagentResult {
                        success: false,
                        cancelled: was_cancelled,
                        error: Some(if was_cancelled {
                            "Subagent was cancelled".to_string()
                        } else {
                            format!("Session error: {e}")
                        }),
                        subagent_id: request.id.clone(),
                        child_session_id: child_session_id.0.to_string(),
                        tool_calls,
                        turns,
                        duration_ms,
                        worktree_path: worktree_path
                            .as_ref()
                            .map(|p| p.to_string_lossy().to_string()),
                        ..Default::default()
                    }
                }
                Err(_) => {
                    cancellation_may_hide_usage = was_cancelled;
                    SubagentResult {
                        success: false,
                        cancelled: was_cancelled,
                        error: Some(if was_cancelled {
                            "Subagent was cancelled".to_string()
                        } else {
                            "Child session dropped unexpectedly".to_string()
                        }),
                        subagent_id: request.id.clone(),
                        child_session_id: child_session_id.0.to_string(),
                        tool_calls,
                        turns,
                        duration_ms,
                        worktree_path: worktree_path
                            .as_ref()
                            .map(|p| p.to_string_lossy().to_string()),
                        ..Default::default()
                    }
                }
            }
        }
    };
    let snapshot_dispose_enabled = ctx.resolve_subagent_worktree_snapshot_enabled();
    let diagnostics_tokens = if result.tool_calls > 0 || result.success {
        child_handle.chat_state_handle.get_projected_tokens().await
    } else {
        0
    };
    completion_data.diagnostics_tokens = diagnostics_tokens;
    let task_budget_usage = task_output_budget.as_ref().map(|budget| budget.usage());
    let (subagent_usage_by_model, subagent_usage_incomplete, output_tokens_used, total_tokens_used) =
        match child_handle.chat_state_handle.try_get_session_usage().await {
            Ok(u) => {
                let output_tokens = u.totals.output_tokens;
                let total_tokens = canonical_total_tokens(&u.totals);
                let has_usage_entries = !u.by_model.is_empty();
                let usage_incomplete = usage_is_incomplete(
                    u.incomplete,
                    cancellation_may_hide_usage,
                    total_tokens,
                    has_usage_entries,
                );
                (
                    Some(u.by_model.into_iter().collect::<Vec<_>>()),
                    usage_incomplete,
                    (!usage_incomplete).then_some(output_tokens),
                    Some(total_tokens),
                )
            }
            Err(()) => (None, true, None, None),
        };
    result.total_tokens_used = total_tokens_used.unwrap_or(0);
    if let Some((task_spent, task_incomplete)) = task_budget_usage {
        result.output_tokens_used = output_tokens_used.unwrap_or(task_spent);
        result.output_usage_incomplete =
            task_incomplete || subagent_usage_incomplete || output_tokens_used.is_none();
    } else {
        result.output_tokens_used = output_tokens_used.unwrap_or(0);
        result.output_usage_incomplete = subagent_usage_incomplete || output_tokens_used.is_none();
    }
    let persisted_output_ref = match persist_subagent_output(&child_session_directory, &result) {
        Ok(output_ref) => output_ref,
        Err(error) => {
            tracing::error!(subagent_id = %request.id, %error, "subagent output artifact failed");
            result.success = false;
            result.cancelled = false;
            result.error = Some(error);
            None
        }
    };
    completion_data.set_persisted_output_ref(
        persisted_output_ref
            .as_ref()
            .map(|artifact| artifact.timeline_ref.clone()),
    );
    let child_result_ref = record_child_result(
        &child_handle.chat_state_handle,
        &result,
        persisted_output_ref.map(|artifact| artifact.timeline_ref),
    )
    .await
    .map_err(|error| {
        tracing::error!(subagent_id = %request.id, %error, "canonical child result failed");
        error
    })
    .ok();
    let fold_acked = record_subagent_usage(
        ctx.parent_cmd_tx.as_ref(),
        request.id.clone(),
        subagent_usage_by_model,
        request.parent_prompt_id.clone(),
        subagent_usage_incomplete,
    )
    .await;
    if !fold_acked {
        tracing::warn!(
            subagent_id = %request.id,
            parent_prompt_id = ?request.parent_prompt_id,
            "subagent usage not applied; parent bill marked incomplete"
        );
        let sticky_prompt = request.parent_prompt_id.clone();
        let marked_by_parent = if let Some(cmd_tx) = ctx.parent_cmd_tx.as_ref() {
            let (respond_to, ack) = tokio::sync::oneshot::channel();
            if cmd_tx
                .send(
                    crate::session::commands::SessionCommand::MarkSubagentUsageNotApplied {
                        parent_prompt_id: sticky_prompt.clone(),
                        respond_to,
                    },
                )
                .is_ok()
            {
                ack.await.is_ok()
            } else {
                false
            }
        } else {
            false
        };
        if !marked_by_parent && let Some(pid) = sticky_prompt {
            let (respond_to, ack) = tokio::sync::oneshot::channel();
            if ctx
                .subagent_event_tx
                .send(SubagentEvent::MarkUsageNotApplied(
                    SubagentMarkUsageNotAppliedRequest {
                        parent_session_id: ctx.parent_session_id.clone(),
                        prompt_id: pid,
                        respond_to,
                    },
                ))
                .is_ok()
            {
                let _ = ack.await;
            }
        }
    }
    let outcome = if result.success {
        ::diagnostics::events::Outcome::Completed
    } else if result.cancelled {
        ::diagnostics::events::Outcome::Cancelled
    } else {
        ::diagnostics::events::Outcome::Error
    };
    ::diagnostics::session_ctx::log_event(::diagnostics::events::SubagentCompleted {
        subagent_id: request.id.clone(),
        parent_session_id: request.parent_session_id.clone(),
        outcome,
        duration_ms: result.duration_ms,
        tool_calls: result.tool_calls,
        tokens_used: if diagnostics_tokens > 0 {
            Some(diagnostics_tokens)
        } else {
            None
        },
    });
    match (
        &ctx.parent_terminal_backend,
        &ctx.parent_notification_handle,
    ) {
        (Some(parent_tb), Some(parent_notif_handle)) => {
            if !request.surface_completion {
                let reparented_task_ids: Vec<String> = parent_tb
                    .list_tasks()
                    .await
                    .into_iter()
                    .filter(|t| {
                        !t.completed && t.owner_session_id.as_deref() == Some(&*child_session_id.0)
                    })
                    .map(|t| t.task_id)
                    .collect();
                if !reparented_task_ids.is_empty()
                    && let Some(cmd_tx) = ctx.parent_cmd_tx.as_ref()
                    && let Some(goal_id) = request.owner.goal_id()
                {
                    let _ = cmd_tx.send(SessionCommand::RecordGoalOwnedTaskIds {
                        goal_id: goal_id.to_owned(),
                        definition_revision: request
                            .owner
                            .goal_definition_revision()
                            .unwrap_or_default(),
                        task_ids: reparented_task_ids,
                    });
                }
            }
            let parent_backend_weak = std::sync::Arc::downgrade(parent_tb);
            parent_tb
                .reparent_notifications(
                    &child_session_id.0,
                    &ctx.parent_session_id,
                    parent_notif_handle.clone(),
                    parent_backend_weak,
                )
                .await;
        }
        (Some(_), None) | (None, Some(_)) => {
            tracing::warn!(
                child_session_id = %child_session_id.0,
                parent_session_id = %ctx.parent_session_id,
                has_terminal_backend = ctx.parent_terminal_backend.is_some(),
                has_notification_handle = ctx.parent_notification_handle.is_some(),
                "skipping reparent_notifications: parent_terminal_backend and \
                 parent_notification_handle must both be Some"
            );
        }
        (None, None) => {}
    }
    let _ = child_handle.cmd_tx.send(SessionCommand::Shutdown);
    if let Some(permission_handle) = &ctx.permission_handle {
        permission_handle.release_child(child_session_id.0.to_string());
    }
    ctx.workspace_ops
        .end_local_session(child_session_id.0.as_ref());
    let mut disposed_snapshot_ref: Option<String> = None;
    let mut worktree_removed = false;
    if let Some(ref wt_path) = worktree_path {
        if snapshot_dispose_enabled {
            let ref_name = format!("refs/grow/subagents/{}", request.id);
            let source_repo = resolve_subagent_source_repo(&ctx);
            match crate::session::worktree::snapshot_subagent_worktree(
                wt_path,
                &source_repo,
                &ref_name,
            )
            .await
            {
                Ok(snapshot_ref) => disposed_snapshot_ref = Some(snapshot_ref),
                Err(e) => {
                    tracing::warn!(
                        subagent_id = %request.id,
                        worktree_path = %wt_path.display(),
                        error = %e,
                        "Failed to snapshot subagent worktree; preserving for review"
                    );
                }
            }
        } else {
            tracing::info!(
                subagent_id = %request.id,
                worktree_path = %wt_path.display(),
                "Worktree preserved for review"
            );
        }
    }
    let terminal_committed = match child_result_ref {
        Some(result_ref) => record_parent_subagent_end(
            parent_chat_state,
            &result,
            Some(result_ref),
            disposed_snapshot_ref.clone(),
        )
        .await
        .map(|()| true)
        .unwrap_or_else(|error| {
            tracing::error!(subagent_id = %request.id, %error, "canonical parent terminal failed");
            false
        }),
        None => false,
    };
    if terminal_committed {
        completion_data.mark_terminal_committed();
        if let Some(ref wt_path) = worktree_path
            && disposed_snapshot_ref.is_some()
        {
            match crate::session::worktree::remove_subagent_worktree(wt_path).await {
                Ok(()) => {
                    worktree_removed = true;
                    tracing::info!(
                        subagent_id = %request.id,
                        worktree_path = %wt_path.display(),
                        "removed subagent worktree after canonical terminal commit"
                    );
                }
                Err(error) => tracing::warn!(
                    subagent_id = %request.id,
                    worktree_path = %wt_path.display(),
                    %error,
                    "canonical terminal committed but subagent worktree removal failed"
                ),
            }
        }
    } else if worktree_path.is_some() {
        tracing::warn!(
            subagent_id = %request.id,
            "preserving subagent worktree because the parent terminal is not canonical"
        );
    }
    if !terminal_committed {
        let canonical_error = match result.error.take() {
            Some(error) => format!(
                "subagent finished but its canonical child→parent terminal chain did not commit: {error}"
            ),
            None => {
                "subagent finished but its canonical child→parent terminal chain did not commit"
                    .to_owned()
            }
        };
        // The immutable artifact remains available for diagnosis/recovery, but
        // no waiter may consume a successful result that the parent Timeline
        // cannot prove. Recovery will close the still-open spawn from durable
        // child facts on the next load.
        result.success = false;
        result.cancelled = false;
        result.output = std::sync::Arc::from("");
        result.error = Some(canonical_error);
    }
    if worktree_removed {
        result.worktree_path = None;
    }
    let success = result.success && !result.cancelled;
    let preview = crate::util::truncate(&result.output, 200);
    let level_fn = if success {
        ::diagnostics::unified_log::info
    } else {
        ::diagnostics::unified_log::error
    };
    level_fn(
        if success {
            "subagent completed"
        } else {
            "subagent failed"
        },
        None,
        Some(serde_json::json!({
            "subagent_id": &request.id,
            "subagent_type": &request.subagent_type,
            "effective_model": tracker_model_id,
            "success": success,
            "cancelled": result.cancelled,
            "duration_ms": result.duration_ms,
            "turns": result.turns,
            "tool_calls": result.tool_calls,
            "output_preview": preview,
            "error": &result.error,
        })),
    );
    admit_completion_receipt_before_result(&request, &result, &mut completion_data).await;
    child_run_output(result, completion_data)
}
