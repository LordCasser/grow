//! Local-only Trajectory query server.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{BufRead, Read, Seek, Write};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware;
use axum::response::{Html, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

const MAX_TRAJECTORY_DEPTH: usize = 32;
const MAX_TRAJECTORY_ENTITIES: usize = 512;
const MAX_TRAJECTORY_FILES: usize = 1_536;
const MAX_TRAJECTORY_SOURCE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_TRAJECTORY_EVENTS: usize = 250_000;
const DEFAULT_TRAJECTORY_PAGE_ROWS: usize = 240;
const MAX_TRAJECTORY_PAGE_ROWS: usize = 1_000;
const TRAJECTORY_OVERVIEW_BINS: usize = 180;
const TRAJECTORY_SUMMARY_CHARS: usize = 320;
const TRAJECTORY_WIRE_FIELD_CHARS: usize = 512;
const TRAJECTORY_DETAIL_PREVIEW_CHARS: usize = 200_000;
const TRAJECTORY_DETAIL_PREVIEW_NODES: usize = 4_000;
const TRAJECTORY_DETAIL_PREVIEW_DEPTH: usize = 10;
const TRAJECTORY_DETAIL_PREVIEW_ITEMS: usize = 80;
const MAX_TRAJECTORY_FULL_DETAIL_BYTES: usize = 4 * 1024 * 1024;
const LEDGER_TAIL_CHECK_BYTES: u64 = 64 * 1024;

#[derive(Default)]
struct TrajectoryReadBudget {
    entities: usize,
    files: usize,
    source_bytes: u64,
    events: usize,
}

impl TrajectoryReadBudget {
    fn enter_entity(&mut self, description: &str, depth: usize) -> anyhow::Result<()> {
        if depth > MAX_TRAJECTORY_DEPTH {
            anyhow::bail!("Trajectory exceeds the nesting depth limit at {description}");
        }
        self.entities = self.entities.saturating_add(1);
        if self.entities > MAX_TRAJECTORY_ENTITIES {
            anyhow::bail!("Trajectory exceeds the entity limit");
        }
        Ok(())
    }

    fn admit_file(&mut self, file: &std::fs::File, description: &str) -> anyhow::Result<()> {
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            anyhow::bail!("Trajectory {description} is not a regular file");
        }
        self.files = self.files.saturating_add(1);
        if self.files > MAX_TRAJECTORY_FILES {
            anyhow::bail!("Trajectory exceeds the source-file limit");
        }
        self.source_bytes = self
            .source_bytes
            .checked_add(metadata.len())
            .ok_or_else(|| anyhow::anyhow!("Trajectory source-byte count overflowed"))?;
        if self.source_bytes > MAX_TRAJECTORY_SOURCE_BYTES {
            anyhow::bail!("Trajectory exceeds the source-byte limit");
        }
        Ok(())
    }

    fn remaining_events(&self) -> usize {
        MAX_TRAJECTORY_EVENTS.saturating_sub(self.events)
    }

    fn admit_events(&mut self, count: usize) -> anyhow::Result<()> {
        self.events = self
            .events
            .checked_add(count)
            .ok_or_else(|| anyhow::anyhow!("Trajectory event count overflowed"))?;
        if self.events > MAX_TRAJECTORY_EVENTS {
            anyhow::bail!("Trajectory exceeds the event limit");
        }
        Ok(())
    }
}

#[derive(Clone)]
struct AppState {
    session_id: String,
    actor_ref: String,
    #[cfg(test)]
    session_dir: PathBuf,
    #[cfg(not(test))]
    storage: super::storage::jsonl::JsonlStorageAdapter,
    #[cfg(not(test))]
    session: super::storage::jsonl::OpenedSession,
    #[cfg(test)]
    sessions_root: PathBuf,
    cache: Arc<Mutex<SessionTrajectoryCache>>,
}

#[derive(Default)]
struct SessionTrajectoryCache {
    session_dir: PathBuf,
    offset: u64,
    prefix_hasher: Option<blake3::Hasher>,
    tail_hash: Option<[u8; 32]>,
    source_stamp: Option<LedgerStamp>,
    timeline: chat_state::Timeline,
    projector: chat_state::TrajectoryProjector,
    sidebands: BTreeMap<String, SidebandCache>,
    workflows: BTreeMap<String, WorkflowJournalCache>,
    children: BTreeMap<String, SessionTrajectoryCache>,
    materialized: Option<MaterializedTrajectory>,
    last_query: Option<(TrajectoryQuery, TrajectoryResponse)>,
    arrival_order: BTreeMap<String, u64>,
    next_arrival: u64,
    materialization_reset: bool,
    #[cfg(test)]
    full_materialization_count: usize,
}

#[derive(Default)]
struct SidebandCache {
    offset: u64,
    prefix_hasher: Option<blake3::Hasher>,
    tail_hash: Option<[u8; 32]>,
    source_stamp: Option<LedgerStamp>,
    timeline: Option<chat_state::SidebandTimeline>,
    dirty_seqs: BTreeSet<u64>,
    materialization_reset: bool,
}

#[derive(Default)]
struct WorkflowJournalCache {
    offset: u64,
    projection: workflow::Journal,
    prefix_hasher: Option<blake3::Hasher>,
    tail_hash: Option<[u8; 32]>,
    source_stamp: Option<LedgerStamp>,
    dirty_seqs: BTreeSet<u64>,
    materialization_reset: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct LedgerStamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
    change_marker: [u64; 4],
}

impl LedgerStamp {
    fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            change_marker: ledger_change_marker(metadata),
        }
    }

    fn same_file_as(self, other: Self) -> bool {
        ledger_same_file(self.change_marker, other.change_marker)
    }
}

#[cfg(unix)]
fn ledger_same_file(left: [u64; 4], right: [u64; 4]) -> bool {
    left[..2] == right[..2]
}

#[cfg(windows)]
fn ledger_same_file(left: [u64; 4], right: [u64; 4]) -> bool {
    left[0] == right[0] && left[3] == right[3]
}

#[cfg(not(any(unix, windows)))]
fn ledger_same_file(_left: [u64; 4], _right: [u64; 4]) -> bool {
    false
}

#[cfg(unix)]
fn ledger_change_marker(metadata: &std::fs::Metadata) -> [u64; 4] {
    use std::os::unix::fs::MetadataExt as _;
    [
        metadata.dev(),
        metadata.ino(),
        metadata.ctime() as u64,
        metadata.ctime_nsec() as u64,
    ]
}

#[cfg(windows)]
fn ledger_change_marker(metadata: &std::fs::Metadata) -> [u64; 4] {
    use std::os::windows::fs::MetadataExt as _;
    [
        metadata.creation_time(),
        metadata.last_access_time(),
        metadata.last_write_time(),
        metadata.file_attributes() as u64,
    ]
}

#[cfg(not(any(unix, windows)))]
fn ledger_change_marker(_metadata: &std::fs::Metadata) -> [u64; 4] {
    [0; 4]
}

struct MaterializedTrajectory {
    revision: [u8; 32],
    rows: Vec<chat_state::TrajectoryRow>,
    positions: HashMap<String, usize>,
    source_ids: BTreeSet<String>,
    source_contexts: BTreeMap<String, TrajectorySourceContext>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TrajectorySourceContext {
    actor_ref: String,
    parent_entry_id: Option<String>,
    path_prefix: Vec<u64>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
struct TrajectoryQuery {
    after: Option<String>,
    before: Option<String>,
    entry: Option<String>,
    layer: Option<String>,
    actor: Option<String>,
    class: Option<String>,
    producer: Option<String>,
    visibility: Option<String>,
    search: Option<String>,
    overview_by: Option<TrajectoryOverviewDimension>,
    limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TrajectoryOverviewDimension {
    #[default]
    Interaction,
    Layer,
    Actor,
    Class,
    Producer,
}

#[derive(Debug, Deserialize)]
struct TrajectoryEventQuery {
    entry: String,
    #[serde(default)]
    full: bool,
}

#[derive(Debug)]
struct TrajectoryEntryNotFound(String);

impl std::fmt::Display for TrajectoryEntryNotFound {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Trajectory entry '{}' was not found", self.0)
    }
}

impl std::error::Error for TrajectoryEntryNotFound {}

#[derive(Debug)]
struct TrajectoryEventTooLarge;

impl std::fmt::Display for TrajectoryEventTooLarge {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Canonical event exceeds the 4 MiB in-browser detail limit. Export the matching entry from timeline.jsonl with an offline tool instead."
        )
    }
}

impl std::error::Error for TrajectoryEventTooLarge {}

/// Compact wire row for list and overview navigation.
///
/// Canonical payloads deliberately live behind the exact-event endpoint so a
/// long session cannot make every polling response proportional to its stored
/// JSON values.
#[derive(Debug, Clone, Serialize)]
struct TrajectoryRowSummary {
    entry_id: String,
    ordinal: usize,
    seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_entry_id: Option<String>,
    nesting_path: Vec<u64>,
    at_ms: i64,
    layer: String,
    actor: String,
    class: String,
    producer: String,
    kind: String,
    state: String,
    visibility: chat_state::SurfaceVisibility,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    step_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
    summary: String,
}

impl From<&chat_state::TrajectoryRow> for TrajectoryRowSummary {
    fn from(row: &chat_state::TrajectoryRow) -> Self {
        Self {
            entry_id: row.entry_id.clone(),
            ordinal: 0,
            seq: row.seq,
            parent_entry_id: row.parent_entry_id.clone(),
            nesting_path: row.nesting_path.clone(),
            at_ms: row.at_ms,
            layer: trajectory_wire_text(&row.layer),
            actor: trajectory_wire_text(&row.actor),
            class: trajectory_wire_text(&row.class),
            producer: trajectory_wire_text(&row.producer),
            kind: trajectory_wire_text(&row.kind),
            state: trajectory_wire_text(&row.state),
            visibility: row.visibility,
            turn_id: row.turn_id.as_deref().map(trajectory_wire_text),
            step_index: row.step_index,
            correlation_id: row.correlation_id.as_deref().map(trajectory_wire_text),
            duration_ms: row.duration_ms,
            summary: crate::util::truncate(&row.summary, TRAJECTORY_SUMMARY_CHARS).to_owned(),
        }
    }
}

fn trajectory_wire_text(value: &str) -> String {
    crate::util::truncate(value, TRAJECTORY_WIRE_FIELD_CHARS).to_owned()
}

