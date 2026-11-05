//! Resolves and applies a complete model route to one session. Ordinary
//! selections use the catalog generation protected by the caller's lock;
//! Workflow children use their immutable Run route instead.
use crate::agent::mvp_agent::MvpAgent;
use crate::session::{
    ControlIntent, SessionCommand, SessionEffortAuthority, SessionSamplingSelection,
};
use acp_transport::protocol as acp;
use sampling_types::parse_reasoning_effort_meta;
use tokio::sync::oneshot;

#[derive(Debug)]
pub(crate) struct ModelSwitchRequest {
    pub session_id: acp::SessionId,
    pub model_id: crate::agent::models::ModelId,
    pub meta: Option<acp::Meta>,
}

impl ModelSwitchRequest {
    pub(crate) fn new(session_id: acp::SessionId, model_id: crate::agent::models::ModelId) -> Self {
        Self {
            session_id,
            model_id,
            meta: None,
        }
    }

    pub(crate) fn meta(mut self, meta: Option<acp::Meta>) -> Self {
        self.meta = meta;
        self
    }
}

pub(crate) struct EnqueuedModelSwitch {
    session_id: acp::SessionId,
    model_id: crate::agent::models::ModelId,
    previous_model_id: crate::agent::models::ModelId,
    response: oneshot::Receiver<
        Result<crate::session::DesiredStateOutcome<crate::agent::models::ModelId>, acp::Error>,
    >,
}

/// Enqueue while holding the same lock as catalog publication. Resolve any
/// in-flight load before acquiring it, and release it before waiting for finish.
pub(crate) fn enqueue(
    agent: &MvpAgent,
    _catalog_transaction: &tokio::sync::MutexGuard<'_, ()>,
    handle: crate::session::SessionHandle,
    args: ModelSwitchRequest,
) -> Result<EnqueuedModelSwitch, acp::Error> {
    let intent = ControlIntent::from_meta(args.meta.as_ref())?;
    let effort = parse_reasoning_effort_meta(args.meta.as_ref());
    if args
        .meta
        .as_ref()
        .is_some_and(|meta| meta.contains_key(sampling_types::REASONING_EFFORT_META_KEY))
        && effort.is_none()
    {
        return Err(acp::Error::invalid_params()
            .data("reasoningEffort must be a canonical string offered by the selected model"));
    }
    let effort_patch = crate::session::effort_patch_from_meta(args.meta.as_ref())?;
    if effort_patch && effort.is_none() {
        return Err(acp::Error::invalid_params().data(format!(
            "{} requires a reasoning-effort value",
            crate::session::EFFORT_PATCH_META_KEY
        )));
    }
    let selection = prepare_selection(
        agent,
        &handle,
        (!effort_patch).then_some(&args.model_id),
        effort,
    )?;
    send_selection(
        handle,
        args.session_id,
        args.model_id,
        selection,
        intent,
        None,
    )
}

/// Model-facing ingress uses only the host-bound handle and gets a distinct
/// actor-admission receipt. The returned application receiver remains owned
/// by the Shell finish task after the tool returns queued.
pub(crate) fn enqueue_tool(
    agent: &MvpAgent,
    _catalog_transaction: &tokio::sync::MutexGuard<'_, ()>,
    handle: crate::session::SessionHandle,
    model: Option<String>,
    effort: Option<sampling_types::ReasoningEffort>,
    admitted: oneshot::Sender<Result<(), String>>,
) -> Result<EnqueuedModelSwitch, acp::Error> {
    if model.is_none() && effort.is_none() {
        return Err(
            acp::Error::invalid_params().data("at least one of model or effort is required")
        );
    }
    let model_id = model.map(crate::agent::models::ModelId::new);
    if let Some(model_id) = &model_id
        && agent.resolve_model_id(model_id)?.info.hidden
    {
        return Err(
            acp::Error::invalid_params().data("Hidden models cannot be selected by a tool.")
        );
    }
    let selection = prepare_selection(agent, &handle, model_id.as_ref(), effort)?;
    let session_id = handle.info.id.clone();
    let model_id = model_id.unwrap_or_else(|| handle.model_route.snapshot().model_id);
    send_selection(
        handle,
        session_id,
        model_id,
        selection,
        None,
        Some(admitted),
    )
}

