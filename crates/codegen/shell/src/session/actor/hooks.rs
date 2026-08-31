//! Client-registered hooks for [`SessionActor`].
//!
//! Hooks registered at `session/new` (`_meta["grow/hooks"]`) come in two flavors,
//! both matched by the agent ([`::hooks::matcher::HookMatcher`], shared with
//! file hooks):
//! - **Gates** (awaited reverse *requests* `grow/hooks/run`):
//!   - `PreToolUse`: a `deny` blocks the tool.
//!   - `Stop` / `SubagentStop` (turn-end gate): a `deny` blocks the agent from
//!     stopping (its `systemMessage` becomes the feedback), `continue: false`
//!     (+ `stopReason`) force-stops overriding blocks, and `additionalContext`
//!     keeps the agent working with non-error feedback: the same vocabulary
//!     file hooks produce, aggregated in [`Self::run_stop_client_hooks`].
//! - **All other events**: fire-and-forget *notifications* `grow/hooks/event`,
//!   observe-only (the callback's return is ignored). Sent per matching callback.

use std::sync::Arc;
use std::time::Duration;

use ::diagnostics::events::ClientHookGateOutcome;
use ::hooks::event::{HookEventEnvelope, HookEventName, HookPayload};
use acp_transport::AcpClientHandler as _;
use acp_transport::protocol as acp;
#[cfg(test)]
use futures::StreamExt as _;
use serde_json::value::RawValue;

use super::{SessionActor, ToolLoop};
use crate::extensions::hooks::{
    ClientHookDecision, ClientHookDispatch, ClientHookGroup, ClientHookResponse,
};
#[cfg(test)]
use crate::sampling::types::ToolCallResponse;

const HOOK_EVENT_METHOD: &str = "grow/hooks/event";
const HOOK_RUN_METHOD: &str = "grow/hooks/run";

/// Default reply deadline for the `PreToolUse` client gate: short because it
/// sits in the interactive tool hot path. On timeout the gate fails open (the
/// tool proceeds). Stop gates use `CLIENT_STOP_GATE_TIMEOUT` instead.
const CLIENT_HOOK_TIMEOUT: Duration = Duration::from_secs(30);

/// Default reply deadline for the `Stop`/`SubagentStop` client gate. A
/// timed-out gate fails open (the agent stops), so too short a default would
/// silently drop a ported goal policy that runs a build or test suite.
const CLIENT_STOP_GATE_TIMEOUT: Duration = Duration::from_secs(600);

pub(super) fn next_hook_config_generation(timeline: Option<&chat_state::Timeline>) -> Option<u64> {
    let previous = timeline
        .into_iter()
        .flat_map(chat_state::Timeline::events)
        .filter_map(|event| match &event.kind {
            chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                config_generation,
                ..
            }) => Some(*config_generation),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    previous.checked_add(1)
}

/// Outcome of the `grow/hooks/run` reverse request, before interpreting it as a
/// decision. Separate so [`classify`] stays pure and unit-testable.
enum ReverseOutcome {
    Responded(Arc<RawValue>),
    Transport(acp::Error),
    Timeout,
}

#[derive(Debug, Clone)]
pub(super) struct PlannedClientHook {
    pub callback_id: String,
    pub timeout: Duration,
}

/// Map a reverse-request outcome to a decision. Malformed / transport / timeout
/// all fail open (the `Default` response = proceed).
fn classify(outcome: ReverseOutcome) -> (ClientHookResponse, ClientHookGateOutcome) {
    match outcome {
        ReverseOutcome::Responded(raw) => {
            match serde_json::from_str::<ClientHookResponse>(raw.get()) {
                Ok(resp) => {
                    let label = match resp.decision {
                        ClientHookDecision::Deny => ClientHookGateOutcome::Denied,
                        ClientHookDecision::Continue => ClientHookGateOutcome::Proceeded,
                    };
                    (resp, label)
                }
                Err(err) => {
                    tracing::warn!(%err, "malformed grow/hooks/run response; failing open");
                    (
                        ClientHookResponse::default(),
                        ClientHookGateOutcome::Malformed,
                    )
                }
            }
        }
        ReverseOutcome::Transport(err) => {
            tracing::warn!(%err, "grow/hooks/run transport error (no client wired?); failing open");
            (
                ClientHookResponse::default(),
                ClientHookGateOutcome::TransportError,
            )
        }
        ReverseOutcome::Timeout => {
            tracing::warn!("grow/hooks/run timed out; failing open");
            (
                ClientHookResponse::default(),
                ClientHookGateOutcome::TimedOut,
            )
        }
    }
}