impl TrajectoryRowSummary {
    fn into_row(self, details: serde_json::Value) -> chat_state::TrajectoryRow {
        chat_state::TrajectoryRow {
            entry_id: self.entry_id,
            seq: self.seq,
            parent_entry_id: self.parent_entry_id,
            nesting_path: self.nesting_path,
            at_ms: self.at_ms,
            layer: self.layer,
            actor: self.actor,
            class: self.class,
            producer: self.producer,
            kind: self.kind,
            state: self.state,
            visibility: self.visibility,
            turn_id: self.turn_id,
            step_index: self.step_index,
            correlation_id: self.correlation_id,
            duration_ms: self.duration_ms,
            summary: self.summary,
            details,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
struct TrajectoryOverviewBin {
    first_entry_id: Option<String>,
    last_entry_id: Option<String>,
    start_ms: i64,
    end_ms: i64,
    counts: BTreeMap<String, usize>,
    failures: usize,
    turns: usize,
    steps: usize,
    max_duration_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
struct TrajectoryOverview {
    dimension: TrajectoryOverviewDimension,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
    counts: BTreeMap<String, usize>,
    bins: Vec<TrajectoryOverviewBin>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrajectoryResponse {
    session_id: String,
    schema_version: u8,
    event_count: usize,
    current_surface_items: usize,
    active_turn: Option<String>,
    active_step: Option<u32>,
    open_request_count: usize,
    open_tool_count: usize,
    open_workflow_count: usize,
    matching_count: usize,
    first_cursor: Option<String>,
    last_cursor: Option<String>,
    has_earlier: bool,
    has_later: bool,
    overview: TrajectoryOverview,
    rows: Vec<TrajectoryRowSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrajectoryEventResponse {
    session_id: String,
    schema_version: u8,
    row: chat_state::TrajectoryRow,
    details_truncated: bool,
}

/// Bind the local server, report the exact URL, then serve until interrupted.
pub async fn serve(
    session_id: &str,
    bind: SocketAddr,
    on_ready: impl FnOnce(&str, &str),
) -> anyhow::Result<()> {
    if !bind.ip().is_loopback() {
        anyhow::bail!("Trajectory server only accepts loopback bind addresses, got {bind}");
    }
    let storage = super::storage::jsonl::JsonlStorageAdapter::new();
    let session = storage
        .open_session_by_id(session_id)?
        .ok_or_else(|| anyhow::anyhow!("session '{session_id}' was not found"))?;
    let canonical_session_id = session.summary().info.id.to_string();
    let session_dir = session.directory().display_path().to_path_buf();
    session
        .directory()
        .open_regular(
            std::ffi::OsStr::new(super::storage::TIMELINE_FILE),
            "Trajectory Timeline ledger",
        )
        .map_err(|error| {
            anyhow::anyhow!(
                "session '{}' has no readable Timeline v{} ledger at {}: {error}",
                canonical_session_id,
                chat_state::TIMELINE_SCHEMA_VERSION,
                session_dir.join(super::storage::TIMELINE_FILE).display()
            )
        })?;

    let listener = tokio::net::TcpListener::bind(bind).await?;
    let local = listener.local_addr()?;
    let token = uuid::Uuid::now_v7().simple().to_string();
    let actor_ref = match session.summary().session_kind.as_deref() {
        Some(kind) if kind.starts_with("subagent") => {
            format!("subagent:{canonical_session_id}")
        }
        _ => "main".into(),
    };
    let state = AppState {
        session_id: canonical_session_id.clone(),
        actor_ref,
        #[cfg(test)]
        session_dir: session_dir.clone(),
        #[cfg(not(test))]
        storage,
        #[cfg(not(test))]
        session,
        #[cfg(test)]
        sessions_root: crate::util::grow_home::grow_home().join("sessions"),
        cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
    };
    let app = trajectory_router(&token, state);
    let url = format!("http://{local}/{token}/");
    on_ready(&canonical_session_id, &url);
    axum::serve(listener, app).await?;
    Ok(())
}

fn trajectory_router(token: &str, state: AppState) -> Router {
    let root = format!("/{token}");
    Router::new()
        .route(&root, get(index))
        .route(&format!("{root}/"), get(index))
        .route(&format!("{root}/api/trajectory"), get(query_trajectory))
        .route(
            &format!("{root}/api/trajectory/event"),
            get(query_trajectory_event),
        )
        .with_state(state)
        .layer(middleware::map_response(add_security_headers))
}

async fn add_security_headers(mut response: Response) -> Response {
    response.headers_mut().extend(response_security_headers());
    response
}

async fn index(headers: HeaderMap) -> Result<(HeaderMap, Html<&'static str>), HttpError> {
    require_local_host(&headers)?;
    Ok((response_security_headers(), Html(PAGE)))
}

async fn query_trajectory(
    State(state): State<AppState>,
    Query(query): Query<TrajectoryQuery>,
    headers: HeaderMap,
) -> Result<(HeaderMap, Json<TrajectoryResponse>), HttpError> {
    require_local_host(&headers)?;
    if query.after.is_some() as u8 + query.before.is_some() as u8 + query.entry.is_some() as u8 > 1
    {
        return Err(http_error(
            StatusCode::BAD_REQUEST,
            "after, before, and entry are mutually exclusive",
        ));
    }
    let response = tokio::task::spawn_blocking(move || query_cached(&state, query))
        .await
        .map_err(internal_error)?
        .map_err(query_error_response)?;
    Ok((response_security_headers(), Json(response)))
}

async fn query_trajectory_event(
    State(state): State<AppState>,
    Query(query): Query<TrajectoryEventQuery>,
    headers: HeaderMap,
) -> Result<(HeaderMap, Json<TrajectoryEventResponse>), HttpError> {
    require_local_host(&headers)?;
    if query.entry.trim().is_empty() {
        return Err(http_error(
            StatusCode::BAD_REQUEST,
            "entry must not be empty",
        ));
    }
    let response = tokio::task::spawn_blocking(move || {
        query_event_cached_with_mode(&state, &query.entry, query.full)
    })
    .await
    .map_err(internal_error)?
    .map_err(query_error_response)?;
    Ok((response_security_headers(), Json(response)))
}

fn query_cached(state: &AppState, query: TrajectoryQuery) -> anyhow::Result<TrajectoryResponse> {
    if query.after.is_some() as u8 + query.before.is_some() as u8 + query.entry.is_some() as u8 > 1
    {
        anyhow::bail!("after, before, and entry are mutually exclusive");
    }
    let mut cache = state
        .cache
        .lock()
        .map_err(|_| anyhow::anyhow!("Trajectory cache lock was poisoned"))?;
    refresh_cached_tree(state, &mut cache)?;
    ensure_materialized(state, &mut cache)?;
    if let Some((cached_query, response)) = &cache.last_query
        && cached_query == &query
    {
        return Ok(response.clone());
    }
    let all_rows = &cache
        .materialized
        .as_ref()
        .expect("Trajectory materialization was initialized")
        .rows;
    let search = query
        .search
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase);
    let layer = query.layer.as_deref().filter(|value| !value.is_empty());
    let actor = query.actor.as_deref().filter(|value| !value.is_empty());
    let class = query.class.as_deref().filter(|value| !value.is_empty());
    let producer = query.producer.as_deref().filter(|value| !value.is_empty());
    let visibility = query
        .visibility
        .as_deref()
        .filter(|value| !value.is_empty());
    let limit = query
        .limit
        .unwrap_or(DEFAULT_TRAJECTORY_PAGE_ROWS)
        .clamp(1, MAX_TRAJECTORY_PAGE_ROWS);
    let matches_query = |row: &chat_state::TrajectoryRow| {
        layer.is_none_or(|value| dimension_matches(&row.layer, value))
            && actor.is_none_or(|value| dimension_matches(&row.actor, value))
            && class.is_none_or(|value| row.class == value)
            && producer.is_none_or(|value| dimension_matches(&row.producer, value))
            && visibility.is_none_or(|value| visibility_name(row.visibility) == value)
            && search.as_ref().is_none_or(|needle| {
                format!(
                    "{} {} {} {} {} {} {} {} {} {} {} {} {}",
                    row.seq,
                    row.entry_id,
                    row.parent_entry_id.as_deref().unwrap_or_default(),
                    serde_json::to_string(&row.nesting_path).unwrap_or_default(),
                    row.layer,
                    row.actor,
                    row.class,
                    row.producer,
                    row.kind,
                    row.state,
                    row.summary,
                    row.turn_id.as_deref().unwrap_or_default(),
                    row.correlation_id.as_deref().unwrap_or_default(),
                )
                .to_lowercase()
                .contains(needle)
            })
    };
    let matching = all_rows
        .iter()
        .filter(|row| matches_query(row))
        .collect::<Vec<_>>();
    let overview = trajectory_overview(
        &matching,
        query
            .overview_by
            .unwrap_or(TrajectoryOverviewDimension::Interaction),
    );
    let matching_count = matching.len();
    let cursor_index = |entry_id: &str| {
        matching
            .iter()
            .position(|row| row.entry_id == entry_id)
            .ok_or_else(|| anyhow::Error::new(TrajectoryEntryNotFound(entry_id.to_owned())))
    };
    let (start, end) = if let Some(entry_id) = query.entry.as_deref() {
        let center = cursor_index(entry_id)?;
        let end = center
            .saturating_add(limit / 2)
            .saturating_add(1)
            .min(matching_count);
        (end.saturating_sub(limit), end)
    } else if let Some(entry_id) = query.after.as_deref() {
        let start = cursor_index(entry_id)?.saturating_add(1);
        (start, (start + limit).min(matching_count))
    } else if let Some(entry_id) = query.before.as_deref() {
        let end = cursor_index(entry_id)?;
        (end.saturating_sub(limit), end)
    } else {
        (matching_count.saturating_sub(limit), matching_count)
    };
    let first_cursor = matching.get(start).map(|row| row.entry_id.clone());
    let last_cursor = end
        .checked_sub(1)
        .and_then(|index| matching.get(index))
        .map(|row| row.entry_id.clone());
    let rows = matching[start..end]
        .iter()
        .enumerate()
        .map(|(offset, row)| {
            let mut summary = TrajectoryRowSummary::from(*row);
            summary.ordinal = start.saturating_add(offset);
            summary
        })
        .collect::<Vec<_>>();
    let response = TrajectoryResponse {
        session_id: state.session_id.clone(),
        schema_version: chat_state::TRAJECTORY_SCHEMA_VERSION,
        event_count: cache.event_count(),
        current_surface_items: cache.timeline.surface_len(),
        active_turn: cache.timeline.active_turn().map(|id| id.0.to_string()),
        active_step: cache.timeline.active_step().map(|id| id.index),
        open_request_count: cache.timeline.open_request_ids().count(),
        open_tool_count: cache.timeline.open_tool_call_ids().count(),
        open_workflow_count: cache.timeline.open_workflow_run_ids().count(),
        matching_count,
        first_cursor,
        last_cursor,
        has_earlier: start > 0,
        has_later: end < matching_count,
        overview,
        rows,
    };
    cache.last_query = Some((query, response.clone()));
    Ok(response)
}

fn ensure_materialized(state: &AppState, cache: &mut SessionTrajectoryCache) -> anyhow::Result<()> {
    let revision = cache.revision();
    if cache
        .materialized
        .as_ref()
        .is_some_and(|materialized| materialized.revision == revision)
    {
        return Ok(());
    }
    let source_ids = cache.source_ids(&state.session_id);
    let requires_rebuild = cache.materialized.as_ref().is_none_or(|materialized| {
        materialized.source_ids != source_ids
            || cache.has_materialization_reset()
            || cache.workflow_topology_changed()
    });
    if requires_rebuild {
        rebuild_materialized(state, cache, revision, source_ids)?;
        cache.last_query = None;
        return Ok(());
    }

    let updates = {
        let contexts = &cache
            .materialized
            .as_ref()
            .expect("Trajectory materialization was initialized")
            .source_contexts;
        cache.collect_dirty_rows(&state.session_id, contexts)?
    };
    let mut materialized = cache
        .materialized
        .take()
        .expect("Trajectory materialization was initialized");
    let mut additions = Vec::new();
    let mut update_ids = HashSet::new();
    for row in updates {
        if !update_ids.insert(row.entry_id.clone()) {
            cache.materialized = Some(materialized);
            anyhow::bail!(
                "Trajectory incremental projection repeated '{}'",
                row.entry_id
            );
        }
        if let Some(index) = materialized.positions.get(&row.entry_id).copied() {
            materialized.rows[index] = row;
        } else {
            additions.push(row);
        }
    }
    sort_rows_chronologically(&mut additions);
    cache.assign_arrival_order(&additions);
    for row in additions {
        let index = materialized.rows.len();
        materialized.positions.insert(row.entry_id.clone(), index);
        materialized.rows.push(row);
    }
    materialized.revision = revision;
    cache.materialized = Some(materialized);
    cache.clear_materialization_changes();
    cache.last_query = None;
    Ok(())
}

fn rebuild_materialized(
    state: &AppState,
    cache: &mut SessionTrajectoryCache,
    revision: [u8; 32],
    source_ids: BTreeSet<String>,
) -> anyhow::Result<()> {
    #[cfg(test)]
    {
        cache.full_materialization_count = cache.full_materialization_count.saturating_add(1);
    }
    let previous = cache.materialized.take();
    let fresh = match collect_cached_rows(state, cache) {
        Ok(rows) => rows,
        Err(error) => {
            cache.materialized = previous;
            return Err(error);
        }
    };
    let rows = if let Some(previous) = previous {
        let mut fresh_by_id = fresh
            .into_iter()
            .map(|row| (row.entry_id.clone(), row))
            .collect::<HashMap<_, _>>();
        let mut rows = Vec::with_capacity(fresh_by_id.len());
        for old in previous.rows {
            if let Some(updated) = fresh_by_id.remove(&old.entry_id) {
                rows.push(updated);
            }
        }
        let mut additions = fresh_by_id.into_values().collect::<Vec<_>>();
        sort_rows_chronologically(&mut additions);
        let returning = additions
            .iter()
            .any(|row| cache.arrival_order.contains_key(&row.entry_id));
        cache.assign_arrival_order(&additions);
        rows.extend(additions);
        if returning {
            rows.sort_by_key(|row| {
                cache
                    .arrival_order
                    .get(&row.entry_id)
                    .copied()
                    .unwrap_or(u64::MAX)
            });
        }
        rows
    } else {
        let mut rows = fresh;
        sort_rows_chronologically(&mut rows);
        cache.assign_arrival_order(&rows);
        rows
    };
    let live_entries = rows
        .iter()
        .map(|row| row.entry_id.as_str())
        .collect::<HashSet<_>>();
    cache
        .arrival_order
        .retain(|entry_id, _| live_entries.contains(entry_id.as_str()));
    let positions = rows
        .iter()
        .enumerate()
        .map(|(index, row)| (row.entry_id.clone(), index))
        .collect();
    let mut source_contexts = BTreeMap::new();
    cache.collect_source_contexts(
        &state.session_id,
        &state.actor_ref,
        None,
        &[],
        &mut source_contexts,
    )?;
    cache.materialized = Some(MaterializedTrajectory {
        revision,
        rows,
        positions,
        source_ids,
        source_contexts,
    });
    cache.clear_materialization_changes();
    Ok(())
}

fn refresh_cached_tree(state: &AppState, cache: &mut SessionTrajectoryCache) -> anyhow::Result<()> {
    let mut visited = BTreeSet::from([state.session_id.clone()]);
    let mut budget = TrajectoryReadBudget::default();
    #[cfg(not(test))]
    {
        let resolver = TrajectorySessionResolver::Storage(&state.storage);
        cache.refresh_tree_from_directory(
            state.session.directory().display_path(),
            state.session.directory(),
            &state.session_id,
            &resolver,
            &mut visited,
            0,
            &mut budget,
        )?;
    }
    #[cfg(test)]
    {
        let resolver = TrajectorySessionResolver::TestRoot(&state.sessions_root);
        let session_dir = find_test_session_dir(&state.sessions_root, &state.session_id)?
            .unwrap_or_else(|| state.session_dir.clone());
        cache.refresh_tree(
            &session_dir,
            &state.session_id,
            &resolver,
            &mut visited,
            0,
            &mut budget,
        )?;
    }
    Ok(())
}

fn collect_cached_rows(
    state: &AppState,
    cache: &SessionTrajectoryCache,
) -> anyhow::Result<Vec<chat_state::TrajectoryRow>> {
    let mut all_rows = Vec::new();
    cache.collect_rows(
        &state.session_id,
        &state.actor_ref,
        None,
        &[],
        &mut all_rows,
    )?;
    let mut causal_paths = HashMap::new();
    for row in &all_rows {
        if let Some(existing) = causal_paths.insert(&row.nesting_path, &row.entry_id) {
            anyhow::bail!(
                "Trajectory entries '{}' and '{}' share causal path {:?}",
                existing,
                row.entry_id,
                row.nesting_path
            );
        }
    }
    let mut entry_ids = HashSet::new();
    if let Some(duplicate) = all_rows
        .iter()
        .find(|row| !entry_ids.insert(row.entry_id.as_str()))
    {
        anyhow::bail!(
            "Trajectory entry id '{}' is not globally unique",
            duplicate.entry_id
        );
    }
    Ok(all_rows)
}

fn sort_rows_chronologically(rows: &mut [chat_state::TrajectoryRow]) {
    rows.sort_by(|left, right| {
        left.at_ms
            .cmp(&right.at_ms)
            .then_with(|| left.nesting_path.cmp(&right.nesting_path))
    });
}

fn query_event_cached(state: &AppState, entry_id: &str) -> anyhow::Result<TrajectoryEventResponse> {
    query_event_cached_with_mode(state, entry_id, false)
}

fn query_event_cached_with_mode(
    state: &AppState,
    entry_id: &str,
    full: bool,
) -> anyhow::Result<TrajectoryEventResponse> {
    let mut cache = state
        .cache
        .lock()
        .map_err(|_| anyhow::anyhow!("Trajectory cache lock was poisoned"))?;
    refresh_cached_tree(state, &mut cache)?;
    ensure_materialized(state, &mut cache)?;
    let materialized = cache
        .materialized
        .as_ref()
        .expect("Trajectory materialization was initialized");
    let index = materialized
        .positions
        .get(entry_id)
        .copied()
        .ok_or_else(|| anyhow::Error::new(TrajectoryEntryNotFound(entry_id.to_owned())))?;
    let materialized = materialized.rows.get(index).cloned().ok_or_else(|| {
        anyhow::anyhow!("Trajectory entry index for '{entry_id}' is outside the materialization")
    })?;
    let (mut row, details_truncated) = cache
        .canonical_row(&state.session_id, &state.actor_ref, entry_id, !full)?
        .ok_or_else(|| anyhow::anyhow!("Trajectory source row '{entry_id}' disappeared"))?;
    row.entry_id = materialized.entry_id;
    row.parent_entry_id = materialized.parent_entry_id;
    row.nesting_path = materialized.nesting_path;
    row.visibility = materialized.visibility;
    Ok(TrajectoryEventResponse {
        session_id: state.session_id.clone(),
        schema_version: chat_state::TRAJECTORY_SCHEMA_VERSION,
        row,
        details_truncated,
    })
}

fn trajectory_row_needs_wire_truncation(row: &chat_state::TrajectoryRow) -> bool {
    wire_text_exceeds(&row.summary, TRAJECTORY_SUMMARY_CHARS)
        || [
            row.layer.as_str(),
            row.actor.as_str(),
            row.class.as_str(),
            row.producer.as_str(),
            row.kind.as_str(),
            row.state.as_str(),
        ]
        .into_iter()
        .any(|value| wire_text_exceeds(value, TRAJECTORY_WIRE_FIELD_CHARS))
        || row
            .turn_id
            .as_deref()
            .is_some_and(|value| wire_text_exceeds(value, TRAJECTORY_WIRE_FIELD_CHARS))
        || row
            .correlation_id
            .as_deref()
            .is_some_and(|value| wire_text_exceeds(value, TRAJECTORY_WIRE_FIELD_CHARS))
}

fn wire_text_exceeds(value: &str, limit: usize) -> bool {
    value.chars().nth(limit).is_some()
}

fn trajectory_detail_preview(
    value: &serde_json::Value,
    nodes: &mut usize,
    chars: &mut usize,
    truncated: &mut bool,
    depth: usize,
) -> serde_json::Value {
    if *nodes >= TRAJECTORY_DETAIL_PREVIEW_NODES || *chars >= TRAJECTORY_DETAIL_PREVIEW_CHARS {
        *truncated = true;
        return serde_json::Value::String("[preview budget exhausted]".into());
    }
    *nodes += 1;
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            value.clone()
        }
        serde_json::Value::String(value) => {
            let remaining = TRAJECTORY_DETAIL_PREVIEW_CHARS.saturating_sub(*chars);
            let limit = remaining.min(8_000);
            let preview = crate::util::truncate(value, limit);
            *chars = chars.saturating_add(preview.chars().count());
            if preview.len() == value.len() {
                serde_json::Value::String(preview.to_owned())
            } else {
                *truncated = true;
                serde_json::Value::String(format!(
                    "{preview}… [{} chars omitted]",
                    value
                        .chars()
                        .count()
                        .saturating_sub(preview.chars().count())
                ))
            }
        }
        serde_json::Value::Array(values) => {
            if depth >= TRAJECTORY_DETAIL_PREVIEW_DEPTH {
                *truncated = true;
                return serde_json::Value::String("[depth omitted]".into());
            }
            let mut output = values
                .iter()
                .take(TRAJECTORY_DETAIL_PREVIEW_ITEMS)
                .map(|value| trajectory_detail_preview(value, nodes, chars, truncated, depth + 1))
                .collect::<Vec<_>>();
            if values.len() > output.len() {
                *truncated = true;
                output.push(serde_json::Value::String(format!(
                    "[{} items omitted]",
                    values.len() - output.len()
                )));
            }
            serde_json::Value::Array(output)
        }
        serde_json::Value::Object(values) => {
            if depth >= TRAJECTORY_DETAIL_PREVIEW_DEPTH {
                *truncated = true;
                return serde_json::Value::String("[depth omitted]".into());
            }
            let mut output = serde_json::Map::new();
            for (index, (key, value)) in values
                .iter()
                .take(TRAJECTORY_DETAIL_PREVIEW_ITEMS)
                .enumerate()
            {
                let remaining = TRAJECTORY_DETAIL_PREVIEW_CHARS.saturating_sub(*chars);
                let key_preview = crate::util::truncate(key, remaining.min(1_024));
                *chars = chars.saturating_add(key_preview.chars().count());
                let preview_key = if key_preview.len() == key.len() {
                    key_preview.to_owned()
                } else {
                    *truncated = true;
                    format!("{key_preview}…#{index}")
                };
                output.insert(
                    preview_key,
                    trajectory_detail_preview(value, nodes, chars, truncated, depth + 1),
                );
            }
            if values.len() > output.len() {
                *truncated = true;
                output.insert(
                    "…".into(),
                    serde_json::Value::String(format!(
                        "[{} fields omitted]",
                        values.len() - output.len()
                    )),
                );
            }
            serde_json::Value::Object(output)
        }
    }
}

fn trajectory_overview(
    rows: &[&chat_state::TrajectoryRow],
    dimension: TrajectoryOverviewDimension,
) -> TrajectoryOverview {
    if rows.is_empty() {
        return TrajectoryOverview {
            dimension,
            ..Default::default()
        };
    }
    let bin_count = rows.len().min(TRAJECTORY_OVERVIEW_BINS);
    let mut overview = TrajectoryOverview {
        dimension,
        start_ms: rows.iter().map(|row| trajectory_start_ms(row)).min(),
        end_ms: rows.iter().map(|row| row.at_ms).max(),
        bins: (0..bin_count)
            .map(|_| TrajectoryOverviewBin::default())
            .collect(),
        ..Default::default()
    };
    for (index, row) in rows.iter().enumerate() {
        let bin_index = index.saturating_mul(bin_count) / rows.len();
        let bin = &mut overview.bins[bin_index.min(bin_count - 1)];
        let start_ms = trajectory_start_ms(row);
        if bin.first_entry_id.is_none() {
            bin.first_entry_id = Some(row.entry_id.clone());
            bin.start_ms = start_ms;
            bin.end_ms = row.at_ms;
        } else {
            bin.start_ms = bin.start_ms.min(start_ms);
            bin.end_ms = bin.end_ms.max(row.at_ms);
        }
        bin.last_entry_id = Some(row.entry_id.clone());
        bin.max_duration_ms = bin.max_duration_ms.max(row.duration_ms.unwrap_or_default());
        bin.failures += usize::from(matches!(row.state.as_str(), "failed" | "cancelled"));
        bin.turns += usize::from(row.kind == "turn.started");
        bin.steps += usize::from(row.kind == "step.started");
        let lane = trajectory_overview_lane(row, dimension);
        *overview.counts.entry(lane.clone()).or_default() += 1;
        *bin.counts.entry(lane).or_default() += 1;
    }
    overview
}

fn trajectory_start_ms(row: &chat_state::TrajectoryRow) -> i64 {
    row.at_ms
        .saturating_sub(i64::try_from(row.duration_ms.unwrap_or_default()).unwrap_or(i64::MAX))
}

fn trajectory_overview_lane(
    row: &chat_state::TrajectoryRow,
    dimension: TrajectoryOverviewDimension,
) -> String {
    let value = match dimension {
        TrajectoryOverviewDimension::Interaction if row.layer.starts_with("tool") => "tools",
        TrajectoryOverviewDimension::Interaction
            if row.layer == "assistant"
                || row.producer.starts_with("model")
                || row.kind.starts_with("request.")
                || row.kind.starts_with("step.") =>
        {
            "model"
        }
        TrajectoryOverviewDimension::Interaction => "input",
        TrajectoryOverviewDimension::Layer => dimension_family(&row.layer),
        TrajectoryOverviewDimension::Actor => dimension_family(&row.actor),
        TrajectoryOverviewDimension::Class => &row.class,
        TrajectoryOverviewDimension::Producer => dimension_family(&row.producer),
    };
    value.to_owned()
}

fn dimension_family(value: &str) -> &str {
    value
        .split_once(['.', ':'])
        .map_or(value, |(family, _)| family)
}

fn read_summary_from_directory(
    directory: &super::storage::ContainedDirectory,
) -> anyhow::Result<super::persistence::Summary> {
    let bytes = directory.read_bounded(
        std::ffi::OsStr::new(super::storage::SUMMARY_FILE),
        "Trajectory session summary",
        super::storage::MAX_SESSION_SUMMARY_BYTES,
    )?;
    let summary: super::persistence::Summary = serde_json::from_slice(&bytes)?;
    summary.validate_current_format()?;
    Ok(summary)
}

fn dimension_matches(actual: &str, filter: &str) -> bool {
    actual == filter
        || actual
            .strip_prefix(filter)
            .is_some_and(|suffix| matches!(suffix.as_bytes().first(), Some(b'.' | b':')))
}

enum TrajectorySessionResolver<'a> {
    Storage(&'a super::storage::jsonl::JsonlStorageAdapter),
    #[cfg(test)]
    TestRoot(&'a Path),
}

impl TrajectorySessionResolver<'_> {
    fn open(
        &self,
        session_id: &str,
    ) -> anyhow::Result<
        Option<(
            PathBuf,
            super::storage::ContainedDirectory,
            super::persistence::Summary,
        )>,
    > {
        match self {
            Self::Storage(storage) => {
                let Some(opened) = storage.open_session_by_id(session_id)? else {
                    return Ok(None);
                };
                Ok(Some((
                    opened.directory().display_path().to_path_buf(),
                    opened.directory().try_clone()?,
                    opened.summary().clone(),
                )))
            }
            #[cfg(test)]
            Self::TestRoot(sessions_root) => {
                let Some(path) = find_test_session_dir(sessions_root, session_id)? else {
                    return Ok(None);
                };
                let directory = super::storage::ContainedDirectory::open(
                    &path,
                    Path::new(""),
                    "Trajectory child session directory",
                    false,
                )?;
                let summary = read_summary_from_directory(&directory)?;
                Ok(Some((path, directory, summary)))
            }
        }
    }
}

#[cfg(test)]
fn find_test_session_dir(
    sessions_root: &Path,
    session_id: &str,
) -> anyhow::Result<Option<PathBuf>> {
    if !sessions_root.is_dir() {
        return Ok(None);
    }
    for cwd in std::fs::read_dir(sessions_root)? {
        let path = cwd?.path().join(session_id);
        if path.is_dir() {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

impl SessionTrajectoryCache {
    fn assign_arrival_order(&mut self, rows: &[chat_state::TrajectoryRow]) {
        for row in rows {
            if self.arrival_order.contains_key(&row.entry_id) {
                continue;
            }
            let order = self.next_arrival;
            self.next_arrival = self.next_arrival.saturating_add(1);
            self.arrival_order.insert(row.entry_id.clone(), order);
        }
    }

    fn source_ids(&self, timeline_id: &str) -> BTreeSet<String> {
        let mut ids = BTreeSet::from([timeline_id.to_owned()]);
        ids.extend(self.sidebands.keys().cloned());
        ids.extend(self.workflows.keys().cloned());
        for (child_id, child) in &self.children {
            ids.extend(child.source_ids(child_id));
        }
        ids
    }

    fn has_materialization_reset(&self) -> bool {
        self.materialization_reset
            || self
                .sidebands
                .values()
                .any(|cache| cache.materialization_reset)
            || self
                .workflows
                .values()
                .any(|cache| cache.materialization_reset)
            || self.children.values().any(Self::has_materialization_reset)
    }

    fn workflow_topology_changed(&self) -> bool {
        self.workflows.values().any(|journal| {
            journal
                .dirty_seqs
                .iter()
                .filter_map(|seq| usize::try_from(*seq).ok())
                .any(|index| {
                    journal
                        .projection
                        .entries()
                        .get(index)
                        .is_some_and(|entry| entry.kind == "spawn_agent")
                })
        }) || self.children.values().any(Self::workflow_topology_changed)
    }

    fn clear_materialization_changes(&mut self) {
        self.materialization_reset = false;
        self.projector.clear_dirty_rows();
        for sideband in self.sidebands.values_mut() {
            sideband.dirty_seqs.clear();
            sideband.materialization_reset = false;
        }
        for workflow in self.workflows.values_mut() {
            workflow.dirty_seqs.clear();
            workflow.materialization_reset = false;
        }
        for child in self.children.values_mut() {
            child.clear_materialization_changes();
        }
    }

    fn timeline_row(
        &self,
        timeline_id: &str,
        actor_ref: &str,
        parent_entry_id: Option<&str>,
        path_prefix: &[u64],
        projected: &chat_state::TrajectoryRow,
        subagent_workflows: &BTreeMap<String, String>,
    ) -> anyhow::Result<chat_state::TrajectoryRow> {
        let entry_id = format!("t:{timeline_id}/{}", projected.seq);
        let mut row = TrajectoryRowSummary::from(projected).into_row(serde_json::Value::Null);
        row.entry_id = entry_id;
        row.parent_entry_id = parent_entry_id.map(str::to_owned);
        row.nesting_path = path_prefix
            .iter()
            .copied()
            .chain(std::iter::once(row.seq))
            .collect();
        let event_index = usize::try_from(row.seq)
            .map_err(|_| anyhow::anyhow!("Timeline {timeline_id} seq exceeds usize"))?;
        let event =
            self.timeline.events().get(event_index).ok_or_else(|| {
                anyhow::anyhow!("Trajectory projector outran Timeline {timeline_id}")
            })?;
        row.actor = match &event.kind {
            chat_state::TimelineEventKind::Workflow(event) => {
                format!("workflow:{}", workflow_run_id(event))
            }
            _ => actor_ref.to_owned(),
        };
        let workflow_parent = match &event.kind {
            chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(spawn)) => {
                spawn.workflow_run_id.as_deref().and_then(|run_id| {
                    self.workflow_agent_parent(timeline_id, run_id, &spawn.subagent_id, path_prefix)
                })
            }
            chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(end)) => {
                subagent_workflows.get(&end.subagent_id).and_then(|run_id| {
                    self.workflow_agent_parent(timeline_id, run_id, &end.subagent_id, path_prefix)
                })
            }
            _ => None,
        };
        if let Some((journal_entry, journal_path)) = workflow_parent {
            row.parent_entry_id = Some(journal_entry);
            row.nesting_path = journal_path
                .into_iter()
                .chain(std::iter::once(row.seq))
                .collect();
        }
        Ok(row)
    }

    fn canonical_row(
        &self,
        timeline_id: &str,
        actor_ref: &str,
        entry_id: &str,
        preview: bool,
    ) -> anyhow::Result<Option<(chat_state::TrajectoryRow, bool)>> {
        let Some((source_id, seq)) = trajectory_entry_parts(entry_id) else {
            return Ok(None);
        };
        if source_id == timeline_id {
            let index = usize::try_from(seq)?;
            let Some(projected) = self.projector.rows().get(index) else {
                return Ok(None);
            };
            let mut row = projected.clone();
            row.entry_id = entry_id.to_owned();
            let event = self
                .timeline
                .events()
                .get(index)
                .ok_or_else(|| anyhow::anyhow!("Trajectory projector outran Timeline"))?;
            row.details = trajectory_event_details(&event.kind)?;
            row.actor = match &event.kind {
                chat_state::TimelineEventKind::Workflow(event) => {
                    format!("workflow:{}", workflow_run_id(event))
                }
                _ => actor_ref.to_owned(),
            };
            return Ok(Some(preview_trajectory_row(&row, preview)));
        }
        if let Some(sideband) = self.sidebands.get(source_id)
            && let Some(timeline) = &sideband.timeline
            && let Some(event) = timeline.events().get(usize::try_from(seq)?)
        {
            let mut attempt_times = BTreeMap::new();
            if let chat_state::SidebandEventKind::Result(result) = &event.kind {
                let attempt = result.source_event_seqs[1];
                if let Some(started) = timeline.events().get(usize::try_from(attempt)?) {
                    attempt_times.insert(attempt, started.at_ms);
                }
            }
            let mut row = sideband_row(event, "", &[], &attempt_times);
            row.details = trajectory_event_details(&event.kind)?;
            return Ok(Some(preview_trajectory_row(&row, preview)));
        }
        if let Some(journal) = self.workflows.get(source_id)
            && let Some(entry) = journal.projection.entries().get(usize::try_from(seq)?)
        {
            let pending = matches!(
                journal
                    .projection
                    .replay_operation(entry.seq, &entry.kind, &entry.req_hash)?,
                Some(workflow::journal::OperationReplay::Pending { .. })
            );
            let mut row = workflow_row(entry, source_id, "", &[], pending);
            row.details = trajectory_event_details(entry)?;
            return Ok(Some(preview_trajectory_row(&row, preview)));
        }
        for (child_id, child) in &self.children {
            if let Some(row) =
                child.canonical_row(child_id, &format!("subagent:{child_id}"), entry_id, preview)?
            {
                return Ok(Some(row));
            }
        }
        Ok(None)
    }

    fn revision(&self) -> [u8; 32] {
        fn update(cache: &SessionTrajectoryCache, hasher: &mut blake3::Hasher) {
            hasher.update(b"session\0");
            hasher.update(&cache.offset.to_le_bytes());
            hasher.update(&ledger_digest(cache.prefix_hasher.as_ref()));
            for (id, sideband) in &cache.sidebands {
                hasher.update(b"sideband\0");
                hasher.update(&(id.len() as u64).to_le_bytes());
                hasher.update(id.as_bytes());
                hasher.update(&sideband.offset.to_le_bytes());
                hasher.update(&ledger_digest(sideband.prefix_hasher.as_ref()));
            }
            for (id, workflow) in &cache.workflows {
                hasher.update(b"workflow\0");
                hasher.update(&(id.len() as u64).to_le_bytes());
                hasher.update(id.as_bytes());
                hasher.update(&workflow.offset.to_le_bytes());
                hasher.update(&ledger_digest(workflow.prefix_hasher.as_ref()));
            }
            for (id, child) in &cache.children {
                hasher.update(b"child\0");
                hasher.update(&(id.len() as u64).to_le_bytes());
                hasher.update(id.as_bytes());
                update(child, hasher);
            }
        }

        let mut hasher = blake3::Hasher::new();
        update(self, &mut hasher);
        *hasher.finalize().as_bytes()
    }

    #[cfg(test)]
    fn refresh_tree(
        &mut self,
        session_dir: &Path,
        timeline_id: &str,
        resolver: &TrajectorySessionResolver<'_>,
        visited: &mut BTreeSet<String>,
        depth: usize,
        budget: &mut TrajectoryReadBudget,
    ) -> anyhow::Result<()> {
        let directory = super::storage::ContainedDirectory::open(
            session_dir,
            Path::new(""),
            "Trajectory session directory",
            false,
        )?;
        self.refresh_tree_from_directory(
            session_dir,
            &directory,
            timeline_id,
            resolver,
            visited,
            depth,
            budget,
        )
    }

    fn refresh_tree_from_directory(
        &mut self,
        session_dir: &Path,
        directory: &super::storage::ContainedDirectory,
        timeline_id: &str,
        resolver: &TrajectorySessionResolver<'_>,
        visited: &mut BTreeSet<String>,
        depth: usize,
        budget: &mut TrajectoryReadBudget,
    ) -> anyhow::Result<()> {
        budget.enter_entity(&format!("session {timeline_id}"), depth)?;
        if !self.session_dir.as_os_str().is_empty() && self.session_dir != session_dir {
            *self = Self::default();
        }
        self.session_dir = session_dir.to_owned();
        let timeline_file = directory.open_regular(
            std::ffi::OsStr::new(super::storage::TIMELINE_FILE),
            "Trajectory Timeline ledger",
        )?;
        budget.admit_file(&timeline_file, "Timeline ledger")?;
        self.refresh_from_directory(directory, budget.remaining_events())?;
        budget.admit_events(self.timeline.events().len())?;
        self.refresh_sidebands(directory, depth, budget)?;
        for sideband_id in self.sidebands.keys() {
            if !visited.insert(sideband_id.clone()) {
                anyhow::bail!(
                    "Timeline identity '{sideband_id}' is linked more than once in the Trajectory tree"
                );
            }
        }
        self.refresh_workflows(directory, timeline_id, visited, depth, budget)?;

        let terminals = self
            .timeline
            .events()
            .iter()
            .filter_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(end)) => {
                    Some((end.subagent_id.clone(), end.clone()))
                }
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        let spawns = self
            .timeline
            .events()
            .iter()
            .filter_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) => Some((event.seq, spawn.clone())),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut seen = BTreeSet::new();
        for (spawn_seq, spawn) in spawns {
            if !visited.insert(spawn.child_session_id.clone()) {
                anyhow::bail!(
                    "child Timeline '{}' is linked more than once in the Trajectory tree",
                    spawn.child_session_id
                );
            }
            let terminal = terminals.get(&spawn.subagent_id);
            let Some((child_dir, child_directory, summary)) =
                resolver.open(&spawn.child_session_id)?
            else {
                if terminal.is_some_and(terminal_requires_child) {
                    anyhow::bail!(
                        "terminal subagent '{}' requires missing child Timeline '{}'",
                        spawn.subagent_id,
                        spawn.child_session_id
                    );
                }
                continue;
            };
            if summary.info.id.to_string() != spawn.child_session_id
                || summary.parent_session_id.as_deref() != Some(timeline_id)
                || !summary
                    .session_kind
                    .as_deref()
                    .is_some_and(|kind| kind.starts_with("subagent"))
            {
                anyhow::bail!(
                    "child session '{}' summary does not match parent spawn t:{}/{}",
                    spawn.child_session_id,
                    timeline_id,
                    spawn_seq.get()
                );
            }
            match child_directory.open_regular(
                std::ffi::OsStr::new(super::storage::TIMELINE_FILE),
                "Trajectory child Timeline ledger",
            ) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if terminal.is_some_and(terminal_requires_child) {
                        anyhow::bail!(
                            "terminal subagent '{}' requires a child Timeline ledger",
                            spawn.subagent_id
                        );
                    }
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
            let child = self
                .children
                .entry(spawn.child_session_id.clone())
                .or_default();
            child.refresh_tree_from_directory(
                &child_dir,
                &child_directory,
                &spawn.child_session_id,
                resolver,
                visited,
                depth.saturating_add(1),
                budget,
            )?;
            if child.timeline.events().is_empty() && terminal.is_none() {
                seen.insert(spawn.child_session_id.clone());
                continue;
            }
            child
                .timeline
                .validate_subagent_seed_link(timeline_id, spawn_seq, &spawn)?;
            if let Some(terminal) = terminal {
                child.timeline.validate_subagent_result_link(
                    timeline_id,
                    spawn_seq,
                    &spawn,
                    terminal,
                )?;
            }
            seen.insert(spawn.child_session_id);
        }
        self.children.retain(|id, _| seen.contains(id));
        self.validate_sidebands(timeline_id)?;
        Ok(())
    }

    fn refresh_from_directory(
        &mut self,
        directory: &super::storage::ContainedDirectory,
        event_limit: usize,
    ) -> anyhow::Result<()> {
        let path = directory.display_path().join(super::storage::TIMELINE_FILE);
        let mut file = directory.open_regular(
            std::ffi::OsStr::new(super::storage::TIMELINE_FILE),
            "Trajectory Timeline ledger",
        )?;
        let opened_stamp = LedgerStamp::from_metadata(&file.metadata()?);
        if self.source_stamp == Some(opened_stamp) {
            return Ok(());
        }
        let replacing = self.source_stamp.is_some();
        let rebuilding = ledger_requires_rebuild(
            self.offset,
            self.prefix_hasher.as_ref(),
            self.tail_hash,
            self.source_stamp,
            opened_stamp,
            &mut file,
        )?;
        let old_offset = self.offset;
        let mut timeline = if rebuilding {
            chat_state::Timeline::default()
        } else {
            std::mem::take(&mut self.timeline)
        };
        let mut projector = if rebuilding {
            chat_state::TrajectoryProjector::default()
        } else {
            std::mem::take(&mut self.projector)
        };
        let mut offset = if rebuilding { 0 } else { self.offset };
        let mut prefix_hasher = if rebuilding {
            None
        } else {
            self.prefix_hasher.clone()
        };
        let mut observed_stamp = opened_stamp;
        let ingestion = (|| -> anyhow::Result<()> {
            loop {
                let (bytes, complete_len) = read_ledger_batch(&mut file, offset, &path)?;
                if complete_len == 0 {
                    let current_stamp = LedgerStamp::from_metadata(&file.metadata()?);
                    if current_stamp != observed_stamp {
                        observed_stamp = current_stamp;
                        continue;
                    }
                    if rebuilding && replacing && offset == 0 {
                        anyhow::bail!("Trajectory replacement has no committed Timeline entry");
                    }
                    break;
                }
                for line in bytes[..complete_len].split(|byte| *byte == b'\n') {
                    if line.is_empty() {
                        continue;
                    }
                    if timeline.events().len() >= event_limit {
                        anyhow::bail!("Trajectory exceeds the event limit");
                    }
                    let event = serde_json::from_slice::<chat_state::TimelineEvent>(line).map_err(
                        |error| anyhow::anyhow!("{} at byte {offset}: {error}", path.display()),
                    )?;
                    timeline.accept(event.clone())?;
                    projector.accept(&event);
                }
                prefix_hasher
                    .get_or_insert_with(blake3::Hasher::new)
                    .update(&bytes[..complete_len]);
                offset = offset.saturating_add(complete_len as u64);
                observed_stamp = LedgerStamp::from_metadata(&file.metadata()?);
            }
            Ok(())
        })();
        if let Err(error) = ingestion {
            if !rebuilding {
                let (restored_timeline, restored_projector) =
                    replay_timeline_prefix(&mut file, old_offset, &path, event_limit)?;
                self.timeline = restored_timeline;
                self.projector = restored_projector;
            }
            return Err(error);
        }
        let tail_hash = match hash_ledger_tail(offset, &mut file) {
            Ok(hash) => hash,
            Err(error) => {
                if !rebuilding {
                    let (restored_timeline, restored_projector) =
                        replay_timeline_prefix(&mut file, old_offset, &path, event_limit)?;
                    self.timeline = restored_timeline;
                    self.projector = restored_projector;
                }
                return Err(error);
            }
        };
        if rebuilding {
            let session_dir = std::mem::take(&mut self.session_dir);
            let arrival_order = std::mem::take(&mut self.arrival_order);
            let next_arrival = self.next_arrival;
            *self = Self::default();
            self.session_dir = session_dir;
            self.arrival_order = arrival_order;
            self.next_arrival = next_arrival;
            self.materialization_reset = replacing;
        }
        self.timeline = timeline;
        self.projector = projector;
        self.offset = offset;
        self.prefix_hasher = prefix_hasher;
        self.tail_hash = tail_hash;
        self.source_stamp = Some(observed_stamp);
        Ok(())
    }

    #[cfg(test)]
    fn refresh(&mut self, path: &Path) -> anyhow::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("test ledger has no parent"))?;
        let directory = super::storage::ContainedDirectory::open(
            parent,
            Path::new(""),
            "Trajectory test directory",
            false,
        )?;
        self.refresh_from_directory(&directory, MAX_TRAJECTORY_EVENTS)
    }

