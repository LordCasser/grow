// Stubs for deleted upload/ and session/repo_changes/ modules.
// These types were removed from the codebase; this file provides
// minimal no-op stubs so the rest of the codebase compiles.
//
// All functions are no-ops. All types are minimal shells.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

// ============================================================================
// session::repo_changes types
// ============================================================================

/// How uploads should be dispatched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadMethod {
    S3 {
        bucket: String,
        region: String,
        credentials_file: Option<String>,
        credentials_content: Option<String>,
        endpoint_url: Option<String>,
    },
    Direct {
        service_account_key: Option<String>,
    },
    Proxy {
        proxy_base_url: String,
        user_token: String,
        deployment_key: Option<String>,
        alpha_test_key: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum BlobCompression {
    None,
    Gzip,
}

/// Configuration for uploading trace artifacts.
#[derive(Debug, Clone)]
pub struct TraceExportConfig {
    pub bucket_url: Option<String>,
    pub service_account_key: Option<String>,
    pub prefix_dir: Option<String>,
    pub gcs_prefix: Option<String>,
    pub absolute_paths: bool,
    pub archive_name_override: Option<String>,
    pub upload_method: UploadMethod,
}

// ============================================================================
// upload::manifest types
// ============================================================================

/// Tracks artifact upload results for a turn.
#[derive(Debug, Clone)]
pub struct ArtifactTracker {
    artifacts: HashMap<String, ArtifactStatus>,
}

/// Status of a single artifact upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactStatus {
    Succeeded,
    Failed { reason: String },
    Skipped,
}

/// Outcome of recording an artifact.
#[derive(Debug, Clone)]
pub enum ArtifactResult {
    Succeeded,
    Failed { reason: String },
    Duplicate,
}

/// Accumulated manifest of all artifacts in a turn.
#[derive(Debug, Clone)]
pub struct Manifest {
    pub artifacts: HashMap<String, ArtifactStatus>,
    pub fully_uploaded: bool,
}

/// Context for artifact upload.
#[derive(Debug, Clone)]
pub struct ArtifactUploadContext {
    pub gcs_config: TraceExportConfig,
    pub artifact_tracker: ArtifactTracker,
}

/// Create a fresh artifact tracker.
pub fn new_artifact_tracker() -> ArtifactTracker {
    ArtifactTracker {
        artifacts: HashMap::new(),
    }
}

/// Record an artifact outcome in the tracker.
pub fn record_artifact(tracker: &ArtifactTracker, _name: &str, _result: ArtifactResult) {}

/// Build a manifest from the current tracker state.
pub fn build_manifest(
    _tracker: &ArtifactTracker,
    _upload_method: UploadMethod,
) -> Manifest {
    Manifest {
        artifacts: HashMap::new(),
        fully_uploaded: true,
    }
}

/// Resolve the upload method from a trace context.
pub fn resolve_upload_method(_ctx: &PromptTraceContext) -> UploadMethod {
    UploadMethod::Direct {
        service_account_key: None,
    }
}

/// Write an error manifest. No-op stub.
pub async fn write_error_manifest(
    _ctx: &PromptTraceContext,
) {}

impl ArtifactTracker {
    /// Return a cloned tracker. Always returns a fresh empty tracker.
    pub fn cloned(&self) -> ArtifactTracker {
        ArtifactTracker {
            artifacts: HashMap::new(),
        }
    }

    /// Return a reference-compatible cloned tracker.
    pub fn as_ref(&self) -> &ArtifactTracker {
        self
    }
}

// ============================================================================
// upload::trace types
// ============================================================================

/// Schema version for GCS trace artifacts.
pub const GCS_SCHEMA_VERSION: &str = "1.0";

/// Reference to a spawned subagent.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SubagentSpawnedRef {
    pub subagent_id: String,
    pub child_session_id: String,
    pub subagent_type: String,
    pub description: String,
    pub persona: Option<String>,
    pub resumed_from: Option<String>,
}

