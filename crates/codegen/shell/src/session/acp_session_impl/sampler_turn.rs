//! Sampler-turn pipeline for `SessionActor`: tool definitions, model auth
//! facts/gates and retry, sampler config reconstruction, sampling-failure
//! recovery, and per-response usage recording.
use super::*;

pub(super) const UNSUPPORTED_IMAGE_PLACEHOLDER: &str = "[Images removed: the active model does not support image input and no usable auxiliary description was available.]";
const IMAGE_RECOVERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(240);
const PERMISSION_JUDGMENT_MAX_ATTEMPTS: usize = 2;
const PERMISSION_JUDGMENT_MAX_OUTPUT_TOKENS: u32 = 1_024;
const PERMISSION_JUDGMENT_RETRY_MESSAGE: &str = "The previous permission judgment attempt returned an empty or invalid structured response, timed out, or failed with a transient provider error. Retry once. Return exactly one JSON object with no Markdown or prose: {\"decision\":\"allow\"|\"deny\",\"reason\":\"brief explanation\"}.";

fn permission_judgment_json_output(
    backend: sampling_types::ApiBackend,
) -> sampling_types::JsonOutputFormat {
    match backend {
        // Chat Completions is a provider-neutral compatibility backend.
        // DeepSeek and BigModel expose JSON Object mode but not OpenAI's
        // `json_schema` wire shape, so use the common contract and validate
        // the exact permission schema locally.
        sampling_types::ApiBackend::ChatCompletions => sampling_types::JsonOutputFormat::JsonObject,
        sampling_types::ApiBackend::Responses | sampling_types::ApiBackend::Messages => {
            sampling_types::JsonOutputFormat::JsonSchema(
                workspace::permission::classifier_output_json_schema(),
            )
        }
    }
}

fn permission_judgment_needs_retry(text: &str) -> bool {
    workspace::permission::parse_classifier_model_text(text)
        == workspace::permission::ClassifierVerdict::Unavailable
}

fn permission_judgment_error_needs_retry(error: &sampling_types::SamplingError) -> bool {
    error.is_retryable() && !error.is_retry_vetoed()
}

/// Match a provider's unconditional model-capability claim without treating
/// an open-ended format/size qualifier as a permanent text-only capability.
/// Provider prose such as "does not support images with animated frames" is
/// about one representation, not image input as a whole.
fn contains_terminal_capability_claim(message: &str, claim: &str) -> bool {
    message.match_indices(claim).any(|(start, _)| {
        let suffix = &message[start + claim.len()..];
        suffix
            .chars()
            .all(|ch| ch.is_whitespace() || ch.is_ascii_punctuation())
    })
}

pub(super) fn is_image_input_unsupported(
    error: &sampler::SamplingErrorInfo,
    image_count: usize,
) -> bool {
    if image_count == 0
        || !matches!(error.kind, sampler::SamplingErrorKind::Api)
        || error.status_code != Some(400)
    {
        return false;
    }

    let message = error.message.to_ascii_lowercase();
    if [
        "invalid image",
        "malformed image",
        "corrupt image",
        "image too large",
        "image size",
        "image dimensions",
        "unsupported image format",
        "alpha channel",
        "transparency",
        "transparent image",
        "content policy",
        "safety policy",
    ]
    .iter()
    .any(|needle| message.contains(needle))
    {
        return false;
    }

    let names_image_content = [
        "image_url",
        "input_image",
        "image input",
        "image content",
        "content type image",
    ]
    .iter()
    .any(|needle| message.contains(needle));
    let text_only_model = [
        "this text-only model only supports text input",
        "this text only model only supports text input",
        "this model only supports text",
        "this model supports only text",
        "this model only accepts text",
        "this model accepts only text",
        "model only supports text",
        "model supports only text",
        "only text input is supported",
        "only text inputs are supported",
        "only text content is supported",
        "text-only model",
        "text only model",
    ]
    .iter()
    .any(|claim| contains_terminal_capability_claim(&message, claim));
    let model_rejects_images = [
        "model does not support images",
        "model doesn't support images",
        "model does not accept images",
        "input_image is not supported by this model",
        "image input is unsupported by this model",
        "image inputs are unsupported by this model",
        "images are not supported by this model",
    ]
    .iter()
    .any(|claim| contains_terminal_capability_claim(&message, claim));
    let image_type_rejected_as_text = [
        "unknown variant",
        "expected `text`",
        "expected text",
        "must be text",
    ]
    .iter()
    .any(|needle| message.contains(needle));

    text_only_model || model_rejects_images || (names_image_content && image_type_rejected_as_text)
}

fn model_image_input_key(
    config: &sampling_types::SamplingConfig,
) -> tools::types::resources::ModelImageInputKey {
    model_image_input_key_parts(
        &config.model,
        &config.api_backend,
        &config.base_url,
        &config.query_params,
    )
}

fn sampler_model_image_input_key(
    config: &sampler::SamplerConfig,
) -> tools::types::resources::ModelImageInputKey {
    model_image_input_key_parts(
        &config.model,
        &config.api_backend,
        &config.base_url,
        &config.query_params,
    )
}

fn model_image_input_key_parts(
    model: &str,
    api_backend: &sampling_types::ApiBackend,
    base_url: &str,
    query_params: &indexmap::IndexMap<String, String>,
) -> tools::types::resources::ModelImageInputKey {
    use sha2::{Digest, Sha256};
    let api_backend = match api_backend {
        sampling_types::ApiBackend::ChatCompletions => "chat_completions",
        sampling_types::ApiBackend::Responses => "responses",
        sampling_types::ApiBackend::Messages => "messages",
    };
    // The configured query is part of the effective provider route (Azure and
    // compatible gateways commonly select deployments this way). Sort it so
    // equivalent maps share one identity; only the digest is persisted, never
    // query values that may contain credentials.
    let mut query = query_params.iter().collect::<Vec<_>>();
    query.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
    let mut endpoint = Sha256::new();
    endpoint.update(base_url.as_bytes());
    for (key, value) in query {
        endpoint.update([0]);
        endpoint.update(key.as_bytes());
        endpoint.update([b'=']);
        endpoint.update(value.as_bytes());
    }
    let endpoint_fingerprint = format!("{:x}", endpoint.finalize());
    tools::types::resources::ModelImageInputKey::new(model, api_backend, endpoint_fingerprint)
}
/// Auth-failure detector for tool errors. Matches strictly on HTTP 401
/// when the error carries a structured status code, mirroring
/// `SamplingError::is_auth_error` in sampling-types: 403 is
/// deliberately excluded because it means "authenticated but forbidden"
/// (content-safety blocks, ZDR-gated requests, remote settings gates), where
/// rereading a BYOK key would be a no-op and would surface to the client as
/// a spurious auth_required teardown.
///
/// String fallbacks remain for tools that surface auth failures without
/// going through the structured `HttpFailure` path (e.g. JSON-only
/// `invalid_token` payloads, BYOK key-validation messages).
pub(super) fn is_auth_tool_error(err: &tool_runtime::ToolError) -> bool {
    if let Some(details) = &err.details
        && let Some(status) = details
            .get(HTTP_STATUS_DETAILS_KEY)
            .and_then(|s| s.as_u64())
    {
        return status == 401;
    }
    let lower = err.to_string().to_ascii_lowercase();
    lower.contains("unauthorized")
        || lower.contains("invalid api key")
        || lower.contains("invalid_token")
}
impl SessionActor {
    pub(super) async fn current_model_image_input_key(
        &self,
    ) -> Option<tools::types::resources::ModelImageInputKey> {
        self.chat_state_handle
            .get_sampling_config()
            .await
            .as_ref()
            .map(model_image_input_key)
    }

    async fn model_image_input_is_unsupported(
        &self,
        key: &tools::types::resources::ModelImageInputKey,
    ) -> bool {
        let bridge = self.agent.borrow().tool_bridge().clone();
        let resources = bridge.shared_resources().await;
        let resources = resources.lock().await;
        resources
            .get::<tools::types::resources::State<tools::types::resources::ModelImageInputState>>()
            .is_some_and(|state| state.is_unsupported(key))
    }

    pub(super) async fn record_unsupported_model_image_input(
        &self,
        key: tools::types::resources::ModelImageInputKey,
    ) -> std::io::Result<bool> {
        let bridge = self.agent.borrow().tool_bridge().clone();
        bridge
            .mark_model_image_input_unsupported_and_flush(key)
            .await
    }

    pub(super) async fn unsupported_current_model_for_images(&self) -> Option<String> {
        let key = self.current_model_image_input_key().await?;
        self.model_image_input_is_unsupported(&key)
            .await
            .then(|| key.model().to_string())
    }

