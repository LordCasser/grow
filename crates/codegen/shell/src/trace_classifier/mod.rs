//! Trace-replay classifier.
//!
//! Reads an offline session trace JSON and replays the Layer-3
//! LazinessDetector classifier against every turn, emitting one JSONL
//! record per turn so an operator can inspect classifier decisions.
//!
//! Production wiring lives in
//! `crate::session::actor::maybe_fire_laziness_check`
//! — this module is a thin replay harness around the same `pub(crate)`
//! helpers. New crate-internal so we get visibility for free without
//! widening the production surface.

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use sampling_types::{ContentPart, ConversationItem, ConversationRequest, SystemItem, UserItem};
use serde::{Deserialize, Serialize};

use crate::session::{
    DebugDecision, LAZINESS_CLASSIFIER_PROMPT, LAZINESS_CLASSIFIER_TIMEOUT_MS,
    LAZINESS_CONTEXT_ITEM_LIMIT, LAZINESS_DEFAULT_MIN_CONFIDENCE, LAZINESS_INCLUDE_REASONING,
    LAZINESS_MIN_ASSISTANT_TURNS, LAZINESS_MIN_USER_TURNS, LAZINESS_USER_PREAMBLE,
    classify_debug_decision, flatten_transcript_for_classifier, format_runtime_state_line,
    laziness_window_start, parse_classifier_output,
};

// Trace JSON shape

/// One element of the top-level trace JSON array.
#[derive(Debug, Deserialize)]
pub struct TurnRecord {
    pub turn: String,
    pub trace: TurnTrace,
}

#[derive(Debug, Deserialize)]
pub struct TurnTrace {
    pub metadata: TurnMetadata,
    /// Post-turn conversation snapshot. Deserializes directly into
    /// the canonical [`ConversationItem`] enum — the production wire
    /// format already matches.
    #[serde(rename = "afterStateHistory")]
    pub after_state_history: Vec<ConversationItem>,
}

#[derive(Debug, Deserialize)]
pub struct TurnMetadata {
    #[serde(default)]
    pub turn_number: Option<u64>,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    /// Original start timestamp, retained as trace metadata only. The gap to
    /// another turn includes user idle time and is not a duration measurement.
    #[serde(default)]
    pub turn_started_at: Option<String>,
    /// Measured duration from this turn's terminal event, when captured.
    #[serde(default)]
    pub turn_duration_ms: Option<u64>,
    /// Live terminal-task count captured at this turn's classifier boundary.
    /// Missing means unknown; tool results cannot reconstruct task completion.
    #[serde(default)]
    pub outstanding_background_tasks: Option<usize>,
}

// Output schema. Borrowed slices throughout so we serialize once
// straight from `process_turn`'s working state without any clones.
// (N2/N12/N15.)

#[derive(Debug, Serialize)]
pub struct ParsedClassifierOut<'a> {
    pub category: &'static str,
    pub confidence: f32,
    pub evidence: &'a str,
}

#[derive(Debug, Serialize)]
pub struct LazinessOut<'a> {
    pub model_id: &'a str,
    pub elapsed_ms: u64,
    pub raw_output: Option<&'a str>,
    pub parsed: Option<ParsedClassifierOut<'a>>,
    pub decision: LazinessDecisionKind,
    /// `classifier_error` / `timeout` / `parse_error`. `None` when a
    /// verdict was produced.
    pub abort_reason: Option<AbortReasonKind>,
    pub error_detail: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LazinessDecisionKind {
    WouldNudge,
    NoNudgeLowConfidence,
    NoNudgeNotStalled,
    Aborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AbortReasonKind {
    ClassifierError,
    Timeout,
    ParseError,
}

#[derive(Debug, Serialize)]
pub struct TurnLine<'a> {
    pub turn_id: &'a str,
    pub turn_number: Option<u64>,
    pub request_id: Option<&'a str>,
    pub items_in_history: usize,
    pub items_after_window_trim: usize,
    /// Backgrounded terminal tasks only (excludes subagents) — what
    /// the Layer-3 classifier `[runtime_state]` line carries
    /// (production: `snapshot_backing_task_count_for_debug_log`).
    pub classifier_backing_task_count: Option<usize>,
    pub laziness_classifier: LazinessOut<'a>,
    /// The resolved `include_reasoning` value used to render the
    /// classifier transcript for this turn. Lives next to
    /// `laziness_classifier` so log-diffs of two A/B runs are obvious.
    pub include_reasoning: bool,
    /// Measured turn duration in seconds, or unknown when the trace omitted it.
    pub turn_elapsed_seconds: Option<u64>,
}

// Summary counters

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    pub turns: usize,
    pub laz_would_nudge: usize,
    pub laz_not_stalled: usize,
    pub laz_low_confidence: usize,
    pub laz_aborted: usize,
}

impl Summary {
    pub fn render(&self) -> String {
        // Lead with the common (non-action) class for parity with the
        // laziness counter ordering. (F28)
        format!(
            "Processed {} turns. Laziness: {} NoNudge-NotStalled, {} NoNudge-LowConfidence, {} WouldNudge, {} Aborted.",
            self.turns,
            self.laz_not_stalled,
            self.laz_low_confidence,
            self.laz_would_nudge,
            self.laz_aborted,
        )
    }
}

// The replay input may contain a measured runtime snapshot. Conversation
// tool results acknowledge calls, not the lifetimes of background processes.

#[async_trait::async_trait]
pub trait ClassifierClient: Send + Sync {
    async fn run(&self, request: ConversationRequest) -> Result<String, String>;
}

/// Production sampler-backed [`ClassifierClient`] used by the CLI
/// binary. Wraps a `sampler::SamplingClient` and pulls the
/// text content out of the response.
pub struct SamplerClassifierClient {
    inner: sampler::SamplingClient,
}

