//! Resolves and applies a complete model route to one session. Ordinary
//! selections use the catalog generation protected by the caller's lock;
//! Workflow children use their immutable Run route instead.
use crate::agent::mvp_agent::MvpAgent;
use crate::session::{ControlIntent, SessionCommand, SessionEffortAuthority};
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

/// Resolve and enqueue a model route while the caller owns the catalog
/// publication lock. Publication reloads use the same lock while sending their
/// actor commands, so catalog generations and user selections enter every
/// session mailbox in one total order. The caller must resolve any in-flight
/// session load before acquiring the lock and must release it before awaiting
/// [`finish`].
pub(crate) fn enqueue(
    agent: &MvpAgent,
    _catalog_transaction: &tokio::sync::MutexGuard<'_, ()>,
    handle: crate::session::SessionHandle,
    args: ModelSwitchRequest,
) -> Result<EnqueuedModelSwitch, acp::Error> {
    tracing::info!("Received set session model request {args:?}");
    ::diagnostics::unified_log::info(
        "model changed",
        Some(args.session_id.0.as_ref()),
        Some(serde_json::json!({"model": args.model_id.0.as_ref()})),
    );
    let intent = ControlIntent::from_meta(args.meta.as_ref())?;
    let effort_override = parse_reasoning_effort_meta(args.meta.as_ref());
    let effort_patch = crate::session::effort_patch_from_meta(args.meta.as_ref())?;
    let ModelSwitchRequest {
        session_id,
        model_id,
        ..
    } = args;
    let previous_model_id = handle.model_route.snapshot().model_id;
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
    if effort_patch {
        let effort = effort_override.ok_or_else(|| {
            acp::Error::invalid_params().data(format!(
                "{} requires a reasoning-effort value",
                crate::session::EFFORT_PATCH_META_KEY
            ))
        })?;
        let authority = if let Some(route) = workflow_route {
            SessionEffortAuthority::Workflow {
                route,
                models_manager: agent.models_manager.clone(),
            }
        } else {
            SessionEffortAuthority::Catalog {
                catalog: std::sync::Arc::new(agent.models_manager.published_catalog()),
                origin_client: handle.origin_client.clone(),
            }
        };
        let (responds_to, response) = oneshot::channel();
        handle
            .cmd_tx
            .send(SessionCommand::PatchSessionEffort {
                effort,
                authority,
                intent,
                responds_to,
            })
            .map_err(|_| acp::Error::internal_error().data("session actor closed"))?;
        return Ok(EnqueuedModelSwitch {
            session_id,
            model_id,
            previous_model_id,
            response,
        });
    }
    let (mut route, catalog) = if let Some(workflow_route) = &workflow_route {
        (
            workflow_route
                .session_route_for(model_id.0.as_ref(), &agent.models_manager, None)
                .map_err(|error| acp::Error::invalid_params().data(error))?,
            None,
        )
    } else {
        let model = agent.resolve_model_id(&model_id)?;
        let catalog = std::sync::Arc::new(agent.models_manager.published_catalog());
        let mut route = catalog
            .resolve_session_route(&model_id, effort_override)
            .filter(|route| route.model_id == model_id)
            .ok_or_else(|| {
                acp::Error::invalid_params().data(format!("model '{}' is not routable", model_id.0))
            })?;
        route.sampling_config =
            agent.prepare_sampling_config_for_model(&model, handle.origin_client.clone());
        (route, Some(catalog))
    };
    if let Some(eff) = effort_override {
        let offered = if let Some(route) = &workflow_route {
            route.supports_reasoning_effort(model_id.0.as_ref(), eff)
        } else {
            agent
                .models_manager
                .model_offers_reasoning_effort(model_id.0.as_ref(), eff)
        };
        if !offered {
            return Err(acp::Error::invalid_params().data(format!(
                "model '{}' does not admit '{}' reasoning effort for this session",
                model_id.0, eff
            )));
        }
        tracing::info!(
            session_id = %session_id.0,
            effort = %eff,
            "set_session_model: applying reasoning_effort override from meta"
        );
        route.sampling_config.reasoning_effort = Some(eff);
    }
    let (responds_to, response) = oneshot::channel();
    handle
        .cmd_tx
        .send(SessionCommand::SetSessionModel {
            route,
            catalog,
            intent,
            responds_to,
        })
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