    async fn resolve_image_description_route(
        &self,
        rejected_key: &tools::types::resources::ModelImageInputKey,
    ) -> Option<(
        sampler::SamplingClient,
        String,
        tools::types::resources::ModelImageInputKey,
    )> {
        let configured_model = self.image_description_model.read().clone()?;
        let mut sampler_config = match self.resolve_aux_sampler_config(&configured_model).await {
            Some(config) => config,
            None => {
                tracing::warn!(
                    configured_model,
                    "configured image description model could not be resolved"
                );
                return None;
            }
        };
        let active_session_config = self.reconstruct_full_config().await;
        crate::agent::config::stamp_session_local_sampler_fields(
            &mut sampler_config,
            &active_session_config,
            Some(self.max_retries.get()),
        );
        let auxiliary_key = sampler_model_image_input_key(&sampler_config);
        if &auxiliary_key == rejected_key {
            tracing::warn!(
                model = auxiliary_key.model(),
                "image description model resolves to the rejected text-only runtime"
            );
            return None;
        }
        if self.model_image_input_is_unsupported(&auxiliary_key).await {
            tracing::info!(
                model = auxiliary_key.model(),
                "skipping known text-only image description runtime"
            );
            return None;
        }
        let model = sampler_config.model.clone();
        let client = match sampler::SamplingClient::new(sampler_config) {
            Ok(client) => client,
            Err(error) => {
                tracing::warn!(%error, model, "failed to initialize image description model");
                return None;
            }
        };
        Some((client, model, auxiliary_key))
    }

    fn image_recovery_notification(report: chat_state::ImageRewriteReport) -> Option<String> {
        match (report.converted_images, report.dropped_images) {
            (0, 0) => None,
            (converted, 0) => Some(format!(
                "当前模型不支持图片输入；已使用辅助模型将 {converted} 张图片转换为文字描述并继续。"
            )),
            (0, dropped) => Some(format!(
                "当前模型不支持图片输入；辅助多模态模型不可用，已移除 {dropped} 张图片并继续。"
            )),
            (converted, dropped) => Some(format!(
                "当前模型不支持图片输入；已转换 {converted} 张图片，并移除 {dropped} 张无法转换的图片，随后继续。"
            )),
        }
    }

    /// Permanently degrade all canonical image groups for one text-only model
    /// runtime, then acknowledge the actor-serialized persistence before the
    /// caller rebuilds a request.
    async fn rewrite_conversation_images_for_text_model(
        &self,
        rejected_key: &tools::types::resources::ModelImageInputKey,
    ) -> Option<chat_state::ImageRewriteReport> {
        use sampling_types::conversation::{ConversationImageSource, conversation_image_groups};

        let conversation = self.chat_state_handle.get_conversation().await;
        let groups = conversation_image_groups(&conversation);
        if groups.is_empty() {
            return Some(chat_state::ImageRewriteReport::default());
        }
        let mut rewrites = groups
            .iter()
            .map(|group| chat_state::ImageRewrite {
                item_index: group.item_index,
                fingerprint: group.fingerprint.clone(),
                expected_image_count: group.image_count(),
                replacement: None,
            })
            .collect::<Vec<_>>();

        if let Some((client, model, auxiliary_key)) =
            self.resolve_image_description_route(rejected_key).await
        {
            let (outline, current_query) =
                crate::session::image_describe::build_read_context(&conversation);
            let deadline = tokio::time::Instant::now() + IMAGE_RECOVERY_TIMEOUT;
            for (group, rewrite) in groups.iter().zip(rewrites.iter_mut()) {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    tracing::warn!(
                        timeout_secs = IMAGE_RECOVERY_TIMEOUT.as_secs(),
                        "image context recovery reached its total timeout"
                    );
                    break;
                }
                let source_kind = match group.source {
                    ConversationImageSource::User => "User",
                    ConversationImageSource::ToolResult => "ToolResult",
                };
                let source_text = tools::util::truncate::truncate_middle(
                    group.source_text.as_ref(),
                    crate::session::image_describe::CURRENT_QUERY_CAP,
                );
                let source_context = format!(
                    "{source_kind} message at conversation position {}; {} image attachment(s), in attachment order. Existing message text:\n{source_text}",
                    group.item_index,
                    group.image_count(),
                );
                let describe = self.image_describe_cache.get_or_describe_urls(
                    client.clone(),
                    &model,
                    &group.image_urls,
                    outline.as_deref(),
                    &current_query,
                    &source_context,
                    &group.fingerprint,
                );
                match tokio::time::timeout(remaining, describe).await {
                    Ok(Ok(description)) => {
                        rewrite.replacement = Some(
                            crate::session::image_describe::render_image_description_block(
                                &description,
                            ),
                        );
                    }
                    Ok(Err(crate::session::image_describe::DescribeError::Sampling(info)))
                        if is_image_input_unsupported(&info, group.image_count()) =>
                    {
                        if let Err(error) = self
                            .record_unsupported_model_image_input(auxiliary_key.clone())
                            .await
                        {
                            tracing::warn!(
                                %error,
                                model,
                                "failed to persist auxiliary model image-input rejection"
                            );
                        }
                        tracing::warn!(
                            model,
                            message = %info.message,
                            "auxiliary model explicitly rejected image input"
                        );
                        break;
                    }
                    Ok(Err(error)) => {
                        tracing::warn!(
                            %error,
                            model,
                            item_index = group.item_index,
                            "auxiliary image description failed; dropping this image group"
                        );
                    }
                    Err(_) => {
                        tracing::warn!(
                            timeout_secs = IMAGE_RECOVERY_TIMEOUT.as_secs(),
                            "image context recovery reached its total timeout"
                        );
                        break;
                    }
                }
            }
        }

