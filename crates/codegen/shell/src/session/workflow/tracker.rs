use std::time::Instant;

use serde::{Deserialize, Serialize};
use tools::implementations::grow_build::workflow::{WorkflowDefinitionId, WorkflowScope};
use workflow::{PauseKind, PhaseMeta, WorkflowOutcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Active,
    UserPaused,
    BackOffPaused,
    NoProgressPaused,
    InfraPaused,
    Blocked,
    BudgetLimited,
    Interrupted,
    Complete,
    Failed,
    Cancelled,
}

impl WorkflowRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::UserPaused => "user_paused",
            Self::BackOffPaused => "back_off_paused",
            Self::NoProgressPaused => "no_progress_paused",
            Self::InfraPaused => "infra_paused",
            Self::Blocked => "blocked",
            Self::BudgetLimited => "budget_limited",
            Self::Interrupted => "interrupted",
            Self::Complete => "complete",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Interrupted | Self::Complete | Self::Failed | Self::Cancelled
        )
    }

    pub fn is_completion_reportable(self) -> bool {
        self.is_terminal() || self == Self::BudgetLimited
    }

    pub fn is_paused(self) -> bool {
        matches!(
            self,
            Self::UserPaused
                | Self::BackOffPaused
                | Self::NoProgressPaused
                | Self::InfraPaused
                | Self::Blocked
                | Self::BudgetLimited
        )
    }

    pub fn is_resumable(self) -> bool {
        self.is_paused() || self == Self::Failed
    }

    pub(crate) fn to_timeline(self) -> chat_state::WorkflowExecutionStatus {
        match self {
            Self::UserPaused => chat_state::WorkflowExecutionStatus::UserPaused,
            Self::BackOffPaused => chat_state::WorkflowExecutionStatus::BackOffPaused,
            Self::NoProgressPaused => chat_state::WorkflowExecutionStatus::NoProgressPaused,
            Self::InfraPaused => chat_state::WorkflowExecutionStatus::InfraPaused,
            Self::Blocked => chat_state::WorkflowExecutionStatus::Blocked,
            Self::BudgetLimited => chat_state::WorkflowExecutionStatus::BudgetLimited,
            Self::Interrupted => chat_state::WorkflowExecutionStatus::Interrupted,
            Self::Complete => chat_state::WorkflowExecutionStatus::Complete,
            Self::Failed => chat_state::WorkflowExecutionStatus::Failed,
            Self::Cancelled => chat_state::WorkflowExecutionStatus::Cancelled,
            Self::Active => unreachable!("an active workflow cannot emit an execution terminal"),
        }
    }

    pub(crate) fn from_timeline(status: chat_state::WorkflowExecutionStatus) -> Self {
        match status {
            chat_state::WorkflowExecutionStatus::UserPaused => Self::UserPaused,
            chat_state::WorkflowExecutionStatus::BackOffPaused => Self::BackOffPaused,
            chat_state::WorkflowExecutionStatus::NoProgressPaused => Self::NoProgressPaused,
            chat_state::WorkflowExecutionStatus::InfraPaused => Self::InfraPaused,
            chat_state::WorkflowExecutionStatus::Blocked => Self::Blocked,
            chat_state::WorkflowExecutionStatus::BudgetLimited => Self::BudgetLimited,
            chat_state::WorkflowExecutionStatus::Interrupted => Self::Interrupted,
            chat_state::WorkflowExecutionStatus::Complete => Self::Complete,
            chat_state::WorkflowExecutionStatus::Failed => Self::Failed,
            chat_state::WorkflowExecutionStatus::Cancelled => Self::Cancelled,
        }
    }

    fn from_pause(kind: PauseKind) -> Self {
        match kind {
            PauseKind::User => Self::UserPaused,
            PauseKind::BackOff => Self::BackOffPaused,
            PauseKind::NoProgress => Self::NoProgressPaused,
            PauseKind::Verification => Self::Blocked,
            PauseKind::Infra => Self::InfraPaused,
        }
    }
}

const WORKFLOW_PAUSE_MESSAGE_MAX_BYTES: usize = 4 * 1024;

fn capped_pause_message(message: impl Into<String>) -> String {
    let message = message.into();
    tools::util::truncate_str(&message, WORKFLOW_PAUSE_MESSAGE_MAX_BYTES).to_string()
}

fn default_label_for(agents: &[WorkflowAgentRow], phase: Option<&str>) -> String {
    let ordinal = agents
        .iter()
        .filter(|a| a.phase.as_deref() == phase)
        .count()
        + 1;
    match phase {
        Some(p) => {
            let slug: String = p
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '-' })
                .collect::<String>()
                .split('-')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("-");
            format!("{slug}-{ordinal}")
        }
        None => format!("agent-{ordinal}"),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowAgentRow {
    pub agent_id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub state: String,
    pub tokens_used: u64,
    pub duration_ms: u64,
}

pub const WORKFLOW_AGENT_ROWS_MAX: usize = 256;

/// Secret-free executable sampler captured for one catalog model.
///
/// `sampling` is the same durable, credential-free projection owned by the
/// session Timeline. The remaining fields are request semantics that live on
/// `sampler::SamplerConfig` rather than `sampling_types::SamplingConfig`.
/// API keys and live callbacks are deliberately absent; they are attached by
/// [`WorkflowRuntimeRoute::sampler_for`] at child admission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkflowSamplerSnapshot {
    /// Fingerprint of every credential-free sampler field before redaction.
    /// This lets a restored Run reattach a live credential only when the
    /// current catalog still describes the exact captured request contract.
    contract_fingerprint: String,
    transport_key: sampling_types::ModelImageInputKey,
    /// Durable audit projection. Endpoint userinfo/query plus literal header
    /// and query values are removed; their executable values live only in the
    /// process-local runtime lease below.
    sampling: sampling_types::SamplingConfig,
    auth_scheme: sampler::AuthScheme,
    force_http1: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_retries: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    idle_timeout_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    origin_client: Option<sampler::OriginClientInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    compactions_remaining: Option<sampling_types::CompactionsRemaining>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    compaction_at_tokens: Option<sampling_types::CompactionAtTokens>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    doom_loop_recovery: Option<sampling_types::DoomLoopRecoveryPolicy>,
}