/// Callback ids that fire for an event, in registration order.
#[cfg(test)]
fn matching_callback_ids<'a>(
    groups: &'a [ClientHookGroup],
    match_value: Option<&str>,
) -> Vec<&'a str> {
    groups
        .iter()
        .filter(|group| ::hooks::matcher::matcher_allows(group.matcher.as_ref(), match_value))
        .flat_map(|group| group.callback_ids.iter().map(String::as_str))
        .collect()
}

fn matching_gate_callbacks(
    groups: &[ClientHookGroup],
    match_value: Option<&str>,
    default_timeout: Duration,
) -> Vec<(String, Duration)> {
    let mut seen = std::collections::HashSet::new();
    groups
        .iter()
        .filter(|group| ::hooks::matcher::matcher_allows(group.matcher.as_ref(), match_value))
        .flat_map(|group| {
            let timeout = group.timeout.unwrap_or(default_timeout);
            group
                .callback_ids
                .iter()
                .map(move |callback_id| (callback_id.clone(), timeout))
        })
        .filter(|(callback_id, _)| seen.insert(callback_id.clone()))
        .collect()
}

/// Serialize a [`ClientHookDispatch`] to reverse-RPC params, or `None` (logged) on the
/// should-never-happen serialization failure; callers then skip that callback (fail open)
/// rather than panic the session actor.
fn dispatch_params(dispatch: &ClientHookDispatch<'_>) -> Option<Arc<RawValue>> {
    serde_json::value::to_raw_value(dispatch)
        .inspect_err(|err| tracing::warn!(%err, "failed to serialize client hook dispatch"))
        .ok()
        .map(Into::into)
}

impl SessionActor {
    pub(super) fn hook_config_generation(&self) -> u64 {
        self.hooks.generation.get()
    }

    fn advance_hook_config_generation(&self) {
        self.hooks.generation.set(
            self.hooks
                .generation
                .get()
                .checked_add(1)
                .expect("Hook config generation exhausted"),
        );
    }

    pub(super) fn replace_hook_registry(
        &self,
        registry: Option<Arc<::hooks::discovery::HookRegistry>>,
    ) {
        *self.hooks.registry.borrow_mut() = registry;
        self.advance_hook_config_generation();
    }

    pub(super) fn replace_client_hooks(&self, hooks: crate::extensions::hooks::ClientHooks) {
        *self.hooks.client_hooks.borrow_mut() = hooks;
        self.advance_hook_config_generation();
    }

    pub(super) fn mark_hook_registry_changed(&self) {
        self.advance_hook_config_generation();
    }

    pub(super) fn plan_client_hooks(&self, envelope: &HookEventEnvelope) -> Vec<PlannedClientHook> {
        let groups = self
            .hooks
            .client_hooks
            .borrow()
            .get(&envelope.hook_event_name)
            .cloned()
            .unwrap_or_default();
        let default_timeout =
            if envelope.hook_event_name.traits().gate == ::hooks::event::GateKind::Stop {
                CLIENT_STOP_GATE_TIMEOUT
            } else {
                CLIENT_HOOK_TIMEOUT
            };
        matching_gate_callbacks(&groups, envelope.payload.match_value(), default_timeout)
            .into_iter()
            .map(|(callback_id, timeout)| PlannedClientHook {
                callback_id,
                timeout,
            })
            .collect()
    }