        let report = self
            .chat_state_handle
            .rewrite_images_and_ack(rewrites, UNSUPPORTED_IMAGE_PLACEHOLDER.to_owned())
            .await?;
        if report.unmatched_images > 0 {
            tracing::warn!(
                unmatched_images = report.unmatched_images,
                "image rewrite snapshot changed before atomic commit; unmatched images were removed"
            );
        }
        if let Some(message) = Self::image_recovery_notification(report) {
            self.send_grow_notification(GrowSessionUpdate::ImageDropped {
                notes: vec![message],
            })
            .await;
        }
        Some(report)
    }

    /// Pre-sampling gate for runtimes already present in the negative cache.
    /// Unknown runtimes remain optimistic and receive the original images.
    pub(super) async fn rewrite_images_for_known_text_model(
        &self,
    ) -> Result<chat_state::ImageRewriteReport, acp::Error> {
        let Some(key) = self.current_model_image_input_key().await else {
            return Ok(chat_state::ImageRewriteReport::default());
        };
        if !self.model_image_input_is_unsupported(&key).await {
            return Ok(chat_state::ImageRewriteReport::default());
        }
        self.rewrite_conversation_images_for_text_model(&key)
            .await
            .ok_or_else(|| {
                acp::Error::internal_error()
                    .data("failed to persist text-only image recovery; sampling was not resumed")
            })
    }

    pub(super) async fn prepare_tool_definitions_timed(&self) -> (Vec<ToolDefinition>, u64) {
        let mcp_wait_start = std::time::Instant::now();
        match self.mcp_strategy {
            McpInitStrategy::Blocking => {
                if !self.mcp_state.lock().await.is_initialized() {
                    tracing::info!(
                        "Blocking strategy: waiting for MCP initialization before first prompt..."
                    );
                    self.wait_for_mcp_initialized().await;
                }
            }
            McpInitStrategy::Progressive => {}
        }
        let mcp_wait_ms = mcp_wait_start.elapsed().as_millis() as u64;
        let defs = self.prepare_tool_definitions_inner().await;
        (defs, mcp_wait_ms)
    }
    pub(super) async fn prepare_tool_definitions(&self) -> Vec<ToolDefinition> {
        self.prepare_tool_definitions_timed().await.0
    }
    /// The exact tool specs a turn sends before the turn-specific
    /// structured-output append. `defs` is the already-resolved tool list from
    /// `prepare_tool_definitions_*`.
    pub(crate) fn turn_base_tool_specs(&self, defs: &[ToolDefinition]) -> Vec<ToolSpec> {
        defs.iter().cloned().map(ToolSpec::from).collect()
    }
    pub(super) async fn prepare_tool_definitions_inner(&self) -> Vec<ToolDefinition> {
        if self
            .subagent_capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.take_mcp_eligibility_change())
        {
            self.register_shared_client_tools().await;
        }
        let bridge = self.agent.borrow().tool_bridge().clone();
        if let Some(capabilities) = &self.subagent_capabilities {
            if capabilities.activate_pending() {
                self.push_system_reminder_with_tag(
                    &capabilities.native_catalog_prompt(),
                    crate::session::subagent_capability::CAPABILITY_CATALOG_TAG,
                );
            }
        }
        let mut defs = bridge.tool_definitions_builtins_only().await;
        if let Some(capabilities) = &self.subagent_capabilities {
            defs.retain(|definition| {
                bridge
                    .tool_kind(&definition.function.name)
                    .is_some_and(|kind| capabilities.allows_kind(kind))
            });
        }
        let delegated_goal_context = bridge
            .read_resource::<tools::implementations::grow_build::update_goal::GoalContextSnapshotResource>()
            .await
            .and_then(|resource| resource.0);
        if let Some(context) = delegated_goal_context.as_ref() {
            use tools::implementations::grow_build::task::types::GoalSubagentRole;
            use tools::types::tool::ToolKind;
            let stage_leaf = matches!(
                context.role,
                GoalSubagentRole::Planner | GoalSubagentRole::Verifier
            );
            defs.retain(|definition| {
                let kind = bridge.tool_kind(&definition.function.name);
                let goal_mutation = matches!(
                    kind,
                    Some(
                        ToolKind::GoalProgressUpdate
                            | ToolKind::GoalReplanRequest
                            | ToolKind::GoalLifecycleUpdate
                    )
                );
                let stage_owned_work = stage_leaf
                    && matches!(
                        kind,
                        Some(
                            ToolKind::Task
                                | ToolKind::BackgroundTaskAction
                                | ToolKind::WaitTasksAction
                                | ToolKind::KillTaskAction
                                | ToolKind::Monitor
                                | ToolKind::Workflow
                        )
                    );
                !goal_mutation && !stage_owned_work
            });
        }
        // A Behavior picker change may land while a regular turn is still
        // running. Tool snapshots/forks must retain that turn's captured mode;
        // otherwise a Normal turn can suddenly expose Goal tools (or lose Plan
        // tools) midway through its identity. Outside a turn the selected
        // Behavior remains the source of truth for session-info/compaction.
        let tool_behavior = if self
            .session_turn_active
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            *self.turn_behavior.lock()
        } else {
            self.behavior.lock().behavior()
        };
        let goal_behavior = tool_behavior == tool_types::BehaviorId::Goal;
        if !goal_behavior && delegated_goal_context.is_none() {
            defs.retain(|definition| {
                !matches!(
                    bridge.tool_kind(&definition.function.name),
                    Some(
                        tools::types::tool::ToolKind::GoalRead
                            | tools::types::tool::ToolKind::GoalProgressUpdate
                            | tools::types::tool::ToolKind::GoalReplanRequest
                            | tools::types::tool::ToolKind::GoalLifecycleUpdate
                    )
                )
            });
        }
        if tool_behavior == tool_types::BehaviorId::DeepResearch {
            // Deep Research foreground turns answer follow-up questions while
            // its private workflow runs, but they are as read-only as the
            // workflow workers. Unknown/unclassified tools fail closed.
            defs.retain(|definition| {
                bridge.tool_scope(&definition.function.name) == Some(tool_protocol::ToolScope::Read)
            });
        }
        let live_behavior = self.behavior.lock().behavior();
        if tool_behavior != tool_types::BehaviorId::Workflow
            || live_behavior != tool_types::BehaviorId::Workflow
        {
            defs.retain(|definition| {
                bridge.tool_kind(&definition.function.name)
                    != Some(tools::types::tool::ToolKind::Workflow)
            });
        }
        let plan_active = tool_behavior == tool_types::BehaviorId::Plan;
        if !plan_active {
            defs.retain(|definition| {
                bridge.tool_kind(&definition.function.name)
                    != Some(tools::types::tool::ToolKind::PlanControl)
            });
        }
        filter_cursor_tools_by_plan_mode(defs, plan_active)
    }
    pub(super) fn model_auth_facts(&self, model_id: &str) -> crate::agent::config::ModelAuthFacts {
        self.model_auth_state(model_id).0
    }
    pub(super) fn model_auth_provider(
        &self,
        model_id: &str,
    ) -> Option<crate::auth::AuthProviderRef> {
        self.model_auth_state(model_id).1
    }
    /// Drop the memoized per-model auth state; see [`Self::model_auth_memo`]
    /// for why each model/credential chokepoint must call this.
    pub(crate) fn invalidate_model_auth_memo(&self) {
        self.model_auth_memo.replace(None);
    }
    /// Reads and populates [`Self::model_auth_memo`]; a fresh `Unknown`
    /// falls back to the last definite entry (see the field's contract).
    fn model_auth_state(
        &self,
        model_id: &str,
    ) -> (
        crate::agent::config::ModelAuthFacts,
        Option<crate::auth::AuthProviderRef>,
    ) {
        use crate::agent::auth_method::ModelByok;
        use crate::session::acp_session::ModelAuthMemo;
        if let Some(memo) = self.model_auth_memo.borrow().as_ref()
            && memo.model_id == model_id
            && memo.facts.byok != ModelByok::Unknown
        {
            return (memo.facts, memo.provider.clone());
        }
        let (fresh, provider) =
            crate::agent::config::resolve_model_auth_facts_and_provider(model_id);
        if fresh.byok == ModelByok::Unknown {
            if let Some(memo) = self.model_auth_memo.borrow().as_ref()
                && memo.model_id == model_id
            {
                return (memo.facts, memo.provider.clone());
            }
            return (fresh, provider);
        }
        *self.model_auth_memo.borrow_mut() = Some(ModelAuthMemo {
            model_id: model_id.to_string(),
            facts: fresh,
            provider: provider.clone(),
        });
        (fresh, provider)
    }
    /// The single writer of a provider mint/rotation into chat-state credentials.
    async fn set_chat_api_key(&self, new_key: String) {
        let mut creds = self.chat_state_handle.get_credentials().await;
        creds.api_key = Some(new_key);
        self.chat_state_handle.update_credentials(creds);
    }
    /// Pre-turn arm for a provider-backed model: mint on a cold cache,
    /// re-mint near expiry, and adopt a rotation chat-state missed. No-op
    /// when `current_key` is already the fresh cached token.
    async fn refresh_provider_token_pre_turn(
        &self,
        provider: &crate::auth::AuthProviderRef,
        current_key: Option<&str>,
        model_id: &str,
    ) {
        match provider.ensure_fresh_token(current_key).await {
            crate::auth::ProviderRefreshOutcome::Rotated(new_key) => {
                tracing::info!(
                    model = %model_id,
                    provider = %provider.name,
                    cold = current_key.is_none(),
                    "auth provider token rotated pre-turn"
                );
                self.set_chat_api_key(new_key).await;
            }
            crate::auth::ProviderRefreshOutcome::Unchanged => {}
            crate::auth::ProviderRefreshOutcome::MintFailed => {
                tracing::warn!(
                    session_id = %self.session_info.id.0,
                    provider = %provider.name,
                    model = %model_id,
                    "auth provider pre-turn refresh failed"
                );
                ::diagnostics::unified_log::warn(
                    "auth provider pre-turn refresh failed",
                    Some(self.session_info.id.0.as_ref()),
                    Some(serde_json::json!({
                        "provider": provider.name,
                        "model": model_id,
                        "cold": current_key.is_none(),
                    })),
                );
            }
            crate::auth::ProviderRefreshOutcome::Unusable => {}
        }
    }
    /// 401 arm for a provider-backed model: re-run the helper once and
    /// resubmit. A missing key means the cold mint failed and the request
    /// went out unauthenticated, so mint instead. Returns `false` when the
    /// fresh-mint guard blocked the re-run or the helper failed; the 401
    /// then surfaces as a terminal error.
    async fn try_provider_401_recovery(&self, provider: &crate::auth::AuthProviderRef) -> bool {
        let rejected_key = self.chat_state_handle.get_credentials().await.api_key;
        let recovered = match rejected_key {
            Some(ref rejected_key) => provider.recover_rejected_token(rejected_key).await,
            None => provider.ensure_fresh_token(None).await.rotated(),
        };
        let Some(new_key) = recovered else {
            tracing::warn!(
                session_id = %self.session_info.id.0,
                provider = %provider.name,
                "auth recovery: sampler 401, provider re-mint declined or failed"
            );
            ::diagnostics::unified_log::warn(
                "auth recovery: sampler 401, provider re-mint declined or failed",
                Some(self.session_info.id.0.as_ref()),
                Some(serde_json::json!({ "provider": provider.name })),
            );
            return false;
        };
        tracing::info!(
            session_id = %self.session_info.id.0,
            provider = %provider.name,
            "auth recovery: sampler 401, auth provider re-mint, retrying"
        );
        ::diagnostics::unified_log::info(
            "auth recovery: sampler 401, auth provider re-mint, retrying",
            Some(self.session_info.id.0.as_ref()),
            None,
        );
        self.set_chat_api_key(new_key).await;
        true
    }
    /// Reconstruct a full `SamplerConfig` (with credentials) by combining
    /// the actor's `SamplingConfig` and `Credentials`. Folds in the
    /// URL-derived headers (cli-chat-proxy auth, the staging auth header)
    /// so the sampler crate stays URL-agnostic.
    pub(super) async fn reconstruct_full_config(&self) -> SamplingConfig {
        let cfg = self
            .chat_state_handle
            .get_sampling_config()
            .await
            .unwrap_or_else(|| sampling_types::SamplingConfig {
                base_url: String::new(),
                model: String::new(),
                output_limit: None,
                temperature: None,
                top_p: None,
                api_backend: Default::default(),
                extra_headers: Default::default(),
                query_params: Default::default(),
                env_http_headers: Default::default(),
                context_window: std::num::NonZeroU64::new(256_000).unwrap(),
                reasoning_effort: None,
                stream_tool_calls: None,
            });
        let creds = self.chat_state_handle.get_credentials().await;
        let model_facts = self.model_auth_facts(cfg.model.as_str());
        let provider = self.model_auth_provider(cfg.model.as_str());
        let api_key = creds.api_key;
        let auth_scheme = model_facts.auth_scheme;
        let mut extra_headers = cfg.extra_headers;
        crate::agent::config::inject_url_derived_headers(
            &mut extra_headers,
            creds.alpha_test_key.as_deref(),
            &cfg.base_url,
        );
        let compaction_at_tokens = self.compaction_at_tokens.get();
        let compactions_remaining = self.compactions_remaining.get();
        if compactions_remaining.is_some() || compaction_at_tokens.is_some() {
            let has_compaction_summary = self
                .chat_state_handle
                .get_last_compaction_prompt_index()
                .await
                .is_some();
            if let Some(value) =
                compactions_remaining.and_then(|c| c.resolve(has_compaction_summary))
            {
                extra_headers.insert("x-compactions-remaining".to_string(), value.to_string());
            }
            if !has_compaction_summary
                && let Some(value) = compaction_at_tokens.and_then(|c| {
                    c.resolve(
                        cfg.context_window.get(),
                        self.compaction.threshold_percent.get(),
                    )
                })
            {
                extra_headers.insert("x-compaction-at".to_string(), value.to_string());
            }
        }
        SamplingConfig {
            api_key,
            base_url: cfg.base_url,
            model: cfg.model,
            output_limit: cfg.output_limit,
            temperature: cfg.temperature,
            top_p: cfg.top_p,
            api_backend: cfg.api_backend,
            auth_scheme,
            extra_headers,
            query_params: cfg.query_params.clone(),
            env_http_headers: cfg.env_http_headers.clone(),
            context_window: cfg.context_window.get(),
            reasoning_effort: cfg.reasoning_effort,
            force_http1: false,
            max_retries: Some(self.max_retries.get()),
            stream_tool_calls: cfg.stream_tool_calls.unwrap_or(false),
            idle_timeout_secs: None,
            origin_client: self.origin_client.clone(),
            attribution_callback: None,
            bearer_resolver: provider
                .as_ref()
                .map(crate::auth::AuthProviderRef::bearer_resolver),
            compactions_remaining: self.compactions_remaining.get(),
            compaction_at_tokens: self.compaction_at_tokens.get(),
            doom_loop_recovery: self.doom_loop_recovery,
        }
    }
    /// Build the provider-valid, read-only authorization view used for a child
    /// permission judgment. Only genuine user-origin task turns cross the
    /// trust boundary; assistant/tool/synthetic content cannot authorize a
    /// fence widening. The source chat state is never mutated or compacted.
    pub(super) async fn child_permission_judgment_items(
        &self,
        judgment: &workspace::permission::PermissionJudgmentRequest,
        input: crate::config::SubagentClassifierInput,
    ) -> Vec<ConversationItem> {
        if input == crate::config::SubagentClassifierInput::RequestOnly {
            let mut request_only = judgment.clone();
            request_only.prompt_type = workspace::permission::ClassifierPromptType::JustCommand;
            return request_only
                .classifier_messages()
                .into_iter()
                .map(|message| match message.role {
                    workspace::permission::ClassifierMessageRole::System => {
                        ConversationItem::system(message.text)
                    }
                    workspace::permission::ClassifierMessageRole::User => {
                        ConversationItem::user(message.text)
                    }
                })
                .collect();
        }

        const BUDGET_PERCENT: u64 = 85;
        const BUDGET_HEADROOM_TOKENS: u64 = 4_000;
        let judgment_message =
            workspace::permission::build_primary_context_judgment_message(judgment);
        let policy = workspace::permission::primary_context_judgment_system_prompt(judgment);
        let mut items = vec![ConversationItem::system(policy)];
        items.extend(
            self.chat_state_handle
                .get_conversation()
                .await
                .into_iter()
                .filter(|item| match item {
                    ConversationItem::User(user) => user.permission_evidence.is_some(),
                    _ => false,
                }),
        );

        // Apply the normal 85%-with-headroom budget to the snapshot copy only.
        // Keep the dedicated authorization policy, then spend the remainder
        // only on recent genuine user-origin task context. Model/tool output
        // and synthetic summaries are deliberately absent from this branch.
        let context_window = self
            .chat_state_handle
            .get_sampling_config()
            .await
            .map(|config| config.context_window.get())
            .unwrap_or(256_000);
        let judgment_tokens = (judgment_message.len() as u64).div_ceil(4);
        let snapshot_budget = (context_window.saturating_mul(BUDGET_PERCENT) / 100)
            .saturating_sub(BUDGET_HEADROOM_TOKENS)
            .saturating_sub(judgment_tokens);
        let estimated_tokens = serde_json::to_vec(&items)
            .map(|json| (json.len() as u64).div_ceil(4))
            .unwrap_or(u64::MAX);
        if estimated_tokens > snapshot_budget {
            let system_index = Some(0usize);
            let mut retained = Vec::new();
            let mut recent = Vec::new();
            for (index, item) in items.into_iter().enumerate() {
                if Some(index) == system_index {
                    retained.push(item);
                } else {
                    recent.push(item);
                }
            }
            let retained_tokens = serde_json::to_vec(&retained)
                .map(|json| (json.len() as u64).div_ceil(4))
                .unwrap_or(snapshot_budget);
            let recent = chat_state::compaction_utils::fit_conversation_to_budget(
                recent,
                snapshot_budget.saturating_sub(retained_tokens),
            );
            retained.extend(recent);
            items = retained;
        }
        items.push(ConversationItem::user(judgment_message));
        items
    }

    /// Install auto-mode permission classifier with a live LLM side-query
    /// (laziness-classifier pattern: `prepare_chat_completion` +
    /// `conversation_collect` on a LocalSet task; channel bridges the
    /// `Send` permission actor). Child requests branch from a read-only
    /// snapshot of the primary conversation; the ephemeral judgment turn and
    /// response are never written back to chat state.
    pub(crate) async fn wire_permission_auto_llm_classifier(self: &Arc<Self>) {
        if self.permissions.has_llm_side_query() {
            return;
        }
        if self.startup_hints.is_subagent {
            tracing::warn!(
                session_id = %self.session_info.id,
                "subagent auto permission classifier was not wired by the primary session"
            );
            return;
        }
        // The shared primary permission actor also classifies independent
        // child `auto` requests while the primary UI remains in `ask`. Wiring
        // is cheap; inference only happens when an auto request arrives.
        let auto_cfg = crate::util::config::resolve_auto_mode_config_from_disk();
        let aux_classifier_sampler = match auto_cfg.classifier_model.as_deref() {
            Some(slug) => self.resolve_auto_classifier_sampler(slug).await.map(Some),
            None => Ok(None),
        };
        let (prompt_type, classifier_reasoning_effort) =
            crate::util::config::auto_mode_classifier_defaults(&auto_cfg);
        let classify_timeout = crate::util::config::auto_mode_classify_timeout(&auto_cfg);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(
            workspace::permission::PermissionJudgmentRequest,
            tokio::sync::oneshot::Sender<Result<String, workspace::permission::ClassifierFailure>>,
        )>();
        // Do not let the classifier channel keep the primary session alive.
        // The permission actor owns `tx`; a strong SessionActor capture here
        // would form a cycle and keep both the side-query worker and audit
        // bridge alive after session teardown.
        let weak_session = Arc::downgrade(self);
        tokio::task::spawn_local(async move {
            while let Some((judgment, mut respond_to)) = rx.recv().await {
                if respond_to.is_closed() {
                    tracing::debug!("permission judgment requester disappeared before dispatch");
                    continue;
                }
                let Some(session) = weak_session.upgrade() else {
                    let _ = respond_to.send(Err(
                        workspace::permission::ClassifierFailure::TransportError(
                            "primary session ended before permission judgment".to_owned(),
                        ),
                    ));
                    break;
                };
                let judgment_future = async {
                    // The total deadline starts before config, credential, and
                    // client preparation so setup latency cannot escape the
                    // bounded time charged to the child tool call.
                    let judgment_deadline = tokio::time::Instant::now() + classify_timeout;
                    let is_child_judgment = judgment.uses_primary_context();
                    let setup = async {
                        let classifier_input = if is_child_judgment {
                            session.subagent_classifier_input
                        } else {
                            crate::config::SubagentClassifierInput::RequestOnly
                        };
                        let (sampling_client, model, reasoning_effort) = if is_child_judgment {
                            let client =
                                session.prepare_chat_completion(false).await.map_err(|e| {
                                    workspace::permission::ClassifierFailure::TransportError(
                                        e.to_string(),
                                    )
                                })?;
                            let model = session
                                .chat_state_handle
                                .get_sampling_config()
                                .await
                                .map(|config| config.model)
                                .unwrap_or_default();
                            // Child safety judgments use the primary session's
                            // active model, but they are classifier calls rather
                            // than continuations of the active turn. Inheriting
                            // a `max` turn effort can consume the whole bounded
                            // attempt window before the short JSON verdict is
                            // produced. Keep effort under the classifier policy
                            // for both primary and child judgment paths.
                            (client, model, classifier_reasoning_effort)
                        } else {
                            let (client, model) = match &aux_classifier_sampler {
                                Ok(Some((client, model))) => (client.clone(), model.clone()),
                                Ok(None) => {
                                    let client =
                                    session.prepare_chat_completion(false).await.map_err(|e| {
                                        workspace::permission::ClassifierFailure::TransportError(
                                            e.to_string(),
                                        )
                                    })?;
                                    let model = session
                                        .chat_state_handle
                                        .get_sampling_config()
                                        .await
                                        .map(|config| config.model)
                                        .unwrap_or_default();
                                    (client, model)
                                }
                                Err(reason) => {
                                    return Err(
                                        workspace::permission::ClassifierFailure::TransportError(
                                            reason.clone(),
                                        ),
                                    );
                                }
                            };
                            (client, model, classifier_reasoning_effort)
                        };
                        let items = if is_child_judgment {
                            session
                                .child_permission_judgment_items(&judgment, classifier_input)
                                .await
                        } else {
                            judgment
                                .classifier_messages()
                                .into_iter()
                                .map(|message| match message.role {
                                    workspace::permission::ClassifierMessageRole::System => {
                                        ConversationItem::system(message.text)
                                    }
                                    workspace::permission::ClassifierMessageRole::User => {
                                        ConversationItem::user(message.text)
                                    }
                                })
                                .collect::<Vec<_>>()
                        };
                        let json_output =
                            permission_judgment_json_output(sampling_client.api_backend());
                        Ok::<_, workspace::permission::ClassifierFailure>((
                            sampling_client,
                            model,
                            reasoning_effort,
                            items,
                            json_output,
                        ))
                    };
                    let (sampling_client, model, reasoning_effort, items, json_output) =
                        tokio::time::timeout_at(judgment_deadline, setup)
                            .await
                            .map_err(|_| workspace::permission::ClassifierFailure::Timeout)??;
                    // One total deadline covers every attempt. Dividing the
                    // remaining budget by the remaining attempts lets a hung
                    // first call retry once without doubling the permission
                    // latency seen by the child.
                    for attempt in 1..=PERMISSION_JUDGMENT_MAX_ATTEMPTS {
                        let mut attempt_items = items.clone();
                        if attempt > 1 {
                            // The failed provider payload is deliberately not
                            // added to the branch. Only the failure category is
                            // carried forward, so untrusted output cannot become
                            // new permission evidence.
                            attempt_items
                                .push(ConversationItem::user(PERMISSION_JUDGMENT_RETRY_MESSAGE));
                        }
                        let request = ConversationRequest {
                            items: attempt_items,
                            tools: vec![],
                            tool_choice: None,
                            model: Some(model.clone()),
                            temperature: None,
                            max_output_tokens: Some(PERMISSION_JUDGMENT_MAX_OUTPUT_TOKENS),
                            json_output: Some(json_output.clone()),
                            reasoning_effort,
                            ..ConversationRequest::default()
                        };
                        let fut = sampling_client.conversation_collect(request);
                        let attempts_remaining = PERMISSION_JUDGMENT_MAX_ATTEMPTS - attempt + 1;
                        let remaining = judgment_deadline
                            .saturating_duration_since(tokio::time::Instant::now());
                        let attempt_budget = remaining / attempts_remaining as u32;
                        let response = match tokio::time::timeout(attempt_budget, fut).await {
                            Ok(Ok(response)) => response,
                            Ok(Err(error))
                                if attempt < PERMISSION_JUDGMENT_MAX_ATTEMPTS
                                    && permission_judgment_error_needs_retry(&error) =>
                            {
                                tracing::warn!(
                                    attempt,
                                    max_attempts = PERMISSION_JUDGMENT_MAX_ATTEMPTS,
                                    backend = ?sampling_client.api_backend(),
                                    "permission judgment hit a transient provider error; retransmitting once"
                                );
                                continue;
                            }
                            Ok(Err(error)) => {
                                return Err(
                                    workspace::permission::ClassifierFailure::TransportError(
                                        error.to_string(),
                                    ),
                                );
                            }
                            Err(_) if attempt < PERMISSION_JUDGMENT_MAX_ATTEMPTS => {
                                tracing::warn!(
                                    attempt,
                                    max_attempts = PERMISSION_JUDGMENT_MAX_ATTEMPTS,
                                    backend = ?sampling_client.api_backend(),
                                    "permission judgment attempt timed out; retransmitting once within the total deadline"
                                );
                                continue;
                            }
                            Err(_) => {
                                return Err(workspace::permission::ClassifierFailure::Timeout);
                            }
                        };
                        let model_text = response.assistant_text();
                        if !permission_judgment_needs_retry(&model_text)
                            || attempt == PERMISSION_JUDGMENT_MAX_ATTEMPTS
                        {
                            return Ok(model_text);
                        }
                        tracing::warn!(
                            attempt,
                            max_attempts = PERMISSION_JUDGMENT_MAX_ATTEMPTS,
                            backend = ?sampling_client.api_backend(),
                            "permission judgment returned invalid structured output; retransmitting once"
                        );
                    }
                    unreachable!("permission judgment attempt loop always returns")
                };
                tokio::pin!(judgment_future);
                let result = tokio::select! {
                    biased;
                    _ = respond_to.closed() => {
                        tracing::debug!("permission judgment requester disappeared; cancelling side-query");
                        continue;
                    }
                    result = &mut judgment_future => result,
                };
                if let Err(error) = &result {
                    tracing::warn!(%error, "permission auto classifier side-query failed");
                }
                let _ = respond_to.send(result);
            }
        });
        let clf = workspace::permission::LlmPermissionClassifier::with_channel(tx, prompt_type);
        debug_assert!(
            clf.has_side_query(),
            "channel-wired classifier must report has_side_query"
        );
        self.permissions.set_classifier_with_side_query(clf, true);
        tracing::info!(
            session_id = %self.session_info.id,
            "Wired live LLM permission auto-mode classifier (session sampling channel)"
        );
    }
    /// Resolve a standalone aux-model `SamplerConfig` from the configured
    /// provider catalog. `None` (including an empty model ID) leaves fallback
    /// policy to the caller: optional classifiers may inherit the active
    /// sampler, while explicitly configured image description must fail
    /// visibly instead of switching models.
    pub(super) async fn resolve_aux_sampler_config(
        &self,
        slug: &str,
    ) -> Option<sampler::SamplerConfig> {
        let creds = self.chat_state_handle.get_credentials().await;
        let models = self.models_manager.models();
        crate::agent::config::resolve_aux_model_sampling_config(
            slug,
            &models,
            creds.alpha_test_key.clone(),
        )
    }
    /// Resolve a dedicated sampler for the Auto-mode classifier model `slug`,
    /// stamping session-local auth/attribution like image-describe (which relies
    /// on the resolver, not a config override, for `base_url`/`api_backend` so
    /// credentials stay consistent). Once a classifier model is configured,
    /// resolution/build failure is terminal for that classifier request; it
    /// must not silently switch to the session model.
    async fn resolve_auto_classifier_sampler(
        &self,
        slug: &str,
    ) -> Result<(sampler::SamplingClient, String), String> {
        let active_session_config = self.reconstruct_full_config().await;
        let mut cfg = self
            .resolve_aux_sampler_config(slug)
            .await
            .ok_or_else(|| format!("configured auto classifier model `{slug}` is unavailable"))?;
        crate::agent::config::stamp_session_local_sampler_fields(
            &mut cfg,
            &active_session_config,
            Some(self.max_retries.get()),
        );
        let model = cfg.model.clone();
        let client = sampler::SamplingClient::new(cfg).map_err(|error| {
            format!("configured auto classifier model `{slug}` could not be initialized: {error}")
        })?;
        Ok((client, model))
    }
    #[tracing::instrument(
        name = "session.prepare_chat_completion",
        skip_all,
        fields(force_http1)
    )]
    pub(super) async fn prepare_chat_completion(
        &self,
        force_http1: bool,
    ) -> Result<sampler::SamplingClient, acp::Error> {
        self.refresh_byok_credential().await;
        let mut full_config = self.reconstruct_full_config().await;
        full_config.force_http1 = force_http1;
        full_config.idle_timeout_secs = Some(self.inference_idle_timeout.get().as_secs());
        let sampling_client =
            sampler::SamplingClient::new(full_config).map_err(|e| self.to_acp_error(e))?;
        Ok(sampling_client)
    }
    /// Push a fresh `SamplerConfig` into the per-session sampler actor
    /// before each turn. Mirrors `prepare_chat_completion`'s
    /// auth-refresh + config rebuild, but routes the result to the
    /// `sampler` instead of constructing a new
    /// `OaiCompatClient`.
    ///
    /// Behaviour parity: we run the same `refresh_byok_credential()`
    /// and `reconstruct_full_config()` so the sampler picks up any
    /// refreshed BYOK credentials. The previous client cache inside
    /// the sampler actor is invalidated automatically by
    /// `update_config`.
    pub(crate) async fn prepare_sampler_for_turn(
        &self,
    ) -> tools::types::resources::ModelImageInputKey {
        self.refresh_byok_credential().await;
        let mut sampler_config = self.reconstruct_full_config().await;
        if self.tool_context.task_output_token_budget.is_some()
            || self.tool_context.sampler_retry_only_before_output
        {
            sampler_config.doom_loop_recovery = None;
        }
        sampler_config.idle_timeout_secs = Some(self.inference_idle_timeout.get().as_secs());
        let image_input_key = sampler_model_image_input_key(&sampler_config);
        self.sampler_handle.update_config(sampler_config);
        image_input_key
    }
    fn log_terminal_failure(&self, error_type: &str, status_code: Option<u16>, message: &str) {
        ::diagnostics::unified_log::warn(
            "turn.terminal_failure",
            Some(self.session_info.id.0.as_ref()),
            Some(serde_json::json!({
                "error_type": error_type,
                "status_code": status_code,
                "message": crate::util::truncate(message, 300),
            })),
        );
    }
    pub(crate) async fn handle_sampling_failure(
        self: &Arc<Self>,
        error: sampler::SamplingErrorInfo,
        request_image_count: usize,
        request_image_input_key: Option<tools::types::resources::ModelImageInputKey>,
    ) -> Result<SamplerFailureRecovery, acp::Error> {
        use sampler::SamplingErrorKind;
        if is_image_input_unsupported(&error, request_image_count)
            && let Some(key) = request_image_input_key
        {
            let model = key.model().to_string();
            let first_rejection = self
                .record_unsupported_model_image_input(key.clone())
                .await
                .map_err(|persist_error| {
                    acp::Error::internal_error().data(format!(
                        "failed to persist text-only model capability: {persist_error}; sampling was not resumed"
                    ))
                })?;
            tracing::warn!(
                model,
                request_image_count,
                first_rejection,
                "model explicitly rejected image input; rewriting canonical image context"
            );
            if self
                .rewrite_conversation_images_for_text_model(&key)
                .await
                .is_none()
            {
                return Err(acp::Error::internal_error()
                    .data("failed to persist text-only image recovery; sampling was not resumed"));
            }
            return Ok(SamplerFailureRecovery::ImageInputUnsupportedAndResubmit);
        }
        if self.tool_context.task_output_token_budget.is_some() {
            self.tool_context.fail_task_output_usage_closed();
            let message = format!(
                "budgeted workflow child model request failed; output grant exhausted: {}",
                error.message
            );
            self.log_terminal_failure("output_budget_usage_unknown", error.status_code, &message);
            return Err(acp::Error::internal_error().data(message));
        }
        if self.tool_context.sampler_retry_only_before_output {
            let handle = self.chat_state_handle.clone();
            tokio::spawn(async move {
                let _ = handle.mark_usage_incomplete(true, true).await;
            });
            let message = format!(
                "workflow child model request failed; usage may understate real spend: {}",
                error.message
            );
            self.log_terminal_failure(
                "workflow_child_sampling_failed",
                error.status_code,
                &message,
            );
            return Err(acp::Error::internal_error().data(message));
        }
        if self.should_compact_on_error(&error).await {
            let cw = error
                .model_metadata
                .as_ref()
                .and_then(|m| m.context_window)
                .expect("should_compact_on_error guarantees context_window");
            {
                let total_tokens = self.chat_state_handle.get_estimated_total_tokens().await;
                let percentage = token_estimation::usage_percentage_u8(total_tokens, cw);
                if let Some(mut cfg) = self.chat_state_handle.get_sampling_config().await
                    && let Some(new_cw) = std::num::NonZeroU64::new(cw)
                    && self.compaction.context_window_override.is_none()
                {
                    cfg.context_window = new_cw;
                    self.chat_state_handle.update_sampling_config(cfg);
                }
                let trigger_info = compaction::AutoCompactTriggerInfo {
                    tokens_used: total_tokens,
                    context_window: cw,
                    percentage,
                    source: "sampler_error_recovery",
                };
                if let Err(e) = self.run_compact_only(trigger_info).await {
                    if Self::is_auth_compact_error(&e) {
                        return Err(self.surface_compact_auth_failure(e).await);
                    }
                    return Err(e);
                }
                return Ok(SamplerFailureRecovery::CompactAndResubmit);
            }
        }
        let detailed_message = error.message.clone();
        if matches!(error.kind, SamplingErrorKind::Api)
            && error.status_code == Some(400)
            && error.message.contains("encrypted_content")
        {
            self.signals_handle()
                .record_error_typed("encrypted_content_mismatch");
            let friendly = "This session's conversation history is incompatible \
                            with the current model. Please start a new session."
                .to_string();
            self.log_terminal_failure("encrypted_content_mismatch", error.status_code, &friendly);
            self.send_grow_notification(GrowSessionUpdate::RetryState(
                crate::extensions::notification::RetryState::Failed {
                    error_type: "encrypted_content_mismatch".to_string(),
                    message: friendly.clone(),
                },
            ))
            .await;
            return Err(acp::Error::invalid_params().data(friendly));
        }
        if matches!(error.kind, SamplingErrorKind::RateLimited) {
            self.log_terminal_failure("rate_limited", error.status_code, &detailed_message);
            self.send_grow_notification(GrowSessionUpdate::RetryState(
                crate::extensions::notification::RetryState::Exhausted {
                    attempts: 0,
                    reason: detailed_message.clone(),
                    is_rate_limited: true,
                },
            ))
            .await;
            let acp_err = acp::Error::new(
                crate::sampling::error::RATE_LIMITED_ERROR_CODE,
                "Rate limited".to_string(),
            )
            .data(detailed_message);
            return Err(acp_err);
        }
        let failed_model_id = self
            .chat_state_handle
            .get_sampling_config()
            .await
            .map(|c| c.model)
            .unwrap_or_default();
        let is_auth_401 =
            error.status_code == Some(401) || matches!(error.kind, SamplingErrorKind::Auth);
        let auth_provider = is_auth_401
            .then(|| self.model_auth_provider(&failed_model_id))
            .flatten();

        if let Some(ref provider) = auth_provider
            && self.try_provider_401_recovery(provider).await
        {
            self.prepare_sampler_for_turn().await;
            return Ok(SamplerFailureRecovery::RefreshByokAndResubmit {
                credential: error.credential,
            });
        }

        // A fail-closed request may have gone out before an env/config key was
        // available. Re-read BYOK once; this is not a credential rejection and
        // therefore must not consume the rejected-key retry budget.
        if is_auth_401
            && auth_provider.is_none()
            && error.credential.is_missing()
            && self.refresh_byok_credential().await
        {
            self.prepare_sampler_for_turn().await;
            return Ok(SamplerFailureRecovery::RefreshByokAndResubmit {
                credential: error.credential,
            });
        }
        if matches!(error.kind, SamplingErrorKind::IdleTimeout) {
            self.signals_handle().record_idle_timeout();
        }
        if matches!(error.kind, SamplingErrorKind::EmptyResponse) {
            if let Some(ref ctx) = error.empty_response_context {
                tracing::warn!(
                    empty_response = true,
                    empty_reason = ctx.reason.as_str(),
                    had_reasoning = ctx.had_reasoning,
                    content_len = ctx.content_len,
                    tool_call_count = ctx.tool_call_count,
                    completion_tokens = ctx.completion_tokens.unwrap_or(0),
                    reasoning_tokens = ctx.reasoning_tokens.unwrap_or(0),
                    finish_reason = ctx.finish_reason_str(),
                    first_choice_seen = ctx.first_choice_seen,
                    model = %ctx.model,
                    "empty response after retries exhausted: {reason}",
                    reason = ctx.reason,
                );
            }
            self.signals_handle().record_error_typed("empty_response");
        }
        let model_auth_facts = self.model_auth_facts(&failed_model_id);
        let auth_mode_str = if let Some(provider) = auth_provider.as_ref() {
            format!("BYOK helper ([auth_provider.{}])", provider.name)
        } else if model_auth_facts.byok == crate::agent::auth_method::ModelByok::Byok {
            "BYOK (api_key/env_key or keyless provider)".to_string()
        } else {
            "BYOK (credential missing or rejected)".to_string()
        };
        let client_version = version::VERSION;
        let is_model_404 =
            error.status_code == Some(404) && detailed_message.contains("does not exist");
        let detailed_message = if is_model_404 || is_auth_401 {
            let current_model = self
                .chat_state_handle
                .get_sampling_config()
                .await
                .map(|c| c.model)
                .unwrap_or_else(|| "unknown".to_string());
            let available: Vec<String> = self
                .models_manager
                .models()
                .values()
                .map(|m| m.model.clone())
                .collect();
            let mut msg = format!("{detailed_message}\n");
            msg.push_str(&format!("\n  Model:     {current_model}"));
            msg.push_str(&format!("\n  Auth:      {auth_mode_str}"));
            if let Some(ref provider) = auth_provider {
                msg.push_str(&format!(
                    "\n  Fix:       check [auth_provider.{}] and the debug log",
                    provider.name
                ));
            } else if is_auth_401
                && model_auth_facts.byok == crate::agent::auth_method::ModelByok::Byok
            {
                msg.push_str(
                    "\n  Fix:       check this model's provider api_key/env_key and endpoint in ~/.grow/config.toml",
                );
            }
            msg.push_str(&format!("\n  Version:   {client_version}"));
            if available.is_empty() {
                msg.push_str("\n  Available: (none)");
            } else {
                msg.push_str(&format!("\n  Available: {}", available.join(", ")));
            }
            if is_model_404 && !available.iter().any(|m| m == &current_model) {
                msg.push_str(&format!(
                    "\n\n  '{}' is not in your available models.",
                    current_model
                ));
                msg.push_str("\n  Switch models with /model or start a new session.");
            }
            msg
        } else {
            detailed_message
        };
        let error_type = if sampling_types::is_context_length_error(&error.message) {
            "context_length"
        } else if is_auth_401 {
            "provider_credentials"
        } else {
            error.kind.as_str()
        };
        let (error_type, detailed_message) = (error_type, detailed_message);
        self.log_terminal_failure(error_type, error.status_code, &detailed_message);
        self.send_grow_notification(GrowSessionUpdate::RetryState(
            crate::extensions::notification::RetryState::Failed {
                error_type: error_type.to_string(),
                message: detailed_message.clone(),
            },
        ))
        .await;
        Err(
            acp::Error::internal_error().data(crate::sampling::error::terminal_error_data(
                detailed_message,
                error.status_code,
                error.kind,
            )),
        )
    }
    /// Drive a single turn through the sampler-based path.
    ///
    /// Calls `prepare_sampler_for_turn` first (auth refresh + config
    /// push), then submits via `SamplerHandle::submit_and_collect` and
    /// returns:
    /// * `Ok(SamplerTurnOutcome::Response(_))` - model responded.
    /// * `Ok(SamplerTurnOutcome::CompactAndResubmit)` - compaction
    ///    ran, the outer turn loop should `continue`.
    /// * `Ok(SamplerTurnOutcome::RefreshByokAndResubmit { .. })` - a BYOK source recovered a 401
    ///    recovery succeeded, credentials refreshed, retry once.
    /// * `Err(acp::Error)` - terminal failure already reported via
    ///    `send_grow_notification(RetryState::Failed)`.
    pub(crate) async fn run_turn_via_sampler(
        self: &Arc<Self>,
        request: ConversationRequest,
    ) -> Result<SamplerTurnOutcome, acp::Error> {
        let request_image_input_key = self.prepare_sampler_for_turn().await;
        let request_image_count = request.image_count();
        let stream_drained_rx = {
            let (tx, rx) = tokio::sync::oneshot::channel();
            *self.turn_stream_drained.lock() = Some(tx);
            rx
        };
        let request_id = sampler::RequestId::random();
        let request_id_str = request_id.as_str().to_string();
        let collect = self
            .sampler_handle
            .submit_and_collect(request_id.clone(), request);
        tokio::pin!(collect);
        let collected = tokio::select! {
            biased;
            _ = super::tool_calls::wait_for_pending_interjection(
                &self.pending_interjections,
            ) => {
                self.sampler_handle.cancel(request_id.clone());
                // Steering restarts sampling inside the same visible turn; it
                // never creates a second foreground owner or terminal event.
                let _ = collect.await;
                self.turn_stream_drained.lock().take();
                tracing::info!(
                    sampler_request_id = request_id_str,
                    "soft-preempted sampling for user steering"
                );
                None
            },
            result = &mut collect => Some(result),
        };
        let Some(collected) = collected else {
            return Ok(SamplerTurnOutcome::Steered);
        };
        match collected {
            Ok((response, metrics)) => {
                let span = tracing::Span::current();
                span.record("request_id", request_id_str.as_str());
                if let Some(ttft) = metrics.time_to_first_token_ms {
                    span.record("ttft_ms", ttft as i64);
                }
                if metrics.attempts > 0 {
                    span.record("attempt", i64::from(metrics.attempts));
                }
                if tokio::time::timeout(std::time::Duration::from_secs(5), stream_drained_rx)
                    .await
                    .is_err()
                {
                    self.turn_stream_drained.lock().take();
                    tracing::warn!(
                        "stream-drain barrier timed out; proceeding to emit tool \
                         calls (eventId ordering may be imperfect this turn)"
                    );
                }
                Ok(SamplerTurnOutcome::Response(
                    Box::new(response),
                    Box::new(metrics),
                ))
            }
            Err(rich_err) => {
                self.turn_stream_drained.lock().take();
                let info = sampler::SamplingErrorInfo::from(&rich_err);
                match self
                    .handle_sampling_failure(
                        info,
                        request_image_count,
                        Some(request_image_input_key),
                    )
                    .await?
                {
                    SamplerFailureRecovery::CompactAndResubmit => {
                        Ok(SamplerTurnOutcome::CompactAndResubmit)
                    }
                    SamplerFailureRecovery::ImageInputUnsupportedAndResubmit => {
                        Ok(SamplerTurnOutcome::ImageInputUnsupportedAndResubmit)
                    }
                    SamplerFailureRecovery::RefreshByokAndResubmit { credential } => {
                        Ok(SamplerTurnOutcome::RefreshByokAndResubmit { credential })
                    }
                }
            }
        }
    }
    /// Re-read the selected model's BYOK source before a request.
    ///
    /// Named helpers are single-flighted by their provider slot. Static and env
    /// keys are re-resolved from effective config so rotation remains external
    /// to Grow. Returns true only when chat-state received a different key.
    pub(crate) async fn refresh_byok_credential(&self) -> bool {
        let current_model_id = self
            .chat_state_handle
            .get_sampling_config()
            .await
            .map(|c| c.model)
            .unwrap_or_default();
        let current_key = self.chat_state_handle.get_credentials().await.api_key;

        if let Some(provider) = self.model_auth_provider(&current_model_id) {
            self.refresh_provider_token_pre_turn(
                &provider,
                current_key.as_deref(),
                &current_model_id,
            )
            .await;
            return self.chat_state_handle.get_credentials().await.api_key != current_key;
        }

        let Some(new_key) = self.reload_api_key_from_config(&current_model_id) else {
            return false;
        };
        if current_key.as_deref() == Some(new_key.as_str()) {
            return false;
        }
        let mut creds = self.chat_state_handle.get_credentials().await;
        creds.api_key = Some(new_key);
        self.chat_state_handle.update_credentials(creds);
        true
    }
    fn reload_api_key_from_config(&self, current_model_id: &str) -> Option<String> {
        let raw_config = crate::config::load_effective_config()
            .map_err(|e| tracing::warn!(error = %e, "Failed to reload config"))
            .ok()?;
        let config = crate::agent::config::Config::new_from_toml_cfg(&raw_config)
            .map_err(|e| tracing::warn!(error = %e, "Failed to parse reloaded config.toml"))
            .ok()?;
        let config_model = config
            .config_models
            .iter()
            .find(|(k, v)| v.model.as_deref().unwrap_or(k.as_str()) == current_model_id)
            .map(|(_, v)| v);
        let Some(model) = config_model else {
            tracing::warn!(
                model = %current_model_id,
                available = ?config.config_models.keys().collect::<Vec<_>>(),
                "Model not found in config.toml [provider.*.models.*]"
            );
            return None;
        };
        let key = crate::agent::config::first_own_credential(
            model.api_key.as_deref(),
            model.env_key.as_ref(),
        );
        if key.is_none() {
            tracing::warn!(
                model = %current_model_id,
                env_key = ?model.env_key,
                "No api_key or env_key resolved for model"
            );
        }
        key
    }
    /// Propagate the model-reported token usage from a turn response into
    /// chat state, the per-prompt usage ledger, and per-turn signals.
    ///
    /// This is the only place per-turn `total_tokens` is refreshed in the
    /// post-sampler-refactor path; without it `state.total_tokens` would
    /// stay frozen at the `estimate_conversation_tokens` seed from
    /// `ChatState::new`, freezing `/context` and corrupting the resume
    /// restore that reads `meta.totalTokens` from `updates.jsonl`.
    /// Resetting `estimated_tokens_since_model = 0` here also keeps the
    /// preflight-overflow guard accurate against the next turn's
    /// tool-result deltas.
    pub(crate) fn record_response_token_usage(
        &self,
        response: &ConversationResponse,
        api_duration_ms: Option<u64>,
    ) {
        if let Some(ref u) = response.usage {
            self.tool_context
                .record_task_model_output(u64::from(u.completion_tokens));
            self.chat_state_handle
                .record_token_usage(u64::from(u.total_tokens));
            self.chat_state_handle.record_last_turn_usage(u.clone());
            self.chat_state_handle.record_model_call_usage(
                response.assistant().and_then(|a| a.model_id.clone()),
                u.clone(),
                api_duration_ms,
                response.cost_usd_ticks,
            );
            self.signals_handle()
                .record_token_usage(u.completion_tokens, u.reasoning_tokens);
        } else if self.tool_context.task_output_token_budget.is_some() {
            self.tool_context.fail_task_output_usage_closed();
            let handle = self.chat_state_handle.clone();
            tokio::spawn(async move {
                let _ = handle.mark_usage_incomplete(true, true).await;
            });
        } else if self.tool_context.sampler_retry_only_before_output {
            let handle = self.chat_state_handle.clone();
            tokio::spawn(async move {
                let _ = handle.mark_usage_incomplete(true, true).await;
            });
        }
    }
    pub(super) async fn record_assistant_response(&self, assistant_item: ConversationItem) {
        self.signals_handle().record_assistant_message();
        if let ConversationItem::Assistant(ref a) = assistant_item {
            tracing::info!(model_id = ?a.model_id, "DEBUG record_assistant_response model_id");
        }
        if let ConversationItem::Assistant(ref a) = assistant_item
            && let Some(first_call) = a.tool_calls.first()
        {
            tracing::info!("Assistant requested tool call: {}", first_call.id);
        }
        self.chat_state_handle
            .push_assistant_response(assistant_item);
    }
}