    fn refresh_sidebands(
        &mut self,
        directory: &super::storage::ContainedDirectory,
        depth: usize,
        budget: &mut TrajectoryReadBudget,
    ) -> anyhow::Result<()> {
        let sideband_ids = self
            .timeline
            .events()
            .iter()
            .filter_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Sideband(spawn) => Some(spawn.sideband_id.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let mut seen = BTreeSet::new();
        for sideband_id in sideband_ids {
            chat_state::validate_sideband_id(&sideband_id)?;
            let sideband_dir = match directory.open_relative(
                &Path::new(super::storage::SIDEBANDS_DIR).join(&sideband_id),
                "Trajectory sideband directory",
                false,
            ) {
                Ok(sideband_dir) => sideband_dir,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            let sideband = self.sidebands.entry(sideband_id.clone()).or_default();
            let ledger = match sideband_dir.open_regular(
                std::ffi::OsStr::new(super::storage::TIMELINE_FILE),
                "Trajectory sideband ledger",
            ) {
                Ok(ledger) => ledger,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            budget.enter_entity(&format!("sideband {sideband_id}"), depth.saturating_add(1))?;
            budget.admit_file(&ledger, "sideband Timeline ledger")?;
            match sideband.refresh_from_directory(&sideband_dir, budget.remaining_events()) {
                Ok(()) => {}
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            }
            if let Some(timeline) = &sideband.timeline {
                budget.admit_events(timeline.events().len())?;
                seen.insert(sideband_id.clone());
            }
        }
        self.sidebands.retain(|id, _| seen.contains(id));
        Ok(())
    }

    fn refresh_workflows(
        &mut self,
        directory: &super::storage::ContainedDirectory,
        timeline_id: &str,
        visited: &mut BTreeSet<String>,
        depth: usize,
        budget: &mut TrajectoryReadBudget,
    ) -> anyhow::Result<()> {
        let spawns = self
            .timeline
            .events()
            .iter()
            .filter_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Workflow(chat_state::WorkflowEvent::Spawned {
                    run_id,
                    name,
                    objective,
                    ..
                }) => Some((
                    run_id.clone(),
                    (event.seq.get(), name.clone(), objective.clone()),
                )),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        let mut seen = BTreeSet::new();
        for (run_id, (spawn_seq, name, objective)) in &spawns {
            if !visited.insert(run_id.clone()) {
                anyhow::bail!(
                    "Workflow identity '{run_id}' is linked more than once in the Trajectory tree"
                );
            }
            super::workflow::store::validate_run_id(run_id)?;
            let run_dir = match directory.open_relative(
                &Path::new("workflows").join(run_id),
                "Trajectory Workflow run directory",
                false,
            ) {
                Ok(run_dir) => run_dir,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            match run_dir.read_bounded(
                std::ffi::OsStr::new("cleared"),
                "Trajectory Workflow cleared marker",
                0,
            ) {
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            let manifest = match run_dir.read_bounded(
                std::ffi::OsStr::new("state.json"),
                "Trajectory Workflow manifest",
                super::workflow::store::MAX_WORKFLOW_MANIFEST_BYTES,
            ) {
                Ok(bytes) => {
                    budget.enter_entity(&format!("Workflow {run_id}"), depth.saturating_add(1))?;
                    let manifest_file = run_dir.open_regular(
                        std::ffi::OsStr::new("state.json"),
                        "Trajectory Workflow manifest",
                    )?;
                    budget.admit_file(&manifest_file, "Workflow manifest")?;
                    serde_json::from_slice::<super::workflow::store::WorkflowRunManifest>(&bytes)?
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    match run_dir.open_regular(
                        std::ffi::OsStr::new("journal.jsonl"),
                        "Trajectory Workflow journal",
                    ) {
                        Ok(_) => anyhow::bail!(
                            "Workflow {run_id} has a journal but no manifest under t:{timeline_id}/{spawn_seq}"
                        ),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error.into()),
                    }
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            let expected_journal = format!("workflows/{run_id}/journal.jsonl");
            if manifest.version != super::workflow::store::WORKFLOW_RUN_MANIFEST_VERSION
                || manifest.state.run_id != *run_id
                || manifest.state.name != *name
                || manifest.state.objective != *objective
                || manifest.state.journal_path.as_deref() != Some(expected_journal.as_str())
            {
                anyhow::bail!("Workflow manifest does not match spawn t:{timeline_id}/{spawn_seq}");
            }
            let journal = self.workflows.entry(run_id.clone()).or_default();
            match run_dir.open_regular(
                std::ffi::OsStr::new("journal.jsonl"),
                "Trajectory Workflow journal",
            ) {
                Ok(journal_file) => {
                    budget.admit_file(&journal_file, "Workflow journal")?;
                    journal.refresh_from_directory(&run_dir, budget.remaining_events())?;
                    budget.admit_events(journal.projection.len())?;
                    validate_workflow_journal_links(&self.timeline, run_id, journal)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    *journal = WorkflowJournalCache::default();
                }
                Err(error) => return Err(error.into()),
            }
            seen.insert(run_id.clone());
        }
        self.workflows.retain(|run_id, _| seen.contains(run_id));
        Ok(())
    }

    fn validate_sidebands(&self, parent_timeline_id: &str) -> anyhow::Result<()> {
        let parents = self
            .timeline
            .events()
            .iter()
            .filter_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Sideband(spawn) => {
                    Some((spawn.sideband_id.as_str(), (event.seq.get(), spawn)))
                }
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        for (sideband_id, sideband) in &self.sidebands {
            let (parent_seq, spawn) =
                parents.get(sideband_id.as_str()).copied().ok_or_else(|| {
                    anyhow::anyhow!(
                        "sideband {sideband_id} has a Timeline but no parent sideband.spawn fact"
                    )
                })?;
            sideband
                .timeline
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("sideband {sideband_id} has no Timeline"))?
                .validate_parent(parent_timeline_id, &self.timeline, parent_seq, spawn)?;
        }
        Ok(())
    }

    fn collect_rows(
        &self,
        timeline_id: &str,
        actor_ref: &str,
        parent_entry_id: Option<&str>,
        path_prefix: &[u64],
        rows: &mut Vec<chat_state::TrajectoryRow>,
    ) -> anyhow::Result<()> {
        let subagent_workflows = self
            .timeline
            .events()
            .iter()
            .filter_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) => spawn
                    .workflow_run_id
                    .as_ref()
                    .map(|run_id| (spawn.subagent_id.clone(), run_id.clone())),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        for projected in self.projector.rows() {
            let row = self.timeline_row(
                timeline_id,
                actor_ref,
                parent_entry_id,
                path_prefix,
                projected,
                &subagent_workflows,
            )?;
            let event = self
                .timeline
                .events()
                .get(usize::try_from(row.seq)?)
                .ok_or_else(|| anyhow::anyhow!("Trajectory projector outran Timeline"))?;
            rows.push(row.clone());
            match &event.kind {
                chat_state::TimelineEventKind::Sideband(spawn) => {
                    self.collect_sideband_rows(
                        &spawn.sideband_id,
                        &row.entry_id,
                        &row.nesting_path,
                        rows,
                    )?;
                }
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) => {
                    if let Some(child) = self.children.get(&spawn.child_session_id) {
                        child.collect_rows(
                            &spawn.child_session_id,
                            &format!("subagent:{}", spawn.child_session_id),
                            Some(&row.entry_id),
                            &row.nesting_path,
                            rows,
                        )?;
                    }
                }
                chat_state::TimelineEventKind::Workflow(chat_state::WorkflowEvent::Spawned {
                    run_id,
                    ..
                }) => {
                    self.collect_workflow_rows(run_id, &row.entry_id, &row.nesting_path, rows)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn collect_source_contexts(
        &self,
        timeline_id: &str,
        actor_ref: &str,
        parent_entry_id: Option<&str>,
        path_prefix: &[u64],
        contexts: &mut BTreeMap<String, TrajectorySourceContext>,
    ) -> anyhow::Result<()> {
        let context = TrajectorySourceContext {
            actor_ref: actor_ref.to_owned(),
            parent_entry_id: parent_entry_id.map(str::to_owned),
            path_prefix: path_prefix.to_vec(),
        };
        if contexts.insert(timeline_id.to_owned(), context).is_some() {
            anyhow::bail!("Trajectory source context repeated '{timeline_id}'");
        }
        let subagent_workflows = self
            .timeline
            .events()
            .iter()
            .filter_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) => spawn
                    .workflow_run_id
                    .as_ref()
                    .map(|run_id| (spawn.subagent_id.clone(), run_id.clone())),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        for projected in self.projector.rows() {
            let row = self.timeline_row(
                timeline_id,
                actor_ref,
                parent_entry_id,
                path_prefix,
                projected,
                &subagent_workflows,
            )?;
            let event = self
                .timeline
                .events()
                .get(usize::try_from(row.seq)?)
                .ok_or_else(|| anyhow::anyhow!("Trajectory projector outran Timeline"))?;
            let nested_context = TrajectorySourceContext {
                actor_ref: String::new(),
                parent_entry_id: Some(row.entry_id.clone()),
                path_prefix: row.nesting_path.clone(),
            };
            match &event.kind {
                chat_state::TimelineEventKind::Sideband(spawn) => {
                    if contexts
                        .insert(spawn.sideband_id.clone(), nested_context)
                        .is_some()
                    {
                        anyhow::bail!("Trajectory source context repeated '{}'", spawn.sideband_id);
                    }
                }
                chat_state::TimelineEventKind::Workflow(chat_state::WorkflowEvent::Spawned {
                    run_id,
                    ..
                }) => {
                    if contexts.insert(run_id.clone(), nested_context).is_some() {
                        anyhow::bail!("Trajectory source context repeated '{run_id}'");
                    }
                }
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) => {
                    if let Some(child) = self.children.get(&spawn.child_session_id) {
                        child.collect_source_contexts(
                            &spawn.child_session_id,
                            &format!("subagent:{}", spawn.child_session_id),
                            Some(&row.entry_id),
                            &row.nesting_path,
                            contexts,
                        )?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn collect_dirty_rows(
        &self,
        timeline_id: &str,
        contexts: &BTreeMap<String, TrajectorySourceContext>,
    ) -> anyhow::Result<Vec<chat_state::TrajectoryRow>> {
        let mut rows = Vec::new();
        self.append_dirty_rows(timeline_id, contexts, &mut rows)?;
        Ok(rows)
    }

    fn append_dirty_rows(
        &self,
        timeline_id: &str,
        contexts: &BTreeMap<String, TrajectorySourceContext>,
        rows: &mut Vec<chat_state::TrajectoryRow>,
    ) -> anyhow::Result<()> {
        let context = contexts.get(timeline_id).ok_or_else(|| {
            anyhow::anyhow!("Trajectory source '{timeline_id}' has no materialized context")
        })?;
        for index in self.projector.dirty_row_indices() {
            let projected = self.projector.rows().get(index).ok_or_else(|| {
                anyhow::anyhow!("Trajectory dirty row {index} is outside the projector")
            })?;
            let subagent_workflows = match self
                .timeline
                .events()
                .get(usize::try_from(projected.seq)?)
                .map(|event| &event.kind)
            {
                Some(chat_state::TimelineEventKind::Subagent(
                    chat_state::SubagentEvent::Ended(end),
                )) => self
                    .timeline
                    .events()
                    .iter()
                    .find_map(|event| match &event.kind {
                        chat_state::TimelineEventKind::Subagent(
                            chat_state::SubagentEvent::Spawned(spawn),
                        ) if spawn.subagent_id == end.subagent_id => spawn
                            .workflow_run_id
                            .as_ref()
                            .map(|run| (end.subagent_id.clone(), run.clone())),
                        _ => None,
                    })
                    .into_iter()
                    .collect(),
                _ => BTreeMap::new(),
            };
            rows.push(self.timeline_row(
                timeline_id,
                &context.actor_ref,
                context.parent_entry_id.as_deref(),
                &context.path_prefix,
                projected,
                &subagent_workflows,
            )?);
        }
        for (sideband_id, sideband) in &self.sidebands {
            let context = contexts.get(sideband_id).ok_or_else(|| {
                anyhow::anyhow!("Trajectory sideband '{sideband_id}' has no materialized context")
            })?;
            let timeline = sideband.timeline.as_ref().ok_or_else(|| {
                anyhow::anyhow!("Trajectory sideband '{sideband_id}' has no Timeline")
            })?;
            for seq in &sideband.dirty_seqs {
                let event = timeline
                    .events()
                    .get(usize::try_from(*seq)?)
                    .ok_or_else(|| anyhow::anyhow!("Trajectory sideband lost seq {seq}"))?;
                let mut attempt_times = BTreeMap::new();
                if let chat_state::SidebandEventKind::Result(result) = &event.kind {
                    let attempt = result.source_event_seqs[1];
                    if let Some(started) = timeline.events().get(usize::try_from(attempt)?) {
                        attempt_times.insert(attempt, started.at_ms);
                    }
                }
                rows.push(sideband_row(
                    event,
                    context
                        .parent_entry_id
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("sideband context has no parent"))?,
                    &context.path_prefix,
                    &attempt_times,
                ));
            }
        }
        for (run_id, journal) in &self.workflows {
            let context = contexts.get(run_id).ok_or_else(|| {
                anyhow::anyhow!("Trajectory Workflow '{run_id}' has no materialized context")
            })?;
            for seq in &journal.dirty_seqs {
                let entry = journal
                    .projection
                    .entries()
                    .get(usize::try_from(*seq)?)
                    .ok_or_else(|| anyhow::anyhow!("Trajectory Workflow lost seq {seq}"))?;
                let pending = matches!(
                    journal
                        .projection
                        .replay_operation(entry.seq, &entry.kind, &entry.req_hash)?,
                    Some(workflow::journal::OperationReplay::Pending { .. })
                );
                rows.push(workflow_row(
                    entry,
                    run_id,
                    context
                        .parent_entry_id
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("Workflow context has no parent"))?,
                    &context.path_prefix,
                    pending,
                ));
            }
        }
        for (child_id, child) in &self.children {
            child.append_dirty_rows(child_id, contexts, rows)?;
        }
        Ok(())
    }

    fn collect_workflow_rows(
        &self,
        run_id: &str,
        parent_entry_id: &str,
        path_prefix: &[u64],
        rows: &mut Vec<chat_state::TrajectoryRow>,
    ) -> anyhow::Result<()> {
        let Some(journal) = self.workflows.get(run_id) else {
            return Ok(());
        };
        for entry in journal.projection.entries() {
            let pending = matches!(
                journal
                    .projection
                    .replay_operation(entry.seq, &entry.kind, &entry.req_hash)?,
                Some(workflow::journal::OperationReplay::Pending { .. })
            );
            rows.push(workflow_row(
                entry,
                run_id,
                parent_entry_id,
                path_prefix,
                pending,
            ));
        }
        Ok(())
    }

    fn workflow_agent_parent(
        &self,
        timeline_id: &str,
        run_id: &str,
        subagent_id: &str,
        path_prefix: &[u64],
    ) -> Option<(String, Vec<u64>)> {
        let spawn_seq = self
            .timeline
            .events()
            .iter()
            .find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Workflow(chat_state::WorkflowEvent::Spawned {
                    run_id: candidate,
                    ..
                }) if candidate == run_id => Some(event.seq.get()),
                _ => None,
            })?;
        let entry = self.workflows.get(run_id).and_then(|journal| {
            journal.projection.entries().iter().find(|entry| {
                entry.kind == "spawn_agent"
                    && entry
                        .result
                        .get("agent_id")
                        .and_then(serde_json::Value::as_str)
                        == Some(subagent_id)
            })
        });
        match entry {
            Some(entry) => {
                let path = path_prefix
                    .iter()
                    .copied()
                    .chain([spawn_seq, 0, entry.seq])
                    .collect();
                Some((format!("t:{run_id}/{}", entry.seq), path))
            }
            None => {
                let path = path_prefix.iter().copied().chain([spawn_seq, 1]).collect();
                Some((format!("t:{timeline_id}/{spawn_seq}"), path))
            }
        }
    }

    fn collect_sideband_rows(
        &self,
        sideband_id: &str,
        parent_entry_id: &str,
        path_prefix: &[u64],
        rows: &mut Vec<chat_state::TrajectoryRow>,
    ) -> anyhow::Result<()> {
        let Some(sideband) = self.sidebands.get(sideband_id) else {
            return Ok(());
        };
        let Some(timeline) = &sideband.timeline else {
            return Ok(());
        };
        let attempt_times = timeline
            .events()
            .iter()
            .filter_map(|event| {
                matches!(event.kind, chat_state::SidebandEventKind::Attempt(_))
                    .then_some((event.seq, event.at_ms))
            })
            .collect::<BTreeMap<_, _>>();
        for event in timeline.events() {
            rows.push(sideband_row(
                event,
                parent_entry_id,
                path_prefix,
                &attempt_times,
            ));
        }
        Ok(())
    }

    fn event_count(&self) -> usize {
        self.timeline.events().len()
            + self
                .workflows
                .values()
                .map(|workflow| workflow.projection.len())
                .sum::<usize>()
            + self
                .sidebands
                .values()
                .filter_map(|sideband| sideband.timeline.as_ref())
                .map(|timeline| timeline.events().len())
                .sum::<usize>()
            + self.children.values().map(Self::event_count).sum::<usize>()
    }
}

fn trajectory_entry_parts(entry_id: &str) -> Option<(&str, u64)> {
    let (source, seq) = entry_id.strip_prefix("t:")?.rsplit_once('/')?;
    Some((source, seq.parse().ok()?))
}

struct TrajectorySizeWriter {
    bytes: usize,
    exceeded: bool,
}

impl Write for TrajectorySizeWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let Some(total) = self.bytes.checked_add(buffer.len()) else {
            self.exceeded = true;
            return Err(std::io::Error::other("Trajectory event size overflowed"));
        };
        if total > MAX_TRAJECTORY_FULL_DETAIL_BYTES {
            self.exceeded = true;
            return Err(std::io::Error::other(
                "Trajectory event exceeds browser limit",
            ));
        }
        self.bytes = total;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn trajectory_event_details<T: Serialize>(value: &T) -> anyhow::Result<serde_json::Value> {
    let mut writer = TrajectorySizeWriter {
        bytes: 0,
        exceeded: false,
    };
    if let Err(error) = serde_json::to_writer(&mut writer, value) {
        if writer.exceeded {
            return Err(anyhow::Error::new(TrajectoryEventTooLarge));
        }
        return Err(error.into());
    }
    serde_json::to_value(value).map_err(Into::into)
}

fn preview_trajectory_row(
    row: &chat_state::TrajectoryRow,
    preview: bool,
) -> (chat_state::TrajectoryRow, bool) {
    if !preview {
        return (row.clone(), false);
    }
    let mut nodes = 0;
    let mut chars = 0;
    let mut truncated = trajectory_row_needs_wire_truncation(row);
    let details =
        trajectory_detail_preview(&row.details, &mut nodes, &mut chars, &mut truncated, 0);
    (TrajectoryRowSummary::from(row).into_row(details), truncated)
}

fn terminal_requires_child(terminal: &chat_state::SubagentTerminalEvent) -> bool {
    terminal.outcome == chat_state::SubagentOutcome::Completed || terminal.result_ref.is_some()
}

fn workflow_run_id(event: &chat_state::WorkflowEvent) -> &str {
    match event {
        chat_state::WorkflowEvent::Spawned { run_id, .. }
        | chat_state::WorkflowEvent::Resumed { run_id, .. }
        | chat_state::WorkflowEvent::Ended { run_id, .. }
        | chat_state::WorkflowEvent::Closed { run_id, .. } => run_id,
    }
}

fn workflow_result_preview(result: &serde_json::Value) -> String {
    match result {
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => crate::util::truncate(value, 220).to_owned(),
        serde_json::Value::Array(values) => format!("array · {} items", values.len()),
        serde_json::Value::Object(values) if values.is_empty() => "object · 0 fields".into(),
        serde_json::Value::Object(values) => {
            let mut keys = values
                .keys()
                .take(6)
                .map(|key| crate::util::truncate(key, 28))
                .collect::<Vec<_>>()
                .join(", ");
            if values.len() > 6 {
                keys.push_str(", …");
            }
            format!("object · {} fields · {keys}", values.len())
        }
    }
}

fn workflow_row(
    entry: &workflow::JournalEntry,
    run_id: &str,
    parent_entry_id: &str,
    path_prefix: &[u64],
    pending: bool,
) -> chat_state::TrajectoryRow {
    let entry_id = format!("t:{run_id}/{}", entry.seq);
    let failed = entry
        .result
        .get(workflow::journal::HOST_ERROR_KEY)
        .and_then(serde_json::Value::as_str);
    let result_preview = workflow_result_preview(&entry.result);
    chat_state::TrajectoryRow {
        entry_id,
        seq: entry.seq,
        parent_entry_id: Some(parent_entry_id.to_owned()),
        nesting_path: path_prefix.iter().copied().chain([0, entry.seq]).collect(),
        at_ms: i64::try_from(entry.at_ms).unwrap_or(i64::MAX),
        layer: "tool.result".into(),
        actor: format!("workflow:{run_id}"),
        class: "message".into(),
        producer: format!("workflow-host:{}", entry.kind),
        kind: format!("workflow.host_call.{}", entry.kind),
        state: if pending {
            "running".into()
        } else if failed.is_some() {
            "failed".into()
        } else {
            "completed".into()
        },
        visibility: chat_state::SurfaceVisibility::LogOnly,
        turn_id: None,
        step_index: None,
        correlation_id: Some(entry.req_hash.clone()),
        duration_ms: None,
        summary: if pending {
            format!("{} · pending", entry.kind)
        } else {
            failed.map_or_else(
                || format!("{} · {result_preview}", entry.kind),
                |error| format!("{} · {}", entry.kind, crate::util::truncate(error, 220)),
            )
        },
        details: serde_json::Value::Null,
    }
}

fn validate_workflow_journal_entry(entry: &workflow::JournalEntry) -> anyhow::Result<()> {
    let host_error = entry
        .result
        .get(workflow::journal::HOST_ERROR_KEY)
        .and_then(serde_json::Value::as_str);
    if host_error.is_some_and(str::is_empty) {
        anyhow::bail!("Workflow journal host error must not be empty");
    }
    if entry.kind == "spawn_agent" && host_error.is_none() {
        let result = serde_json::from_value::<workflow::AgentResult>(entry.result.clone())?;
        if result.agent_id.trim().is_empty() {
            anyhow::bail!("Workflow spawn_agent result has an empty agent id");
        }
    }
    Ok(())
}

fn project_workflow_entry(
    projection: &mut workflow::Journal,
    entry: workflow::JournalEntry,
    event_limit: usize,
) -> anyhow::Result<()> {
    projection.project_physical_entry(entry.clone())?;
    if projection.len() > event_limit {
        anyhow::bail!("Trajectory exceeds the event limit");
    }
    if matches!(
        projection.replay_operation(entry.seq, &entry.kind, &entry.req_hash)?,
        Some(workflow::journal::OperationReplay::Completed(_))
    ) {
        let index = usize::try_from(entry.seq)?;
        let logical = projection.entries().get(index).ok_or_else(|| {
            anyhow::anyhow!("Workflow projection lost logical entry {}", entry.seq)
        })?;
        validate_workflow_journal_entry(logical)?;
    }
    Ok(())
}

fn validate_workflow_journal_links(
    timeline: &chat_state::Timeline,
    run_id: &str,
    journal: &WorkflowJournalCache,
) -> anyhow::Result<()> {
    let owned_subagents = timeline
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(spawn))
                if spawn.workflow_run_id.as_deref() == Some(run_id) =>
            {
                Some(spawn.subagent_id.as_str())
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let mut linked = BTreeSet::new();
    for entry in journal.projection.entries() {
        if entry.kind != "spawn_agent"
            || !matches!(
                journal
                    .projection
                    .replay_operation(entry.seq, &entry.kind, &entry.req_hash)?,
                Some(workflow::journal::OperationReplay::Completed(_))
            )
            || entry
                .result
                .get(workflow::journal::HOST_ERROR_KEY)
                .is_some()
        {
            continue;
        }
        let result = serde_json::from_value::<workflow::AgentResult>(entry.result.clone())?;
        if !owned_subagents.contains(result.agent_id.as_str()) {
            anyhow::bail!(
                "Workflow journal agent '{}' has no owned subagent spawn",
                result.agent_id
            );
        }
        if !linked.insert(result.agent_id.clone()) {
            anyhow::bail!(
                "Workflow journal links subagent '{}' more than once",
                result.agent_id
            );
        }
    }
    Ok(())
}

impl SidebandCache {
    fn refresh_from_directory(
        &mut self,
        directory: &super::storage::ContainedDirectory,
        event_limit: usize,
    ) -> anyhow::Result<()> {
        let path = directory.display_path().join(super::storage::TIMELINE_FILE);
        let mut file = directory.open_regular(
            std::ffi::OsStr::new(super::storage::TIMELINE_FILE),
            "Trajectory sideband ledger",
        )?;
        let opened_stamp = LedgerStamp::from_metadata(&file.metadata()?);
        if self.source_stamp == Some(opened_stamp) {
            return Ok(());
        }
        let replacing = self.source_stamp.is_some();
        let rebuilding = ledger_requires_rebuild(
            self.offset,
            self.prefix_hasher.as_ref(),
            self.tail_hash,
            self.source_stamp,
            opened_stamp,
            &mut file,
        )?;
        let old_offset = self.offset;
        let mut timeline = if rebuilding {
            None
        } else {
            self.timeline.take()
        };
        let mut offset = if rebuilding { 0 } else { self.offset };
        let mut prefix_hasher = if rebuilding {
            None
        } else {
            self.prefix_hasher.clone()
        };
        let mut dirty_seqs = if rebuilding {
            BTreeSet::new()
        } else {
            std::mem::take(&mut self.dirty_seqs)
        };
        let mut observed_stamp = opened_stamp;
        let ingestion = (|| -> anyhow::Result<()> {
            loop {
                let (bytes, complete_len) = read_ledger_batch(&mut file, offset, &path)?;
                if complete_len == 0 {
                    let current_stamp = LedgerStamp::from_metadata(&file.metadata()?);
                    if current_stamp != observed_stamp {
                        observed_stamp = current_stamp;
                        continue;
                    }
                    if rebuilding && replacing && offset == 0 {
                        anyhow::bail!("Trajectory replacement has no committed sideband entry");
                    }
                    break;
                }
                for line in bytes[..complete_len].split(|byte| *byte == b'\n') {
                    if line.is_empty() {
                        continue;
                    }
                    if timeline
                        .as_ref()
                        .map_or(0, |timeline| timeline.events().len())
                        >= event_limit
                    {
                        anyhow::bail!("Trajectory exceeds the event limit");
                    }
                    let event = serde_json::from_slice::<chat_state::SidebandEvent>(line).map_err(
                        |error| anyhow::anyhow!("{} at byte {offset}: {error}", path.display()),
                    )?;
                    let current = match &mut timeline {
                        Some(timeline) => timeline,
                        None => timeline.insert(chat_state::SidebandTimeline::new(
                            event.sideband_id.clone(),
                        )?),
                    };
                    let seq = event.seq;
                    current.accept(event)?;
                    dirty_seqs.insert(seq);
                }
                prefix_hasher
                    .get_or_insert_with(blake3::Hasher::new)
                    .update(&bytes[..complete_len]);
                offset = offset.saturating_add(complete_len as u64);
                observed_stamp = LedgerStamp::from_metadata(&file.metadata()?);
            }
            Ok(())
        })();
        if let Err(error) = ingestion {
            if !rebuilding {
                self.timeline = replay_sideband_prefix(&mut file, old_offset, &path, event_limit)?;
                self.dirty_seqs = self
                    .timeline
                    .as_ref()
                    .into_iter()
                    .flat_map(|timeline| timeline.events().iter().map(|event| event.seq))
                    .collect();
                self.materialization_reset = true;
            }
            return Err(error);
        }
        let tail_hash = match hash_ledger_tail(offset, &mut file) {
            Ok(hash) => hash,
            Err(error) => {
                if !rebuilding {
                    self.timeline =
                        replay_sideband_prefix(&mut file, old_offset, &path, event_limit)?;
                    self.dirty_seqs = self
                        .timeline
                        .as_ref()
                        .into_iter()
                        .flat_map(|timeline| timeline.events().iter().map(|event| event.seq))
                        .collect();
                    self.materialization_reset = true;
                }
                return Err(error);
            }
        };
        if rebuilding {
            *self = Self::default();
            self.materialization_reset = replacing;
        }
        self.timeline = timeline;
        self.dirty_seqs = dirty_seqs;
        self.offset = offset;
        self.prefix_hasher = prefix_hasher;
        self.tail_hash = tail_hash;
        self.source_stamp = Some(observed_stamp);
        Ok(())
    }

    #[cfg(test)]
    fn refresh(&mut self, path: &Path) -> anyhow::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("test ledger has no parent"))?;
        let directory = super::storage::ContainedDirectory::open(
            parent,
            Path::new(""),
            "Trajectory sideband test directory",
            false,
        )?;
        self.refresh_from_directory(&directory, MAX_TRAJECTORY_EVENTS)
    }
}

impl WorkflowJournalCache {
    fn refresh_from_directory(
        &mut self,
        directory: &super::storage::ContainedDirectory,
        event_limit: usize,
    ) -> anyhow::Result<()> {
        let path = directory.display_path().join("journal.jsonl");
        let mut file = directory.open_regular(
            std::ffi::OsStr::new("journal.jsonl"),
            "Trajectory Workflow journal",
        )?;
        let opened = file.metadata()?;
        if !opened.is_file() || opened.len() > workflow::journal::MAX_JOURNAL_BYTES {
            anyhow::bail!("Workflow journal changed during open: {}", path.display());
        }
        let opened_stamp = LedgerStamp::from_metadata(&opened);
        if self.source_stamp == Some(opened_stamp) {
            return Ok(());
        }
        let replacing = self.source_stamp.is_some();
        let rebuilding = ledger_requires_rebuild(
            self.offset,
            self.prefix_hasher.as_ref(),
            self.tail_hash,
            self.source_stamp,
            opened_stamp,
            &mut file,
        )?;
        let old_offset = self.offset;
        let mut projection = if rebuilding {
            workflow::Journal::default()
        } else {
            std::mem::take(&mut self.projection)
        };
        let mut dirty_seqs = if rebuilding {
            BTreeSet::new()
        } else {
            std::mem::take(&mut self.dirty_seqs)
        };
        let mut offset = if rebuilding { 0 } else { self.offset };
        let mut prefix_hasher = if rebuilding {
            None
        } else {
            self.prefix_hasher.clone()
        };
        let mut observed_stamp = opened_stamp;
        let ingestion = (|| -> anyhow::Result<()> {
            loop {
                let (bytes, complete_len) = read_ledger_batch(&mut file, offset, &path)?;
                if complete_len == 0 {
                    let current = file.metadata()?;
                    if !current.is_file() || current.len() > workflow::journal::MAX_JOURNAL_BYTES {
                        anyhow::bail!("Workflow journal changed during read: {}", path.display());
                    }
                    let current_stamp = LedgerStamp::from_metadata(&current);
                    if current_stamp != observed_stamp {
                        observed_stamp = current_stamp;
                        continue;
                    }
                    if rebuilding && replacing && offset == 0 {
                        anyhow::bail!("Trajectory replacement has no committed Workflow entry");
                    }
                    break;
                }
                for line in bytes[..complete_len].split(|byte| *byte == b'\n') {
                    if line.is_empty() {
                        continue;
                    }
                    let entry = serde_json::from_slice::<workflow::JournalEntry>(line)?;
                    dirty_seqs.insert(entry.seq);
                    project_workflow_entry(&mut projection, entry, event_limit)?;
                }
                prefix_hasher
                    .get_or_insert_with(blake3::Hasher::new)
                    .update(&bytes[..complete_len]);
                offset = offset.saturating_add(complete_len as u64);
                let current = file.metadata()?;
                if !current.is_file() || current.len() > workflow::journal::MAX_JOURNAL_BYTES {
                    anyhow::bail!("Workflow journal changed during read: {}", path.display());
                }
                observed_stamp = LedgerStamp::from_metadata(&current);
            }
            Ok(())
        })();
        if let Err(error) = ingestion {
            if !rebuilding {
                self.projection =
                    replay_workflow_prefix(&mut file, old_offset, &path, event_limit)?;
                self.dirty_seqs = self
                    .projection
                    .entries()
                    .iter()
                    .map(|entry| entry.seq)
                    .collect();
                self.materialization_reset = true;
            }
            return Err(error);
        }
        let tail_hash = match hash_ledger_tail(offset, &mut file) {
            Ok(hash) => hash,
            Err(error) => {
                if !rebuilding {
                    self.projection =
                        replay_workflow_prefix(&mut file, old_offset, &path, event_limit)?;
                    self.dirty_seqs = self
                        .projection
                        .entries()
                        .iter()
                        .map(|entry| entry.seq)
                        .collect();
                    self.materialization_reset = true;
                }
                return Err(error);
            }
        };
        if rebuilding {
            *self = Self::default();
            self.materialization_reset = replacing;
        }
        self.projection = projection;
        self.dirty_seqs = dirty_seqs;
        self.offset = offset;
        self.prefix_hasher = prefix_hasher;
        self.tail_hash = tail_hash;
        self.source_stamp = Some(observed_stamp);
        Ok(())
    }

    #[cfg(test)]
    fn refresh(&mut self, path: &Path) -> anyhow::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("test ledger has no parent"))?;
        let directory = super::storage::ContainedDirectory::open(
            parent,
            Path::new(""),
            "Trajectory Workflow test directory",
            false,
        )?;
        self.refresh_from_directory(&directory, MAX_TRAJECTORY_EVENTS)
    }
}

fn sideband_row(
    event: &chat_state::SidebandEvent,
    parent_entry_id: &str,
    path_prefix: &[u64],
    attempt_times: &BTreeMap<u64, i64>,
) -> chat_state::TrajectoryRow {
    let entry_id = format!("t:{}/{}", event.sideband_id, event.seq);
    let (kind, state, producer, summary, duration_ms) = match &event.kind {
        chat_state::SidebandEventKind::Request(request) => (
            "sideband.request",
            "created".into(),
            "core",
            format!("{} · {}", request.purpose.as_str(), request.route.model),
            None,
        ),
        chat_state::SidebandEventKind::Attempt(attempt) => (
            "sideband.attempt",
            "started".into(),
            "model",
            attempt.feedback.as_deref().map_or_else(
                || format!("attempt {}", attempt.attempt_no),
                |feedback| {
                    format!(
                        "attempt {} · {}",
                        attempt.attempt_no,
                        crate::util::truncate(feedback, 180)
                    )
                },
            ),
            None,
        ),
        chat_state::SidebandEventKind::Result(result) => {
            let attempt = result.source_event_seqs[1];
            let duration = attempt_times
                .get(&attempt)
                .and_then(|started| event.at_ms.checked_sub(*started))
                .and_then(|duration| u64::try_from(duration).ok());
            (
                "sideband.result",
                "completed".into(),
                "model",
                crate::util::truncate(&result.raw_output, 240).to_string(),
                duration,
            )
        }
        chat_state::SidebandEventKind::End(end) => (
            "sideband.end",
            match end.outcome {
                chat_state::SidebandOutcome::Completed => "completed",
                chat_state::SidebandOutcome::Failed => "failed",
                chat_state::SidebandOutcome::Cancelled => "cancelled",
            }
            .into(),
            "core",
            end.error
                .as_deref()
                .map(|error| crate::util::truncate(error, 240).to_string())
                .unwrap_or_else(|| "completed".into()),
            None,
        ),
    };
    chat_state::TrajectoryRow {
        entry_id,
        seq: event.seq,
        parent_entry_id: Some(parent_entry_id.to_owned()),
        nesting_path: path_prefix
            .iter()
            .copied()
            .chain(std::iter::once(event.seq))
            .collect(),
        at_ms: event.at_ms,
        layer: "meta".into(),
        actor: format!("sideband:{}", event.sideband_id),
        class: "auxiliary".into(),
        producer: producer.into(),
        kind: kind.into(),
        state,
        visibility: chat_state::SurfaceVisibility::LogOnly,
        turn_id: None,
        step_index: None,
        correlation_id: Some(event.sideband_id.clone()),
        duration_ms,
        summary,
        details: serde_json::Value::Null,
    }
}

fn visibility_name(value: chat_state::SurfaceVisibility) -> &'static str {
    match value {
        chat_state::SurfaceVisibility::Current => "current",
        chat_state::SurfaceVisibility::Shadowed => "shadowed",
        chat_state::SurfaceVisibility::LogOnly => "log_only",
    }
}

type HttpError = (StatusCode, HeaderMap, String);

fn http_error(status: StatusCode, message: impl Into<String>) -> HttpError {
    (status, response_security_headers(), message.into())
}

fn require_local_host(headers: &HeaderMap) -> Result<(), HttpError> {
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| http_error(StatusCode::FORBIDDEN, "missing Host header"))?;
    let authority = host
        .parse::<axum::http::uri::Authority>()
        .map_err(|_| http_error(StatusCode::FORBIDDEN, "invalid Host header"))?;
    let authority_host = authority.host();
    let ip_host = authority_host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(authority_host);
    let local = authority_host.eq_ignore_ascii_case("localhost")
        || ip_host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
    if local {
        Ok(())
    } else {
        Err(http_error(
            StatusCode::FORBIDDEN,
            "non-loopback Host rejected",
        ))
    }
}