impl WorkflowSamplerSnapshot {
    fn from_sampler(config: &sampler::SamplerConfig) -> Result<Self, &'static str> {
        let context_window = std::num::NonZeroU64::new(config.context_window)
            .ok_or("Workflow sampler context window must be non-zero")?;
        if config.temperature.is_some_and(|value| !value.is_finite())
            || config.top_p.is_some_and(|value| !value.is_finite())
        {
            return Err("Workflow sampler floating-point fields must be finite");
        }
        let mut credential_free = config.clone();
        credential_free.api_key = None;
        credential_free.attribution_callback = None;
        credential_free.bearer_resolver = None;
        let encoded = serde_json::to_vec(&credential_free)
            .map_err(|_| "Workflow sampler contract could not be encoded")?;
        if encoded.len() > 1024 * 1024 {
            return Err("Workflow sampler contract exceeds the 1 MiB limit");
        }
        let contract_fingerprint = blake3::hash(&encoded).to_hex().to_string();
        let transport_key = sampling_types::model_image_input_key_from_parts(
            &config.model,
            &config.api_backend,
            &config.base_url,
            &config.query_params,
        );
        Ok(Self {
            contract_fingerprint,
            transport_key,
            sampling: sampling_types::SamplingConfig {
                base_url: workflow_safe_endpoint_label(&config.base_url),
                model: config.model.clone(),
                output_limit: config.output_limit,
                temperature: config.temperature,
                top_p: config.top_p,
                api_backend: config.api_backend.clone(),
                extra_headers: config
                    .extra_headers
                    .keys()
                    .map(|name| (name.clone(), "<runtime>".to_owned()))
                    .collect(),
                query_params: config
                    .query_params
                    .keys()
                    .map(|name| (name.clone(), "<runtime>".to_owned()))
                    .collect(),
                env_http_headers: config.env_http_headers.clone(),
                context_window,
                reasoning_effort: config.reasoning_effort,
                stream_tool_calls: Some(config.stream_tool_calls),
            },
            auth_scheme: config.auth_scheme,
            force_http1: config.force_http1,
            max_retries: config.max_retries,
            idle_timeout_secs: config.idle_timeout_secs,
            origin_client: config.origin_client.clone(),
            compactions_remaining: config.compactions_remaining,
            compaction_at_tokens: config.compaction_at_tokens,
            doom_loop_recovery: config.doom_loop_recovery,
        })
    }

    fn matches(&self, config: &sampler::SamplerConfig) -> bool {
        Self::from_sampler(config).is_ok_and(|candidate| {
            candidate.contract_fingerprint == self.contract_fingerprint
                && candidate.transport_key == self.transport_key
        })
    }

    fn rebuild(&self, runtime: &WorkflowSamplerRuntime) -> sampler::SamplerConfig {
        sampler::SamplerConfig {
            api_key: runtime.api_key.clone(),
            base_url: runtime.base_url.clone(),
            model: self.sampling.model.clone(),
            output_limit: self.sampling.output_limit,
            temperature: self.sampling.temperature,
            top_p: self.sampling.top_p,
            api_backend: self.sampling.api_backend.clone(),
            auth_scheme: self.auth_scheme,
            extra_headers: runtime.extra_headers.clone(),
            query_params: runtime.query_params.clone(),
            env_http_headers: self.sampling.env_http_headers.clone(),
            context_window: self.sampling.context_window.get(),
            force_http1: self.force_http1,
            max_retries: self.max_retries,
            stream_tool_calls: self.sampling.stream_tool_calls.unwrap_or(false),
            idle_timeout_secs: self.idle_timeout_secs,
            reasoning_effort: self.sampling.reasoning_effort,
            origin_client: self.origin_client.clone(),
            attribution_callback: runtime.attribution_callback.clone(),
            bearer_resolver: runtime.bearer_resolver.clone(),
            compactions_remaining: self.compactions_remaining,
            compaction_at_tokens: self.compaction_at_tokens,
            doom_loop_recovery: self.doom_loop_recovery,
        }
    }

    fn transport_key(&self) -> sampling_types::ModelImageInputKey {
        self.transport_key.clone()
    }
}

fn workflow_safe_endpoint_label(base_url: &str) -> String {
    let Ok(url) = url::Url::parse(base_url) else {
        return format!(
            "opaque:blake3:{}",
            blake3::hash(base_url.as_bytes()).to_hex()
        );
    };
    let origin = url.origin().ascii_serialization();
    if origin == "null" {
        format!(
            "opaque:blake3:{}",
            blake3::hash(base_url.as_bytes()).to_hex()
        )
    } else {
        origin
    }
}

/// Credential-bearing fields are process-local and intentionally skipped by
/// Workflow Run serialization. Custom `Debug` prevents accidental key output.
#[derive(Clone, Default)]
struct WorkflowSamplerRuntime {
    api_key: Option<String>,
    base_url: String,
    extra_headers: indexmap::IndexMap<String, String>,
    query_params: indexmap::IndexMap<String, String>,
    attribution_callback: Option<sampler::SharedAttributionCallback>,
    bearer_resolver: Option<sampler::SharedBearerResolver>,
}

impl std::fmt::Debug for WorkflowSamplerRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkflowSamplerRuntime(<redacted>)")
    }
}

impl From<&sampler::SamplerConfig> for WorkflowSamplerRuntime {
    fn from(config: &sampler::SamplerConfig) -> Self {
        Self {
            api_key: config.api_key.clone(),
            base_url: config.base_url.clone(),
            extra_headers: config.extra_headers.clone(),
            query_params: config.query_params.clone(),
            attribution_callback: config.attribution_callback.clone(),
            bearer_resolver: config.bearer_resolver.clone(),
        }
    }
}

/// Immutable model route captured when a Workflow Run is created.
///
/// Workflow Definitions, launch arguments, and this catalog form one
/// execution snapshot. `samplers` is durable and credential-free;
/// `runtime` is the volatile credential attachment for the same entries.
/// A later catalog reload only affects future Runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowRuntimeRoute {
    model_id: String,
    samplers: std::collections::BTreeMap<String, WorkflowSamplerSnapshot>,
    #[serde(skip, default)]
    runtime: std::collections::BTreeMap<String, WorkflowSamplerRuntime>,
}

impl PartialEq for WorkflowRuntimeRoute {
    fn eq(&self, other: &Self) -> bool {
        self.model_id == other.model_id && self.samplers == other.samplers
    }
}