    /// Build a [`HookEventEnvelope`] with this session's common fields filled (session id,
    /// cwd, workspace root, timestamp). Single source of truth for envelope shape; every
    /// fire site goes through here.
    pub(super) fn make_hook_envelope(
        &self,
        hook_event_name: HookEventName,
        prompt_id: Option<String>,
        payload: HookPayload,
    ) -> HookEventEnvelope {
        HookEventEnvelope {
            hook_event_name,
            session_id: self.session_id_string(),
            cwd: self.session_info.cwd.clone(),
            workspace_root: self.hook_workspace_root(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            transcript_path: self.get_transcript_path(),
            client_identifier: None,
            prompt_id,
            permission_mode: Some(self.permission_mode_label().to_string()),
            payload,
        }
    }

    /// Build the envelope for an observe-only event, fire observe client hooks for it, and
    /// return the envelope for any subsequent file-hook dispatch. One call so a fire site
    /// can't build the envelope but forget to notify.
    #[cfg(test)]
    pub(super) fn fire_hook(
        &self,
        hook_event_name: HookEventName,
        prompt_id: Option<String>,
        payload: HookPayload,
    ) -> HookEventEnvelope {
        let envelope = self.make_hook_envelope(hook_event_name, prompt_id, payload);
        self.notify_client_hooks(&envelope);
        envelope
    }

    /// Block a tool call denied by a `PreToolUse` hook (file- or client-side),
    /// emitting the shared diagnostics + UI side-effects and returning the
    /// [`ToolLoop::HookDenied`] the caller should propagate.
    pub(super) async fn deny_tool(
        &self,
        model_call_id: &str,
        tool_call_id: &acp::ToolCallId,
        tool_name: String,
        hook_name: String,
        reason: String,
    ) -> Result<ToolLoop, acp::Error> {
        tracing::info!(%tool_name, %hook_name, %reason, "tool call denied by pre_tool_use hook");
        ::diagnostics::session_ctx::log_event(::diagnostics::events::HookBlocked {
            hook_name: hook_name.clone(),
        });
        self.handle_tool_not_executed(
            model_call_id,
            tool_call_id,
            format!("Hook denied: {reason}"),
        )
        .await?;
        self.send_hook_annotation(&format!(
            "\u{26a0} `{tool_name}` blocked by hook `{hook_name}`: {reason}"
        ))
        .await;
        Ok(ToolLoop::HookDenied { hook_name })
    }

    pub(super) async fn run_client_gate_callback(
        &self,
        callback_id: &str,
        timeout: Duration,
        tool_name: Option<&str>,
        envelope: &HookEventEnvelope,
    ) -> (ClientHookResponse, Duration, ClientHookGateOutcome) {
        let dispatch = ClientHookDispatch {
            hook_callback_id: callback_id,
            envelope,
        };
        let started = tokio::time::Instant::now();
        let (response, gate_outcome) = classify(self.send_hook_run(&dispatch, timeout).await);
        let elapsed = started.elapsed();
        let _ = tool_name;
        (response, elapsed, gate_outcome)
    }

    pub(super) fn notify_planned_client_hook(
        &self,
        callback_id: &str,
        envelope: &HookEventEnvelope,
    ) -> bool {
        let dispatch = ClientHookDispatch {
            hook_callback_id: callback_id,
            envelope,
        };
        let Some(params) = dispatch_params(&dispatch) else {
            return false;
        };
        self.notifications
            .gateway
            .forward_fire_and_forget(acp::ExtNotification::new(HOOK_EVENT_METHOD, params));
        true
    }

    /// Run the client-registered `PreToolUse` hooks for `call`, firing
    /// `grow/hooks/run` once per matching callback with the shared `envelope` (the
    /// same payload file hooks and observe events receive).
    ///
    /// Returns `Some(ToolLoop::HookDenied)` on the first deny, else `None`.
    #[cfg(test)]
    pub(super) async fn run_pre_tool_use_client_hook(
        &self,
        call: &ToolCallResponse,
        tool_call_id: &acp::ToolCallId,
        envelope: &HookEventEnvelope,
    ) -> Result<Option<ToolLoop>, acp::Error> {
        // Clone the matched groups so we don't hold the `client_hooks` borrow across the
        // dispatch awaits below.
        let Some(groups) = self
            .hooks
            .client_hooks
            .borrow()
            .get(&HookEventName::PreToolUse)
            .cloned()
        else {
            return Ok(None);
        };
        // Match on the resolved target (in the envelope) so a client deny matcher
        // keyed on the real MCP tool gates a meta-dispatch call, matching the
        // observe path (`notify_client_hooks`). Equals `function.name` otherwise.
        let tool_name = envelope
            .payload
            .match_value()
            .unwrap_or(call.function.name.as_str());

        let callbacks = matching_gate_callbacks(&groups, Some(tool_name), CLIENT_HOOK_TIMEOUT);
        let mut pending = futures::stream::FuturesUnordered::new();
        for (callback_id, timeout) in callbacks {
            pending.push(async move {
                let result = self
                    .run_client_gate_callback(&callback_id, timeout, Some(tool_name), envelope)
                    .await;
                (callback_id, result)
            });
        }
        while let Some((callback_id, (response, _elapsed, _outcome))) = pending.next().await {
            if response.decision == ClientHookDecision::Deny {
                let reason = response
                    .system_message
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| "blocked by client hook".to_string());
                return Ok(Some(
                    self.deny_tool(
                        &call.id,
                        tool_call_id,
                        tool_name.to_owned(),
                        // Name the specific callback so diagnostics / the UI annotation can
                        // attribute the block, not collapse every client hook to "client".
                        format!("client:{callback_id}"),
                        reason,
                    )
                    .await?,
                ));
            }
        }
        Ok(None)
    }