#[cfg(test)]
mod image_input_rejection_tests {
    use super::*;

    fn sampling_api_error(
        status: reqwest::StatusCode,
        should_retry: Option<bool>,
    ) -> sampling_types::SamplingError {
        sampling_types::SamplingError::Api {
            status,
            message: "provider error".into(),
            model_metadata: None,
            retry_after_secs: None,
            should_retry,
        }
    }

    #[test]
    fn permission_judgment_retries_only_recoverable_provider_errors() {
        assert!(permission_judgment_error_needs_retry(&sampling_api_error(
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            None,
        )));
        assert!(!permission_judgment_error_needs_retry(&sampling_api_error(
            reqwest::StatusCode::BAD_REQUEST,
            None,
        )));
        assert!(!permission_judgment_error_needs_retry(&sampling_api_error(
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            Some(false),
        )));
    }

    fn api_400(message: &str) -> sampler::SamplingErrorInfo {
        sampler::SamplingErrorInfo {
            kind: sampler::SamplingErrorKind::Api,
            status_code: Some(400),
            message: message.to_string(),
            is_retryable: false,
            retry_after_secs: None,
            model_metadata: None,
            empty_response_context: None,
            doom_loop_triggers: None,
            doom_loop_aborted_at_chunk: None,
            credential: sampling_types::SentCredential::Unknown,
        }
    }