impl SamplerClassifierClient {
    pub fn new(inner: sampler::SamplingClient) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl ClassifierClient for SamplerClassifierClient {
    async fn run(&self, request: ConversationRequest) -> Result<String, String> {
        self.inner
            .conversation_collect(request)
            .await
            .map(|r| {
                r.assistant()
                    .map(|a| a.content.as_ref().to_owned())
                    .unwrap_or_default()
            })
            .map_err(|e| e.to_string())
    }
}

/// Build the two-item `[System, User]` classifier request. Matches
/// `maybe_fire_laziness_check` exactly: same prompt, same wrapper
/// text ([`LAZINESS_USER_PREAMBLE`]) and same omitted sampling preferences.
///
/// `classifier_backing_task_count` is a measured terminal-task count; missing
/// measurements stay unknown rather than becoming a fabricated zero.
pub fn build_classifier_request(
    items: &[ConversationItem],
    classifier_backing_task_count: Option<usize>,
    model_id: &str,
    include_reasoning: bool,
    turn_elapsed_seconds: Option<u64>,
) -> ConversationRequest {
    let runtime_state =
        format_runtime_state_line(classifier_backing_task_count, turn_elapsed_seconds);
    let transcript_text = flatten_transcript_for_classifier(items, include_reasoning);

    let convo_items = vec![
        ConversationItem::System(SystemItem {
            content: std::sync::Arc::<str>::from(LAZINESS_CLASSIFIER_PROMPT),
        }),
        ConversationItem::User(UserItem {
            content: vec![ContentPart::Text {
                text: std::sync::Arc::<str>::from(format!(
                    "{LAZINESS_USER_PREAMBLE}\
                     === BEGIN TRANSCRIPT ===\n\
                     {runtime_state}\
                     {transcript_text}\
                     === END TRANSCRIPT ===\n"
                )),
            }],
            synthetic_reason: None,
            permission_evidence: None,
            ..Default::default()
        }),
    ];

    ConversationRequest {
        items: convo_items,
        tools: vec![],
        tool_choice: None,
        model: Some(model_id.to_owned()),
        reasoning_effort: None,
        ..ConversationRequest::default()
    }
}

fn decision_kind(d: DebugDecision) -> LazinessDecisionKind {
    match d {
        DebugDecision::WouldNudge => LazinessDecisionKind::WouldNudge,
        DebugDecision::NoNudgeNotStalled => LazinessDecisionKind::NoNudgeNotStalled,
        DebugDecision::NoNudgeLowConfidence => LazinessDecisionKind::NoNudgeLowConfidence,
        DebugDecision::Aborted => LazinessDecisionKind::Aborted,
        // Classifier returned stalled_* above threshold; harness blocked
        // injection because the session was not in an active goal.
        DebugDecision::SuppressedNotGoalMode => LazinessDecisionKind::WouldNudge,
    }
}

/// Owned-string twin of [`LazinessOut`]. Held on [`TurnData`];
/// `as_line()` lends out a borrowed `LazinessOut<'_>` view so the
/// JSONL serialization is zero-clone. (N2/N15)
#[derive(Debug)]
pub struct LazinessOwned {
    pub model_id: String,
    pub elapsed_ms: u64,
    pub raw_output: Option<String>,
    pub parsed: Option<ParsedClassifierOwned>,
    pub decision: LazinessDecisionKind,
    pub abort_reason: Option<AbortReasonKind>,
    pub error_detail: Option<String>,
}

#[derive(Debug)]
pub struct ParsedClassifierOwned {
    pub category: &'static str,
    pub confidence: f32,
    pub evidence: String,
}

/// Run a single turn end-to-end against the classifier client.
///
/// `started` is captured AFTER the request is built so `elapsed_ms`
/// reflects only the sampler call wall-clock (F36) — directly
/// comparable to production's `LazinessFireOutcome::Verdict`
/// `classifier_elapsed_ms` which times the same span.
///
/// Production runs the sampler call inside a `tokio::select!` with a
/// biased timeout arm and an abort poller. We use the simpler
/// `tokio::time::timeout` (F9) — replay has no user-input to abort on,
/// so the abort poller is dead weight, and the biased ordering only
/// matters for a "user cancels while sampler is still running"
/// race that can't happen offline.
async fn classify_turn(
    items: &[ConversationItem],
    classifier_backing_task_count: Option<usize>,
    model_id: &str,
    min_confidence: f32,
    include_reasoning: bool,
    turn_elapsed_seconds: Option<u64>,
    client: &dyn ClassifierClient,
) -> LazinessOwned {
    let request = build_classifier_request(
        items,
        classifier_backing_task_count,
        model_id,
        include_reasoning,
        turn_elapsed_seconds,
    );
    let started = std::time::Instant::now();
    let timeout = Duration::from_millis(LAZINESS_CLASSIFIER_TIMEOUT_MS);
    let outcome = tokio::time::timeout(timeout, client.run(request)).await;
    let elapsed_ms = started.elapsed().as_millis() as u64;

    match outcome {
        Err(_) => LazinessOwned {
            model_id: model_id.to_owned(),
            elapsed_ms,
            parsed: None,
            decision: LazinessDecisionKind::Aborted,
            abort_reason: Some(AbortReasonKind::Timeout),
            error_detail: None,
            raw_output: None,
        },
        Ok(Err(detail)) => LazinessOwned {
            model_id: model_id.to_owned(),
            elapsed_ms,
            parsed: None,
            decision: LazinessDecisionKind::Aborted,
            abort_reason: Some(AbortReasonKind::ClassifierError),
            error_detail: Some(detail),
            raw_output: None,
        },
        Ok(Ok(raw_text)) => match parse_classifier_output(&raw_text) {
            Err(parse_err) => LazinessOwned {
                model_id: model_id.to_owned(),
                elapsed_ms,
                raw_output: Some(raw_text),
                parsed: None,
                decision: LazinessDecisionKind::Aborted,
                abort_reason: Some(AbortReasonKind::ParseError),
                error_detail: Some(parse_err.to_string()),
            },
            Ok(parsed) => {
                let decision = classify_debug_decision(&parsed, min_confidence);
                LazinessOwned {
                    model_id: model_id.to_owned(),
                    elapsed_ms,
                    raw_output: Some(raw_text),
                    parsed: Some(ParsedClassifierOwned {
                        category: parsed.category.as_const_str(),
                        confidence: parsed.confidence,
                        evidence: parsed.evidence,
                    }),
                    decision: decision_kind(decision),
                    abort_reason: None,
                    error_detail: None,
                }
            }
        },
    }
}

fn bump_summary(summary: &mut Summary, line: &TurnLine<'_>) {
    summary.turns += 1;
    match line.laziness_classifier.decision {
        LazinessDecisionKind::WouldNudge => summary.laz_would_nudge += 1,
        LazinessDecisionKind::NoNudgeNotStalled => summary.laz_not_stalled += 1,
        LazinessDecisionKind::NoNudgeLowConfidence => summary.laz_low_confidence += 1,
        LazinessDecisionKind::Aborted => summary.laz_aborted += 1,
    }
}

// Top-level per-turn pipeline.

/// Per-turn working data. `turn_id` and `request_id` borrow from the
/// source `TurnRecord` so we don't clone identifiers we already
/// own elsewhere; the rest is owned because it's produced fresh
/// (classifier output) and outlives any single
/// `as_line()` call.
pub struct TurnData<'a> {
    pub turn_id: &'a str,
    pub turn_number: Option<u64>,
    pub request_id: Option<&'a str>,
    pub items_in_history: usize,
    pub items_after_window_trim: usize,
    pub classifier_backing_task_count: Option<usize>,
    pub laziness: LazinessOwned,
    /// Resolved `include_reasoning` value the classifier ran with —
    /// CLI override winning over `LAZINESS_INCLUDE_REASONING`. Surfaced
    /// in the per-turn JSONL line so two runs that differ only in this
    /// flag are easy to diff.
    pub include_reasoning: bool,
    /// See [`TurnLine::turn_elapsed_seconds`].
    pub turn_elapsed_seconds: Option<u64>,
}

impl<'a> TurnData<'a> {
    /// Lend a borrowed `TurnLine<'_>` view. Zero clones — every
    /// string-typed field is a borrow into a field on `self`
    /// (or transitively into the source `TurnRecord`). (N2/N15)
    pub fn as_line(&self) -> TurnLine<'_> {
        TurnLine {
            turn_id: self.turn_id,
            turn_number: self.turn_number,
            request_id: self.request_id,
            items_in_history: self.items_in_history,
            items_after_window_trim: self.items_after_window_trim,
            classifier_backing_task_count: self.classifier_backing_task_count,
            laziness_classifier: LazinessOut {
                model_id: self.laziness.model_id.as_str(),
                elapsed_ms: self.laziness.elapsed_ms,
                raw_output: self.laziness.raw_output.as_deref(),
                parsed: self.laziness.parsed.as_ref().map(|p| ParsedClassifierOut {
                    category: p.category,
                    confidence: p.confidence,
                    evidence: p.evidence.as_str(),
                }),
                decision: self.laziness.decision,
                abort_reason: self.laziness.abort_reason,
                error_detail: self.laziness.error_detail.as_deref(),
            },
            include_reasoning: self.include_reasoning,
            turn_elapsed_seconds: self.turn_elapsed_seconds,
        }
    }
}

/// Replay a single turn record against the classifier. Pure
/// over `client`; the binary uses the sampler-backed implementation,
/// tests use a recorded-response double.
///
/// `turn_elapsed_seconds` is a measured duration for this turn. It remains
/// unknown when the trace contains no measurement, including the last turn.
pub async fn process_turn<'a>(
    record: &'a TurnRecord,
    model_id: &str,
    min_confidence: f32,
    include_reasoning: bool,
    turn_elapsed_seconds: Option<u64>,
    client: &dyn ClassifierClient,
) -> TurnData<'a> {
    let history = record.trace.after_state_history.as_slice();
    let items_in_history = history.len();

    let backing_task_count = record.trace.metadata.outstanding_background_tasks;

    // Trim to the classifier window without cloning the history —
    // the slice borrows directly out of the record. (F12)
    let window_start = laziness_window_start(
        history,
        LAZINESS_CONTEXT_ITEM_LIMIT,
        LAZINESS_MIN_USER_TURNS,
        LAZINESS_MIN_ASSISTANT_TURNS,
    );
    let trimmed = &history[window_start..];
    let items_after_window_trim = trimmed.len();

    let laziness = classify_turn(
        trimmed,
        backing_task_count,
        model_id,
        min_confidence,
        include_reasoning,
        turn_elapsed_seconds,
        client,
    )
    .await;

    TurnData {
        turn_id: record.turn.as_str(),
        turn_number: record.trace.metadata.turn_number,
        request_id: record.trace.metadata.request_id.as_deref(),
        items_in_history,
        items_after_window_trim,
        classifier_backing_task_count: backing_task_count,
        laziness,
        include_reasoning,
        turn_elapsed_seconds,
    }
}