/// Per-prompt metadata for trace uploads.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PromptMetadata {
    pub schema_version: String,
    pub session_id: String,
    pub turn_number: u64,
    pub request_id: String,
    pub turn_started_at: String,
    pub repo_root: Option<String>,
    pub remote_url: Option<String>,
    pub user_id: Option<String>,
    pub user_email: Option<String>,
    pub team_id: Option<String>,
    pub client_source: Option<String>,
    pub client_version: Option<String>,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub experiment_id: Option<String>,
    pub host_os: String,
    pub host_arch: String,
    pub prompt_has_image: Option<bool>,
    pub prompt_was_truncated: Option<bool>,
    pub prompt_verbatim: Option<bool>,
    pub cwd: Option<String>,
    pub agent_type: Option<String>,
    pub shell_version: Option<String>,
    pub workspace_type: Option<String>,
    pub sandbox: SandboxTelemetry,
}

/// Per-turn result metadata for trace uploads.
#[derive(Debug, Clone)]
pub struct TurnResultMetadata<S = serde_json::Value, D = serde_json::Value> {
    pub schema_version: &'static str,
    pub request_id: String,
    pub completed: bool,
    pub stop_reason: Option<String>,
    pub total_tokens: Option<u64>,
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub error: Option<String>,
    pub finished_at: String,
    pub signals: Option<S>,
    pub turn_delta: Option<D>,
    pub start_prompt_mode: Option<String>,
    pub end_prompt_mode: Option<String>,
    pub resolved_model: Option<String>,
    pub subagents_spawned: Vec<SubagentSpawnedRef>,
}

/// Telemetry about the sandbox environment.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SandboxTelemetry {
    pub is_sandbox: bool,
    pub sandbox_type: Option<String>,
}

/// Collect local sandbox telemetry.
pub fn local_sandbox_telemetry() -> SandboxTelemetry {
    SandboxTelemetry {
        is_sandbox: false,
        sandbox_type: None,
    }
}

/// Metadata type for session uploads.
#[derive(Debug, Clone)]
pub enum SessionMetadataType {
    Session,
    Turn,
}

/// Dynamic resolver trait for session state building.
pub trait DynamicResolver: Send + Sync {
    fn resolve(&self) -> TraceExportConfig;
}

/// Error from session state building.
#[derive(Debug)]
pub struct SessionStateBuildError(String);

impl std::fmt::Display for SessionStateBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SessionStateBuildError: {}", self.0)
    }
}

impl std::error::Error for SessionStateBuildError {}

/// Trait for resolving a trace export config.
pub trait TraceExportSource: Send + Sync {
    fn resolve(&self) -> TraceExportConfig;
}

impl TraceExportSource for TraceExportConfig {
    fn resolve(&self) -> TraceExportConfig {
        self.clone()
    }
}

/// Stub for the removed UploadRetryPolicy.
#[derive(Debug, Clone, Default)]
pub struct UploadRetryPolicy {}

// ---- upload_* / spawn_* functions (all no-ops) ----

pub fn upload_tool_definitions(
    _gcs_config: TraceExportConfig,
    _auth_manager: impl Send,
    _tool_defs: impl Send,
    _artifact_tracker: Option<&ArtifactTracker>,
) -> impl std::future::Future<Output = ()> + Send {
    async {}
}

pub async fn upload_session_state(
    _ctx: &PromptTraceContext,
    _phase: &str,
    _session_copy_rx: impl Send,
    _wait: UploadWait,
) {
}

pub async fn upload_metadata(_ctx: &PromptTraceContext, _metadata: PromptMetadata) {}

pub async fn upload_subagent_metadata(
    _ctx: impl Send,
    _subagent_id: impl Send,
    _metadata: impl Send,
    _auth_manager: Arc<crate::auth::AuthManager>,
) {
}