impl WorkflowRuntimeRoute {
    pub(crate) fn capture(
        model_id: impl Into<String>,
        default_sampler: sampler::SamplerConfig,
        models_manager: &crate::agent::models::ModelsManager,
        alpha_test_key: Option<String>,
    ) -> Result<Self, &'static str> {
        let model_id = model_id.into();
        let mut samplers = std::collections::BTreeMap::new();
        let mut runtime = std::collections::BTreeMap::new();
        // A Workflow Definition has the same explicit model authority as an
        // ordinary Task. Snapshot only the canonical Task-selectable catalog;
        // the Host intentionally resolves from this immutable Run route later.
        for (catalog_id, entry) in models_manager.task_selectable_models() {
            let credentials = crate::agent::config::resolve_credentials(&entry);
            let mut config = crate::agent::config::sampling_config_for_model(
                &entry,
                credentials,
                alpha_test_key.clone(),
            );
            config.bearer_resolver = entry
                .effective_auth_provider()
                .map(crate::auth::AuthProviderRef::bearer_resolver);
            samplers.insert(
                catalog_id.clone(),
                WorkflowSamplerSnapshot::from_sampler(&config)?,
            );
            runtime.insert(catalog_id, WorkflowSamplerRuntime::from(&config));
        }
        samplers.insert(
            model_id.clone(),
            WorkflowSamplerSnapshot::from_sampler(&default_sampler)?,
        );
        runtime.insert(
            model_id.clone(),
            WorkflowSamplerRuntime::from(&default_sampler),
        );
        let route = Self {
            model_id,
            samplers,
            runtime,
        };
        route.validate()?;
        Ok(route)
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        model_id: impl Into<String>,
        reasoning_effort: Option<sampling_types::ReasoningEffort>,
        transport_key: sampling_types::ModelImageInputKey,
    ) -> Result<Self, &'static str> {
        let model_id = model_id.into();
        let config = sampler::SamplerConfig {
            model: transport_key.model().to_owned(),
            base_url: "https://workflow.test.invalid/v1".to_owned(),
            api_backend: sampling_types::ApiBackend::Responses,
            context_window: 200_000,
            reasoning_effort,
            ..Default::default()
        };
        let mut snapshot = WorkflowSamplerSnapshot::from_sampler(&config)?;
        snapshot.transport_key = transport_key;
        let route = Self {
            model_id: model_id.clone(),
            samplers: std::collections::BTreeMap::from([(model_id.clone(), snapshot)]),
            runtime: std::collections::BTreeMap::from([(
                model_id,
                WorkflowSamplerRuntime::from(&config),
            )]),
        };
        route.validate()?;
        Ok(route)
    }

    #[cfg(test)]
    pub(crate) fn with_test_model(
        mut self,
        model_id: impl Into<String>,
        reasoning_effort: Option<sampling_types::ReasoningEffort>,
        transport_key: sampling_types::ModelImageInputKey,
    ) -> Result<Self, &'static str> {
        let model_id = model_id.into();
        let config = sampler::SamplerConfig {
            model: transport_key.model().to_owned(),
            base_url: "https://workflow.test.invalid/v1".to_owned(),
            api_backend: sampling_types::ApiBackend::Responses,
            context_window: 200_000,
            reasoning_effort,
            ..Default::default()
        };
        let mut snapshot = WorkflowSamplerSnapshot::from_sampler(&config)?;
        snapshot.transport_key = transport_key;
        self.samplers.insert(model_id.clone(), snapshot);
        self.runtime
            .insert(model_id, WorkflowSamplerRuntime::from(&config));
        self.validate()?;
        Ok(self)
    }

    pub(crate) fn model_id(&self) -> &str {
        &self.model_id
    }

    pub(crate) fn reasoning_effort(&self) -> Option<sampling_types::ReasoningEffort> {
        self.reasoning_effort_for(&self.model_id).flatten()
    }

    pub(crate) fn transport_key(&self) -> sampling_types::ModelImageInputKey {
        self.transport_for(&self.model_id)
            .expect("validated Workflow route contains its default model")
    }

    pub(crate) fn transport_for(
        &self,
        model_id: &str,
    ) -> Option<sampling_types::ModelImageInputKey> {
        self.samplers
            .get(model_id)
            .map(WorkflowSamplerSnapshot::transport_key)
    }

    pub(crate) fn reasoning_effort_for(
        &self,
        model_id: &str,
    ) -> Option<Option<sampling_types::ReasoningEffort>> {
        self.samplers
            .get(model_id)
            .map(|snapshot| snapshot.sampling.reasoning_effort)
    }

    /// Resolve one Run-owned model without consulting live catalog routing.
    /// A still-matching catalog entry may refresh only its credential-bearing
    /// runtime fields. If the entry changed or disappeared, the original
    /// process-local credential attachment remains authoritative.
    pub(crate) fn sampler_for(
        &self,
        model_id: &str,
        models_manager: &crate::agent::models::ModelsManager,
        alpha_test_key: Option<String>,
    ) -> Result<sampler::SamplerConfig, String> {
        let snapshot = self.samplers.get(model_id).ok_or_else(|| {
            format!("model '{model_id}' was not present in the Workflow Run sampler snapshot")
        })?;
        if let Some(entry) =
            crate::agent::config::find_model_by_catalog_id(&models_manager.models(), model_id)
        {
            let credentials = crate::agent::config::resolve_credentials(entry);
            let mut candidate =
                crate::agent::config::sampling_config_for_model(entry, credentials, alpha_test_key);
            candidate.bearer_resolver = entry
                .effective_auth_provider()
                .map(crate::auth::AuthProviderRef::bearer_resolver);
            let refreshed = snapshot.rebuild(&WorkflowSamplerRuntime::from(&candidate));
            if snapshot.matches(&refreshed) {
                return Ok(refreshed);
            }
        }
        self.runtime
            .get(model_id)
            .map(|runtime| snapshot.rebuild(runtime))
            .ok_or_else(|| {
                format!(
                    "Workflow Run sampler '{model_id}' cannot be restored because its credential source is unavailable or its catalog contract changed"
                )
            })
    }

    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        let durable_bytes = self.samplers.values().try_fold(0usize, |total, snapshot| {
            serde_json::to_vec(snapshot)
                .ok()
                .and_then(|bytes| total.checked_add(bytes.len()))
        });
        if self.model_id.trim().is_empty()
            || self.samplers.is_empty()
            || self.samplers.len() > 512
            // A route is only one field of the 512 KiB Workflow manifest. The
            // complete initial manifest is checked again before Timeline spawn;
            // this shared ceiling only rejects routes that can never fit.
            || durable_bytes.is_none_or(|bytes| {
                bytes > crate::session::workflow::store::MAX_WORKFLOW_MANIFEST_BYTES as usize
            })
            || !self.samplers.contains_key(&self.model_id)
            || self.samplers.iter().any(|(model_id, snapshot)| {
                model_id.trim().is_empty()
                    || snapshot.sampling.model.trim().is_empty()
                    || !snapshot.transport_key.is_valid()
                    || snapshot.contract_fingerprint.len() != 64
            })
        {
            return Err("Workflow runtime route is incomplete");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowRunState {
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_id: Option<WorkflowDefinitionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_scope: Option<WorkflowScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_hash: Option<String>,
    pub save_prompt: bool,
    pub revision: u64,
    /// Monotonic identity of the current execution attempt. This is part of
    /// the durable run state because Timeline resume boundaries validate it
    /// across process restarts.
    pub execution_epoch: u64,
    pub runtime_route: WorkflowRuntimeRoute,
    pub name: String,
    pub objective: String,
    pub status: WorkflowRunStatus,
    pub phases: Vec<PhaseMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_budget: Option<u64>,
    pub max_concurrency: u16,
    pub agents_used: u64,
    pub agent_usage_incomplete: bool,
    pub elapsed_ms_floor: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub journal_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<WorkflowAgentRow>,
}

fn default_max_concurrency() -> u16 {
    tools::implementations::grow_build::workflow::WorkflowToolInput::DEFAULT_MAX_CONCURRENCY
}

impl WorkflowRunState {
    /// Validate the durable projection after its lifecycle has been reconciled
    /// against Timeline at the per-Run restore boundary.
    pub(crate) fn validate_restored_projection(&self) -> Result<(), &'static str> {
        self.runtime_route.validate()?;
        if self.status == WorkflowRunStatus::Active {
            return Err(
                "Workflow restore received an active manifest instead of a Timeline-reconciled projection",
            );
        }
        Ok(())
    }

    fn advance_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    /// Reconcile the mutable manifest projection to the canonical lifecycle
    /// reconstructed from Timeline. Returns whether the manifest changed.
    pub(crate) fn reconcile_lifecycle_after_restore(
        &mut self,
        execution_epoch: u64,
        status: WorkflowRunStatus,
        message: Option<String>,
        execution_was_open: bool,
    ) -> bool {
        let message = message.map(capped_pause_message);
        if self.execution_epoch == execution_epoch
            && self.status == status
            && self.pause_message == message
            && !execution_was_open
        {
            return false;
        }
        self.execution_epoch = execution_epoch;
        self.status = status;
        self.pause_message = message.clone();
        if status != WorkflowRunStatus::Complete {
            self.result_summary = None;
        }
        if execution_was_open {
            self.agent_usage_incomplete = true;
        }
        self.advance_revision();
        true
    }
}

#[derive(Debug, Default)]
pub struct WorkflowTracker {
    runs: Vec<TrackedRun>,
    status_reported_revisions: std::collections::HashMap<String, u64>,
}

#[derive(Debug)]
struct TrackedRun {
    state: WorkflowRunState,
    active_since: Option<Instant>,
}

impl WorkflowTracker {
    pub fn start_run(
        &mut self,
        run_id: String,
        name: String,
        objective: String,
        phases: Vec<PhaseMeta>,
        agent_budget: Option<u64>,
        journal_path: Option<String>,
        runtime_route: WorkflowRuntimeRoute,
    ) -> WorkflowRunState {
        runtime_route
            .validate()
            .expect("new Workflow Runs require a validated runtime route");
        let name = {
            let taken = |candidate: &str| self.runs.iter().any(|r| r.state.name == candidate);
            if !taken(&name) {
                name
            } else {
                let mut n = 2;
                loop {
                    let candidate = format!("{name}-{n}");
                    if !taken(&candidate) {
                        break candidate;
                    }
                    n += 1;
                }
            }
        };
        let mut state = WorkflowRunState {
            run_id,
            definition_id: None,
            definition_scope: None,
            definition_hash: None,
            save_prompt: false,
            revision: 0,
            execution_epoch: 0,
            runtime_route,
            name,
            objective,
            status: WorkflowRunStatus::Active,
            phases,
            current_phase: None,
            agent_budget,
            max_concurrency: default_max_concurrency(),
            agents_used: 0,
            agent_usage_incomplete: false,
            elapsed_ms_floor: 0,
            pause_message: None,
            journal_path,
            result_summary: None,
            agents: Vec::new(),
        };
        state.advance_revision();
        self.runs.push(TrackedRun {
            state: state.clone(),
            active_since: Some(Instant::now()),
        });
        state
    }

    pub fn set_definition_provenance(
        &mut self,
        run_id: &str,
        definition_id: WorkflowDefinitionId,
        definition_scope: WorkflowScope,
        definition_hash: String,
    ) -> Option<WorkflowRunState> {
        let run = self
            .runs
            .iter_mut()
            .find(|run| run.state.run_id == run_id)?;
        if run.state.definition_id.as_ref() != Some(&definition_id)
            || run.state.definition_scope != Some(definition_scope)
            || run.state.definition_hash.as_deref() != Some(definition_hash.as_str())
        {
            run.state.definition_id = Some(definition_id);
            run.state.definition_scope = Some(definition_scope);
            run.state.definition_hash = Some(definition_hash);
            run.state.advance_revision();
        }
        Some(run.state.clone())
    }