/// Read measured durations from each turn independently. Never infer execution
/// time from the next user input: that interval also includes user idle time.
pub fn compute_turn_elapsed_seconds(trace: &[TurnRecord]) -> Vec<Option<u64>> {
    trace
        .iter()
        .map(|record| record.trace.metadata.turn_duration_ms.map(|ms| ms / 1_000))
        .collect()
}

// CLI plumbing

/// Inputs to [`run`]. Constructed by the bin's clap layer.
pub struct RunArgs {
    pub trace: PathBuf,
    pub output: Option<PathBuf>,
    pub model_id: String,
    pub api_base_url: String,
    pub api_key: Option<String>,
    /// Per-model `min_confidence` override (F6). The CLI clap layer
    /// validates this is in `[0.0, 1.0]` before constructing the
    /// struct (N5). Defaults to [`LAZINESS_DEFAULT_MIN_CONFIDENCE`]
    /// via [`Self::min_confidence_value`].
    pub min_confidence: Option<f32>,
    /// CLI override for the `[assistant reasoning]` emission flag.
    /// `None` defers to the harness default
    /// ([`LAZINESS_INCLUDE_REASONING`]). The trace_classify binary
    /// has no per-model config to consult, so this is the *only*
    /// override surface for the offline tool — production resolves
    /// `LazinessDetectorPerModelConfig::include_reasoning` separately.
    pub include_reasoning: Option<bool>,
}

impl RunArgs {
    /// Effective min_confidence threshold (override or default).
    pub fn min_confidence_value(&self) -> f32 {
        self.min_confidence
            .unwrap_or(LAZINESS_DEFAULT_MIN_CONFIDENCE)
    }

    /// Effective `include_reasoning` value (CLI override or harness
    /// default). Threaded into `process_turn` and surfaced on the
    /// per-turn JSONL line.
    pub fn include_reasoning_value(&self) -> bool {
        self.include_reasoning.unwrap_or(LAZINESS_INCLUDE_REASONING)
    }
}

/// Validate `min_confidence` is finite and in the closed unit
/// interval. Used by clap's `value_parser` (N5).
pub fn validate_min_confidence(raw: &str) -> std::result::Result<f32, String> {
    let v: f32 = raw
        .parse()
        .map_err(|e: std::num::ParseFloatError| e.to_string())?;
    if !v.is_finite() {
        return Err(format!("min_confidence must be finite, got {v}"));
    }
    if !(0.0..=1.0).contains(&v) {
        return Err(format!("min_confidence must be in [0.0, 1.0], got {v}"));
    }
    Ok(v)
}

/// Maximum trace file size accepted by [`parse_trace_file`]. Per-session
/// traces are typically a few MB; this bound protects against accidentally
/// pointing the tool at a 10 GB log dump that would OOM the parser. (N7)
pub const MAX_TRACE_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// Read and parse the full trace from `path`. Loads into memory in
/// one go because `serde_json` does not stream JSON arrays without a
/// custom incremental driver; the [`MAX_TRACE_FILE_BYTES`] check
/// keeps the memory budget predictable. For traces beyond that
/// bound, split the array per-turn upstream and call this once per
/// chunk. (N7)
pub fn parse_trace_file(path: &Path) -> Result<Vec<TurnRecord>> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let meta = file
        .metadata()
        .with_context(|| format!("stat {}", path.display()))?;
    if meta.len() > MAX_TRACE_FILE_BYTES {
        return Err(anyhow!(
            "trace file {} is {} bytes; exceeds MAX_TRACE_FILE_BYTES = {}. \
             Split the array per turn upstream if you really want to replay this.",
            path.display(),
            meta.len(),
            MAX_TRACE_FILE_BYTES,
        ));
    }
    // `from_reader` over a `BufReader` keeps the actual I/O bounded
    // by `BufReader`'s default buffer, even though serde still
    // materialises the full `Vec<TurnRecord>` into memory.
    let reader = BufReader::new(file);
    serde_json::from_reader(reader)
        .with_context(|| format!("parse trace {} as Vec<TurnRecord>", path.display()))
}

/// Resolve the offline sampler credential from `--api-key` or the provider-
/// neutral `$LLM_API_KEY`. Product login credentials are never inference
/// credentials.
pub async fn resolve_api_key(explicit: Option<&str>) -> Result<String> {
    if let Some(k) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(k.to_owned());
    }
    let env_key = std::env::var("LLM_API_KEY").ok();
    if let Some(k) = env_key.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(k.to_owned());
    }
    Err(anyhow!("no API key: pass --api-key or set LLM_API_KEY"))
}

/// Resolve the API key and build the sampler client. Crate-internal:
/// the only caller is [`run`], which has already destructured
/// `RunArgs` so the owned strings can move into `SamplerConfig`
/// instead of being cloned.
async fn build_sampler_client(
    base_url: String,
    model: String,
    api_key: Option<&str>,
) -> Result<sampler::SamplingClient> {
    let resolved = resolve_api_key(api_key).await?;
    let config = sampler::SamplerConfig {
        api_key: Some(resolved),
        base_url,
        model,
        ..sampler::SamplerConfig::default()
    };
    sampler::SamplingClient::new(config).map_err(|e| anyhow!("build SamplingClient: {e}"))
}

/// End-to-end entry point used by the binary. Writes one JSONL line
/// per turn to `args.output` (or stdout). Per-turn failures are
/// surfaced inside the line — they never abort the whole run.
pub async fn run(args: RunArgs) -> Result<Summary> {
    let RunArgs {
        trace: trace_path,
        output,
        model_id,
        api_base_url,
        api_key,
        min_confidence,
        include_reasoning,
    } = args;

    // Fail fast on bad paths so the user doesn't wait for sampler
    // setup before discovering a typo. (F31/F32)
    if !trace_path.is_file() {
        return Err(anyhow!(
            "--trace path is not a regular file: {}",
            trace_path.display()
        ));
    }
    if let Some(out) = output.as_ref()
        && let Some(parent) = out.parent()
        && !parent.as_os_str().is_empty()
        && !parent.is_dir()
    {
        return Err(anyhow!(
            "--output parent directory does not exist: {}",
            parent.display()
        ));
    }

    let trace = parse_trace_file(&trace_path)?;
    // `model_id` is needed twice (sampler config + per-turn request
    // header). The single clone here is the only unavoidable one;
    // `api_base_url` and `api_key` move straight into the sampler.
    let sampling_client =
        build_sampler_client(api_base_url, model_id.clone(), api_key.as_deref()).await?;
    let client = SamplerClassifierClient::new(sampling_client);
    let min_confidence = min_confidence.unwrap_or(LAZINESS_DEFAULT_MIN_CONFIDENCE);
    let include_reasoning = include_reasoning.unwrap_or(LAZINESS_INCLUDE_REASONING);
    run_with_client(
        &trace,
        &model_id,
        min_confidence,
        include_reasoning,
        output.as_deref(),
        &client,
    )
    .await
}

/// Construct the line-buffered sink (file or stdout) and delegate to
/// [`run_with_writer`]. Each branch is spelled out separately because
/// `StdoutLock` is `!Send`; the binary uses
/// `#[tokio::main(flavor = "current_thread")]` so this is fine.
pub async fn run_with_client(
    trace: &[TurnRecord],
    model_id: &str,
    min_confidence: f32,
    include_reasoning: bool,
    output: Option<&Path>,
    client: &dyn ClassifierClient,
) -> Result<Summary> {
    match output {
        Some(path) => {
            let file = std::fs::File::create(path)
                .with_context(|| format!("create {}", path.display()))?;
            let mut sink = std::io::LineWriter::new(file);
            run_with_writer(
                trace,
                model_id,
                min_confidence,
                include_reasoning,
                &mut sink,
                client,
            )
            .await
        }
        None => {
            let stdout = std::io::stdout();
            let mut sink = std::io::LineWriter::new(stdout.lock());
            run_with_writer(
                trace,
                model_id,
                min_confidence,
                include_reasoning,
                &mut sink,
                client,
            )
            .await
        }
    }
}