    /// Run the client `Stop`/`SubagentStop` gate for a turn-end envelope.
    /// Unlike the `PreToolUse` gate (first deny wins), every callback's response
    /// is aggregated into a [`StopDispatchResult`] (a `deny` maps to a block).
    #[cfg(test)]
    pub(super) async fn run_stop_client_hooks(
        &self,
        envelope: &HookEventEnvelope,
    ) -> ::hooks::dispatcher::StopDispatchResult {
        use ::hooks::result::HookRunResult;

        let mut out = ::hooks::dispatcher::StopDispatchResult::default();
        // Clone: don't hold the borrow across awaits (see run_pre_tool_use_client_hook).
        let Some(groups) = self
            .hooks
            .client_hooks
            .borrow()
            .get(&envelope.hook_event_name)
            .cloned()
        else {
            return out;
        };

        let match_value = envelope.payload.match_value();
        let callbacks = matching_gate_callbacks(&groups, match_value, CLIENT_STOP_GATE_TIMEOUT);
        let responses = futures::future::join_all(callbacks.into_iter().map(
            |(callback_id, timeout)| async move {
                let result = self
                    .run_client_gate_callback(&callback_id, timeout, match_value, envelope)
                    .await;
                (callback_id, result)
            },
        ))
        .await;
        for (callback_id, (response, elapsed, _outcome)) in responses {
            let hook_name = format!("client:{callback_id}");
            let block_reason = (response.decision == ClientHookDecision::Deny).then(|| {
                response
                    .system_message
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| "blocked by client hook".to_string())
            });
            let stop_reason = (response.continue_ == Some(false)).then(|| {
                response
                    .stop_reason
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| "stopped by client hook".to_string())
            });

            let detail = ::hooks::dispatcher::stop_detail(
                stop_reason.is_some(),
                stop_reason.as_deref(),
                block_reason.as_deref(),
            );
            out.results.push(match detail {
                Some(detail) => HookRunResult::Blocked {
                    hook_name: hook_name.clone(),
                    detail,
                    elapsed,
                    http_info: None,
                },
                None => HookRunResult::Success {
                    hook_name: hook_name.clone(),
                    elapsed,
                    http_info: None,
                },
            });