    pub fn set_save_prompt(&mut self, run_id: &str, save_prompt: bool) -> Option<WorkflowRunState> {
        let run = self
            .runs
            .iter_mut()
            .find(|run| run.state.run_id == run_id)?;
        if run.state.save_prompt != save_prompt {
            run.state.save_prompt = save_prompt;
            run.state.advance_revision();
        }
        Some(run.state.clone())
    }

    pub fn resume_run(
        &mut self,
        run_id: &str,
        new_agent_budget: Option<u64>,
    ) -> Option<WorkflowRunState> {
        let run = self.run_mut(run_id)?;
        if !run.state.status.is_resumable() {
            return None;
        }
        let candidate_budget = match new_agent_budget {
            Some(raised) => Some(raised.max(run.state.agent_budget.unwrap_or(0))),
            None => run.state.agent_budget,
        };
        let raised = new_agent_budget.is_some_and(|b| b > run.state.agent_budget.unwrap_or(0));
        if run.state.status == WorkflowRunStatus::BudgetLimited
            && (!raised || candidate_budget.is_some_and(|limit| run.state.agents_used >= limit))
        {
            return None;
        }
        if let Some(budget) = candidate_budget {
            run.state.agent_budget = Some(budget);
        }
        run.state.status = WorkflowRunStatus::Active;
        run.state.pause_message = None;
        run.state.result_summary = None;
        for agent in &mut run.state.agents {
            if agent.state == "running" {
                agent.state = "cancelled".to_string();
            }
        }
        run.state.advance_revision();
        run.active_since = Some(Instant::now());
        run.state.execution_epoch = run.state.execution_epoch.saturating_add(1);
        let state = run.state.clone();
        Some(state)
    }

    pub fn set_max_concurrency(
        &mut self,
        run_id: &str,
        max_concurrency: u16,
    ) -> Option<WorkflowRunState> {
        let run = self.run_mut(run_id)?;
        run.state.max_concurrency = max_concurrency;
        run.state.advance_revision();
        Some(run.state.clone())
    }

    pub fn set_phase(&mut self, run_id: &str, title: &str) -> Option<WorkflowRunState> {
        let run = self.run_mut(run_id)?;
        if run.state.current_phase.as_deref() != Some(title) {
            run.state.current_phase = Some(title.to_string());
            run.state.advance_revision();
        }
        Some(run.state.clone())
    }

    #[cfg(test)]
    pub(crate) fn remaining_agents(&self, run_id: &str) -> Option<Option<u64>> {
        let run = self.runs.iter().find(|run| run.state.run_id == run_id)?;
        Some(
            run.state
                .agent_budget
                .map(|limit| limit.saturating_sub(run.state.agents_used)),
        )
    }

    pub(crate) fn reserve_agents(
        &mut self,
        run_id: &str,
        count: u64,
    ) -> Result<WorkflowRunState, (u64, u64)> {
        let Some(run) = self.run_mut(run_id) else {
            return Err((count, 0));
        };
        let requested = run.state.agents_used.saturating_add(count);
        if let Some(limit) = run.state.agent_budget
            && requested > limit
        {
            return Err((requested, limit));
        }
        run.state.agents_used = requested;
        run.state.advance_revision();
        Ok(run.state.clone())
    }

    pub(crate) fn release_agents(&mut self, run_id: &str, count: u64) -> Option<WorkflowRunState> {
        let run = self.run_mut(run_id)?;
        run.state.agents_used = run.state.agents_used.saturating_sub(count);
        run.state.advance_revision();
        Some(run.state.clone())
    }

    pub(crate) fn execution_epoch(&self, run_id: &str) -> Option<u64> {
        self.runs
            .iter()
            .find(|run| run.state.run_id == run_id)
            .map(|run| run.state.execution_epoch)
    }

    pub(crate) fn reconcile_agents_used(
        &mut self,
        run_id: &str,
        used: u64,
    ) -> Option<WorkflowRunState> {
        let run = self.run_mut(run_id)?;
        if run.state.agents_used != used {
            run.state.agents_used = used;
            run.state.advance_revision();
        }
        Some(run.state.clone())
    }

    pub fn agent_started(&mut self, run_id: &str, mut row: WorkflowAgentRow) -> String {
        let Some(run) = self.run_mut(run_id) else {
            return if row.label.is_empty() {
                "agent".to_string()
            } else {
                row.label
            };
        };
        if row.phase.is_none() {
            row.phase = run.state.current_phase.clone();
        }
        if row.label.is_empty() {
            row.label = default_label_for(&run.state.agents, row.phase.as_deref());
        }
        let label = row.label.clone();
        run.state.advance_revision();
        run.state.agents.push(row);
        if run.state.agents.len() > WORKFLOW_AGENT_ROWS_MAX
            && let Some(pos) = run.state.agents.iter().position(|a| a.state != "running")
        {
            run.state.agents.remove(pos);
        }
        label
    }

    pub fn agent_running(&mut self, run_id: &str, agent_id: &str) -> Option<WorkflowRunState> {
        let run = self.run_mut(run_id)?;
        let agent = run
            .state
            .agents
            .iter_mut()
            .find(|agent| agent.agent_id == agent_id)?;
        agent.state = "running".to_string();
        run.state.advance_revision();
        Some(run.state.clone())
    }

    /// Point a roster row at a fresh child session id. Contract retries
    /// spawn a new child session per attempt; the row must follow so live
    /// progress lookups and transcript clicks resolve to the current child.
    pub fn rebind_agent_id(&mut self, run_id: &str, agent_id: &str, new_agent_id: &str) {
        let Some(run) = self.run_mut(run_id) else {
            return;
        };
        if let Some(row) = run.state.agents.iter_mut().find(|a| a.agent_id == agent_id) {
            row.agent_id = new_agent_id.to_string();
            run.state.advance_revision();
        }
    }

    pub fn agent_finished(
        &mut self,
        run_id: &str,
        agent_id: &str,
        state: &str,
        tokens_used: u64,
        duration_ms: u64,
    ) {
        let Some(run) = self.run_mut(run_id) else {
            return;
        };
        if let Some(row) = run.state.agents.iter_mut().find(|a| a.agent_id == agent_id) {
            row.state = state.to_string();
            row.tokens_used = tokens_used;
            row.duration_ms = duration_ms;
            run.state.advance_revision();
        }
    }

    pub fn pause_user(
        &mut self,
        run_id: &str,
        message: Option<String>,
    ) -> Option<WorkflowRunState> {
        let run = self.run_mut(run_id)?;
        if run.state.status != WorkflowRunStatus::Active {
            return Some(run.state.clone());
        }
        run.fold_elapsed();
        run.state.status = WorkflowRunStatus::UserPaused;
        run.state.pause_message = message.map(capped_pause_message);
        run.state.advance_revision();
        Some(run.state.clone())
    }

    pub fn interrupt(
        &mut self,
        run_id: &str,
        message: impl Into<String>,
    ) -> Option<WorkflowRunState> {
        let run = self.run_mut(run_id)?;
        run.fold_elapsed();
        let message = capped_pause_message(message);
        run.state.status = WorkflowRunStatus::Interrupted;
        run.state.pause_message = Some(message.clone());
        run.state.result_summary = None;
        run.state.advance_revision();
        Some(run.state.clone())
    }

    pub fn close_cancelled(&mut self, run_id: &str) -> Option<WorkflowRunState> {
        let run = self.run_mut(run_id)?;
        if !run.state.status.is_resumable() {
            return None;
        }
        run.fold_elapsed();
        run.state.status = WorkflowRunStatus::Cancelled;
        run.state.pause_message = Some("cancelled while no execution was active".into());
        run.state.result_summary = None;
        run.state.advance_revision();
        Some(run.state.clone())
    }