fn prepare_selection(
    agent: &MvpAgent,
    handle: &crate::session::SessionHandle,
    model_id: Option<&crate::agent::models::ModelId>,
    effort: Option<sampling_types::ReasoningEffort>,
) -> Result<SessionSamplingSelection, acp::Error> {
    let workflow_route = handle
        .workflow_run_id
        .as_deref()
        .map(|run_id| {
            handle
                .workflow_tracker
                .lock()
                .get(run_id)
                .map(|state| state.runtime_route.clone())
                .ok_or_else(|| {
                    acp::Error::invalid_params()
                        .data(format!("Workflow Run '{run_id}' is no longer registered"))
                })
        })
        .transpose()?;
    let Some(model_id) = model_id else {
        let effort = effort.ok_or_else(|| {
            acp::Error::invalid_params().data("an effort-only selection requires effort")
        })?;
        let authority = if let Some(route) = workflow_route {
            SessionEffortAuthority::Workflow {
                route: Box::new(route),
                models_manager: agent.models_manager.clone(),
            }
        } else {
            SessionEffortAuthority::Catalog {
                catalog: std::sync::Arc::new(agent.models_manager.published_catalog()),
                origin_client: handle.origin_client.clone(),
            }
        };
        return Ok(SessionSamplingSelection::Effort { effort, authority });
    };
    let (mut route, catalog) = if let Some(workflow_route) = &workflow_route {
        (
            workflow_route
                .session_route_for(model_id.0.as_ref(), &agent.models_manager, None)
                .map_err(|error| acp::Error::invalid_params().data(error))?,
            None,
        )
    } else {
        let model = agent.resolve_model_id(model_id)?;
        if !model.info.user_selectable {
            return Err(acp::Error::invalid_params()
                .data("This model isn't allowed by your allowed_models setting."));
        }
        let catalog = std::sync::Arc::new(agent.models_manager.published_catalog());
        let mut route = catalog
            .resolve_session_route(model_id, effort)
            .filter(|route| route.model_id == *model_id)
            .ok_or_else(|| {
                acp::Error::invalid_params().data(format!("model '{}' is not routable", model_id.0))
            })?;
        route.sampling_config =
            agent.prepare_sampling_config_for_model(&model, handle.origin_client.clone());
        (route, Some(catalog))
    };
    if let Some(effort) = effort {
        let offered = if let Some(route) = &workflow_route {
            route.supports_reasoning_effort(model_id.0.as_ref(), effort)
        } else {
            agent
                .models_manager
                .model_offers_reasoning_effort(model_id.0.as_ref(), effort)
        };
        if !offered {
            return Err(acp::Error::invalid_params().data(format!(
                "model '{}' does not admit '{}' reasoning effort for this session",
                model_id.0, effort
            )));
        }
        route.sampling_config.reasoning_effort = Some(effort);
    }
    Ok(SessionSamplingSelection::Model { route, catalog })
}

fn send_selection(
    handle: crate::session::SessionHandle,
    session_id: acp::SessionId,
    model_id: crate::agent::models::ModelId,
    selection: SessionSamplingSelection,
    intent: Option<ControlIntent>,
    admitted: Option<oneshot::Sender<Result<(), String>>>,
) -> Result<EnqueuedModelSwitch, acp::Error> {
    let previous_model_id = handle.model_route.snapshot().model_id;
    let (responds_to, response) = oneshot::channel();
    let command = if let Some(admitted) = admitted {
        SessionCommand::AdmitToolSampling {
            selection,
            admitted,
            responds_to,
        }
    } else {
        match selection {
            SessionSamplingSelection::Model { route, catalog } => SessionCommand::SetSessionModel {
                route,
                catalog,
                intent,
                responds_to,
            },
            SessionSamplingSelection::Effort { effort, authority } => {
                SessionCommand::PatchSessionEffort {
                    effort,
                    authority,
                    intent,
                    responds_to,
                }
            }
        }
    };
    handle
        .cmd_tx
        .send(command)
        .map_err(|_| acp::Error::internal_error().data("session actor closed"))?;
    Ok(EnqueuedModelSwitch {
        session_id,
        model_id,
        previous_model_id,
        response,
    })
}

/// Wait for an already-enqueued route to reach its session-local step boundary
/// (or idle fast path) and publish the resulting client notification. This
/// phase must run without the global catalog lock.
pub(crate) async fn finish(
    agent: &MvpAgent,
    enqueued: EnqueuedModelSwitch,
) -> Result<acp::SetSessionConfigOptionResponse, acp::Error> {
    let EnqueuedModelSwitch {
        session_id,
        model_id,
        previous_model_id,
        response,
    } = enqueued;
    let outcome = response
        .await
        .map_err(|_| acp::Error::internal_error().data("failed to set session model"))??;
    let status = match outcome {
        crate::session::DesiredStateOutcome::Applied(updated_model) => {
            ::diagnostics::session_ctx::log_event(::diagnostics::events::ModelSwitched {
                session_id: session_id.0.to_string(),
                previous_model_id: previous_model_id.0.to_string(),
                new_model_id: updated_model.0.to_string(),
                success: true,
                error_code: None,
                required_agent_type: None,
                current_agent_type: None,
            });
            serde_json::json!({ "status": "applied", "model": updated_model })
        }
        crate::session::DesiredStateOutcome::InFlight => {
            serde_json::json!({ "status": "in_flight" })
        }
        crate::session::DesiredStateOutcome::Superseded => {
            serde_json::json!({ "status": "superseded" })
        }
    };
    let state = agent.model_state(Some(&session_id));
    Ok(acp::SetSessionConfigOptionResponse::new(
        agent.session_config_options(Some(&session_id), &state),
    )
    .meta(status.as_object().cloned()))
}