fn internal_error(error: impl std::fmt::Display) -> HttpError {
    tracing::error!(error = %error, "Trajectory query failed");
    http_error(StatusCode::INTERNAL_SERVER_ERROR, "Trajectory query failed")
}

fn query_error_response(error: anyhow::Error) -> HttpError {
    if error.downcast_ref::<TrajectoryEntryNotFound>().is_some() {
        http_error(StatusCode::NOT_FOUND, error.to_string())
    } else if error.downcast_ref::<TrajectoryEventTooLarge>().is_some() {
        http_error(StatusCode::PAYLOAD_TOO_LARGE, error.to_string())
    } else {
        internal_error(error)
    }
}

fn response_security_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(
            "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
        ),
    );
    headers.insert(
        header::HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    headers
}

fn replay_timeline_prefix(
    file: &mut std::fs::File,
    offset: u64,
    path: &Path,
    event_limit: usize,
) -> anyhow::Result<(chat_state::Timeline, chat_state::TrajectoryProjector)> {
    file.seek(std::io::SeekFrom::Start(0))?;
    let mut reader = std::io::BufReader::new((&mut *file).take(offset));
    let mut timeline = chat_state::Timeline::default();
    let mut projector = chat_state::TrajectoryProjector::default();
    let mut line = Vec::new();
    let mut consumed = 0_u64;
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        consumed = consumed.saturating_add(read as u64);
        if line.last() != Some(&b'\n') {
            anyhow::bail!("{} has an uncommitted cached prefix", path.display());
        }
        line.pop();
        if line.is_empty() {
            continue;
        }
        if timeline.events().len() >= event_limit {
            anyhow::bail!("Trajectory exceeds the event limit");
        }
        let event = serde_json::from_slice::<chat_state::TimelineEvent>(&line)?;
        timeline.accept(event.clone())?;
        projector.accept(&event);
    }
    if consumed != offset {
        anyhow::bail!(
            "{} changed while restoring its cached prefix",
            path.display()
        );
    }
    Ok((timeline, projector))
}