pub async fn upload_turn_result<S: Send, D: Send>(
    _ctx: &PromptTraceContext,
    _result: &TurnResultMetadata<S, D>,
    _wait: UploadWait,
) {
}

pub async fn upload_full_prompt_txt(
    _ctx: &PromptTraceContext,
    _prompt_text: &str,
) {
}

pub async fn upload_harness_session_archive(
    _ctx: &PromptTraceContext,
    _archive_path: impl Send,
) -> Result<String, String> {
    Ok(String::new())
}

pub async fn upload_images<T>(
    _ctx: &PromptTraceContext,
    _images: &[T],
) {
}

pub async fn upload_plugin_state<T>(
    _ctx: &PromptTraceContext,
    _plugin_registry: Option<&T>,
) {
}

pub async fn upload_turn_messages(
    _ctx: &PromptTraceContext,
    _messages: impl Send,
    _wait: UploadWait,
) {
}

pub async fn upload_unified_log(
    _ctx: &PromptTraceContext,
    _wait: UploadWait,
) {
}

pub async fn upload_artifact_to_gcs(
    _ctx: &PromptTraceContext,
    _path: &str,
    _data: &[u8],
    _content_type: &str,
    _wait: UploadWait,
) -> Result<(), String> {
    Ok(())
}

pub async fn upload_small_artifact(
    _ctx: &PromptTraceContext,
    _path: &str,
    _data: &[u8],
    _content_type: &str,
) {
}

pub async fn upload_streaming_partial(
    _ctx: &PromptTraceContext,
    _capture: &StreamingPartialCapture,
    _wait: UploadWait,
) {
}

pub async fn upload_session_metadata(_ctx: &PromptTraceContext, _metadata: &serde_json::Value) {}

pub async fn upload_memory_state(
    _ctx: &PromptTraceContext,
    _memory_bytes: &[u8],
) {
}

pub async fn upload_permission_events(
    _ctx: &PromptTraceContext,
    _events: &serde_json::Value,
) {
}

pub fn build_chat_history_session_state(
    _messages: impl Send,
) -> String {
    String::new()
}

pub fn spawn_startup_spill_reconcile(
    _grow_home: std::path::PathBuf,
    _queue: Option<UploadQueue>,
) {
}

pub async fn flush_upload_queue(_queue: &UploadQueue) {}

pub async fn blocking_attempt_budget(_queue: &UploadQueue) -> bool {
    true
}

pub async fn flush_then_write_error_manifest(
    _ctx: &PromptTraceContext,
    _deadline: tokio::time::Instant,
) {}

pub async fn spawn_purge_stale_upload_scratch() {}

pub fn spawn_upload_queue(
    _grow_home: &std::path::Path,
    _gcs_config: &TraceExportConfig,
    _version: Option<&str>,
    _auth_manager: Arc<crate::auth::AuthManager>,
) -> UploadQueue {
    UploadQueue::new()
}

pub async fn upload_trace_artifact_deferred(
    _ctx: &PromptTraceContext,
    _path: impl Send,
    _data: impl Send,
    _content_type: impl Send,
) -> Result<(), String> {
    Ok(())
}

pub async fn upload_trace_artifact(
    _ctx: &PromptTraceContext,
    _path: impl Send,
    _data: impl Send,
    _content_type: impl Send,
    _wait: impl Send,
) -> Result<(), String> {
    Ok(())
}

// ============================================================================
// upload::turn types
// ============================================================================

/// Upload queue.
#[derive(Debug, Clone)]
pub struct UploadQueue {
    stats: Arc<UploadQueueStats>,
}

/// Statistics for the upload queue.
#[derive(Debug, Default)]
pub struct UploadQueueStats {
    pub enqueued: AtomicU64,
    pub uploaded: AtomicU64,
    pub failed: AtomicU64,
    pub enqueue_fallbacks: AtomicU64,
    pub circuit_breaker_trips: AtomicU64,
    pub pending: AtomicU64,
    pub pending_bytes: AtomicU64,
}