    pub fn apply_outcome(
        &mut self,
        run_id: &str,
        outcome: &WorkflowOutcome,
    ) -> Option<WorkflowRunState> {
        let run = self.run_mut(run_id)?;
        run.fold_elapsed();
        if matches!(
            run.state.status,
            WorkflowRunStatus::Interrupted
                | WorkflowRunStatus::Failed
                | WorkflowRunStatus::Cancelled
        ) {
            return Some(run.state.clone());
        }
        match outcome {
            WorkflowOutcome::Completed { result } => {
                run.state.status = WorkflowRunStatus::Complete;
                run.state.result_summary = Some(summarize_result(result));
                run.state.advance_revision();
            }
            WorkflowOutcome::Paused { kind, message } => {
                run.state.status = WorkflowRunStatus::from_pause(*kind);
                run.state.pause_message = Some(capped_pause_message(message.clone()));
                run.state.advance_revision();
            }
            WorkflowOutcome::BudgetExceeded { message } => {
                run.state.status = WorkflowRunStatus::BudgetLimited;
                let hint = if run.state.agents_used >= workflow::MAX_AGENT_BUDGET {
                    "finished work is kept, but this run reached the maximum agent budget and \
                     cannot be resumed; start a new run"
                } else {
                    "finished work is kept; resume the run with a higher absolute agent budget \
                     to continue"
                };
                run.state.pause_message = Some(capped_pause_message(format!("{message} — {hint}")));
                run.state.advance_revision();
            }
            WorkflowOutcome::Cancelled => {
                run.state.status = WorkflowRunStatus::Cancelled;
                run.state.advance_revision();
            }
            WorkflowOutcome::Failed { error } => {
                run.state.status = WorkflowRunStatus::Failed;
                let error = capped_pause_message(error.clone());
                run.state.pause_message = Some(error.clone());
                run.state.advance_revision();
            }
        }
        Some(run.state.clone())
    }

    pub fn clear_run(&mut self, run_id: &str) -> Option<WorkflowRunState> {
        let idx = self.runs.iter().position(|r| r.state.run_id == run_id)?;
        let mut removed = self.runs.remove(idx);
        removed.fold_elapsed();
        Some(removed.state)
    }

    pub fn get(&self, run_id: &str) -> Option<WorkflowRunState> {
        self.runs
            .iter()
            .find(|r| r.state.run_id == run_id)
            .map(|r| r.state.clone())
    }

    pub fn list(&self) -> Vec<WorkflowRunState> {
        self.runs.iter().map(|r| r.state.clone()).collect()
    }

    pub(crate) fn has_active_run(&self) -> bool {
        self.runs
            .iter()
            .any(|run| run.state.status == WorkflowRunStatus::Active)
    }

    pub(crate) fn has_runs(&self) -> bool {
        !self.runs.is_empty()
    }

    pub fn elapsed_ms(&self, run_id: &str) -> u64 {
        self.runs
            .iter()
            .find(|r| r.state.run_id == run_id)
            .map(TrackedRun::live_elapsed_ms)
            .unwrap_or(0)
    }

    pub fn from_snapshot(snapshots: Vec<WorkflowRunState>) -> Result<Self, &'static str> {
        for state in &snapshots {
            state.validate_restored_projection()?;
        }
        let runs = snapshots
            .into_iter()
            .map(|mut state| {
                let mut cancelled_ghost = false;
                for agent in &mut state.agents {
                    if agent.state == "running" {
                        agent.state = "cancelled".to_string();
                        cancelled_ghost = true;
                    }
                }
                if cancelled_ghost {
                    state.advance_revision();
                }
                TrackedRun {
                    state,
                    active_since: None,
                }
            })
            .collect::<Vec<TrackedRun>>();
        Ok(Self {
            runs,
            status_reported_revisions: std::collections::HashMap::new(),
        })
    }

    pub fn snapshot(&self) -> Vec<WorkflowRunState> {
        self.list()
    }

    pub fn take_status_report(&mut self) -> Vec<WorkflowRunState> {
        let live: Vec<&TrackedRun> = self
            .runs
            .iter()
            .filter(|r| !r.state.status.is_completion_reportable())
            .collect();
        let moved = live.iter().any(|r| {
            self.status_reported_revisions.get(&r.state.run_id) != Some(&r.state.revision)
        });
        if !moved {
            return Vec::new();
        }
        let report: Vec<WorkflowRunState> = live
            .iter()
            .map(|run| {
                let mut state = run.state.clone();
                state.elapsed_ms_floor = run.live_elapsed_ms();
                state
            })
            .collect();
        let current_ids: std::collections::HashSet<&str> =
            self.runs.iter().map(|r| r.state.run_id.as_str()).collect();
        self.status_reported_revisions
            .retain(|id, _| current_ids.contains(id.as_str()));
        for state in &report {
            self.status_reported_revisions
                .insert(state.run_id.clone(), state.revision);
        }
        report
    }

    fn run_mut(&mut self, run_id: &str) -> Option<&mut TrackedRun> {
        self.runs.iter_mut().find(|r| r.state.run_id == run_id)
    }
}

impl TrackedRun {
    fn live_elapsed_ms(&self) -> u64 {
        self.state.elapsed_ms_floor.saturating_add(
            self.active_since
                .map(|since| since.elapsed().as_millis() as u64)
                .unwrap_or(0),
        )
    }

    fn fold_elapsed(&mut self) {
        if self.active_since.is_some() {
            self.state.elapsed_ms_floor = self.live_elapsed_ms();
            self.active_since = None;
        }
    }
}

fn summarize_result(result: &serde_json::Value) -> String {
    const MAX: usize = 16 * 1024;
    let text = match result {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "done".to_string(),
        serde_json::Value::Object(map) => match (
            map.get("report").and_then(|r| r.as_str()),
            map.get("path").and_then(|p| p.as_str()),
        ) {
            (Some(report), Some(path)) => format!("{report}\n\n_Full report: {path}_"),
            (Some(report), None) => report.to_string(),
            _ => serde_json::Value::Object(map.clone()).to_string(),
        },
        other => other.to_string(),
    };
    if text.len() > MAX {
        let mut end = MAX;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &text[..end])
    } else {
        text
    }
}