fn replay_workflow_prefix(
    file: &mut std::fs::File,
    offset: u64,
    path: &Path,
    event_limit: usize,
) -> anyhow::Result<workflow::Journal> {
    file.seek(std::io::SeekFrom::Start(0))?;
    let mut reader = std::io::BufReader::new((&mut *file).take(offset));
    let mut projection = workflow::Journal::default();
    let mut line = Vec::new();
    let mut consumed = 0_u64;
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        consumed = consumed.saturating_add(read as u64);
        if line.last() != Some(&b'\n') {
            anyhow::bail!("{} has an uncommitted cached prefix", path.display());
        }
        line.pop();
        if line.is_empty() {
            continue;
        }
        let entry = serde_json::from_slice::<workflow::JournalEntry>(&line)?;
        project_workflow_entry(&mut projection, entry, event_limit)?;
    }
    if consumed != offset {
        anyhow::bail!(
            "{} changed while restoring its cached prefix",
            path.display()
        );
    }
    Ok(projection)
}

fn replay_sideband_prefix(
    file: &mut std::fs::File,
    offset: u64,
    path: &Path,
    event_limit: usize,
) -> anyhow::Result<Option<chat_state::SidebandTimeline>> {
    file.seek(std::io::SeekFrom::Start(0))?;
    let mut reader = std::io::BufReader::new((&mut *file).take(offset));
    let mut timeline: Option<chat_state::SidebandTimeline> = None;
    let mut line = Vec::new();
    let mut consumed = 0_u64;
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        consumed = consumed.saturating_add(read as u64);
        if line.last() != Some(&b'\n') {
            anyhow::bail!("{} has an uncommitted cached prefix", path.display());
        }
        line.pop();
        if line.is_empty() {
            continue;
        }
        if timeline
            .as_ref()
            .map_or(0, |timeline| timeline.events().len())
            >= event_limit
        {
            anyhow::bail!("Trajectory exceeds the event limit");
        }
        let event = serde_json::from_slice::<chat_state::SidebandEvent>(&line)?;
        let current = match &mut timeline {
            Some(timeline) => timeline,
            None => timeline.insert(chat_state::SidebandTimeline::new(
                event.sideband_id.clone(),
            )?),
        };
        current.accept(event)?;
    }
    if consumed != offset {
        anyhow::bail!(
            "{} changed while restoring its cached prefix",
            path.display()
        );
    }
    Ok(timeline)
}