impl UploadQueue {
    pub fn new() -> Self {
        Self {
            stats: Arc::new(UploadQueueStats::default()),
        }
    }

    pub fn spawn(
        _grow_home: &std::path::Path,
        _config: Arc<dyn TraceExportSource>,
        _retry_policy: UploadRetryPolicy,
    ) -> Arc<Self> {
        Arc::new(Self::new())
    }

    pub fn stats(&self) -> &UploadQueueStats {
        &self.stats
    }

    pub fn stats_arc(&self) -> Arc<UploadQueueStats> {
        self.stats.clone()
    }

    pub async fn drain(&self, _timeout: Duration) -> u64 {
        0
    }
}

/// Whether to wait for upload confirmation.
#[derive(Debug, Clone)]
pub enum UploadWait {
    Confirm,
    NoWait,
    Defer { deadline: tokio::time::Instant },
}

/// Outcome of an upload operation.
#[derive(Debug, Clone)]
pub enum UploadOutcome {
    Confirmed,
    Deferred,
    Failed { reason: String, status_code: Option<u16> },
}

/// Reason why trace was uploaded (or not).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceUploadReason {
    FeatureOn,
    FeatureOff,
    ZdrTeam,
    NoBucket,
    NoAuth,
    Other,
}

impl TraceUploadReason {
    pub fn from_upload_method(_method: &Option<UploadMethod>) -> Self {
        Self::FeatureOn
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FeatureOn => "feature_on",
            Self::FeatureOff => "feature_off",
            Self::ZdrTeam => "zdr_team",
            Self::NoBucket => "no_bucket",
            Self::NoAuth => "no_auth",
            Self::Other => "other",
        }
    }
}

/// Synthetic turn trace request.
#[derive(Debug)]
pub struct SyntheticTurnTraceRequest {
    pub session_id: agent_client_protocol::SessionId,
    pub prompt_id: String,
    pub completion_rx: tokio::sync::oneshot::Receiver<
        Result<
            crate::session::commands::PromptTurnOk,
            agent_client_protocol::Error,
        >,
    >,
    pub before_session_copy_rx: tokio::sync::oneshot::Receiver<anyhow::Result<crate::session::persistence::SessionStateCopy>>,
}

/// Context for a prompt's trace upload lifecycle.
#[derive(Clone)]
pub struct PromptTraceContext {
    pub gcs_config: TraceExportConfig,
    pub session_info: crate::session::info::Info,
    pub turn_number: u64,
    pub session_handle: crate::session::SessionHandle,
    pub session_registry_enabled: bool,
    pub upload_queue: Option<UploadQueue>,
    pub artifact_tracker: ArtifactTracker,
    pub auth_manager: Arc<crate::auth::AuthManager>,
}

impl PromptTraceContext {
    pub fn artifact_upload_context(&self) -> ArtifactUploadContext {
        ArtifactUploadContext {
            gcs_config: self.gcs_config.clone(),
            artifact_tracker: self.artifact_tracker.clone(),
        }
    }
}

/// Spawn a fire-and-forget upload task.
pub fn spawn_upload_task<F: std::future::Future + Send + 'static>(_name: &str, _f: F) {}

/// Complete a prompt trace upload (no-op stub).
pub async fn complete_prompt_trace(
    _ctx: PromptTraceContext,
    _permission_events: impl Send,
    _session_copy_rx: impl Send,
    _turn_messages: impl Send,
    _streaming_partial: impl Send,
    _wait: UploadWait,
) -> Result<bool, String> {
    Ok(true)
}

/// Capture of streaming partial data for trace upload.
#[derive(Debug, Clone)]
pub struct StreamingPartialCapture {
    pub reason: Option<String>,
    pub data: Vec<u8>,
    pub path: Option<String>,
    pub content_type: Option<String>,
    pub model_id: Option<String>,
}