#[cfg(test)]
pub(crate) fn test_runtime_route() -> WorkflowRuntimeRoute {
    WorkflowRuntimeRoute::for_test(
        "test-model",
        None,
        sampling_types::ModelImageInputKey::new("test-model", "responses", "test-endpoint"),
    )
    .expect("valid Workflow test route")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager_with_model(
        catalog_id: &str,
        entry: crate::agent::config::ModelEntry,
    ) -> crate::agent::models::ModelsManager {
        crate::agent::models::ModelsManager::new(
            indexmap::IndexMap::from([(catalog_id.to_owned(), entry)]),
            agent_client_protocol::ModelId::new(catalog_id),
            crate::agent::config::Config::default(),
        )
    }

    fn workflow_model_entry(
        wire_model: &str,
        temperature: f32,
    ) -> crate::agent::config::ModelEntry {
        let mut entry = crate::agent::config::ModelEntry::baseline(wire_model);
        entry.info.base_url = "https://api.example.test/v1".to_owned();
        entry.info.temperature = Some(temperature);
        entry.info.api_backend = sampling_types::ApiBackend::Responses;
        entry
    }

    #[test]
    fn runtime_route_serialization_redacts_credentials_and_literal_transport_values() {
        let manager = crate::agent::models::ModelsManager::default();
        let config = sampler::SamplerConfig {
            api_key: Some("super-secret-api-key".to_owned()),
            base_url: "https://user:password@example.test/v1?token=url-secret".to_owned(),
            model: "wire-model".to_owned(),
            api_backend: sampling_types::ApiBackend::Responses,
            extra_headers: indexmap::IndexMap::from([(
                "Authorization".to_owned(),
                "Bearer header-secret".to_owned(),
            )]),
            query_params: indexmap::IndexMap::from([(
                "api_key".to_owned(),
                "query-secret".to_owned(),
            )]),
            context_window: 200_000,
            ..Default::default()
        };
        let route = WorkflowRuntimeRoute::capture("catalog/model", config, &manager, None).unwrap();
        let encoded = serde_json::to_string(&route).unwrap();
        for secret in [
            "super-secret-api-key",
            "password",
            "url-secret",
            "header-secret",
            "query-secret",
        ] {
            assert!(!encoded.contains(secret), "serialized secret: {secret}");
        }

        let restored: WorkflowRuntimeRoute = serde_json::from_str(&encoded).unwrap();
        assert!(
            restored
                .sampler_for("catalog/model", &manager, None)
                .unwrap_err()
                .contains("credential source is unavailable")
        );
        let live = route.sampler_for("catalog/model", &manager, None).unwrap();
        assert_eq!(live.api_key.as_deref(), Some("super-secret-api-key"));
        assert_eq!(live.extra_headers["Authorization"], "Bearer header-secret");
        assert_eq!(live.query_params["api_key"], "query-secret");
    }

    #[test]
    fn existing_run_keeps_sampler_when_catalog_changes_or_removes_model() {
        let original_entry = workflow_model_entry("wire-model", 0.2);
        let original_manager = manager_with_model("catalog/model", original_entry.clone());
        let original = crate::agent::config::sampling_config_for_model(
            &original_entry,
            crate::agent::config::resolve_credentials(&original_entry),
            None,
        );
        let route =
            WorkflowRuntimeRoute::capture("catalog/model", original, &original_manager, None)
                .unwrap();

        let changed_manager =
            manager_with_model("catalog/model", workflow_model_entry("wire-model", 0.9));
        assert_eq!(
            route
                .sampler_for("catalog/model", &changed_manager, None)
                .unwrap()
                .temperature,
            Some(0.2)
        );
        assert_eq!(
            route
                .sampler_for(
                    "catalog/model",
                    &crate::agent::models::ModelsManager::default(),
                    None,
                )
                .unwrap()
                .temperature,
            Some(0.2)
        );
    }

    #[test]
    fn runtime_route_excludes_models_rejected_by_task_selection() {
        let allowed = workflow_model_entry("wire-allowed", 0.2);
        let mut denied = workflow_model_entry("wire-denied", 0.2);
        denied.info.user_selectable = false;
        let manager = crate::agent::models::ModelsManager::new(
            indexmap::IndexMap::from([
                ("catalog/allowed".to_owned(), allowed.clone()),
                ("catalog/denied".to_owned(), denied),
            ]),
            agent_client_protocol::ModelId::new("catalog/allowed"),
            crate::agent::config::Config::default(),
        );
        let sampler = crate::agent::config::sampling_config_for_model(
            &allowed,
            crate::agent::config::resolve_credentials(&allowed),
            None,
        );
        let route =
            WorkflowRuntimeRoute::capture("catalog/allowed", sampler, &manager, None).unwrap();

        assert!(route.sampler_for("catalog/allowed", &manager, None).is_ok());
        assert!(
            route
                .sampler_for("catalog/denied", &manager, None)
                .unwrap_err()
                .contains("not present")
        );
    }

    #[test]
    fn restored_route_rehydrates_only_an_exact_catalog_contract() {
        let entry = workflow_model_entry("wire-model", 0.2);
        let manager = manager_with_model("catalog/model", entry.clone());
        let sampler = crate::agent::config::sampling_config_for_model(
            &entry,
            crate::agent::config::resolve_credentials(&entry),
            None,
        );
        let route =
            WorkflowRuntimeRoute::capture("catalog/model", sampler, &manager, None).unwrap();
        let restored: WorkflowRuntimeRoute =
            serde_json::from_str(&serde_json::to_string(&route).unwrap()).unwrap();
        assert_eq!(
            restored
                .sampler_for("catalog/model", &manager, None)
                .unwrap()
                .temperature,
            Some(0.2)
        );

        let changed = manager_with_model("catalog/model", workflow_model_entry("wire-model", 0.9));
        assert_eq!(
            restored
                .sampler_for("catalog/model", &changed, None)
                .unwrap()
                .temperature,
            Some(0.2),
            "durable sampler fields come from the Run snapshot"
        );

        let mut sensitive_change = workflow_model_entry("wire-model", 0.2);
        sensitive_change.info.extra_headers.insert(
            "Authorization".to_owned(),
            "Bearer changed-secret".to_owned(),
        );
        let changed = manager_with_model("catalog/model", sensitive_change);
        assert!(
            restored
                .sampler_for("catalog/model", &changed, None)
                .is_err()
        );
    }

    fn tracker_with_run() -> (WorkflowTracker, String) {
        let mut t = WorkflowTracker::default();
        let state = t.start_run(
            "wf_1".into(),
            "goal".into(),
            "ship it".into(),
            vec![],
            Some(1000),
            None,
            test_runtime_route(),
        );
        assert_eq!(state.status, WorkflowRunStatus::Active);
        (t, "wf_1".into())
    }

    #[test]
    fn outcome_transitions() {
        let (mut t, id) = tracker_with_run();
        let s = t
            .apply_outcome(
                &id,
                &WorkflowOutcome::Paused {
                    kind: PauseKind::BackOff,
                    message: "cap".into(),
                },
            )
            .unwrap();
        assert_eq!(s.status, WorkflowRunStatus::BackOffPaused);
        assert_eq!(s.pause_message.as_deref(), Some("cap"));

        let s = t.resume_run(&id, None).unwrap();
        assert_eq!(s.status, WorkflowRunStatus::Active);
        assert!(s.pause_message.is_none());

        let s = t
            .apply_outcome(
                &id,
                &WorkflowOutcome::Completed {
                    result: serde_json::json!("shipped"),
                },
            )
            .unwrap();
        assert_eq!(s.status, WorkflowRunStatus::Complete);
        assert_eq!(s.result_summary.as_deref(), Some("shipped"));
    }

    #[test]
    fn same_definition_keeps_independent_runs_and_unique_handles() {
        let mut tracker = WorkflowTracker::default();
        let first = tracker.start_run(
            "wf_1".into(),
            "review".into(),
            "first".into(),
            vec![],
            Some(16),
            None,
            test_runtime_route(),
        );
        let second = tracker.start_run(
            "wf_2".into(),
            "review".into(),
            "second".into(),
            vec![],
            Some(16),
            None,
            test_runtime_route(),
        );
        assert_eq!(first.name, "review");
        assert_eq!(second.name, "review-2");
        tracker
            .apply_outcome(
                "wf_1",
                &WorkflowOutcome::Completed {
                    result: serde_json::json!("done"),
                },
            )
            .unwrap();
        assert_eq!(
            tracker.get("wf_1").unwrap().status,
            WorkflowRunStatus::Complete
        );
        assert_eq!(
            tracker.get("wf_2").unwrap().status,
            WorkflowRunStatus::Active
        );
    }

    #[test]
    fn user_definition_is_a_normal_workflow_run() {
        let (mut tracker, run_id) = tracker_with_run();
        assert!(tracker.has_active_run());
        assert!(tracker.has_runs());
        tracker
            .set_definition_provenance(
                &run_id,
                WorkflowDefinitionId::new("user:deep-research"),
                WorkflowScope::User,
                "hash".into(),
            )
            .unwrap();
        assert!(tracker.has_active_run());
        assert!(tracker.has_runs());
        let report = tracker.take_status_report();
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].definition_scope, Some(WorkflowScope::User));

        tracker.pause_user(&run_id, None).unwrap();
        assert!(!tracker.has_active_run());
        assert!(tracker.has_runs());
        let report = tracker.take_status_report();
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].status, WorkflowRunStatus::UserPaused);
    }

    #[test]
    fn rebind_agent_id_points_row_at_retry_child_and_bumps_revision() {
        let (mut t, id) = tracker_with_run();
        t.agent_started(
            &id,
            WorkflowAgentRow {
                agent_id: "child-attempt-1".into(),
                label: "worker".into(),
                phase: None,
                model: None,
                state: "running".into(),
                tokens_used: 0,
                duration_ms: 0,
            },
        );
        let before = t.get(&id).unwrap().revision;
        t.rebind_agent_id(&id, "child-attempt-1", "child-attempt-2");
        let run = t.get(&id).unwrap();
        assert_eq!(run.agents.len(), 1);
        assert_eq!(run.agents[0].agent_id, "child-attempt-2");
        assert_eq!(run.agents[0].label, "worker");
        assert_eq!(run.agents[0].state, "running");
        assert!(run.revision > before);

        t.agent_finished(&id, "child-attempt-2", "done", 42, 1_000);
        assert_eq!(t.get(&id).unwrap().agents[0].state, "done");
    }

    #[test]
    fn snapshot_restore_rejects_active_without_timeline_reconciliation() {
        let (t, _) = tracker_with_run();
        assert!(WorkflowTracker::from_snapshot(t.snapshot()).is_err());
    }

    #[test]
    fn snapshot_restore_cancels_ghost_agent_rows() {
        let (mut t, id) = tracker_with_run();
        t.agent_started(
            &id,
            WorkflowAgentRow {
                agent_id: "child".into(),
                label: "worker".into(),
                phase: None,
                model: None,
                state: "running".into(),
                tokens_used: 0,
                duration_ms: 0,
            },
        );
        t.interrupt(&id, "process_interrupted");
        let restored = WorkflowTracker::from_snapshot(t.snapshot()).unwrap();
        let run = restored.get(&id).unwrap();
        assert_eq!(run.status, WorkflowRunStatus::Interrupted);
        assert_eq!(run.agents[0].state, "cancelled");
    }

    #[test]
    fn resume_rejects_nonresumable_states() {
        let (mut t, id) = tracker_with_run();
        t.interrupt(&id, "lost executor").unwrap();
        assert!(t.resume_run(&id, None).is_none());
        assert_eq!(t.get(&id).unwrap().status, WorkflowRunStatus::Interrupted);

        let (mut t, id) = tracker_with_run();
        t.apply_outcome(&id, &WorkflowOutcome::Cancelled);
        assert!(t.resume_run(&id, None).is_none());
        assert_eq!(t.get(&id).unwrap().status, WorkflowRunStatus::Cancelled);

        let (mut t, id) = tracker_with_run();
        t.apply_outcome(
            &id,
            &WorkflowOutcome::Completed {
                result: serde_json::json!("done"),
            },
        );
        assert!(t.resume_run(&id, None).is_none());
        assert_eq!(t.get(&id).unwrap().status, WorkflowRunStatus::Complete);
    }

    #[test]
    fn failed_run_resumes_to_active_bumps_epoch_and_cancels_ghost_agents() {
        let (mut t, id) = tracker_with_run();
        t.agent_started(
            &id,
            WorkflowAgentRow {
                agent_id: "child".into(),
                label: "worker".into(),
                phase: None,
                model: None,
                state: "running".into(),
                tokens_used: 0,
                duration_ms: 0,
            },
        );
        t.apply_outcome(
            &id,
            &WorkflowOutcome::Failed {
                error: "scratch byte quota exceeded".into(),
            },
        );
        let failed = t.get(&id).unwrap();
        assert_eq!(failed.status, WorkflowRunStatus::Failed);
        assert!(failed.status.is_resumable());
        assert!(failed.status.is_terminal());
        assert!(!failed.status.is_paused());
        assert_eq!(t.execution_epoch(&id), Some(0));

        let resumed = t.resume_run(&id, None).unwrap();
        assert_eq!(resumed.status, WorkflowRunStatus::Active);
        assert!(resumed.pause_message.is_none());
        assert_eq!(
            resumed.agents[0].state, "cancelled",
            "ghost running agent rows must be cancelled on resume"
        );
        assert_eq!(t.execution_epoch(&id), Some(1));
    }

    #[test]
    fn apply_outcome_does_not_demote_interrupted_to_complete() {
        let (mut t, id) = tracker_with_run();
        t.interrupt(&id, "settlement persist failed");
        assert_eq!(t.get(&id).unwrap().status, WorkflowRunStatus::Interrupted);
        t.apply_outcome(
            &id,
            &WorkflowOutcome::Completed {
                result: serde_json::json!("done"),
            },
        );
        let run = t.get(&id).unwrap();
        assert_eq!(run.status, WorkflowRunStatus::Interrupted);
    }

    #[test]
    fn apply_outcome_does_not_demote_interrupted_to_cancelled() {
        let (mut t, id) = tracker_with_run();
        t.interrupt(&id, "settlement persist failed");
        t.apply_outcome(&id, &WorkflowOutcome::Cancelled);
        assert_eq!(t.get(&id).unwrap().status, WorkflowRunStatus::Interrupted);
    }

    #[test]
    fn apply_outcome_does_not_demote_cancelled_to_complete() {
        let (mut t, id) = tracker_with_run();
        t.apply_outcome(&id, &WorkflowOutcome::Cancelled);
        assert_eq!(t.get(&id).unwrap().status, WorkflowRunStatus::Cancelled);
        t.apply_outcome(
            &id,
            &WorkflowOutcome::Completed {
                result: serde_json::json!("done"),
            },
        );
        let run = t.get(&id).unwrap();
        assert_eq!(run.status, WorkflowRunStatus::Cancelled);
    }

    #[test]
    fn apply_outcome_does_not_demote_cancelled_to_budget_exceeded() {
        let (mut t, id) = tracker_with_run();
        t.apply_outcome(&id, &WorkflowOutcome::Cancelled);
        t.apply_outcome(
            &id,
            &WorkflowOutcome::BudgetExceeded {
                message: "spent".into(),
            },
        );
        let run = t.get(&id).unwrap();
        assert_eq!(run.status, WorkflowRunStatus::Cancelled);
    }

    #[test]
    fn release_agents_returns_used_to_prior_saturating() {
        let (mut t, id) = tracker_with_run();
        t.reserve_agents(&id, 100).unwrap();
        assert_eq!(t.get(&id).unwrap().agents_used, 100);
        let before_rev = t.get(&id).unwrap().revision;
        let state = t.release_agents(&id, 40).unwrap();
        assert_eq!(state.agents_used, 60);
        assert!(state.revision > before_rev);
        assert_eq!(t.remaining_agents(&id), Some(Some(940)));
        let state = t.release_agents(&id, 999).unwrap();
        assert_eq!(state.agents_used, 0);
        assert_eq!(t.remaining_agents(&id), Some(Some(1000)));
        assert!(t.release_agents("missing", 1).is_none());
    }

    #[test]
    fn reconcile_agents_used_sets_absolute_count() {
        let (mut t, id) = tracker_with_run();
        t.reserve_agents(&id, 5).unwrap();
        let before_rev = t.get(&id).unwrap().revision;
        let state = t.reconcile_agents_used(&id, 2).unwrap();
        assert_eq!(state.agents_used, 2);
        assert!(state.revision > before_rev);
        assert_eq!(t.remaining_agents(&id), Some(Some(998)));
        let steady_rev = t.get(&id).unwrap().revision;
        t.reconcile_agents_used(&id, 2).unwrap();
        assert_eq!(
            t.get(&id).unwrap().revision,
            steady_rev,
            "reconciling to an unchanged value must not churn the revision"
        );
        assert!(t.reconcile_agents_used("missing", 0).is_none());
    }

    #[test]
    fn execution_epoch_advances_only_on_resume() {
        let (mut t, id) = tracker_with_run();
        assert_eq!(t.execution_epoch(&id), Some(0));
        t.reserve_agents(&id, 1).unwrap();
        assert_eq!(
            t.execution_epoch(&id),
            Some(0),
            "non-resume mutations must not advance the execution epoch"
        );
        t.pause_user(&id, None);
        assert_eq!(
            t.resume_run(&id, None).map(|s| s.status),
            Some(WorkflowRunStatus::Active)
        );
        assert_eq!(
            t.execution_epoch(&id),
            Some(1),
            "resume starts a new execution so a stale watcher's captured epoch no longer matches"
        );
        t.pause_user(&id, None);
        t.resume_run(&id, None);
        assert_eq!(t.execution_epoch(&id), Some(2));
        assert_eq!(t.execution_epoch("missing"), None);
    }

    #[test]
    fn execution_epoch_survives_snapshot_restore() {
        let (mut tracker, run_id) = tracker_with_run();
        tracker.pause_user(&run_id, None);
        tracker.resume_run(&run_id, None).unwrap();
        tracker.pause_user(&run_id, None);
        let snapshot = tracker.get(&run_id).unwrap();
        assert_eq!(snapshot.execution_epoch, 1);

        let mut restored = WorkflowTracker::from_snapshot(vec![snapshot]).unwrap();
        restored.resume_run(&run_id, None).unwrap();
        assert_eq!(restored.execution_epoch(&run_id), Some(2));
    }

    #[test]
    fn resume_run_insufficient_raise_after_overshoot_leaves_budget_unchanged() {
        let (mut t, id) = tracker_with_run();
        t.run_mut(&id).unwrap().state.agents_used = 1200;
        t.apply_outcome(
            &id,
            &WorkflowOutcome::BudgetExceeded {
                message: "spent".into(),
            },
        );
        assert_eq!(t.get(&id).unwrap().agent_budget, Some(1000));
        assert!(t.resume_run(&id, Some(1100)).is_none());
        assert_eq!(t.get(&id).unwrap().agent_budget, Some(1000));
        assert_eq!(t.get(&id).unwrap().status, WorkflowRunStatus::BudgetLimited);
    }

    #[test]
    fn budget_limited_resume_preserves_cap_and_stays_stopped() {
        let (mut t, id) = tracker_with_run();
        t.apply_outcome(
            &id,
            &WorkflowOutcome::BudgetExceeded {
                message: "spent".into(),
            },
        );
        assert!(t.resume_run(&id, None).is_none());
        assert_eq!(t.get(&id).unwrap().agent_budget, Some(1000));
    }

    #[test]
    fn budget_limited_resume_with_raised_cap_reactivates() {
        let (mut t, id) = tracker_with_run();
        assert_eq!(t.take_status_report().len(), 1);
        t.reserve_agents(&id, 1000).ok();
        t.apply_outcome(
            &id,
            &WorkflowOutcome::BudgetExceeded {
                message: "spent".into(),
            },
        );
        assert!(t.take_status_report().is_empty());
        assert!(t.resume_run(&id, Some(500)).is_none());
        let state = t.resume_run(&id, Some(1024)).unwrap();
        assert_eq!(state.status, WorkflowRunStatus::Active);
        assert_eq!(state.agent_budget, Some(1024));
        let resumed_status = t.take_status_report();
        assert_eq!(resumed_status.len(), 1);
        assert_eq!(resumed_status[0].revision, state.revision);
        let completed = t.apply_outcome(
            &id,
            &WorkflowOutcome::Completed {
                result: serde_json::json!("ok"),
            },
        );
        assert_eq!(completed.unwrap().status, WorkflowRunStatus::Complete);
    }

    #[test]
    fn restored_budget_limited_run_stays_out_of_live_status_reports() {
        let (mut t, id) = tracker_with_run();
        t.reserve_agents(&id, 1000).ok();
        t.apply_outcome(
            &id,
            &WorkflowOutcome::BudgetExceeded {
                message: "spent".into(),
            },
        );

        let mut restored = WorkflowTracker::from_snapshot(t.snapshot()).unwrap();
        assert!(restored.take_status_report().is_empty());

        let resumed = restored.resume_run(&id, Some(1_024)).unwrap();
        assert_eq!(resumed.status, WorkflowRunStatus::Active);
        let completed = restored.apply_outcome(
            &id,
            &WorkflowOutcome::Completed {
                result: serde_json::json!("done later"),
            },
        );
        assert_eq!(completed.unwrap().status, WorkflowRunStatus::Complete);
        assert!(restored.take_status_report().is_empty());
    }

    #[test]
    fn active_status_snapshot_includes_live_elapsed_without_folding_it() {
        let (mut t, id) = tracker_with_run();
        let run = t.run_mut(&id).unwrap();
        run.state.elapsed_ms_floor = 2_000;
        run.active_since = Some(Instant::now() - std::time::Duration::from_millis(5_000));

        let report = t.take_status_report();
        assert_eq!(report.len(), 1);
        assert!(report[0].elapsed_ms_floor >= 7_000);
        assert_eq!(t.get(&id).unwrap().elapsed_ms_floor, 2_000);
    }

    #[test]
    fn phase_dedupes_consecutive() {
        let (mut t, id) = tracker_with_run();
        let revision = t.get(&id).unwrap().revision;
        t.set_phase(&id, "Scan");
        let changed_revision = t.get(&id).unwrap().revision;
        t.set_phase(&id, "Scan");
        let run = t.get(&id).unwrap();
        assert!(changed_revision > revision);
        assert_eq!(run.revision, changed_revision);
    }

    #[test]
    fn reservations_accumulate_and_shrink_remaining() {
        let (mut t, id) = tracker_with_run();
        let state = t.reserve_agents(&id, 125).unwrap();
        assert_eq!(state.agents_used, 125);
        assert_eq!(t.remaining_agents(&id), Some(Some(875)));
        let state = t.reserve_agents(&id, 50).unwrap();
        assert_eq!(state.agents_used, 175);
        assert_eq!(t.remaining_agents(&id), Some(Some(825)));
        assert!(!state.agent_usage_incomplete);
    }

    #[test]
    fn incomplete_accounting_remains_sticky_across_reservations() {
        let (mut t, id) = tracker_with_run();
        t.run_mut(&id).unwrap().state.agent_usage_incomplete = true;
        t.reserve_agents(&id, 100).unwrap();
        let state = t.reserve_agents(&id, 40).unwrap();
        assert_eq!(state.agents_used, 140);
        assert!(state.agent_usage_incomplete);
        assert_eq!(t.remaining_agents(&id), Some(Some(860)));
        let state = t.reserve_agents(&id, 25).unwrap();
        assert_eq!(state.agents_used, 165);
        assert!(state.agent_usage_incomplete);
    }

    #[test]
    fn reservation_over_budget_is_atomic() {
        let (mut t, id) = tracker_with_run();
        t.reserve_agents(&id, 900).unwrap();
        assert_eq!(t.reserve_agents(&id, 300).unwrap_err(), (1200, 1000));
        assert_eq!(t.get(&id).unwrap().agents_used, 900);
        assert_eq!(t.remaining_agents(&id), Some(Some(100)));
    }

    #[test]
    fn unbudgeted_run_reserves_without_remaining() {
        let mut t = WorkflowTracker::default();
        let id = "wf_unlimited";
        t.start_run(
            id.into(),
            "demo".into(),
            "obj".into(),
            Vec::new(),
            None,
            None,
            test_runtime_route(),
        );
        t.reserve_agents(id, 20).ok();
        assert_eq!(t.get(id).unwrap().agents_used, 20);
        assert_eq!(t.remaining_agents(id), Some(None));
    }

    #[test]
    fn revisions_advance_with_authoritative_state() {
        let (mut t, id) = tracker_with_run();
        let started = t.get(&id).unwrap().revision;
        let phase = t.set_phase(&id, "Scan").unwrap().revision;
        assert!(phase > started);
        assert_eq!(t.set_phase(&id, "Scan").unwrap().revision, phase);

        let provenance = t
            .set_definition_provenance(
                &id,
                WorkflowDefinitionId::new("project:scan"),
                WorkflowScope::Project,
                "definition-hash".into(),
            )
            .unwrap()
            .revision;
        assert!(provenance > phase);
        assert_eq!(
            t.set_definition_provenance(
                &id,
                WorkflowDefinitionId::new("project:scan"),
                WorkflowScope::Project,
                "definition-hash".into(),
            )
            .unwrap()
            .revision,
            provenance
        );
        let save_prompt = t.set_save_prompt(&id, true).unwrap().revision;
        assert!(save_prompt > provenance);
        assert_eq!(t.set_save_prompt(&id, true).unwrap().revision, save_prompt);

        t.agent_started(
            &id,
            WorkflowAgentRow {
                agent_id: "child-1".into(),
                label: "scanner".into(),
                phase: None,
                model: None,
                state: "running".into(),
                tokens_used: 0,
                duration_ms: 0,
            },
        );
        let spawned = t.get(&id).unwrap().revision;
        assert!(spawned > save_prompt);
        t.agent_finished(&id, "child-1", "done", 12, 34);
        assert!(t.get(&id).unwrap().revision > spawned);
    }
}