fn read_ledger_batch(
    file: &mut std::fs::File,
    offset: u64,
    path: &Path,
) -> anyhow::Result<(Vec<u8>, usize)> {
    file.seek(std::io::SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take(super::storage::MAX_JSONL_ENTRY_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)?;
    let complete_len = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    if complete_len == 0 && bytes.len() as u64 > super::storage::MAX_JSONL_ENTRY_BYTES {
        let mut buffer = [0_u8; 8 * 1024];
        let mut committed = false;
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            if buffer[..read].contains(&b'\n') {
                committed = true;
                break;
            }
        }
        if committed {
            anyhow::bail!(
                "Trajectory ledger entry exceeds {} bytes at {} byte {}",
                super::storage::MAX_JSONL_ENTRY_BYTES,
                path.display(),
                offset
            );
        }
        return Ok((Vec::new(), 0));
    }
    if bytes[..complete_len]
        .split(|byte| *byte == b'\n')
        .any(|line| {
            !line.is_empty()
                && (line.len() as u64).saturating_add(1) > super::storage::MAX_JSONL_ENTRY_BYTES
        })
    {
        anyhow::bail!(
            "Trajectory ledger entry exceeds {} bytes at {} byte {}",
            super::storage::MAX_JSONL_ENTRY_BYTES,
            path.display(),
            offset
        );
    }
    Ok((bytes, complete_len))
}

fn ledger_digest(hasher: Option<&blake3::Hasher>) -> [u8; 32] {
    hasher.map_or([0; 32], |hasher| *hasher.finalize().as_bytes())
}

fn ledger_requires_rebuild(
    offset: u64,
    prefix_hasher: Option<&blake3::Hasher>,
    tail_hash: Option<[u8; 32]>,
    previous_stamp: Option<LedgerStamp>,
    opened_stamp: LedgerStamp,
    file: &mut std::fs::File,
) -> anyhow::Result<bool> {
    let Some(previous_stamp) = previous_stamp else {
        return Ok(true);
    };
    if !previous_stamp.same_file_as(opened_stamp) || opened_stamp.len < offset {
        return Ok(true);
    }
    if offset == 0 {
        return Ok(false);
    }
    if opened_stamp.len > offset {
        // The authoritative writer only grows an existing ledger by append.
        // Checking its last committed block protects the append boundary
        // without making every live poll proportional to the full history.
        return Ok(!ledger_tail_matches(offset, tail_hash, file)?);
    }
    let Some(expected) = prefix_hasher.map(|hasher| *hasher.finalize().as_bytes()) else {
        return Ok(true);
    };
    Ok(!hash_ledger_prefix(offset, file)?.is_some_and(|actual| actual == expected))
}

fn ledger_tail_matches(
    offset: u64,
    expected: Option<[u8; 32]>,
    file: &mut std::fs::File,
) -> anyhow::Result<bool> {
    if offset == 0 {
        return Ok(expected.is_none());
    }
    let Some(expected) = expected else {
        return Ok(false);
    };
    Ok(hash_ledger_tail(offset, file)?.is_some_and(|actual| actual == expected))
}

fn hash_ledger_tail(offset: u64, file: &mut std::fs::File) -> anyhow::Result<Option<[u8; 32]>> {
    if offset == 0 {
        return Ok(None);
    }
    let length = offset.min(LEDGER_TAIL_CHECK_BYTES);
    file.seek(std::io::SeekFrom::Start(offset - length))?;
    let mut bytes = vec![0; usize::try_from(length)?];
    file.read_exact(&mut bytes)?;
    Ok(Some(*blake3::hash(&bytes).as_bytes()))
}

fn hash_ledger_prefix(offset: u64, file: &mut std::fs::File) -> anyhow::Result<Option<[u8; 32]>> {
    file.seek(std::io::SeekFrom::Start(0))?;
    let mut hasher = blake3::Hasher::new();
    let mut remaining = offset;
    let mut buffer = [0_u8; 16 * 1024];
    while remaining > 0 {
        let wanted = usize::try_from(remaining.min(buffer.len() as u64))?;
        let read = file.read(&mut buffer[..wanted])?;
        if read == 0 {
            return Ok(None);
        }
        hasher.update(&buffer[..read]);
        remaining -= read as u64;
    }
    Ok(Some(*hasher.finalize().as_bytes()))
}