/// Take streaming partial from session.
pub async fn take_streaming_partial(
    _cmd_tx: &tokio::sync::mpsc::UnboundedSender<crate::session::SessionCommand>,
    _prompt_id: String,
    _synthetic_committed: bool,
    _model: Option<String>,
) -> Option<StreamingPartialCapture> {
    None
}

/// Parse `ask_user_question` flag from session metadata.
pub fn parse_ask_user_question_from_meta(
    _meta: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Option<bool> {
    None
}

// ============================================================================
// upload::gcs types
// ============================================================================

/// Default bucket for session traces.
pub const SESSION_TRACES_BUCKET: &str = "grow-session-traces";

/// Upload auth diagnostics to GCS (no-op).
pub async fn upload_to_auth_diagnostics(
    _log_bytes: &[u8],
    _user_id: &str,
    _upload_method: &UploadMethod,
    _auth_manager: Arc<crate::auth::AuthManager>,
) {
}

// ============================================================================
// Additional stubs for functions previously in upload/turn.rs and upload/manifest.rs
// ============================================================================

/// Write an upload manifest (no-op).
pub async fn write_upload_manifest(
    _ctx: &PromptTraceContext,
    _manifest: &Manifest,
) {
}

/// Look up the per-session model, falling back to the default.
pub fn lookup_session_model(
    _sessions: &std::collections::HashMap<agent_client_protocol::SessionId, crate::session::SessionHandle>,
    _session_id: Option<&agent_client_protocol::SessionId>,
    default_model: &agent_client_protocol::ModelId,
) -> agent_client_protocol::ModelId {
    default_model.clone()
}

/// Parse agent profile name from session metadata.
pub fn parse_agent_profile_from_meta(_meta: Option<&serde_json::Map<String, serde_json::Value>>) -> Option<grow_agent::AgentDefinition> {
    None
}

/// Apply yolo mode to matching sessions, returning the count of sessions updated.
pub fn apply_yolo_mode_to_matching_sessions(
    _sessions: &mut std::collections::HashMap<agent_client_protocol::SessionId, crate::session::SessionHandle>,
    _client_name: Option<&str>,
    _enabled: bool,
) -> usize {
    0
}

// ============================================================================
// StorageClient stub
// ============================================================================

/// Stub for the removed StorageClient.
#[derive(Debug, Clone)]
pub struct StorageClient;

/// Stub for the removed RetryConfig.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    initial_delay: Duration,
    max_retries: usize,
    multiplier: f64,
    jitter_factor: f64,
}

impl RetryConfig {
    pub fn new() -> Self {
        Self {
            initial_delay: Duration::from_secs(1),
            max_retries: 3,
            multiplier: 2.0,
            jitter_factor: 0.1,
        }
    }

    pub fn with_initial_delay(mut self, delay: Duration) -> Self {
        self.initial_delay = delay;
        self
    }

    pub fn with_max_retries(mut self, max: usize) -> Self {
        self.max_retries = max;
        self
    }

    pub fn with_multiplier(mut self, multiplier: f64) -> Self {
        self.multiplier = multiplier;
        self
    }

    pub fn with_jitter_factor(mut self, factor: f64) -> Self {
        self.jitter_factor = factor;
        self
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageClient {
    pub fn new(_url: &str, _token: &str) -> Self {
        Self
    }

    pub fn with_retry_config(self, _config: RetryConfig) -> Self {
        self
    }

    pub async fn upload(
        &self,
        _path: &str,
        _data: &[u8],
        _content_type: &str,
    ) -> Result<StorageUploadResult, String> {
        Ok(StorageUploadResult::default())
    }
}

/// Stub for storage upload response.
#[derive(Debug, Clone, Default)]
pub struct StorageUploadResult {
    pub bucket: String,
    pub path: String,
    pub size: u64,
    pub content_type: String,
    pub generation: u64,
}