/// Drives the per-turn loop against an arbitrary writer. Factored
/// out so tests can pass a `Vec<u8>` and exercise the same
/// serialization path the stdout/file branches use. (N1)
pub async fn run_with_writer<W: std::io::Write + ?Sized>(
    trace: &[TurnRecord],
    model_id: &str,
    min_confidence: f32,
    include_reasoning: bool,
    sink: &mut W,
    client: &dyn ClassifierClient,
) -> Result<Summary> {
    let mut summary = Summary::default();
    let elapsed_per_turn = compute_turn_elapsed_seconds(trace);
    for (record, &turn_elapsed_seconds) in trace.iter().zip(elapsed_per_turn.iter()) {
        let data = process_turn(
            record,
            model_id,
            min_confidence,
            include_reasoning,
            turn_elapsed_seconds,
            client,
        )
        .await;
        let line = data.as_line();
        serde_json::to_writer(&mut *sink, &line)?;
        sink.write_all(b"\n")?;
        bump_summary(&mut summary, &line);
    }
    sink.flush()?;
    Ok(summary)
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use sampling_types::{AssistantItem, ToolCall, ToolResultItem};

    /// Tiny synthetic 4-turn fixture so the end-to-end tests can run
    /// in CI without external trace artifacts. (F16)
    ///
    /// Per-turn `afterStateHistory` is CUMULATIVE — turn_N's history
    /// contains turn_0..N's items plus turn_N's own. This mirrors the
    /// production trace serializer, which snapshots the full
    /// conversation at turn end. (N4)
    const SYNTHETIC_TRACE: &str = r#"[
        {
            "turn": "turn_0",
            "trace": {
                "metadata": {"turn_number": 0, "request_id": "req-0", "session_id": "sess-x"},
                "afterStateHistory": [
                    {"type": "system", "content": "sys"},
                    {"type": "user", "content": [{"type": "text", "text": "do thing"}]},
                    {"type": "assistant", "content": "on it",
                     "tool_calls": [{"id": "t0", "name": "todo_write",
                       "arguments": "{\"merge\":false,\"todos\":[{\"id\":\"a\",\"content\":\"step1\",\"status\":\"in_progress\"}]}"}]},
                    {"type": "tool_result", "tool_call_id": "t0", "content": "ok"}
                ]
            }
        },
        {
            "turn": "turn_1",
            "trace": {
                "metadata": {"turn_number": 1, "request_id": "req-1", "session_id": "sess-x"},
                "afterStateHistory": [
                    {"type": "system", "content": "sys"},
                    {"type": "user", "content": [{"type": "text", "text": "do thing"}]},
                    {"type": "assistant", "content": "on it",
                     "tool_calls": [{"id": "t0", "name": "todo_write",
                       "arguments": "{\"merge\":false,\"todos\":[{\"id\":\"a\",\"content\":\"step1\",\"status\":\"in_progress\"}]}"}]},
                    {"type": "tool_result", "tool_call_id": "t0", "content": "ok"},
                    {"type": "user", "content": [{"type": "text", "text": "go"}]},
                    {"type": "assistant", "content": "starting subagent",
                     "tool_calls": [{"id": "s1", "name": "spawn_subagent", "arguments": "{}"}]}
                ]
            }
        },
        {
            "turn": "turn_2",
            "trace": {
                "metadata": {"turn_number": 2, "request_id": "req-2", "session_id": "sess-x"},
                "afterStateHistory": [
                    {"type": "system", "content": "sys"},
                    {"type": "user", "content": [{"type": "text", "text": "do thing"}]},
                    {"type": "assistant", "content": "on it",
                     "tool_calls": [{"id": "t0", "name": "todo_write",
                       "arguments": "{\"merge\":false,\"todos\":[{\"id\":\"a\",\"content\":\"step1\",\"status\":\"in_progress\"}]}"}]},
                    {"type": "tool_result", "tool_call_id": "t0", "content": "ok"},
                    {"type": "user", "content": [{"type": "text", "text": "go"}]},
                    {"type": "assistant", "content": "starting subagent",
                     "tool_calls": [{"id": "s1", "name": "spawn_subagent", "arguments": "{}"}]},
                    {"type": "tool_result", "tool_call_id": "s1", "content": "subagent done"},
                    {"type": "user", "content": [{"type": "text", "text": "now"}]},
                    {"type": "assistant", "content": "backgrounding monitor",
                     "tool_calls": [{"id": "m1", "name": "monitor", "arguments": "{}"}]}
                ]
            }
        },
        {
            "turn": "turn_3",
            "trace": {
                "metadata": {"turn_number": 3, "request_id": "req-3", "session_id": "sess-x"},
                "afterStateHistory": [
                    {"type": "system", "content": "sys"},
                    {"type": "user", "content": [{"type": "text", "text": "do thing"}]},
                    {"type": "assistant", "content": "on it",
                     "tool_calls": [{"id": "t0", "name": "todo_write",
                       "arguments": "{\"merge\":false,\"todos\":[{\"id\":\"a\",\"content\":\"step1\",\"status\":\"in_progress\"}]}"}]},
                    {"type": "tool_result", "tool_call_id": "t0", "content": "ok"},
                    {"type": "user", "content": [{"type": "text", "text": "go"}]},
                    {"type": "assistant", "content": "starting subagent",
                     "tool_calls": [{"id": "s1", "name": "spawn_subagent", "arguments": "{}"}]},
                    {"type": "tool_result", "tool_call_id": "s1", "content": "subagent done"},
                    {"type": "user", "content": [{"type": "text", "text": "now"}]},
                    {"type": "assistant", "content": "backgrounding monitor",
                     "tool_calls": [{"id": "m1", "name": "monitor", "arguments": "{}"}]},
                    {"type": "tool_result", "tool_call_id": "m1", "content": "monitor stopped"},
                    {"type": "user", "content": [{"type": "text", "text": "wrap"}]},
                    {"type": "assistant", "content": "marking done",
                     "tool_calls": [{"id": "t1", "name": "todo_write",
                       "arguments": "{\"merge\":true,\"todos\":[{\"id\":\"a\",\"status\":\"completed\"}]}"}]}
                ]
            }
        }
    ]"#;

    fn parse_synthetic() -> Vec<TurnRecord> {
        serde_json::from_str(SYNTHETIC_TRACE).expect("synthetic trace parses")
    }

    fn assistant_with_tool_calls(tool_calls: Vec<ToolCall>) -> ConversationItem {
        ConversationItem::Assistant(AssistantItem {
            content: String::new().into(),
            tool_calls,
            model_id: None,
            model_fingerprint: None,
            reasoning_effort: None,
        })
    }

    fn tc(id: &str, name: &str, args: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: name.to_owned(),
            arguments: args.into(),
        }
    }

    /// 1. Synthetic trace deserialize round-trip.
    #[test]
    fn synthetic_trace_round_trips() {
        let trace = parse_synthetic();
        assert_eq!(trace.len(), 4);
        for (i, r) in trace.iter().enumerate() {
            assert_eq!(r.turn, format!("turn_{i}"));
            assert_eq!(r.trace.metadata.turn_number, Some(i as u64));
        }
        // N4: per-turn histories are cumulative (monotonically growing).
        let lens: Vec<usize> = trace
            .iter()
            .map(|r| r.trace.after_state_history.len())
            .collect();
        assert!(
            lens.windows(2).all(|w| w[0] < w[1]),
            "history is monotone-growing: {lens:?}",
        );
    }

    #[test]
    fn reference_trace_round_trips_when_available() {
        let path =
            Path::new("/root/traces/trace-019e4888-ded4-7632-9bb5-a7964974d34e-all-turns.json");
        if !path.exists() {
            eprintln!(
                "SKIP reference_trace_round_trips_when_available: {} not present",
                path.display()
            );
            return;
        }
        let trace = parse_trace_file(path).expect("parse");
        assert_eq!(trace.len(), 4);
        assert!(
            trace[2]
                .trace
                .metadata
                .request_id
                .as_deref()
                .unwrap_or("")
                .starts_with("subagent"),
            "turn_2 request_id should start with `subagent`"
        );
    }

    /// 4. End-to-end with stubbed classifier.
    struct StubClient(String);

    #[async_trait::async_trait]
    impl ClassifierClient for StubClient {
        async fn run(&self, _request: ConversationRequest) -> Result<String, String> {
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn end_to_end_synthetic_with_stub() {
        let trace = parse_synthetic();
        let stub = StubClient(
            r#"{"category":"not_stalled_complete","confidence":0.9,"evidence":"stub"}"#.to_owned(),
        );
        let tmpdir = tempfile::tempdir().expect("tempdir");
        let out = tmpdir.path().join("out.jsonl");
        let summary = run_with_client(
            &trace,
            "stub-model",
            LAZINESS_DEFAULT_MIN_CONFIDENCE,
            LAZINESS_INCLUDE_REASONING,
            Some(&out),
            &stub,
        )
        .await
        .expect("run");
        assert_eq!(summary.turns, 4);
        assert_eq!(summary.laz_not_stalled, 4);
        assert_eq!(summary.laz_aborted, 0);

        let body = std::fs::read_to_string(&out).expect("read out");
        let mut lines = 0;
        for line in body.lines() {
            let v: serde_json::Value = serde_json::from_str(line).expect("line is json");
            assert!(v.get("turn_id").is_some());
            assert!(v.get("laziness_classifier").is_some());
            assert!(v.get("classifier_backing_task_count").is_some());
            // The resolved `include_reasoning` is surfaced on every
            // per-turn record so two A/B runs can be diffed line by
            // line.
            assert_eq!(
                v.get("include_reasoning"),
                Some(&serde_json::Value::Bool(LAZINESS_INCLUDE_REASONING))
            );
            lines += 1;
        }
        assert_eq!(lines, 4);
    }

    /// N9: every non-Aborted laziness decision discriminator round-trips
    /// correctly through the JSONL.
    #[tokio::test]
    async fn end_to_end_per_category_jsonl_round_trip() {
        let trace = parse_synthetic();
        let stubs = [
            (
                LazinessDecisionKind::NoNudgeNotStalled,
                "not_stalled_complete",
                0.9_f32,
                "no_nudge_not_stalled",
            ),
            (
                LazinessDecisionKind::WouldNudge,
                "stalled_narration",
                0.9,
                "would_nudge",
            ),
            (
                LazinessDecisionKind::NoNudgeLowConfidence,
                "stalled_narration",
                0.4,
                "no_nudge_low_confidence",
            ),
        ];
        for (expected_kind, category, confidence, wire_str) in stubs {
            let stub = StubClient(format!(
                r#"{{"category":"{category}","confidence":{confidence},"evidence":"e"}}"#
            ));
            let mut buf = Vec::<u8>::new();
            run_with_writer(
                &trace,
                "stub",
                LAZINESS_DEFAULT_MIN_CONFIDENCE,
                LAZINESS_INCLUDE_REASONING,
                &mut buf,
                &stub,
            )
            .await
            .expect("run");
            for line in std::str::from_utf8(&buf).expect("utf8").lines() {
                let v: serde_json::Value = serde_json::from_str(line).expect("json");
                let dec = v
                    .pointer("/laziness_classifier/decision")
                    .and_then(|d| d.as_str())
                    .expect("decision");
                assert_eq!(dec, wire_str, "decision wire-string for {expected_kind:?}");
            }
        }
    }

    /// F22: parse-error line carries the right discriminator + detail.
    #[tokio::test]
    async fn parse_error_is_surfaced_per_turn() {
        let trace = parse_synthetic();
        let stub = StubClient("not json at all".into());
        let tmpdir = tempfile::tempdir().expect("tempdir");
        let out = tmpdir.path().join("out.jsonl");
        let summary = run_with_client(
            &trace,
            "stub-model",
            LAZINESS_DEFAULT_MIN_CONFIDENCE,
            LAZINESS_INCLUDE_REASONING,
            Some(&out),
            &stub,
        )
        .await
        .expect("run");
        assert_eq!(summary.turns, 4);
        assert_eq!(summary.laz_aborted, 4);
        let body = std::fs::read_to_string(&out).expect("read out");
        for line in body.lines() {
            let v: serde_json::Value = serde_json::from_str(line).expect("line is json");
            let laz = v.get("laziness_classifier").expect("laziness_classifier");
            assert_eq!(
                laz.get("abort_reason").and_then(|x| x.as_str()),
                Some("parse_error")
            );
            assert!(
                laz.get("error_detail")
                    .and_then(|x| x.as_str())
                    .is_some_and(|s| !s.is_empty()),
                "error_detail populated",
            );
            assert_eq!(
                laz.get("decision").and_then(|x| x.as_str()),
                Some("aborted")
            );
            assert!(laz.get("raw_output").and_then(|x| x.as_str()).is_some());
        }
    }

    // F17 — fidelity test. Capture site for the most-recent
    // `ConversationRequest`. `RefCell` would be the natural pick for
    // a single-threaded test, but the `ClassifierClient` trait is
    // `Send + Sync` (so production `SamplingClient` callers can hold
    // it via `Arc<dyn ClassifierClient>` across threads) and
    // `RefCell: !Sync`. `std::sync::Mutex<T>` is the minimal Sync
    // interior-mutability primitive — the lock is always uncontended
    // here (single-threaded `#[tokio::test]`) so the cost is one
    // atomic CAS per call.
    struct CapturingStub {
        last: std::sync::Mutex<Option<ConversationRequest>>,
        response: String,
    }

    impl CapturingStub {
        fn new(response: &str) -> Self {
            Self {
                last: std::sync::Mutex::new(None),
                response: response.to_owned(),
            }
        }
        fn take(&self) -> ConversationRequest {
            self.last
                .lock()
                .expect("mutex poisoned")
                .take()
                .expect("request captured")
        }
    }

    #[async_trait::async_trait]
    impl ClassifierClient for CapturingStub {
        async fn run(&self, request: ConversationRequest) -> Result<String, String> {
            *self.last.lock().expect("mutex poisoned") = Some(request);
            Ok(self.response.clone())
        }
    }

    #[tokio::test]
    async fn classifier_request_matches_production_shape() {
        let trace = parse_synthetic();
        let stub = CapturingStub::new(
            r#"{"category":"not_stalled_complete","confidence":0.9,"evidence":"e"}"#,
        );
        let _ = process_turn(
            &trace[0],
            "fidelity-model",
            LAZINESS_DEFAULT_MIN_CONFIDENCE,
            LAZINESS_INCLUDE_REASONING,
            None,
            &stub,
        )
        .await;
        let req = stub.take();

        // Exactly two items: System(classifier prompt) + User(transcript).
        assert_eq!(req.items.len(), 2);
        match &req.items[0] {
            // N3: assert against the shared production constant, not a
            // re-typed literal — drift on either side fails the test.
            ConversationItem::System(s) => {
                assert_eq!(s.content.as_ref(), LAZINESS_CLASSIFIER_PROMPT)
            }
            other => panic!("expected System, got {other:?}"),
        }
        let user_text = match &req.items[1] {
            ConversationItem::User(u) => {
                assert!(u.synthetic_reason.is_none());
                assert_eq!(u.content.len(), 1);
                match &u.content[0] {
                    ContentPart::Text { text } => text.clone(),
                    other => panic!("expected Text, got {other:?}"),
                }
            }
            other => panic!("expected User, got {other:?}"),
        };
        // N3: assert against the shared `LAZINESS_USER_PREAMBLE`.
        assert!(
            user_text.starts_with(LAZINESS_USER_PREAMBLE),
            "user text starts with the shared preamble const",
        );
        assert!(
            user_text.contains("=== BEGIN TRANSCRIPT ===\n[runtime_state]"),
            "user text contains runtime_state line right after BEGIN sentinel",
        );
        assert!(user_text.ends_with("=== END TRANSCRIPT ===\n"));

        assert_eq!(req.model.as_deref(), Some("fidelity-model"));
        assert!(req.temperature.is_none());
        assert!(req.max_output_tokens.is_none());
        assert!(req.reasoning_effort.is_none());
        assert!(req.tools.is_empty());
        assert!(req.tool_choice.is_none());
    }

    /// N11: the captured transcript ends with the most recent item's
    /// marker text — proves `laziness_window_start` does not drop the
    /// tail.
    #[tokio::test]
    async fn captured_transcript_ends_with_most_recent_item() {
        // Build a 50-item history with a unique marker on the LAST
        // user item.
        let mut hist: Vec<ConversationItem> = Vec::new();
        hist.push(ConversationItem::System(SystemItem {
            content: "sys".into(),
        }));
        for i in 0..24 {
            hist.push(ConversationItem::User(UserItem {
                content: vec![ContentPart::Text {
                    text: format!("u{i}").into(),
                }],
                synthetic_reason: None,
                permission_evidence: None,
                ..Default::default()
            }));
            hist.push(ConversationItem::Assistant(AssistantItem {
                content: format!("a{i}").into(),
                tool_calls: vec![],
                model_id: None,
                model_fingerprint: None,
                reasoning_effort: None,
            }));
        }
        hist.push(ConversationItem::User(UserItem {
            content: vec![ContentPart::Text {
                text: "ulast-MARKER-tail".into(),
            }],
            synthetic_reason: None,
            permission_evidence: None,
            ..Default::default()
        }));

        let record = TurnRecord {
            turn: "turn_synth".into(),
            trace: TurnTrace {
                metadata: TurnMetadata {
                    turn_number: Some(0),
                    request_id: Some("r".into()),
                    session_id: Some("s".into()),
                    turn_started_at: None,
                    turn_duration_ms: None,
                    outstanding_background_tasks: None,
                },
                after_state_history: hist,
            },
        };

        let stub = CapturingStub::new(
            r#"{"category":"not_stalled_complete","confidence":0.9,"evidence":"e"}"#,
        );
        let _ = process_turn(
            &record,
            "m",
            LAZINESS_DEFAULT_MIN_CONFIDENCE,
            LAZINESS_INCLUDE_REASONING,
            None,
            &stub,
        )
        .await;
        let req = stub.take();
        let user_text = match &req.items[1] {
            ConversationItem::User(u) => match &u.content[0] {
                ContentPart::Text { text } => text.clone(),
                _ => panic!(),
            },
            _ => panic!(),
        };
        assert!(
            user_text.contains("ulast-MARKER-tail"),
            "transcript retained the most-recent user item: {user_text}",
        );
    }

    /// F17/sub: per-model `min_confidence` override flows through.
    #[tokio::test]
    async fn min_confidence_override_is_threaded_through() {
        let trace = parse_synthetic();
        let stub = StubClient(
            r#"{"category":"stalled_narration","confidence":0.65,"evidence":"e"}"#.to_owned(),
        );
        let data_default = process_turn(
            &trace[0],
            "m",
            LAZINESS_DEFAULT_MIN_CONFIDENCE,
            LAZINESS_INCLUDE_REASONING,
            None,
            &stub,
        )
        .await;
        assert_eq!(
            data_default.laziness.decision,
            LazinessDecisionKind::NoNudgeLowConfidence,
        );
        let data_low =
            process_turn(&trace[0], "m", 0.5, LAZINESS_INCLUDE_REASONING, None, &stub).await;
        assert_eq!(data_low.laziness.decision, LazinessDecisionKind::WouldNudge);
    }

    /// The `include_reasoning` flag is threaded all the way through
    /// `process_turn` and surfaced on the per-turn JSONL line — so two
    /// A/B runs can be byte-diffed and the difference attributed to
    /// the flag.
    #[tokio::test]
    async fn include_reasoning_override_is_threaded_through_and_surfaced_on_jsonl() {
        let trace = parse_synthetic();
        let stub = StubClient(
            r#"{"category":"not_stalled_complete","confidence":0.9,"evidence":"e"}"#.to_owned(),
        );

        let data_on = process_turn(
            &trace[0],
            "m",
            LAZINESS_DEFAULT_MIN_CONFIDENCE,
            true,
            None,
            &stub,
        )
        .await;
        assert!(data_on.include_reasoning);
        let line_on = serde_json::to_value(data_on.as_line()).expect("serialize on");
        assert_eq!(
            line_on.get("include_reasoning"),
            Some(&serde_json::json!(true))
        );

        let data_off = process_turn(
            &trace[0],
            "m",
            LAZINESS_DEFAULT_MIN_CONFIDENCE,
            false,
            None,
            &stub,
        )
        .await;
        assert!(!data_off.include_reasoning);
        let line_off = serde_json::to_value(data_off.as_line()).expect("serialize off");
        assert_eq!(
            line_off.get("include_reasoning"),
            Some(&serde_json::json!(false))
        );
    }

    /// `RunArgs::include_reasoning_value` resolves the CLI flag to
    /// the harness default when absent.
    #[test]
    fn run_args_include_reasoning_value_resolves_to_harness_default_when_absent() {
        let args = RunArgs {
            trace: PathBuf::from("ignored"),
            output: None,
            model_id: "m".into(),
            api_base_url: "https://x".into(),
            api_key: None,
            min_confidence: None,
            include_reasoning: None,
        };
        assert_eq!(args.include_reasoning_value(), LAZINESS_INCLUDE_REASONING);
    }

    #[test]
    fn run_args_include_reasoning_value_honors_cli_override() {
        let args_off = RunArgs {
            trace: PathBuf::from("ignored"),
            output: None,
            model_id: "m".into(),
            api_base_url: "https://x".into(),
            api_key: None,
            min_confidence: None,
            include_reasoning: Some(false),
        };
        assert!(!args_off.include_reasoning_value());

        let args_on = RunArgs {
            trace: PathBuf::from("ignored"),
            output: None,
            model_id: "m".into(),
            api_base_url: "https://x".into(),
            api_key: None,
            min_confidence: None,
            include_reasoning: Some(true),
        };
        assert!(args_on.include_reasoning_value());
    }

    /// N1: stdout-vs-file branches actually exercise the same writer
    /// code path. Both go through `run_with_writer`; the file branch
    /// hands it a `LineWriter<File>`, the test hands it a `Vec<u8>`.
    /// Byte-equal after normalising `elapsed_ms`.
    #[tokio::test]
    async fn stdout_and_file_branches_byte_equal() {
        let trace = parse_synthetic();
        let stub = StubClient(
            r#"{"category":"not_stalled_complete","confidence":0.9,"evidence":"stub"}"#.to_owned(),
        );

        let tmpdir = tempfile::tempdir().expect("tempdir");
        let path = tmpdir.path().join("a.jsonl");
        run_with_client(
            &trace,
            "stub",
            LAZINESS_DEFAULT_MIN_CONFIDENCE,
            LAZINESS_INCLUDE_REASONING,
            Some(&path),
            &stub,
        )
        .await
        .expect("file");
        let file_body = std::fs::read_to_string(&path).expect("read");

        // Exercise the SAME loop body via `run_with_writer` with a
        // Vec<u8> sink — the same path the stdout branch takes.
        let mut buf = Vec::<u8>::new();
        run_with_writer(
            &trace,
            "stub",
            LAZINESS_DEFAULT_MIN_CONFIDENCE,
            LAZINESS_INCLUDE_REASONING,
            &mut buf,
            &stub,
        )
        .await
        .expect("buf");
        let buf_body = String::from_utf8(buf).expect("utf8");

        fn strip_elapsed(s: &str) -> String {
            let re = regex::Regex::new(r#""elapsed_ms":\d+"#).expect("regex");
            re.replace_all(s, r#""elapsed_ms":0"#).into_owned()
        }
        assert_eq!(strip_elapsed(&file_body), strip_elapsed(&buf_body));
    }

    /// BYOK precedence is explicit flag, then provider-neutral environment.
    #[tokio::test]
    #[serial_test::serial]
    async fn resolve_api_key_precedence() {
        let saved = std::env::var("LLM_API_KEY").ok();
        unsafe { std::env::remove_var("LLM_API_KEY") };

        assert!(resolve_api_key(None).await.is_err());
        assert!(resolve_api_key(Some("")).await.is_err());
        assert!(resolve_api_key(Some("   ")).await.is_err());
        assert_eq!(
            resolve_api_key(Some("from-flag")).await.expect("flag key"),
            "from-flag"
        );

        unsafe { std::env::set_var("LLM_API_KEY", "from-env") };
        assert_eq!(resolve_api_key(None).await.expect("env key"), "from-env");
        assert_eq!(
            resolve_api_key(Some("from-flag")).await.expect("flag key"),
            "from-flag"
        );
        assert_eq!(
            resolve_api_key(Some(" ")).await.expect("env key"),
            "from-env"
        );

        unsafe { std::env::set_var("LLM_API_KEY", "   ") };
        assert!(resolve_api_key(None).await.is_err());

        match saved {
            Some(value) => unsafe { std::env::set_var("LLM_API_KEY", value) },
            None => unsafe { std::env::remove_var("LLM_API_KEY") },
        }
    }

    /// F21: pin the operator-visible summary string.
    #[test]
    fn summary_render_format_is_pinned() {
        let s = Summary {
            turns: 4,
            laz_would_nudge: 0,
            laz_not_stalled: 2,
            laz_low_confidence: 1,
            laz_aborted: 1,
        };
        assert_eq!(
            s.render(),
            "Processed 4 turns. Laziness: 2 NoNudge-NotStalled, 1 NoNudge-LowConfidence, 0 WouldNudge, 1 Aborted."
        );
    }

    /// F25: synthetic 50-item history is trimmed by `window_start`.
    #[tokio::test]
    async fn laziness_window_trim_is_applied() {
        let mut hist: Vec<ConversationItem> = Vec::new();
        hist.push(ConversationItem::System(SystemItem {
            content: "sys".into(),
        }));
        for i in 0..24 {
            hist.push(ConversationItem::User(UserItem {
                content: vec![ContentPart::Text {
                    text: format!("u{i}").into(),
                }],
                synthetic_reason: None,
                permission_evidence: None,
                ..Default::default()
            }));
            hist.push(ConversationItem::Assistant(AssistantItem {
                content: format!("a{i}").into(),
                tool_calls: vec![],
                model_id: None,
                model_fingerprint: None,
                reasoning_effort: None,
            }));
        }
        hist.push(ConversationItem::User(UserItem {
            content: vec![ContentPart::Text {
                text: "ulast".into(),
            }],
            synthetic_reason: None,
            permission_evidence: None,
            ..Default::default()
        }));

        let record = TurnRecord {
            turn: "turn_synth".into(),
            trace: TurnTrace {
                metadata: TurnMetadata {
                    turn_number: Some(0),
                    request_id: Some("r".into()),
                    session_id: Some("s".into()),
                    turn_started_at: None,
                    turn_duration_ms: None,
                    outstanding_background_tasks: None,
                },
                after_state_history: hist,
            },
        };

        let stub = StubClient(
            r#"{"category":"not_stalled_complete","confidence":0.9,"evidence":"stub"}"#.to_owned(),
        );
        let data = process_turn(
            &record,
            "m",
            LAZINESS_DEFAULT_MIN_CONFIDENCE,
            LAZINESS_INCLUDE_REASONING,
            None,
            &stub,
        )
        .await;
        assert_eq!(data.items_in_history, 50);
        assert!(data.items_after_window_trim <= data.items_in_history);
        assert!(
            data.items_after_window_trim >= LAZINESS_CONTEXT_ITEM_LIMIT,
            "trimmed window respects min-user/min-assistant minimums",
        );
    }

    /// Every classifier decision is counted in the right bucket.
    #[test]
    fn bump_summary_buckets_every_decision() {
        let lazs = [
            LazinessDecisionKind::WouldNudge,
            LazinessDecisionKind::NoNudgeNotStalled,
            LazinessDecisionKind::NoNudgeLowConfidence,
            LazinessDecisionKind::Aborted,
        ];

        for l in lazs {
            let mut s = Summary::default();
            let line = TurnLine {
                turn_id: "t",
                turn_number: None,
                request_id: None,
                items_in_history: 0,
                items_after_window_trim: 0,
                classifier_backing_task_count: None,
                laziness_classifier: LazinessOut {
                    model_id: "m",
                    elapsed_ms: 0,
                    parsed: None,
                    decision: l,
                    abort_reason: None,
                    error_detail: None,
                    raw_output: None,
                },
                include_reasoning: LAZINESS_INCLUDE_REASONING,
                turn_elapsed_seconds: None,
            };
            bump_summary(&mut s, &line);
            let laz_bucket = match l {
                LazinessDecisionKind::WouldNudge => s.laz_would_nudge,
                LazinessDecisionKind::NoNudgeNotStalled => s.laz_not_stalled,
                LazinessDecisionKind::NoNudgeLowConfidence => s.laz_low_confidence,
                LazinessDecisionKind::Aborted => s.laz_aborted,
            };
            assert_eq!(laz_bucket, 1, "laz {l:?} bucket");
            assert_eq!(s.turns, 1);
        }
    }

    #[test]
    fn decision_kind_is_exhaustive() {
        assert_eq!(
            decision_kind(DebugDecision::WouldNudge),
            LazinessDecisionKind::WouldNudge
        );
        assert_eq!(
            decision_kind(DebugDecision::NoNudgeNotStalled),
            LazinessDecisionKind::NoNudgeNotStalled
        );
        assert_eq!(
            decision_kind(DebugDecision::NoNudgeLowConfidence),
            LazinessDecisionKind::NoNudgeLowConfidence
        );
        assert_eq!(
            decision_kind(DebugDecision::Aborted),
            LazinessDecisionKind::Aborted
        );
        assert_eq!(
            decision_kind(DebugDecision::SuppressedNotGoalMode),
            LazinessDecisionKind::WouldNudge
        );
    }

    /// N5: clap value-parser rejects out-of-range / non-finite floats.
    #[test]
    fn validate_min_confidence_rejects_bad_values() {
        assert!(validate_min_confidence("0.0").is_ok());
        assert!(validate_min_confidence("1.0").is_ok());
        assert!(validate_min_confidence("0.5").is_ok());
        assert!(validate_min_confidence("-0.1").is_err());
        assert!(validate_min_confidence("1.1").is_err());
        assert!(validate_min_confidence("nan").is_err());
        assert!(validate_min_confidence("inf").is_err());
        assert!(validate_min_confidence("not-a-float").is_err());
    }

    /// N7: `parse_trace_file` enforces a size bound, and known-good
    /// inputs under the bound parse cleanly. We don't allocate 256 MB
    /// on disk just to exercise the rejection arm — the const value
    /// is read directly and the parse path is exercised on the
    /// in-tree synthetic.
    #[test]
    fn parse_trace_file_round_trips_under_bound() {
        const _BOUND_SANITY: () = assert!(MAX_TRACE_FILE_BYTES > 1024);
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("ok.json");
        std::fs::write(&path, SYNTHETIC_TRACE).expect("write");
        let parsed = parse_trace_file(&path).expect("parse");
        assert_eq!(parsed.len(), 4);
    }

    /// N10: ordering-independent failure path — when both `--trace`
    /// is missing AND `--output` parent is missing AND `--api-key` is
    /// absent (with env unset), the error message names the FIRST
    /// failure, which is `--trace`.
    #[tokio::test]
    #[serial_test::serial]
    async fn run_rejects_missing_trace_path() {
        let prev = std::env::var("GROW_API_KEY").ok();
        unsafe { std::env::remove_var("GROW_API_KEY") };
        let tmp = tempfile::tempdir().expect("tempdir");
        let args = RunArgs {
            trace: PathBuf::from("/definitely/does/not/exist.json"),
            output: None,
            model_id: "m".into(),
            api_base_url: "https://x".into(),
            // The trace check runs before credential resolution.
            api_key: None,
            min_confidence: None,
            include_reasoning: None,
        };
        let err = run(args).await.expect_err("missing trace");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("--trace path is not a regular file"),
            "msg: {msg}"
        );
        match prev {
            Some(v) => unsafe { std::env::set_var("GROW_API_KEY", v) },
            None => unsafe { std::env::remove_var("GROW_API_KEY") },
        }
    }

    fn make_record(turn_id: &str, turn_started_at: Option<&str>) -> TurnRecord {
        TurnRecord {
            turn: turn_id.to_owned(),
            trace: TurnTrace {
                metadata: TurnMetadata {
                    turn_number: None,
                    request_id: None,
                    session_id: None,
                    turn_started_at: turn_started_at.map(str::to_owned),
                    turn_duration_ms: None,
                    outstanding_background_tasks: None,
                },
                after_state_history: vec![],
            },
        }
    }

    #[test]
    fn replay_uses_only_measured_turn_duration() {
        let mut trace = vec![
            make_record("first", Some("2026-09-06T00:00:00Z")),
            make_record("last", Some("2026-09-07T00:00:00Z")),
        ];
        assert_eq!(compute_turn_elapsed_seconds(&trace), vec![None, None]);
        trace[0].trace.metadata.turn_duration_ms = Some(60_999);
        trace[1].trace.metadata.turn_duration_ms = Some(0);
        assert_eq!(
            compute_turn_elapsed_seconds(&trace),
            vec![Some(60), Some(0)]
        );
    }

    #[tokio::test]
    async fn background_start_receipt_does_not_fabricate_a_zero_live_count() {
        let mut record = make_record("background", None);
        record.trace.after_state_history = vec![
            assistant_with_tool_calls(vec![tc(
                "call-bg",
                "run_terminal_command",
                r#"{"is_background":true}"#,
            )]),
            ConversationItem::ToolResult(ToolResultItem {
                tool_call_id: "call-bg".into(),
                content: "backgrounded, task_id=task-123".into(),
                images: vec![],
            }),
        ];
        for measured in [None, Some(0), Some(1)] {
            record.trace.metadata.outstanding_background_tasks = measured;
            let stub = CapturingStub::new(
                r#"{"category":"not_stalled_complete","confidence":0.9,"evidence":"e"}"#,
            );
            let result = process_turn(
                &record,
                "m",
                LAZINESS_DEFAULT_MIN_CONFIDENCE,
                false,
                None,
                &stub,
            )
            .await;
            assert_eq!(result.classifier_backing_task_count, measured);
            let request = stub.take();
            let text = request.items[1].text_content();
            let runtime = text
                .lines()
                .find(|line| line.starts_with("[runtime_state]"))
                .unwrap();
            match measured {
                Some(value) => assert_eq!(
                    runtime,
                    format!("[runtime_state] outstanding_background_tasks={value}")
                ),
                None => assert_eq!(runtime, "[runtime_state]"),
            }
        }
    }

    #[tokio::test]
    async fn runtime_state_line_includes_turn_elapsed_seconds_when_present() {
        let trace = parse_synthetic();
        let stub = CapturingStub::new(
            r#"{"category":"not_stalled_complete","confidence":0.9,"evidence":"e"}"#,
        );
        let _ = process_turn(
            &trace[0],
            "m",
            LAZINESS_DEFAULT_MIN_CONFIDENCE,
            LAZINESS_INCLUDE_REASONING,
            Some(629),
            &stub,
        )
        .await;
        let req = stub.take();
        let user_text = match &req.items[1] {
            ConversationItem::User(u) => match &u.content[0] {
                ContentPart::Text { text } => text.clone(),
                _ => panic!(),
            },
            _ => panic!(),
        };
        assert!(
            user_text.contains("[runtime_state] turn_elapsed_seconds=629\n"),
            "runtime_state line carries turn_elapsed_seconds: {user_text}",
        );
    }

    #[tokio::test]
    async fn runtime_state_line_omits_turn_elapsed_seconds_when_absent() {
        let trace = parse_synthetic();
        let stub = CapturingStub::new(
            r#"{"category":"not_stalled_complete","confidence":0.9,"evidence":"e"}"#,
        );
        let _ = process_turn(
            &trace[0],
            "m",
            LAZINESS_DEFAULT_MIN_CONFIDENCE,
            LAZINESS_INCLUDE_REASONING,
            None,
            &stub,
        )
        .await;
        let req = stub.take();
        let user_text = match &req.items[1] {
            ConversationItem::User(u) => match &u.content[0] {
                ContentPart::Text { text } => text.clone(),
                _ => panic!(),
            },
            _ => panic!(),
        };
        // Constrain the negative assertion to the runtime_state line
        // slice — a future refactor that inlines any prompt language
        // mentioning `turn_elapsed_seconds` into the user wrapper
        // won't cause this test to fire for the wrong reason.
        let begin = "=== BEGIN TRANSCRIPT ===\n";
        let begin_pos = user_text.find(begin).expect("BEGIN sentinel present");
        let after_begin = &user_text[begin_pos + begin.len()..];
        let runtime_state_line = after_begin
            .split_once('\n')
            .map(|(line, _)| line)
            .expect("runtime_state line terminated with newline");
        assert_eq!(
            runtime_state_line, "[runtime_state]",
            "runtime_state line omits turn_elapsed_seconds when absent",
        );
        assert!(
            !runtime_state_line.contains("turn_elapsed_seconds"),
            "no stray turn_elapsed_seconds key when None: {runtime_state_line}",
        );
    }

    /// JSONL carries only explicit measurements, independent of next-turn time.
    #[tokio::test]
    async fn end_to_end_jsonl_carries_turn_elapsed_seconds_field() {
        let trace = vec![
            TurnRecord {
                turn: "turn_0".into(),
                trace: TurnTrace {
                    metadata: TurnMetadata {
                        turn_number: Some(0),
                        request_id: Some("r0".into()),
                        session_id: Some("s".into()),
                        turn_started_at: Some("2026-05-21T03:29:30+00:00".into()),
                        turn_duration_ms: Some(60_000),
                        outstanding_background_tasks: None,
                    },
                    after_state_history: vec![ConversationItem::User(UserItem {
                        content: vec![ContentPart::Text { text: "hi".into() }],
                        synthetic_reason: None,
                        permission_evidence: None,
                        ..Default::default()
                    })],
                },
            },
            TurnRecord {
                turn: "turn_1".into(),
                trace: TurnTrace {
                    metadata: TurnMetadata {
                        turn_number: Some(1),
                        request_id: Some("r1".into()),
                        session_id: Some("s".into()),
                        turn_started_at: Some("2026-05-21T03:30:30+00:00".into()),
                        turn_duration_ms: None,
                        outstanding_background_tasks: None,
                    },
                    after_state_history: vec![ConversationItem::User(UserItem {
                        content: vec![ContentPart::Text { text: "hi".into() }],
                        synthetic_reason: None,
                        permission_evidence: None,
                        ..Default::default()
                    })],
                },
            },
        ];
        let stub = StubClient(
            r#"{"category":"not_stalled_complete","confidence":0.9,"evidence":"e"}"#.to_owned(),
        );
        let mut buf = Vec::<u8>::new();
        run_with_writer(
            &trace,
            "m",
            LAZINESS_DEFAULT_MIN_CONFIDENCE,
            LAZINESS_INCLUDE_REASONING,
            &mut buf,
            &stub,
        )
        .await
        .expect("run");
        let body = String::from_utf8(buf).expect("utf8");
        let mut lines = body.lines();
        let first: serde_json::Value =
            serde_json::from_str(lines.next().expect("first line")).expect("json");
        let second: serde_json::Value =
            serde_json::from_str(lines.next().expect("second line")).expect("json");
        assert_eq!(
            first.get("turn_elapsed_seconds"),
            Some(&serde_json::json!(60))
        );
        assert_eq!(
            second.get("turn_elapsed_seconds"),
            Some(&serde_json::Value::Null)
        );
    }

    /// End-to-end against the reference trace (skipped when absent):
    /// No execution duration is inferred from old start-only metadata. The
    /// stub forces `stalled_false_completion` to confirm the
    /// JSONL surfaces the new category cleanly.
    #[tokio::test]
    async fn reference_trace_turn_1_carries_elapsed_and_new_category() {
        let path =
            Path::new("/root/traces/trace-019e4888-ded4-7632-9bb5-a7964974d34e-all-turns.json");
        if !path.exists() {
            eprintln!(
                "SKIP reference_trace_turn_1_carries_elapsed_and_new_category: {} not present",
                path.display(),
            );
            return;
        }
        let trace = parse_trace_file(path).expect("parse reference trace");
        let stub = StubClient(
            r#"{"category":"stalled_false_completion","confidence":0.9,"evidence":"unbacked completion claims in final message"}"#
                .to_owned(),
        );
        let mut buf = Vec::<u8>::new();
        run_with_writer(
            &trace,
            "stub-model",
            LAZINESS_DEFAULT_MIN_CONFIDENCE,
            LAZINESS_INCLUDE_REASONING,
            &mut buf,
            &stub,
        )
        .await
        .expect("run");
        let body = String::from_utf8(buf).expect("utf8");
        let lines: Vec<serde_json::Value> = body
            .lines()
            .map(|l| serde_json::from_str(l).expect("json"))
            .collect();
        assert_eq!(lines.len(), trace.len());
        let turn_1 = &lines[1];
        assert_eq!(
            turn_1.get("turn_elapsed_seconds"),
            Some(&serde_json::Value::Null)
        );
        // The classifier's stubbed verdict must be surfaced in the
        // parsed-category field on the JSONL line.
        let category = turn_1
            .pointer("/laziness_classifier/parsed/category")
            .and_then(|v| v.as_str())
            .expect("category present");
        assert_eq!(category, "stalled_false_completion");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn run_rejects_missing_output_parent() {
        let prev = std::env::var("GROW_API_KEY").ok();
        unsafe { std::env::remove_var("GROW_API_KEY") };
        let tmp = tempfile::tempdir().expect("tempdir");
        let trace_path = tmp.path().join("trace.json");
        std::fs::write(&trace_path, "[]").expect("write");
        let args = RunArgs {
            trace: trace_path,
            output: Some(PathBuf::from("/definitely/does/not/exist/out.jsonl")),
            model_id: "m".into(),
            api_base_url: "https://x".into(),
            api_key: None,
            min_confidence: None,
            include_reasoning: None,
        };
        let err = run(args).await.expect_err("bad parent");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("--output parent directory does not exist"),
            "msg: {msg}"
        );
        match prev {
            Some(v) => unsafe { std::env::set_var("GROW_API_KEY", v) },
            None => unsafe { std::env::remove_var("GROW_API_KEY") },
        }
    }
}