    #[test]
    fn recognizes_image_content_type_rejections() {
        for message in [
            "Failed to deserialize messages[18]: unknown variant `image_url`, expected `text`",
            "input_image is not supported by this model",
            "This text-only model only supports text input",
            "Only text input is supported",
            "This model does not support images",
            "Unsupported image content type; content must be text",
        ] {
            assert!(
                is_image_input_unsupported(&api_400(message), 1),
                "expected rejection match for {message:?}"
            );
        }
    }

    #[test]
    fn rejects_false_positives_and_requires_image_api_400() {
        for message in [
            "invalid image_url: could not decode image",
            "input_image is too large",
            "unsupported image format: image/webp",
            "data URLs in image_url are not supported; provide an HTTPS URL",
            "image_url content type image/svg+xml is unsupported",
            "could not fetch image_url from the provided URL",
            "image dimensions exceed the provider limit",
            "model does not support images with alpha channels",
            "model does not support images with animated frames",
            "model does not support images in CMYK format",
            "model does not support images over 20 MB",
            "model does not accept images encoded as progressive JPEG",
            "model does not support images: animated GIF frames are rejected",
            "model does not support images, except static PNG",
            "invalid image_url.detail: only accepts text values",
            "this model only supports text and image inputs; audio is unavailable",
            "model supports only text or image_url content",
            "this model only accepts text values for image_url.detail",
            "transparent image input is not supported",
            "image content rejected by content policy",
            "invalid_request_error: malformed tool arguments",
        ] {
            assert!(
                !is_image_input_unsupported(&api_400(message), 1),
                "unexpected rejection match for {message:?}"
            );
        }
        assert!(!is_image_input_unsupported(
            &api_400("unknown variant `image_url`, expected `text`"),
            0,
        ));
        let mut wrong_status = api_400("unknown variant `image_url`, expected `text`");
        wrong_status.status_code = Some(413);
        assert!(!is_image_input_unsupported(&wrong_status, 1));
        wrong_status.status_code = Some(400);
        wrong_status.kind = sampler::SamplingErrorKind::Http;
        assert!(!is_image_input_unsupported(&wrong_status, 1));
    }

    #[test]
    fn image_input_identity_includes_canonical_query_route() {
        let backend = sampling_types::ApiBackend::ChatCompletions;
        let left = indexmap::indexmap! {
            "deployment".to_owned() => "vision".to_owned(),
            "api-version".to_owned() => "2026-01-01".to_owned(),
        };
        let reordered = indexmap::indexmap! {
            "api-version".to_owned() => "2026-01-01".to_owned(),
            "deployment".to_owned() => "vision".to_owned(),
        };
        let text = indexmap::indexmap! {
            "deployment".to_owned() => "text".to_owned(),
            "api-version".to_owned() => "2026-01-01".to_owned(),
        };
        let left = model_image_input_key_parts("model", &backend, "https://api.test", &left);
        let reordered =
            model_image_input_key_parts("model", &backend, "https://api.test", &reordered);
        let text = model_image_input_key_parts("model", &backend, "https://api.test", &text);
        assert_eq!(
            left, reordered,
            "query insertion order is not route identity"
        );
        assert_ne!(
            left, text,
            "different routed deployments must not share state"
        );
    }
}