const PAGE: &str = include_str!("trajectory.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trajectory_read_budget_is_global_across_the_tree() {
        let mut depth = TrajectoryReadBudget::default();
        assert!(
            depth
                .enter_entity("too-deep", MAX_TRAJECTORY_DEPTH + 1)
                .unwrap_err()
                .to_string()
                .contains("depth")
        );

        let mut entities = TrajectoryReadBudget {
            entities: MAX_TRAJECTORY_ENTITIES,
            ..Default::default()
        };
        assert!(entities.enter_entity("one-too-many", 0).is_err());

        let mut events = TrajectoryReadBudget {
            events: MAX_TRAJECTORY_EVENTS,
            ..Default::default()
        };
        assert!(events.admit_events(1).is_err());
    }

    #[test]
    fn trajectory_responses_are_private_and_non_embeddable() {
        let headers = response_security_headers();
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
        assert_eq!(headers["referrer-policy"], "no-referrer");
        assert_eq!(headers["x-content-type-options"], "nosniff");
        assert_eq!(headers["x-frame-options"], "DENY");
        let csp = headers["content-security-policy"].to_str().unwrap();
        assert!(csp.contains("connect-src 'self'"));
        assert!(csp.contains("frame-ancestors 'none'"));
    }

    #[tokio::test]
    async fn token_routes_serve_both_page_forms_and_the_api() {
        let temp = tempfile::tempdir().unwrap();
        let session_dir = temp.path().join("session");
        std::fs::create_dir_all(&session_dir).unwrap();
        let timeline =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::user(
                "inspect the event",
            )])
            .unwrap();
        write_timeline(
            &session_dir.join(super::super::storage::TIMELINE_FILE),
            &timeline,
        );
        let state = AppState {
            session_id: "canonical-session".into(),
            actor_ref: "main".into(),
            session_dir,
            sessions_root: temp.path().join("sessions"),
            cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, trajectory_router("secret", state))
                .await
                .unwrap();
        });
        let client = reqwest::Client::new();

        for path in ["/secret", "/secret/"] {
            let response = client
                .get(format!("http://{address}{path}"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "path {path}");
            assert!(response.text().await.unwrap().contains("Grow Trajectory"));
        }
        let response = client
            .get(format!("http://{address}/secret/api/trajectory"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["sessionId"], "canonical-session");
        assert_eq!(body["rows"].as_array().unwrap().len(), 1);
        assert!(body["rows"][0].get("details").is_none());
        assert_eq!(body["overview"]["dimension"], "interaction");
        assert_eq!(body["overview"]["counts"]["input"], 1);

        let response = client
            .get(format!(
                "http://{address}/secret/api/trajectory/event?entry=t%3Acanonical-session%2F0"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = response.json().await.unwrap();
        assert!(body["row"].get("details").is_some());

        let response = client
            .get(format!(
                "http://{address}/secret/api/trajectory/event?entry=t%3Amissing%2F0"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()["x-frame-options"], "DENY");

        let response = client
            .get(format!(
                "http://{address}/secret/api/trajectory?after=a&before=b"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");

        for path in [
            "/secret/api/trajectory?limit=not-a-number",
            "/secret/api/trajectory/event",
            "/secret/missing",
        ] {
            let response = client
                .get(format!("http://{address}{path}"))
                .send()
                .await
                .unwrap();
            assert!(response.status().is_client_error(), "path {path}");
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        }

        use std::io::Write as _;
        writeln!(
            std::fs::OpenOptions::new()
                .append(true)
                .open(
                    temp.path()
                        .join("session")
                        .join(super::super::storage::TIMELINE_FILE)
                )
                .unwrap(),
            "{{not-json}}"
        )
        .unwrap();
        let response = client
            .get(format!("http://{address}/secret/api/trajectory"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.text().await.unwrap(), "Trajectory query failed");

        server.abort();
        let _ = server.await;
    }

    fn write_timeline(path: &Path, timeline: &chat_state::Timeline) {
        let body = timeline
            .events()
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(path, body).unwrap();
    }

    fn write_timeline_with_start_time(path: &Path, timeline: &chat_state::Timeline, start_ms: i64) {
        let body = timeline
            .events()
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, mut event)| {
                event.at_ms = start_ms.saturating_add(index as i64);
                serde_json::to_string(&event).unwrap()
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(path, body).unwrap();
    }

    fn write_child_session(
        sessions_root: &Path,
        cwd: &str,
        session_id: &str,
        parent_session_id: &str,
        timeline: &chat_state::Timeline,
    ) -> PathBuf {
        let session_dir = sessions_root
            .join(crate::util::grow_home::encode_cwd_dirname(cwd))
            .join(session_id);
        std::fs::create_dir_all(&session_dir).unwrap();
        let info = client_support::session::Info {
            id: agent_client_protocol::SessionId::new(session_id.to_owned()),
            cwd: cwd.to_owned(),
        };
        let mut summary = super::super::persistence::Summary::new(
            &info,
            agent_client_protocol::ModelId::new("model"),
        )
        .unwrap();
        summary.parent_session_id = Some(parent_session_id.to_owned());
        summary.session_kind = Some("subagent".into());
        std::fs::write(
            session_dir.join(super::super::storage::SUMMARY_FILE),
            serde_json::to_vec(&summary).unwrap(),
        )
        .unwrap();
        write_timeline(
            &session_dir.join(super::super::storage::TIMELINE_FILE),
            timeline,
        );
        session_dir
    }

    fn subagent_spawn(
        subagent_id: &str,
        child_session_id: &str,
        child_cwd: &str,
    ) -> chat_state::SubagentSpawnEvent {
        chat_state::SubagentSpawnEvent {
            subagent_id: subagent_id.into(),
            child_session_id: child_session_id.into(),
            security_parent_session_id: "parent-session".into(),
            subagent_type: "explore".into(),
            description: "inspect architecture".into(),
            prompt: "trace the canonical state".into(),
            context_source: chat_state::SubagentContextSource::New,
            source_ref: None,
            context_normalized: false,
            resumed_from: None,
            parent_prompt_id: None,
            capability_mode: None,
            permission_mode: None,
            effective_permission_mode: None,
            workflow_run_id: None,
            goal_id: None,
            surface_completion: true,
            child_cwd: child_cwd.into(),
            worktree_path: None,
            effective_model_id: "model".into(),
            model_transport_key: sampling_types::ModelImageInputKey::new(
                "model",
                "responses",
                "test-endpoint",
            ),
            reasoning_effort: None,
        }
    }

    fn subagent_seed(
        parent_timeline_id: &str,
        parent_spawn_seq: u64,
        subagent_id: &str,
    ) -> chat_state::SubagentSeedEvent {
        chat_state::SubagentSeedEvent {
            parent_timeline_id: parent_timeline_id.into(),
            parent_spawn_seq,
            subagent_id: subagent_id.into(),
            security_parent_session_id: parent_timeline_id.into(),
            context_source: chat_state::SubagentContextSource::New,
            source_ref: None,
            normalized: false,
        }
    }

    #[test]
    fn cache_ignores_then_consumes_an_incomplete_tail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.jsonl");
        let timeline =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::user("hello")])
                .unwrap();
        let line = serde_json::to_string(&timeline.events()[0]).unwrap();
        let mut bytes = format!("{line}\n{{\"version\":").into_bytes();
        bytes.extend_from_slice(&[0xe2, 0x82]);
        std::fs::write(&path, bytes).unwrap();
        let mut cache = SessionTrajectoryCache::default();
        cache.refresh(&path).unwrap();
        assert_eq!(cache.timeline.events().len(), 1);

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        use std::io::Write;
        file.write_all(b"}\n").unwrap();
        assert!(
            cache.refresh(&path).is_err(),
            "completed malformed tail is rejected"
        );
    }

    #[test]
    fn timeline_cache_detects_same_length_ledger_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.jsonl");
        let first =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::user("alpha")])
                .unwrap();
        let second =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::user("bravo")])
                .unwrap();
        write_timeline(&path, &first);
        let first_metadata = std::fs::metadata(&path).unwrap();
        let first_len = first_metadata.len();
        let first_mtime = filetime::FileTime::from_last_modification_time(&first_metadata);

        let mut cache = SessionTrajectoryCache::default();
        cache.refresh(&path).unwrap();
        assert_eq!(cache.timeline.surface()[0].text_content(), "alpha");

        write_timeline(&path, &second);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), first_len);
        filetime::set_file_mtime(&path, first_mtime).unwrap();
        cache.refresh(&path).unwrap();
        assert_eq!(cache.timeline.events().len(), 1);
        assert_eq!(cache.timeline.surface()[0].text_content(), "bravo");
    }

    #[test]
    fn timeline_cache_hashes_the_entire_large_consumed_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.jsonl");
        let large = |byte: char| {
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::user(
                byte.to_string().repeat(10_000),
            )])
            .unwrap()
        };
        let first = large('a');
        write_timeline(&path, &first);
        assert!(std::fs::metadata(&path).unwrap().len() > 8192);
        let mut cache = SessionTrajectoryCache::default();
        cache.refresh(&path).unwrap();

        let second = large('b');
        write_timeline(&path, &second);
        cache.refresh(&path).unwrap();
        assert!(cache.timeline.surface()[0].text_content().starts_with('b'));

        let mut third = large('c');
        third
            .append(
                sampling_types::ConversationItem::assistant("new tail"),
                chat_state::MessageCause::Assistant,
            )
            .unwrap();
        write_timeline(&path, &third);
        cache.refresh(&path).unwrap();
        assert_eq!(cache.timeline.events().len(), 2);
        assert!(cache.timeline.surface()[0].text_content().starts_with('c'));
    }

    #[test]
    fn sideband_cache_hashes_the_entire_large_consumed_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.jsonl");
        let sideband_id = "018f0000-0000-7000-8000-000000000099";
        let event = |byte: char| chat_state::SidebandEvent {
            version: chat_state::SIDEBAND_SCHEMA_VERSION,
            sideband_id: sideband_id.into(),
            seq: 0,
            at_ms: 1,
            kind: chat_state::SidebandEventKind::Request(chat_state::SidebandRequest {
                purpose: chat_state::SidebandPurpose::ContextRecall,
                prompt: byte.to_string().repeat(10_000),
                source_refs: Vec::new(),
                budget_policy: chat_state::SidebandBudgetPolicy {
                    max_attempts: 1,
                    max_input_tokens_per_attempt: 1,
                    max_output_tokens_per_attempt: None,
                },
                route: chat_state::SidebandRoute {
                    model: "model".into(),
                    backend: sampling_types::ApiBackend::Responses,
                },
                initiator_ref: "parent/1".into(),
                executor: "executor".into(),
                output_schema: None,
            }),
        };
        let write = |events: &[chat_state::SidebandEvent]| {
            let bytes = events
                .iter()
                .map(|event| format!("{}\n", serde_json::to_string(event).unwrap()))
                .collect::<String>();
            std::fs::write(&path, bytes).unwrap();
        };
        write(&[event('a')]);
        assert!(std::fs::metadata(&path).unwrap().len() > 8192);
        let mut cache = SidebandCache::default();
        cache.refresh(&path).unwrap();
        write(&[event('b')]);
        cache.refresh(&path).unwrap();
        let events = cache.timeline.as_ref().unwrap().events();
        let chat_state::SidebandEventKind::Request(request) = &events[0].kind else {
            panic!("expected request");
        };
        assert!(request.prompt.starts_with('b'));

        let third = event('c');
        let terminal = chat_state::SidebandEvent {
            version: chat_state::SIDEBAND_SCHEMA_VERSION,
            sideband_id: sideband_id.into(),
            seq: 1,
            at_ms: 2,
            kind: chat_state::SidebandEventKind::End(chat_state::SidebandEnd {
                outcome: chat_state::SidebandOutcome::Failed,
                error: Some("stopped".into()),
            }),
        };
        write(&[third, terminal]);
        cache.refresh(&path).unwrap();
        let events = cache.timeline.as_ref().unwrap().events();
        assert_eq!(events.len(), 2);
        let chat_state::SidebandEventKind::Request(request) = &events[0].kind else {
            panic!("expected request");
        };
        assert!(request.prompt.starts_with('c'));
    }

    #[cfg(unix)]
    #[test]
    fn timeline_cache_rejects_symlinked_ledgers() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.jsonl");
        let link = dir.path().join("timeline.jsonl");
        let timeline =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::user("secret")])
                .unwrap();
        write_timeline(&target, &timeline);
        symlink(&target, &link).unwrap();

        let error = SessionTrajectoryCache::default()
            .refresh(&link)
            .expect_err("Trajectory must not follow ledger symlinks");
        assert!(error.to_string().contains("symlink"), "{error:#}");
    }

    #[test]
    fn malformed_batch_does_not_partially_advance_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.jsonl");
        let mut timeline =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::user("one")])
                .unwrap();
        std::fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::to_string(&timeline.events()[0]).unwrap()
            ),
        )
        .unwrap();
        let mut cache = SessionTrajectoryCache::default();
        cache.refresh(&path).unwrap();
        let committed_offset = cache.offset;

        let event = timeline
            .append(
                sampling_types::ConversationItem::assistant("two"),
                chat_state::MessageCause::Assistant,
            )
            .unwrap();
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{}", serde_json::to_string(&event).unwrap()).unwrap();
        writeln!(file, "{{not-json}}").unwrap();

        assert!(cache.refresh(&path).is_err());
        assert_eq!(cache.offset, committed_offset);
        assert_eq!(cache.timeline.events().len(), 1);
        assert_eq!(cache.projector.rows().len(), 1);
    }

    #[test]
    fn malformed_timeline_replacement_preserves_the_verified_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.jsonl");
        let first =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::user("first")])
                .unwrap();
        let replacement =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::user(
                "replacement",
            )])
            .unwrap();
        write_timeline(&path, &first);
        let mut cache = SessionTrajectoryCache::default();
        cache.refresh(&path).unwrap();
        let committed_offset = cache.offset;

        std::fs::write(
            &path,
            format!(
                "{}\n{{not-json}}\n",
                serde_json::to_string(&replacement.events()[0]).unwrap()
            ),
        )
        .unwrap();
        assert!(cache.refresh(&path).is_err());
        assert_eq!(cache.offset, committed_offset);
        assert_eq!(cache.timeline.surface()[0].text_content(), "first");
        assert_eq!(cache.projector.rows().len(), 1);
    }

    #[test]
    fn workflow_journal_cache_detects_same_length_tail_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        let entry = |result: &str| workflow::JournalEntry {
            seq: 0,
            kind: "log".into(),
            req_hash: "abcd".into(),
            result: serde_json::Value::String(result.into()),
            at_ms: 1,
        };
        let write = |entry: &workflow::JournalEntry| {
            std::fs::write(
                &path,
                format!("{}\n", serde_json::to_string(entry).unwrap()),
            )
            .unwrap();
        };
        write(&entry("aa"));
        let first_mtime =
            filetime::FileTime::from_last_modification_time(&std::fs::metadata(&path).unwrap());
        let mut cache = WorkflowJournalCache::default();
        cache.refresh(&path).unwrap();
        assert_eq!(
            cache.projection.entries()[0].result,
            serde_json::json!("aa")
        );

        write(&entry("bb"));
        filetime::set_file_mtime(&path, first_mtime).unwrap();
        cache.refresh(&path).unwrap();
        assert_eq!(cache.projection.len(), 1);
        assert_eq!(
            cache.projection.entries()[0].result,
            serde_json::json!("bb")
        );
    }

    #[test]
    fn workflow_cache_hashes_the_entire_large_consumed_prefix_and_rebuilds_before_append() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        let entry = |seq, byte: char| workflow::JournalEntry {
            seq,
            kind: "log".into(),
            req_hash: format!("hash-{seq}"),
            result: serde_json::Value::String(byte.to_string().repeat(10_000)),
            at_ms: seq,
        };
        let write = |entries: &[workflow::JournalEntry]| {
            let bytes = entries
                .iter()
                .map(|entry| format!("{}\n", serde_json::to_string(entry).unwrap()))
                .collect::<String>();
            std::fs::write(&path, bytes).unwrap();
        };
        write(&[entry(0, 'a')]);
        assert!(std::fs::metadata(&path).unwrap().len() > 8192);
        let mut cache = WorkflowJournalCache::default();
        cache.refresh(&path).unwrap();
        write(&[entry(0, 'b')]);
        cache.refresh(&path).unwrap();
        assert_eq!(
            cache.projection.entries()[0]
                .result
                .as_str()
                .unwrap()
                .as_bytes()[0],
            b'b'
        );

        write(&[entry(0, 'c'), entry(1, 'd')]);
        cache.refresh(&path).unwrap();
        assert_eq!(cache.projection.len(), 2);
        assert_eq!(
            cache.projection.entries()[0]
                .result
                .as_str()
                .unwrap()
                .as_bytes()[0],
            b'c'
        );
    }

    #[test]
    fn workflow_journal_malformed_batch_does_not_partially_advance_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        let entry = |seq| workflow::JournalEntry {
            seq,
            kind: "log".into(),
            req_hash: format!("hash-{seq}"),
            result: serde_json::Value::Null,
            at_ms: seq,
        };
        std::fs::write(
            &path,
            format!("{}\n", serde_json::to_string(&entry(0)).unwrap()),
        )
        .unwrap();
        let mut cache = WorkflowJournalCache::default();
        cache.refresh(&path).unwrap();
        let committed_offset = cache.offset;
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{}", serde_json::to_string(&entry(1)).unwrap()).unwrap();
        writeln!(file, "{{not-json}}").unwrap();

        assert!(cache.refresh(&path).is_err());
        assert_eq!(cache.projection.len(), 1);
        assert_eq!(cache.offset, committed_offset);
    }

    #[test]
    fn malformed_workflow_replacement_preserves_the_verified_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        let entry = |result: &str| workflow::JournalEntry {
            seq: 0,
            kind: "log".into(),
            req_hash: "request".into(),
            result: serde_json::Value::String(result.into()),
            at_ms: 1,
        };
        std::fs::write(
            &path,
            format!("{}\n", serde_json::to_string(&entry("first")).unwrap()),
        )
        .unwrap();
        let mut cache = WorkflowJournalCache::default();
        cache.refresh(&path).unwrap();
        let committed_offset = cache.offset;

        std::fs::write(
            &path,
            format!(
                "{}\n{{not-json}}\n",
                serde_json::to_string(&entry("replacement")).unwrap()
            ),
        )
        .unwrap();
        assert!(cache.refresh(&path).is_err());
        assert_eq!(cache.offset, committed_offset);
        assert_eq!(
            cache.projection.entries()[0].result,
            serde_json::json!("first")
        );
    }

    #[test]
    fn workflow_journal_cache_folds_pending_and_completed_physical_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        let pending = workflow::JournalEntry {
            seq: 0,
            kind: "spawn_agent".into(),
            req_hash: "request".into(),
            result: serde_json::json!({"__workflow_operation_pending": "operation"}),
            at_ms: 1,
        };
        let completed = workflow::JournalEntry {
            result: serde_json::json!({
                "agent_id": "agent-1",
                "success": true,
                "output": "completed",
                "cancelled": false,
                "tokens_used": 1,
                "duration_ms": 1
            }),
            at_ms: 2,
            ..pending.clone()
        };
        std::fs::write(
            &path,
            format!(
                "{}\n{}\n",
                serde_json::to_string(&pending).unwrap(),
                serde_json::to_string(&completed).unwrap()
            ),
        )
        .unwrap();

        let mut cache = WorkflowJournalCache::default();
        cache.refresh(&path).unwrap();

        assert_eq!(cache.projection.len(), 1);
        assert_eq!(cache.projection.entries()[0].result["agent_id"], "agent-1");
    }

    #[test]
    fn workflow_result_preview_never_serializes_large_structured_values() {
        let scalar =
            workflow_result_preview(&serde_json::Value::String("payload".repeat(1_000_000)));
        assert_eq!(scalar.chars().count(), 220);

        let structured = workflow_result_preview(&serde_json::json!({
            "large": "payload".repeat(1_000_000),
            "status": "completed",
        }));
        assert!(structured.starts_with("object · 2 fields · "));
        assert!(structured.contains("large"));
        assert!(structured.contains("status"));
        assert!(!structured.contains("payload"));
        assert!(structured.len() < 128);
    }

    #[test]
    fn workflow_journal_is_nested_under_spawn_and_uses_run_identity() {
        let dir = tempfile::tempdir().unwrap();
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Spawned {
                    run_id: "wf_debug".into(),
                    execution_epoch: 0,
                    name: "debug".into(),
                    objective: "trace host calls".into(),
                },
            ))
            .unwrap();
        let mut child_spawn = subagent_spawn("sa-debug", "child-debug", "/tmp");
        child_spawn.workflow_run_id = Some("wf_debug".into());
        timeline
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Spawned(child_spawn),
            ))
            .unwrap();
        timeline
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Ended(chat_state::SubagentTerminalEvent {
                    subagent_id: "sa-debug".into(),
                    child_session_id: "child-debug".into(),
                    outcome: chat_state::SubagentOutcome::Cancelled,
                    duration_ms: 4,
                    tool_calls: 0,
                    turns: 0,
                    tokens_used: 7,
                    error: Some("cancelled".into()),
                    result_ref: None,
                    snapshot_ref: None,
                }),
            ))
            .unwrap();
        timeline
            .record(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Ended {
                    run_id: "wf_debug".into(),
                    execution_epoch: 0,
                    status: chat_state::WorkflowExecutionStatus::Complete,
                    duration_ms: 9,
                    message: None,
                },
            ))
            .unwrap();
        write_timeline(&dir.path().join("timeline.jsonl"), &timeline);

        let run_dir = dir.path().join("workflows/wf_debug");
        std::fs::create_dir_all(&run_dir).unwrap();
        let mut tracker = super::super::workflow::tracker::WorkflowTracker::default();
        let mut state = tracker.start_run(
            "wf_debug".into(),
            "debug".into(),
            "trace host calls".into(),
            Vec::new(),
            Some(4),
            Some("workflows/wf_debug/journal.jsonl".into()),
            super::super::workflow::tracker::WorkflowRuntimeRoute::for_test(
                "test-model",
                None,
                sampling_types::ModelImageInputKey::new("test-model", "responses", "test-endpoint"),
            )
            .unwrap(),
        );
        state = tracker
            .apply_outcome(
                "wf_debug",
                &workflow::WorkflowOutcome::Completed {
                    result: serde_json::json!("done"),
                },
            )
            .unwrap_or(state);
        let manifest = super::super::workflow::store::WorkflowRunManifest {
            version: super::super::workflow::store::WORKFLOW_RUN_MANIFEST_VERSION,
            state,
            script_revision: 0,
        };
        std::fs::write(
            run_dir.join("state.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let entries = [
            workflow::JournalEntry {
                seq: 0,
                kind: "spawn_agent".into(),
                req_hash: "hash-0".into(),
                result: serde_json::json!({
                    "agent_id": "sa-debug",
                    "success": false,
                    "output": "cancelled",
                    "cancelled": true,
                    "tokens_used": 7,
                    "duration_ms": 4
                }),
                at_ms: 2,
            },
            workflow::JournalEntry {
                seq: 1,
                kind: "budget".into(),
                req_hash: "hash-1".into(),
                result: serde_json::json!({"remaining": 3}),
                at_ms: 3,
            },
        ];
        std::fs::write(
            run_dir.join("journal.jsonl"),
            entries
                .iter()
                .map(|entry| serde_json::to_string(entry).unwrap())
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .unwrap();

        let state = AppState {
            session_id: "session".into(),
            actor_ref: "main".into(),
            session_dir: dir.path().to_owned(),
            sessions_root: dir.path().join("sessions"),
            cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
        };
        let response = query_cached(
            &state,
            TrajectoryQuery {
                actor: Some("workflow".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(response.event_count, 6);
        assert_eq!(response.rows.len(), 4);
        let first_host_call = response
            .rows
            .iter()
            .find(|row| row.entry_id == "t:wf_debug/0")
            .unwrap();
        assert_eq!(
            first_host_call.parent_entry_id.as_deref(),
            Some("t:session/0")
        );
        assert_eq!(first_host_call.nesting_path, [0, 0, 0]);
        assert_eq!(first_host_call.actor, "workflow:wf_debug");
        assert!(
            response
                .rows
                .iter()
                .any(|row| row.entry_id == "t:wf_debug/1")
        );

        let unfiltered = query_cached(&state, TrajectoryQuery::default()).unwrap();
        let child = unfiltered
            .rows
            .iter()
            .find(|row| row.entry_id == "t:session/1")
            .unwrap();
        assert_eq!(child.parent_entry_id.as_deref(), Some("t:wf_debug/0"));
        assert_eq!(child.nesting_path, [0, 0, 0, 1]);
    }

    #[test]
    fn after_query_returns_the_next_page_without_skipping() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.jsonl");
        let timeline = chat_state::Timeline::from_seed(vec![
            sampling_types::ConversationItem::user("zero"),
            sampling_types::ConversationItem::assistant("one"),
            sampling_types::ConversationItem::user("two"),
        ])
        .unwrap();
        let body = timeline
            .events()
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&path, body).unwrap();
        let state = AppState {
            session_id: "session".into(),
            actor_ref: "main".into(),
            session_dir: dir.path().to_owned(),
            sessions_root: dir.path().join("sessions"),
            cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
        };

        let response = query_cached(
            &state,
            TrajectoryQuery {
                after: Some("t:session/0".into()),
                limit: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(response.matching_count, 3);
        assert_eq!(response.rows.len(), 1);
        assert_eq!(response.rows[0].seq, 1);
        assert_eq!(response.rows[0].ordinal, 1);
        assert!(response.has_earlier);
        assert!(response.has_later);
        let summary_wire = serde_json::to_value(&response.rows[0]).unwrap();
        assert!(summary_wire.get("details").is_none());
        let detail = query_event_cached(&state, "t:session/1").unwrap();
        assert_ne!(detail.row.details, serde_json::Value::Null);
    }

    #[test]
    fn summary_window_does_not_transfer_large_canonical_payloads() {
        let dir = tempfile::tempdir().unwrap();
        let payload = "canonical-payload-marker".repeat(100_000);
        let timeline =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::user(
                payload.clone(),
            )])
            .unwrap();
        write_timeline(&dir.path().join("timeline.jsonl"), &timeline);
        let state = AppState {
            session_id: "session".into(),
            actor_ref: "main".into(),
            session_dir: dir.path().to_owned(),
            sessions_root: dir.path().join("sessions"),
            cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
        };

        let summary =
            serde_json::to_vec(&query_cached(&state, TrajectoryQuery::default()).unwrap()).unwrap();
        {
            let cache = state.cache.lock().unwrap();
            assert!(
                cache
                    .projector
                    .rows()
                    .iter()
                    .all(|row| row.details.is_null())
            );
            assert!(
                cache
                    .materialized
                    .as_ref()
                    .unwrap()
                    .rows
                    .iter()
                    .all(|row| row.details.is_null())
            );
        }
        let detail = query_event_cached(&state, "t:session/0").unwrap();
        let detail_wire = serde_json::to_vec(&detail).unwrap();
        let full =
            serde_json::to_vec(&query_event_cached_with_mode(&state, "t:session/0", true).unwrap())
                .unwrap();
        assert!(
            summary.len() < 16 * 1024,
            "summary was {} bytes",
            summary.len()
        );
        assert!(detail.details_truncated);
        assert!(detail_wire.len() < 256 * 1024);
        assert!(full.len() > payload.len());
    }

    #[test]
    fn appended_root_event_updates_materialization_without_full_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.jsonl");
        let timeline = chat_state::Timeline::from_seed(vec![
            sampling_types::ConversationItem::user("first"),
            sampling_types::ConversationItem::assistant("second"),
        ])
        .unwrap();
        std::fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::to_string(&timeline.events()[0]).unwrap()
            ),
        )
        .unwrap();
        let state = AppState {
            session_id: "session".into(),
            actor_ref: "main".into(),
            session_dir: dir.path().to_owned(),
            sessions_root: dir.path().join("sessions"),
            cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
        };

        assert_eq!(
            query_cached(&state, TrajectoryQuery::default())
                .unwrap()
                .event_count,
            1
        );
        {
            let cache = state.cache.lock().unwrap();
            assert_eq!(cache.full_materialization_count, 1);
        }
        use std::io::Write as _;
        writeln!(
            std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap(),
            "{}",
            serde_json::to_string(&timeline.events()[1]).unwrap()
        )
        .unwrap();

        let response = query_cached(&state, TrajectoryQuery::default()).unwrap();
        assert_eq!(response.event_count, 2);
        assert_eq!(response.rows.len(), 2);
        let cache = state.cache.lock().unwrap();
        assert_eq!(cache.full_materialization_count, 1);
        assert_eq!(cache.materialized.as_ref().unwrap().positions.len(), 2);
    }

    #[test]
    fn canonical_browser_detail_limit_is_explicit() {
        let value = "x".repeat(MAX_TRAJECTORY_FULL_DETAIL_BYTES + 1);
        let error = trajectory_event_details(&value).unwrap_err();
        assert!(error.downcast_ref::<TrajectoryEventTooLarge>().is_some());
        let response = query_error_response(error);
        assert_eq!(response.0, StatusCode::PAYLOAD_TOO_LARGE);
        assert!(response.2.contains("timeline.jsonl"));
    }

    #[test]
    fn bounded_preview_also_limits_large_outer_fields() {
        let timeline =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::user("hello")])
                .unwrap();
        let mut row = timeline.trajectory().rows.remove(0);
        let large = "outer-field".repeat(100_000);
        row.producer = large.clone();
        row.correlation_id = Some(large.clone());
        row.turn_id = Some(large);

        let (preview, truncated) = preview_trajectory_row(&row, true);
        let wire = serde_json::to_vec(&preview).unwrap();
        assert!(truncated);
        assert!(wire.len() < 16 * 1024, "preview was {} bytes", wire.len());
    }

    #[test]
    fn before_query_returns_the_immediately_preceding_page() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.jsonl");
        let timeline = chat_state::Timeline::from_seed(vec![
            sampling_types::ConversationItem::user("zero"),
            sampling_types::ConversationItem::assistant("one"),
            sampling_types::ConversationItem::user("two"),
        ])
        .unwrap();
        let body = timeline
            .events()
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&path, body).unwrap();
        let state = AppState {
            session_id: "session".into(),
            actor_ref: "main".into(),
            session_dir: dir.path().to_owned(),
            sessions_root: dir.path().join("sessions"),
            cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
        };

        let response = query_cached(
            &state,
            TrajectoryQuery {
                before: Some("t:session/2".into()),
                limit: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(response.rows[0].seq, 1);
        assert_eq!(response.rows[0].ordinal, 1);
        assert!(response.has_earlier);
        assert!(response.has_later);
        assert_eq!(response.first_cursor.as_deref(), Some("t:session/1"));
        assert_eq!(response.last_cursor.as_deref(), Some("t:session/1"));
    }

    #[test]
    fn repeated_query_cache_invalidates_when_the_ledger_advances() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.jsonl");
        let mut timeline =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::user("first")])
                .unwrap();
        write_timeline(&path, &timeline);
        let state = AppState {
            session_id: "session".into(),
            actor_ref: "main".into(),
            session_dir: dir.path().to_owned(),
            sessions_root: dir.path().join("sessions"),
            cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
        };

        let first = query_cached(&state, TrajectoryQuery::default()).unwrap();
        let cached = query_cached(&state, TrajectoryQuery::default()).unwrap();
        assert_eq!(first.event_count, 1);
        assert_eq!(cached.event_count, 1);
        assert!(state.cache.lock().unwrap().last_query.is_some());

        timeline
            .append(
                sampling_types::ConversationItem::assistant("second"),
                chat_state::MessageCause::Assistant,
            )
            .unwrap();
        write_timeline(&path, &timeline);

        let advanced = query_cached(&state, TrajectoryQuery::default()).unwrap();
        assert_eq!(advanced.event_count, 2);
        assert_eq!(advanced.rows.len(), 2);
    }

    #[test]
    fn live_cursor_keeps_backdated_new_entries_at_the_observed_tail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.jsonl");
        let mut timeline = chat_state::Timeline::from_seed(vec![
            sampling_types::ConversationItem::user("first"),
            sampling_types::ConversationItem::assistant("second"),
        ])
        .unwrap();
        write_timeline(&path, &timeline);
        let state = AppState {
            session_id: "session".into(),
            actor_ref: "main".into(),
            session_dir: dir.path().to_owned(),
            sessions_root: dir.path().join("sessions"),
            cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
        };
        let initial = query_cached(&state, TrajectoryQuery::default()).unwrap();
        assert_eq!(initial.last_cursor.as_deref(), Some("t:session/1"));

        let mut backdated = timeline
            .append(
                sampling_types::ConversationItem::assistant("arrived later"),
                chat_state::MessageCause::Assistant,
            )
            .unwrap();
        backdated.at_ms = timeline.events()[0].at_ms.saturating_sub(1);
        use std::io::Write as _;
        writeln!(
            std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap(),
            "{}",
            serde_json::to_string(&backdated).unwrap()
        )
        .unwrap();

        let response = query_cached(
            &state,
            TrajectoryQuery {
                after: Some("t:session/1".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(response.rows.len(), 1);
        assert_eq!(response.rows[0].entry_id, "t:session/2");
    }

    #[test]
    fn query_rejects_ambiguous_bidirectional_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.jsonl");
        std::fs::write(&path, "").unwrap();
        let state = AppState {
            session_id: "session".into(),
            actor_ref: "main".into(),
            session_dir: dir.path().to_owned(),
            sessions_root: dir.path().join("sessions"),
            cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
        };
        let error = query_cached(
            &state,
            TrajectoryQuery {
                after: Some("t:session/1".into()),
                before: Some("t:session/2".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("mutually exclusive"));
    }

    #[test]
    fn host_guard_rejects_dns_rebinding_names() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "attacker.example:1234".parse().unwrap());
        assert_eq!(
            require_local_host(&headers).unwrap_err().0,
            StatusCode::FORBIDDEN
        );
        headers.insert(header::HOST, "127.0.0.1:1234".parse().unwrap());
        assert!(require_local_host(&headers).is_ok());
        headers.insert(header::HOST, "[::1]:1234".parse().unwrap());
        assert!(require_local_host(&headers).is_ok());
    }

    #[test]
    fn page_separates_timeline_views_from_four_dimension_filters() {
        assert!(PAGE.contains("id=\"track\""));
        assert!(PAGE.contains("id=\"overviewBy\""));
        assert!(PAGE.contains("<option value=\"interaction\">Interaction flow</option>"));
        assert!(PAGE.contains("<option value=\"layer\">Layer</option>"));
        assert!(PAGE.contains("<option value=\"actor\">Actor</option>"));
        assert!(PAGE.contains("<option value=\"class\">Class</option>"));
        assert!(PAGE.contains("<option value=\"producer\">Producer</option>"));
        assert!(PAGE.contains("aria-label=\"Trajectory filters\""));
        assert!(PAGE.contains("id=\"layer\""));
        assert!(PAGE.contains("id=\"actor\""));
        assert!(PAGE.contains("id=\"class\""));
        assert!(PAGE.contains("id=\"producer\""));
        assert!(PAGE.contains("<option value=\"governance\">Governance</option>"));
        assert!(PAGE.contains("OVERSCAN=20"));
        assert!(PAGE.contains("parent_entry_id"));
        assert!(PAGE.contains("nesting_path"));
        assert!(PAGE.contains("history.replaceState"));
        assert!(PAGE.contains("api/trajectory/event"));
        assert!(PAGE.contains("MAX_WINDOW_ROWS=1800"));
        assert!(PAGE.contains("params.set('before',displayRows[0].entry_id)"));
        assert!(PAGE.contains("params.set('after',displayRows.at(-1).entry_id)"));
        assert!(!PAGE.contains("id=\"older\""));
        assert!(!PAGE.contains("id=\"later\""));
        assert!(!PAGE.contains("Following live"));
        assert!(!PAGE.contains("Resume tail"));
        assert!(PAGE.contains("id=\"liveStatus\" role=\"status\""));
        assert!(PAGE.contains("id=\"follow\"") && PAGE.contains("hidden><span class=\"live-dot\""));
        assert!(PAGE.contains("Jump to live"));
        assert!(PAGE.contains("event.deltaY<0"));
        assert!(PAGE.contains("boundary-label"));
        assert!(PAGE.contains("paired-event"));
        assert!(PAGE.contains("summaryCaption"));
        assert!(PAGE.contains("active · step"));
    }

    #[test]
    fn page_keeps_detail_disclosure_owned_by_the_selected_entry() {
        assert!(PAGE.contains("items=items.map(row=>({...row}));"));
        assert!(PAGE.contains("const selectionChanged=selected!==entry;"));
        assert!(PAGE.contains("else if(!$('payload').open)"));
        assert!(PAGE.contains("copyController==null&&detailController==null"));
        assert!(PAGE.contains("if(refreshDetail)loadDetail(selected,true)"));
        assert!(PAGE.contains("height:clamp(180px,42dvh,520px)"));
        assert!(PAGE.contains("overscroll-behavior:contain"));
    }

    #[test]
    fn four_dimension_filters_intersect() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.jsonl");
        let timeline = chat_state::Timeline::from_seed(vec![
            sampling_types::ConversationItem::user("ask"),
            sampling_types::ConversationItem::assistant("answer"),
        ])
        .unwrap();
        let body = timeline
            .events()
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&path, body).unwrap();
        let state = AppState {
            session_id: "child".into(),
            actor_ref: "subagent:child".into(),
            session_dir: dir.path().to_owned(),
            sessions_root: dir.path().join("sessions"),
            cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
        };

        let response = query_cached(
            &state,
            TrajectoryQuery {
                layer: Some("user".into()),
                actor: Some("subagent".into()),
                class: Some("message".into()),
                producer: Some("user".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(response.rows.len(), 1);
        assert_eq!(response.rows[0].entry_id, "t:child/0");
        assert_eq!(response.rows[0].actor, "subagent:child");
        assert_eq!(response.rows[0].kind, "user.message");

        let grouped = query_cached(
            &state,
            TrajectoryQuery {
                overview_by: Some(TrajectoryOverviewDimension::Layer),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            grouped.overview.dimension,
            TrajectoryOverviewDimension::Layer
        );
        assert_eq!(grouped.overview.counts.get("user"), Some(&1));
        assert_eq!(grouped.overview.counts.get("assistant"), Some(&1));
        assert!(grouped.overview.bins.iter().all(|bin| {
            bin.counts
                .keys()
                .all(|key| matches!(key.as_str(), "user" | "assistant"))
        }));
    }

    #[test]
    fn sideband_timeline_is_nested_under_its_spawn_and_filterable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.jsonl");
        let sideband_id = uuid::Uuid::now_v7().to_string();
        let mut timeline =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::user(
                "authorize this",
            )])
            .unwrap();
        timeline
            .record(chat_state::TimelineEventKind::Sideband(
                chat_state::SidebandSpawnEvent {
                    sideband_id: sideband_id.clone(),
                    purpose: chat_state::SidebandPurpose::PermissionJudgment,
                    source_refs: vec![chat_state::TimelineRangeRef {
                        timeline_id: "session".into(),
                        first_seq: 0,
                        last_seq: 0,
                    }],
                },
            ))
            .unwrap();
        let body = timeline
            .events()
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&path, body).unwrap();

        let sideband_path = dir
            .path()
            .join("sidebands")
            .join(&sideband_id)
            .join("timeline.jsonl");
        std::fs::create_dir_all(sideband_path.parent().unwrap()).unwrap();
        let mut sideband = chat_state::SidebandTimeline::new(sideband_id.clone()).unwrap();
        for kind in [
            chat_state::SidebandEventKind::Request(chat_state::SidebandRequest {
                purpose: chat_state::SidebandPurpose::PermissionJudgment,
                prompt: "judge".into(),
                source_refs: vec![chat_state::TimelineRangeRef {
                    timeline_id: "session".into(),
                    first_seq: 0,
                    last_seq: 0,
                }],
                budget_policy: chat_state::SidebandBudgetPolicy {
                    max_attempts: 1,
                    max_input_tokens_per_attempt: 1,
                    max_output_tokens_per_attempt: Some(1),
                },
                route: chat_state::SidebandRoute {
                    model: "model".into(),
                    backend: sampling_types::ApiBackend::Responses,
                },
                initiator_ref: format!("t:session/sideband:{sideband_id}"),
                executor: "main".into(),
                output_schema: Some(serde_json::json!({"type": "object"})),
            }),
            chat_state::SidebandEventKind::Attempt(chat_state::SidebandAttempt {
                attempt_no: 1,
                input_refs: vec![chat_state::TimelineRangeRef {
                    timeline_id: "session".into(),
                    first_seq: 0,
                    last_seq: 0,
                }],
                assembly_manifest: chat_state::SidebandAssemblyManifest {
                    strategy: "all-sources".into(),
                    strategy_version: 1,
                    source_revision: Some(1),
                    context_surface_ids: Vec::new(),
                    selected_surface_ids: Vec::new(),
                    materialized_input_tokens: 1,
                    max_output_tokens: Some(1),
                },
                feedback: None,
            }),
            chat_state::SidebandEventKind::Result(chat_state::SidebandResult {
                raw_output: r#"{"decision":"allow","reason":"safe"}"#.into(),
                structured_output: Some(serde_json::json!({"decision": "allow", "reason": "safe"})),
                usage: chat_state::SidebandUsage::default(),
                finish: "stop".into(),
                source_event_seqs: [0, 1],
                evidence_refs: Vec::new(),
            }),
            chat_state::SidebandEventKind::End(chat_state::SidebandEnd {
                outcome: chat_state::SidebandOutcome::Completed,
                error: None,
            }),
        ] {
            let event = sideband.prepare(kind).unwrap();
            sideband.accept(event).unwrap();
        }
        let body = sideband
            .events()
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&sideband_path, body).unwrap();

        let state = AppState {
            session_id: "session".into(),
            actor_ref: "main".into(),
            session_dir: dir.path().to_owned(),
            sessions_root: dir.path().join("sessions"),
            cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
        };
        let response = query_cached(
            &state,
            TrajectoryQuery {
                actor: Some("sideband".into()),
                class: Some("auxiliary".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(response.rows.len(), 4);
        assert!(
            response
                .rows
                .iter()
                .all(|row| row.parent_entry_id.as_deref() == Some("t:session/1"))
        );
        assert!(
            response
                .rows
                .iter()
                .enumerate()
                .all(|(seq, row)| row.nesting_path == [1, seq as u64])
        );
        assert_eq!(response.first_cursor, Some(format!("t:{sideband_id}/0")));
        assert_eq!(response.last_cursor, Some(format!("t:{sideband_id}/3")));
        assert_eq!(response.rows[0].entry_id, format!("t:{sideband_id}/0"));
        assert_eq!(response.rows[0].kind, "sideband.request");
        assert_eq!(response.rows[3].kind, "sideband.end");

        let mut tampered = sideband.events().to_vec();
        let chat_state::SidebandEventKind::Request(request) = &mut tampered[0].kind else {
            unreachable!()
        };
        request.purpose = chat_state::SidebandPurpose::SessionTitle;
        let body = tampered
            .iter()
            .map(|event| serde_json::to_string(event).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&sideband_path, body).unwrap();
        let tampered_state = AppState {
            session_id: state.session_id.clone(),
            actor_ref: state.actor_ref.clone(),
            session_dir: state.session_dir.clone(),
            sessions_root: state.sessions_root.clone(),
            cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
        };
        let error = query_cached(&tampered_state, TrajectoryQuery::default()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("does not match its parent spawn")
        );
    }

    #[test]
    fn recursively_merges_child_ledgers_with_stable_causal_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root_dir = dir.path().join("root");
        let sessions_root = dir.path().join("sessions");
        std::fs::create_dir_all(&root_dir).unwrap();

        let mut root =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::user(
                "root prompt",
            )])
            .unwrap();
        let mut child_spawn_fact = subagent_spawn("worker", "child-session", "/child");
        // The seed's security parent is the concrete parent session, not the
        // generic fixture value used by unrelated trajectory tests.
        child_spawn_fact.security_parent_session_id = "root-session".into();
        let child_spawn = root
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Spawned(child_spawn_fact),
            ))
            .unwrap();
        root.append(
            sampling_types::ConversationItem::assistant("root continued"),
            chat_state::MessageCause::Assistant,
        )
        .unwrap();
        write_timeline_with_start_time(
            &root_dir.join(super::super::storage::TIMELINE_FILE),
            &root,
            1,
        );

        let mut child = chat_state::Timeline::default();
        child
            .record(chat_state::TimelineEventKind::SubagentSeed(subagent_seed(
                "root-session",
                child_spawn.seq.get(),
                "worker",
            )))
            .unwrap();
        child
            .append(
                sampling_types::ConversationItem::user("child prompt"),
                chat_state::MessageCause::User,
            )
            .unwrap();
        let mut grandchild_spawn_fact =
            subagent_spawn("nested-worker", "grandchild-session", "/grandchild");
        grandchild_spawn_fact.security_parent_session_id = "child-session".into();
        let grandchild_spawn = child
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Spawned(grandchild_spawn_fact),
            ))
            .unwrap();
        child
            .append(
                sampling_types::ConversationItem::assistant("child continued"),
                chat_state::MessageCause::Assistant,
            )
            .unwrap();
        let child_dir = write_child_session(
            &sessions_root,
            "/child",
            "child-session",
            "root-session",
            &child,
        );
        write_timeline_with_start_time(
            &child_dir.join(super::super::storage::TIMELINE_FILE),
            &child,
            10,
        );

        let mut grandchild = chat_state::Timeline::default();
        grandchild
            .record(chat_state::TimelineEventKind::SubagentSeed(subagent_seed(
                "child-session",
                grandchild_spawn.seq.get(),
                "nested-worker",
            )))
            .unwrap();
        grandchild
            .append(
                sampling_types::ConversationItem::assistant("nested result"),
                chat_state::MessageCause::Assistant,
            )
            .unwrap();
        let grandchild_dir = write_child_session(
            &sessions_root,
            "/grandchild",
            "grandchild-session",
            "child-session",
            &grandchild,
        );
        write_timeline_with_start_time(
            &grandchild_dir.join(super::super::storage::TIMELINE_FILE),
            &grandchild,
            20,
        );

        let state = AppState {
            session_id: "root-session".into(),
            actor_ref: "main".into(),
            session_dir: root_dir,
            sessions_root,
            cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
        };
        let response = query_cached(&state, TrajectoryQuery::default()).unwrap();
        let identities = response
            .rows
            .iter()
            .map(|row| {
                (
                    row.entry_id.as_str(),
                    row.parent_entry_id.as_deref(),
                    row.nesting_path.as_slice(),
                    row.actor.as_str(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            identities,
            vec![
                ("t:root-session/0", None, &[0][..], "main"),
                ("t:root-session/1", None, &[1][..], "main"),
                ("t:root-session/2", None, &[2][..], "main"),
                (
                    "t:child-session/0",
                    Some("t:root-session/1"),
                    &[1, 0][..],
                    "subagent:child-session",
                ),
                (
                    "t:child-session/1",
                    Some("t:root-session/1"),
                    &[1, 1][..],
                    "subagent:child-session",
                ),
                (
                    "t:child-session/2",
                    Some("t:root-session/1"),
                    &[1, 2][..],
                    "subagent:child-session",
                ),
                (
                    "t:child-session/3",
                    Some("t:root-session/1"),
                    &[1, 3][..],
                    "subagent:child-session",
                ),
                (
                    "t:grandchild-session/0",
                    Some("t:child-session/2"),
                    &[1, 2, 0][..],
                    "subagent:grandchild-session",
                ),
                (
                    "t:grandchild-session/1",
                    Some("t:child-session/2"),
                    &[1, 2, 1][..],
                    "subagent:grandchild-session",
                ),
            ]
        );
        assert_eq!(response.event_count, 9);

        let filtered = query_cached(
            &state,
            TrajectoryQuery {
                actor: Some("subagent:grandchild-session".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(filtered.rows.len(), 2);
        assert_eq!(
            filtered.first_cursor.as_deref(),
            Some("t:grandchild-session/0")
        );
        assert_eq!(
            filtered.last_cursor.as_deref(),
            Some("t:grandchild-session/1")
        );

        let focused = query_cached(
            &state,
            TrajectoryQuery {
                entry: Some("t:grandchild-session/1".into()),
                limit: Some(3),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(focused.rows.len(), 3);
        assert!(focused.has_earlier);
        assert!(!focused.has_later);
        assert!(
            focused
                .rows
                .iter()
                .any(|row| row.entry_id == "t:grandchild-session/1")
        );
    }

    #[test]
    fn rejects_child_ledger_whose_seed_does_not_match_parent_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let root_dir = dir.path().join("root");
        let sessions_root = dir.path().join("sessions");
        std::fs::create_dir_all(&root_dir).unwrap();
        let mut root = chat_state::Timeline::default();
        root.record(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Spawned(subagent_spawn("worker", "child-session", "/child")),
        ))
        .unwrap();
        write_timeline(&root_dir.join(super::super::storage::TIMELINE_FILE), &root);
        let mut child = chat_state::Timeline::default();
        child
            .record(chat_state::TimelineEventKind::SubagentSeed(subagent_seed(
                "another-parent",
                99,
                "worker",
            )))
            .unwrap();
        write_child_session(
            &sessions_root,
            "/child",
            "child-session",
            "root-session",
            &child,
        );
        let state = AppState {
            session_id: "root-session".into(),
            actor_ref: "main".into(),
            session_dir: root_dir,
            sessions_root,
            cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
        };
        let error = query_cached(&state, TrajectoryQuery::default()).unwrap_err();
        assert!(error.to_string().contains("child seed-source"));
    }

    #[test]
    fn missing_child_is_only_valid_for_unreferenced_failed_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let root_dir = dir.path().join("root");
        let sessions_root = dir.path().join("sessions");
        std::fs::create_dir_all(&root_dir).unwrap();
        let mut failed = chat_state::Timeline::default();
        failed
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Spawned(subagent_spawn(
                    "worker",
                    "missing-child",
                    "/child",
                )),
            ))
            .unwrap();
        failed
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Ended(chat_state::SubagentTerminalEvent {
                    subagent_id: "worker".into(),
                    child_session_id: "missing-child".into(),
                    outcome: chat_state::SubagentOutcome::Failed,
                    duration_ms: 1,
                    tool_calls: 0,
                    turns: 0,
                    tokens_used: 0,
                    error: Some("child was never published".into()),
                    result_ref: None,
                    snapshot_ref: None,
                }),
            ))
            .unwrap();
        write_timeline(
            &root_dir.join(super::super::storage::TIMELINE_FILE),
            &failed,
        );
        let state = AppState {
            session_id: "root-session".into(),
            actor_ref: "main".into(),
            session_dir: root_dir.clone(),
            sessions_root: sessions_root.clone(),
            cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
        };
        assert_eq!(
            query_cached(&state, TrajectoryQuery::default())
                .unwrap()
                .rows
                .len(),
            2
        );

        let mut completed = chat_state::Timeline::default();
        completed
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Spawned(subagent_spawn(
                    "worker",
                    "missing-child",
                    "/child",
                )),
            ))
            .unwrap();
        completed
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Ended(chat_state::SubagentTerminalEvent {
                    subagent_id: "worker".into(),
                    child_session_id: "missing-child".into(),
                    outcome: chat_state::SubagentOutcome::Completed,
                    duration_ms: 1,
                    tool_calls: 0,
                    turns: 0,
                    tokens_used: 0,
                    error: None,
                    result_ref: Some(chat_state::TimelineRangeRef {
                        timeline_id: "missing-child".into(),
                        first_seq: 1,
                        last_seq: 1,
                    }),
                    snapshot_ref: None,
                }),
            ))
            .unwrap();
        write_timeline(
            &root_dir.join(super::super::storage::TIMELINE_FILE),
            &completed,
        );
        let completed_state = AppState {
            session_id: state.session_id.clone(),
            actor_ref: state.actor_ref.clone(),
            session_dir: root_dir,
            sessions_root,
            cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
        };
        let error = query_cached(&completed_state, TrajectoryQuery::default()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("requires missing child Timeline")
        );
    }

    #[tokio::test]
    async fn server_rejects_non_loopback_bind_addresses() {
        let error = serve("missing", "0.0.0.0:0".parse().unwrap(), |_, _| {})
            .await
            .unwrap_err();
        assert!(error.to_string().contains("loopback"));
    }
}