            out.absorb(
                &hook_name,
                ::hooks::dispatcher::StopSignals {
                    block_reason,
                    stop_reason,
                    additional_context: response
                        .additional_context
                        .filter(|c| !c.trim().is_empty()),
                },
            );
        }
        out
    }

    /// Issue one `grow/hooks/run` reverse request, bounded by a per-callback `timeout`.
    async fn send_hook_run(
        &self,
        dispatch: &ClientHookDispatch<'_>,
        timeout: Duration,
    ) -> ReverseOutcome {
        let Some(params) = dispatch_params(dispatch) else {
            return ReverseOutcome::Transport(acp::Error::internal_error());
        };
        let ext_request = acp::ExtRequest::new(HOOK_RUN_METHOD, params);
        match tokio::time::timeout(timeout, self.notifications.gateway.ext_method(ext_request))
            .await
        {
            Ok(Ok(raw)) => ReverseOutcome::Responded(raw.0),
            Ok(Err(err)) => ReverseOutcome::Transport(err),
            Err(_) => ReverseOutcome::Timeout,
        }
    }

    /// Fire observe-only client hooks for `envelope`'s event: send an
    /// `grow/hooks/event` notification to each matching registered callback.
    /// Fire-and-forget (no decision is consumed); independent of file hooks, so it
    /// runs even when no on-disk hook registry exists. No-op when nothing is registered.
    #[cfg(test)]
    pub(super) fn notify_client_hooks(&self, envelope: &HookEventEnvelope) {
        for planned in self.plan_client_hooks(envelope) {
            self.notify_planned_client_hook(&planned.callback_id, envelope);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(value: serde_json::Value) -> Arc<RawValue> {
        serde_json::value::to_raw_value(&value).unwrap().into()
    }

    /// Only an explicit `deny` blocks; malformed/transport/timeout all proceed. The second
    /// tuple element is the diagnostics outcome, distinct per fail-open mode.
    #[test]
    fn classify_only_deny_blocks() {
        let (denied, outcome) = classify(ReverseOutcome::Responded(raw(
            serde_json::json!({ "decision": "deny" }),
        )));
        assert_eq!(denied.decision, ClientHookDecision::Deny);
        assert!(matches!(outcome, ClientHookGateOutcome::Denied));

        let (cont, outcome) = classify(ReverseOutcome::Responded(raw(
            serde_json::json!({ "decision": "continue" }),
        )));
        assert_eq!(cont.decision, ClientHookDecision::Continue);
        assert!(matches!(outcome, ClientHookGateOutcome::Proceeded));

        // Unknown decisions are malformed and fail open.
        let (unknown, outcome) = classify(ReverseOutcome::Responded(raw(
            serde_json::json!({ "decision": "maybe_later" }),
        )));
        assert_eq!(unknown.decision, ClientHookDecision::Continue);
        assert!(matches!(outcome, ClientHookGateOutcome::Malformed));

        // Every failure mode falls open to Continue, but reports a distinct outcome.
        let (malformed, outcome) = classify(ReverseOutcome::Responded(raw(
            serde_json::json!({ "decision": 123 }),
        )));
        assert_eq!(malformed.decision, ClientHookDecision::Continue);
        assert!(matches!(outcome, ClientHookGateOutcome::Malformed));

        let (transport, outcome) =
            classify(ReverseOutcome::Transport(acp::Error::internal_error()));
        assert_eq!(transport.decision, ClientHookDecision::Continue);
        assert!(matches!(outcome, ClientHookGateOutcome::TransportError));

        let (timeout, outcome) = classify(ReverseOutcome::Timeout);
        assert_eq!(timeout.decision, ClientHookDecision::Continue);
        assert!(matches!(outcome, ClientHookGateOutcome::TimedOut));
    }

    /// Tool events filter by matcher (matcher-less groups always fire); a non-tool
    /// event (`None`) fires every group regardless of its matcher.
    #[test]
    fn matching_callback_ids_filters_by_matcher() {
        use ::hooks::matcher::HookMatcher;

        let groups = vec![
            ClientHookGroup {
                matcher: Some(HookMatcher::new("run_terminal_command").unwrap()),
                callback_ids: vec!["bash_only".to_string()],
                timeout: None,
            },
            ClientHookGroup {
                matcher: None,
                callback_ids: vec!["all_a".to_string(), "all_b".to_string()],
                timeout: None,
            },
            ClientHookGroup {
                matcher: Some(HookMatcher::new("read_file").unwrap()),
                callback_ids: vec!["read_only".to_string()],
                timeout: None,
            },
        ];

        assert_eq!(
            matching_callback_ids(&groups, Some("run_terminal_command")),
            ["bash_only", "all_a", "all_b"]
        );
        assert_eq!(
            matching_callback_ids(&groups, Some("list_dir")),
            ["all_a", "all_b"]
        );
        assert_eq!(
            matching_callback_ids(&groups, None),
            ["bash_only", "all_a", "all_b", "read_only"]
        );
    }

    #[test]
    fn matching_gate_callbacks_are_deduplicated_in_registration_order() {
        let groups = vec![
            ClientHookGroup {
                matcher: None,
                callback_ids: vec!["first".into(), "shared".into()],
                timeout: Some(Duration::from_secs(1)),
            },
            ClientHookGroup {
                matcher: None,
                callback_ids: vec!["shared".into(), "last".into()],
                timeout: Some(Duration::from_secs(2)),
            },
        ];
        let callbacks = matching_gate_callbacks(&groups, None, CLIENT_HOOK_TIMEOUT);
        assert_eq!(
            callbacks,
            vec![
                ("first".into(), Duration::from_secs(1)),
                ("shared".into(), Duration::from_secs(1)),
                ("last".into(), Duration::from_secs(2)),
            ]
        );
    }

    #[test]
    fn restored_hook_generation_advances_past_durable_maximum() {
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(chat_state::TimelineEventKind::Hook(
                chat_state::HookEvent::Triggered {
                    occurrence_id: "restored-hook".into(),
                    event: chat_state::HookEventType::SessionStart,
                    gate: chat_state::HookGateKind::Observe,
                    cause: chat_state::HookCause::Session {
                        session_id: "session".into(),
                    },
                    config_generation: 41,
                    handlers: Vec::new(),
                },
            ))
            .unwrap();
        timeline
            .record(chat_state::TimelineEventKind::Hook(
                chat_state::HookEvent::Completed {
                    occurrence_id: "restored-hook".into(),
                    decision: chat_state::HookAggregateDecision::Observe,
                },
            ))
            .unwrap();

        assert_eq!(next_hook_config_generation(None), Some(1));
        assert_eq!(next_hook_config_generation(Some(&timeline)), Some(42));
    }

    #[test]
    fn exhausted_restored_hook_generation_fails_closed() {
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(chat_state::TimelineEventKind::Hook(
                chat_state::HookEvent::Triggered {
                    occurrence_id: "exhausted-hook".into(),
                    event: chat_state::HookEventType::SessionStart,
                    gate: chat_state::HookGateKind::Observe,
                    cause: chat_state::HookCause::Session {
                        session_id: "session".into(),
                    },
                    config_generation: u64::MAX,
                    handlers: Vec::new(),
                },
            ))
            .unwrap();
        assert_eq!(next_hook_config_generation(Some(&timeline)), None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn hook_configuration_replacement_advances_generation() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
                let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                let actor = crate::session::actor::tests::support::create_test_actor(
                    0,
                    256_000,
                    85,
                    gateway_tx,
                    persistence_tx,
                )
                .await;
                let initial = actor.hook_config_generation();
                actor.replace_client_hooks(Default::default());
                assert_eq!(actor.hook_config_generation(), initial + 1);
                actor.replace_hook_registry(None);
                assert_eq!(actor.hook_config_generation(), initial + 2);
                actor.mark_hook_registry_changed();
                assert_eq!(actor.hook_config_generation(), initial + 3);
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    #[should_panic(expected = "Hook config generation exhausted")]
    async fn hook_configuration_generation_exhaustion_fails_closed() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
                let (persistence_tx, _persistence_rx) = tokio::sync::mpsc::unbounded_channel();
                let actor = crate::session::actor::tests::support::create_test_actor(
                    0,
                    256_000,
                    85,
                    gateway_tx,
                    persistence_tx,
                )
                .await;
                actor.hooks.generation.set(u64::MAX);
                actor.mark_hook_registry_changed();
            })
            .await;
    }
}
