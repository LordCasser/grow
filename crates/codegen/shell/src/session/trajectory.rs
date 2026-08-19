//! Local-only Trajectory query server.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Html;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct AppState {
    session_id: String,
    actor_ref: String,
    timeline_path: PathBuf,
    sidebands_dir: PathBuf,
    cache: Arc<Mutex<TrajectoryCache>>,
}

#[derive(Default)]
struct TrajectoryCache {
    offset: u64,
    timeline: chat_state::Timeline,
    projector: chat_state::TrajectoryProjector,
    sidebands: BTreeMap<String, SidebandCache>,
}

#[derive(Default)]
struct SidebandCache {
    offset: u64,
    events: Vec<chat_state::SidebandEvent>,
}

#[derive(Debug, Default, Deserialize)]
struct TrajectoryQuery {
    after: Option<u64>,
    before: Option<u64>,
    layer: Option<String>,
    actor: Option<String>,
    class: Option<String>,
    producer: Option<String>,
    visibility: Option<String>,
    search: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrajectoryResponse {
    session_id: String,
    schema_version: u8,
    event_count: usize,
    current_surface_items: usize,
    active_turn: Option<String>,
    active_step: Option<u32>,
    open_requests: Vec<String>,
    open_tools: Vec<String>,
    matching_count: usize,
    first_seq: Option<u64>,
    last_seq: Option<u64>,
    has_earlier: bool,
    rows: Vec<chat_state::TrajectoryRow>,
}

/// Bind the local server, report the exact URL, then serve until interrupted.
pub async fn serve(
    session_id: &str,
    bind: SocketAddr,
    on_ready: impl FnOnce(&str),
) -> anyhow::Result<()> {
    if !bind.ip().is_loopback() {
        anyhow::bail!("Trajectory server only accepts loopback bind addresses, got {bind}");
    }
    let session_dir = super::persistence::find_session_dir_by_id(session_id)
        .ok_or_else(|| anyhow::anyhow!("session '{session_id}' was not found"))?;
    let timeline_path = session_dir.join(super::storage::TIMELINE_FILE);
    if !timeline_path.is_file() {
        anyhow::bail!(
            "session '{}' has no Timeline v6 ledger at {}",
            session_id,
            timeline_path.display()
        );
    }

    let listener = tokio::net::TcpListener::bind(bind).await?;
    let local = listener.local_addr()?;
    let token = uuid::Uuid::now_v7().simple().to_string();
    let state = AppState {
        session_id: session_id.to_owned(),
        actor_ref: session_actor_ref(&session_dir, session_id)?,
        timeline_path,
        sidebands_dir: session_dir.join("sidebands"),
        cache: Arc::new(Mutex::new(TrajectoryCache::default())),
    };
    let app = Router::new().nest(
        &format!("/{token}"),
        Router::new()
            .route("/", get(index))
            .route("/api/trajectory", get(query_trajectory))
            .with_state(state),
    );
    let url = format!("http://{local}/{token}/");
    on_ready(&url);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index(headers: HeaderMap) -> Result<Html<&'static str>, (StatusCode, String)> {
    require_local_host(&headers)?;
    Ok(Html(PAGE))
}

async fn query_trajectory(
    State(state): State<AppState>,
    Query(query): Query<TrajectoryQuery>,
    headers: HeaderMap,
) -> Result<Json<TrajectoryResponse>, (StatusCode, String)> {
    require_local_host(&headers)?;
    if query.after.is_some() && query.before.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            "after and before are mutually exclusive".into(),
        ));
    }
    let response = tokio::task::spawn_blocking(move || query_cached(&state, query))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Ok(Json(response))
}

fn query_cached(state: &AppState, query: TrajectoryQuery) -> anyhow::Result<TrajectoryResponse> {
    if query.after.is_some() && query.before.is_some() {
        anyhow::bail!("after and before are mutually exclusive");
    }
    let mut cache = state
        .cache
        .lock()
        .map_err(|_| anyhow::anyhow!("Trajectory cache lock was poisoned"))?;
    cache.refresh(&state.timeline_path)?;
    cache.refresh_sidebands(&state.sidebands_dir)?;
    let mut all_rows = cache.projector.rows().to_vec();
    for row in &mut all_rows {
        row.entry_id = format!("t:{}/{}", state.session_id, row.seq);
        row.actor.clone_from(&state.actor_ref);
    }
    all_rows.extend(cache.sideband_rows(&state.session_id)?);
    all_rows.sort_by_key(|row| {
        (
            row.parent_seq.unwrap_or(row.seq),
            row.parent_seq.is_some(),
            row.seq,
        )
    });
    let search = query.search.as_deref().map(str::to_lowercase);
    let layer = query.layer.as_deref().filter(|value| !value.is_empty());
    let actor = query.actor.as_deref().filter(|value| !value.is_empty());
    let class = query.class.as_deref().filter(|value| !value.is_empty());
    let producer = query.producer.as_deref().filter(|value| !value.is_empty());
    let visibility = query
        .visibility
        .as_deref()
        .filter(|value| !value.is_empty());
    let limit = query.limit.unwrap_or(2_000).clamp(1, 10_000);
    let matching = all_rows
        .iter()
        .filter(|row| {
            query
                .after
                .is_none_or(|after| row.parent_seq.unwrap_or(row.seq) > after)
        })
        .filter(|row| {
            query
                .before
                .is_none_or(|before| row.parent_seq.unwrap_or(row.seq) < before)
        })
        .filter(|row| layer.is_none_or(|value| dimension_matches(&row.layer, value)))
        .filter(|row| actor.is_none_or(|value| dimension_matches(&row.actor, value)))
        .filter(|row| class.is_none_or(|value| row.class == value))
        .filter(|row| producer.is_none_or(|value| dimension_matches(&row.producer, value)))
        .filter(|row| {
            visibility.is_none_or(|visibility| visibility_name(row.visibility) == visibility)
        })
        .filter(|row| {
            search.as_ref().is_none_or(|needle| {
                format!(
                    "{} {} {} {} {} {} {} {} {} {} {} {}",
                    row.seq,
                    row.entry_id,
                    row.layer,
                    row.actor,
                    row.class,
                    row.producer,
                    row.kind,
                    row.state,
                    row.summary,
                    row.turn_id.as_deref().unwrap_or_default(),
                    row.correlation_id.as_deref().unwrap_or_default(),
                    row.details,
                )
                .to_lowercase()
                .contains(needle)
            })
        })
        .collect::<Vec<_>>();
    let matching_count = matching.len();
    let root_sequences = matching
        .iter()
        .map(|row| row.parent_seq.unwrap_or(row.seq))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let root_count = root_sequences.len();
    let (start, end) = if query.after.is_some() {
        (0, root_count.min(limit))
    } else {
        (root_count.saturating_sub(limit), root_count)
    };
    let has_earlier = query.after.is_none() && start > 0;
    let selected_roots = root_sequences[start..end]
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let rows = matching
        .into_iter()
        .filter(|row| selected_roots.contains(&row.parent_seq.unwrap_or(row.seq)))
        .cloned()
        .collect::<Vec<_>>();
    let first_seq = root_sequences.get(start).copied();
    let last_seq = end
        .checked_sub(1)
        .and_then(|index| root_sequences.get(index).copied());
    let sideband_event_count = cache
        .sidebands
        .values()
        .map(|sideband| sideband.events.len())
        .sum::<usize>();
    Ok(TrajectoryResponse {
        session_id: state.session_id.clone(),
        schema_version: chat_state::TIMELINE_SCHEMA_VERSION,
        event_count: cache.timeline.events().len() + sideband_event_count,
        current_surface_items: cache.timeline.surface_len(),
        active_turn: cache.timeline.active_turn().map(|id| id.0.to_string()),
        active_step: cache.timeline.active_step().map(|id| id.index),
        open_requests: cache
            .timeline
            .open_request_ids()
            .map(str::to_owned)
            .collect(),
        open_tools: cache
            .timeline
            .open_tool_call_ids()
            .map(str::to_owned)
            .collect(),
        matching_count,
        first_seq,
        last_seq,
        has_earlier,
        rows,
    })
}

fn session_actor_ref(session_dir: &Path, session_id: &str) -> anyhow::Result<String> {
    let bytes = std::fs::read(session_dir.join(super::storage::SUMMARY_FILE))?;
    let summary: super::persistence::Summary = serde_json::from_slice(&bytes)?;
    summary.validate_current_format()?;
    let actor = match summary.session_kind.as_deref() {
        Some(kind) if kind.starts_with("subagent") => format!("subagent:{session_id}"),
        Some(kind) if kind.starts_with("workflow") => format!("workflow:{session_id}"),
        _ => "main".into(),
    };
    Ok(actor)
}

fn dimension_matches(actual: &str, filter: &str) -> bool {
    actual == filter
        || actual
            .strip_prefix(filter)
            .is_some_and(|suffix| matches!(suffix.as_bytes().first(), Some(b'.' | b':')))
}

impl TrajectoryCache {
    fn refresh(&mut self, path: &Path) -> anyhow::Result<()> {
        let file_len = std::fs::metadata(path)?.len();
        if file_len < self.offset {
            *self = Self::default();
        }
        let mut file = std::fs::File::open(path)?;
        file.seek(std::io::SeekFrom::Start(self.offset))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let complete_len = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        if complete_len == 0 {
            return Ok(());
        }
        let mut timeline = self.timeline.clone();
        let mut accepted = Vec::new();
        for line in bytes[..complete_len].split(|byte| *byte == b'\n') {
            if line.is_empty() {
                continue;
            }
            let event =
                serde_json::from_slice::<chat_state::TimelineEvent>(line).map_err(|error| {
                    anyhow::anyhow!("{} at byte {}: {error}", path.display(), self.offset)
                })?;
            timeline.accept(event.clone())?;
            accepted.push(event);
        }
        for event in &accepted {
            self.projector.accept(event);
        }
        self.timeline = timeline;
        self.offset += complete_len as u64;
        Ok(())
    }

    fn refresh_sidebands(&mut self, directory: &Path) -> anyhow::Result<()> {
        if !directory.exists() {
            self.sidebands.clear();
            return Ok(());
        }
        let mut entries = std::fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        let mut seen = BTreeSet::new();
        for entry in entries {
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let sideband_id = entry.file_name().to_string_lossy().into_owned();
            chat_state::validate_sideband_id(&sideband_id)?;
            let path = entry.path().join(super::storage::TIMELINE_FILE);
            if !path.is_file() {
                continue;
            }
            let sideband = self.sidebands.entry(sideband_id.clone()).or_default();
            sideband.refresh(&path)?;
            if !sideband.events.is_empty() {
                seen.insert(sideband_id.clone());
            }
        }
        self.sidebands.retain(|id, _| seen.contains(id));
        Ok(())
    }

    fn sideband_rows(
        &self,
        parent_timeline_id: &str,
    ) -> anyhow::Result<Vec<chat_state::TrajectoryRow>> {
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
        let mut rows = Vec::new();
        for (sideband_id, sideband) in &self.sidebands {
            let (parent_seq, spawn) =
                parents.get(sideband_id.as_str()).copied().ok_or_else(|| {
                    anyhow::anyhow!(
                        "sideband {sideband_id} has a Timeline but no parent sideband.spawn fact"
                    )
                })?;
            chat_state::SidebandTimeline::from_events(sideband.events.clone())?.validate_parent(
                parent_timeline_id,
                parent_seq,
                spawn,
            )?;
            let attempt_times = sideband
                .events
                .iter()
                .filter_map(|event| {
                    matches!(event.kind, chat_state::SidebandEventKind::Attempt(_))
                        .then_some((event.seq, event.at_ms))
                })
                .collect::<BTreeMap<_, _>>();
            for event in &sideband.events {
                rows.push(sideband_row(event, parent_seq, &attempt_times));
            }
        }
        Ok(rows)
    }
}

impl SidebandCache {
    fn refresh(&mut self, path: &Path) -> anyhow::Result<()> {
        let file_len = std::fs::metadata(path)?.len();
        if file_len < self.offset {
            *self = Self::default();
        }
        let mut file = std::fs::File::open(path)?;
        file.seek(std::io::SeekFrom::Start(self.offset))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let complete_len = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        if complete_len == 0 {
            return Ok(());
        }
        let mut events = self.events.clone();
        for line in bytes[..complete_len].split(|byte| *byte == b'\n') {
            if line.is_empty() {
                continue;
            }
            let event =
                serde_json::from_slice::<chat_state::SidebandEvent>(line).map_err(|error| {
                    anyhow::anyhow!("{} at byte {}: {error}", path.display(), self.offset)
                })?;
            events.push(event);
        }
        chat_state::SidebandTimeline::from_events(events.clone())?;
        self.events = events;
        self.offset += complete_len as u64;
        Ok(())
    }
}

fn sideband_row(
    event: &chat_state::SidebandEvent,
    parent_seq: u64,
    attempt_times: &BTreeMap<u64, i64>,
) -> chat_state::TrajectoryRow {
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
        entry_id: format!("t:sideband:{}/{}", event.sideband_id, event.seq),
        seq: event.seq,
        parent_seq: Some(parent_seq),
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
        details: serde_json::to_value(&event.kind).unwrap_or(serde_json::Value::Null),
    }
}

fn visibility_name(value: chat_state::SurfaceVisibility) -> &'static str {
    match value {
        chat_state::SurfaceVisibility::Current => "current",
        chat_state::SurfaceVisibility::Shadowed => "shadowed",
        chat_state::SurfaceVisibility::LogOnly => "log_only",
    }
}

fn require_local_host(headers: &HeaderMap) -> Result<(), (StatusCode, String)> {
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| (StatusCode::FORBIDDEN, "missing Host header".into()))?;
    let authority = host
        .parse::<axum::http::uri::Authority>()
        .map_err(|_| (StatusCode::FORBIDDEN, "invalid Host header".into()))?;
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
        Err((StatusCode::FORBIDDEN, "non-loopback Host rejected".into()))
    }
}

fn internal_error(error: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

const PAGE: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Grow Trajectory</title><style>
:root{color-scheme:dark;--bg:#0b0d10;--panel:#12151a;--line:#272c35;--muted:#89919f;--text:#e7eaf0;--accent:#7dd3fc;--green:#86efac;--yellow:#fde68a;--red:#fca5a5}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--text);font:13px ui-monospace,SFMono-Regular,Menlo,monospace;height:100vh;overflow:hidden}
header{height:58px;display:flex;align-items:center;gap:18px;padding:0 18px;border-bottom:1px solid var(--line);background:#0e1116}.brand{font-weight:800;letter-spacing:.08em}.session{color:var(--accent)}.stats{display:flex;gap:14px;color:var(--muted);margin-left:auto}.live{color:var(--green)}
.controls{min-height:88px;display:flex;align-content:center;align-items:center;flex-wrap:wrap;gap:8px;padding:8px 18px;border-bottom:1px solid var(--line);background:var(--panel)}input,select,button{height:34px;background:#0b0e13;color:var(--text);border:1px solid var(--line);border-radius:6px;padding:0 9px;font:inherit}input{width:min(340px,34vw)}button{cursor:pointer}button:hover{border-color:#4b5563}.follow.on{color:var(--green);border-color:#28623e}
.overview{height:64px;display:grid;grid-template-columns:58px minmax(0,1fr);border-bottom:1px solid var(--line);background:#0e1116}.lane-labels{position:relative;border-right:1px solid var(--line);color:var(--muted);font-size:9px}.lane-labels span{position:absolute;right:5px}.lane-labels span:nth-child(1){top:8px}.lane-labels span:nth-child(2){top:27px}.lane-labels span:nth-child(3){top:46px}.track{position:relative;overflow:hidden}.track::before,.track::after{content:"";position:absolute;left:0;right:0;border-top:1px solid #191e26}.track::before{top:21px}.track::after{top:42px}.span{position:absolute;height:8px;top:calc(7px + var(--lane) * 20px);left:var(--left);width:max(2px,var(--width));min-width:2px;border:0;border-radius:2px;padding:0;background:var(--muted);opacity:.78;cursor:pointer}.span.input{background:var(--accent)}.span.model{background:#c4b5fd}.span.tools{background:var(--yellow)}.span.failed,.span.cancelled{background:var(--red)}.span.shadowed{opacity:.25}.span:hover,.span.selected{opacity:1;box-shadow:0 0 0 1px var(--bg),0 0 0 2px var(--accent)}.turn-mark{position:absolute;top:0;bottom:0;width:1px;background:#334155;pointer-events:none}
main{height:calc(100vh - 210px);display:grid;grid-template-columns:minmax(0,1fr) 420px}.ledger{overflow:auto}.inspector{border-left:1px solid var(--line);background:#0e1116;overflow:auto;padding:16px}.inspector h3{margin:0 0 12px}.inspector pre{white-space:pre-wrap;word-break:break-word;color:#cbd5e1;line-height:1.5}.empty{color:var(--muted)}
table{width:100%;min-width:1280px;border-collapse:collapse;table-layout:fixed}thead{position:sticky;top:0;background:#151920;z-index:2}th{text-align:left;color:var(--muted);font-weight:500;padding:9px 8px;border-bottom:1px solid var(--line)}td{padding:8px;border-bottom:1px solid #1b2028;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}tr{cursor:pointer}tbody tr:hover,tbody tr.selected{background:#151b24}.seq{width:62px;color:#657083}.time{width:92px}.class{width:92px}.layer{width:112px}.actor{width:124px}.kind{width:150px}.producer{width:116px}.state{width:88px}.turn{width:72px}.duration{width:82px;text-align:right}.summary{width:auto}.message .kind{color:var(--accent)}.audit .kind{color:var(--green)}.auxiliary .kind{color:#c4b5fd}.failed,.cancelled{color:var(--red)}.retrying{color:var(--yellow)}.shadowed{opacity:.48}.pill{padding:2px 6px;border:1px solid var(--line);border-radius:999px}
@media(max-width:900px){main{grid-template-columns:1fr}.inspector{display:none}.stats{display:none}.turn{display:none}}
</style></head><body>
<header><span class="brand">GROW / TRAJECTORY</span><span class="session" id="session">loading…</span><div class="stats"><span id="counts"></span><span id="position"></span><span class="live" id="health">● live</span></div></header>
<div class="controls"><input id="search" placeholder="Search coordinates, kind, id, payload…"><select id="layer"><option value="">all layers</option><option>system</option><option>user</option><option>assistant</option><option>tool</option><option>plugin</option><option>meta</option></select><select id="actor"><option value="">all actors</option><option>main</option><option>subagent</option><option>workflow</option><option>sideband</option></select><select id="class"><option value="">all classes</option><option>message</option><option>lifecycle</option><option>governance</option><option>audit</option><option>auxiliary</option></select><select id="producer"><option value="">all producers</option><option>core</option><option>model</option><option>tool</option><option>hook</option><option>plugin</option><option>skill</option><option>mcp</option><option>user</option></select><select id="visibility"><option value="">all surfaces</option><option value="current">current</option><option value="shadowed">shadowed</option><option value="log_only">log only</option></select><button id="older">load earlier</button><button class="follow on" id="follow">tail follow</button><button id="refresh">refresh</button></div>
<div class="overview"><div class="lane-labels"><span>INPUT</span><span>MODEL</span><span>TOOLS</span></div><div class="track" id="track"></div></div>
<main><div class="ledger" id="ledger"><table><thead><tr><th class="seq">seq</th><th class="time">time</th><th class="class">class</th><th class="layer">layer</th><th class="actor">actor</th><th class="kind">kind</th><th class="producer">producer</th><th class="state">state</th><th class="turn">turn/step</th><th class="duration">duration</th><th class="summary">summary</th></tr></thead><tbody id="rows"></tbody></table></div><aside class="inspector"><h3>Event inspector</h3><div class="empty" id="hint">Select an event to inspect its canonical payload and four-dimensional identity.</div><pre id="details"></pre></aside></main>
<script>
const $=id=>document.getElementById(id), rows=$('rows'), ledger=$('ledger'), track=$('track');let follow=true,selected=null,timer,latestData=null,olderRows=[],hasEarlier=false;
function esc(v){return String(v??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]))}
function time(ms){return new Date(ms).toLocaleTimeString([], {hour12:false})}function duration(ms){return ms==null?'—':ms<1000?ms+' ms':(ms/1000).toFixed(2)+' s'}
function lane(r){if(r.layer.startsWith('tool'))return['tools',2];if(r.layer==='assistant'||r.producer.startsWith('model')||r.kind.startsWith('request.')||r.kind.startsWith('step.'))return['model',1];return['input',0]}
function drawOverview(items){if(!items.length){track.innerHTML='';return}const starts=items.map(r=>r.at_ms-(r.duration_ms??0)),ends=items.map(r=>r.at_ms),min=Math.min(...starts),max=Math.max(...ends,min+1),domain=max-min;const marks=items.filter(r=>r.kind==='turn.started').map(r=>`<i class="turn-mark" style="left:${((r.at_ms-min)/domain)*100}%"></i>`).join('');const spans=items.map(r=>{const [laneKind,n]=lane(r),start=r.at_ms-(r.duration_ms??0),left=((start-min)/domain)*100,width=Math.max(.12,((Math.max(r.duration_ms??0,1))/domain)*100);return `<button class="span ${laneKind} ${esc(r.state)} ${esc(r.visibility)}" data-entry="${esc(r.entry_id)}" style="--lane:${n};--left:${left}%;--width:${width}%" title="${esc(r.entry_id)} ${esc(r.kind)} · ${esc(r.summary)}"></button>`}).join('');track.innerHTML=marks+spans;track.querySelectorAll('.span').forEach(span=>span.onclick=()=>focusEvent(span.dataset.entry))}
function draw(data){$('session').textContent=data.sessionId;$('counts').textContent=`${data.eventCount} events · ${data.currentSurfaceItems} surface · ${data.matchingCount} matched`;$('position').textContent=data.activeTurn==null?'idle':`turn ${data.activeTurn} / step ${data.activeStep??'—'}`;$('older').disabled=!hasEarlier;rows.innerHTML=data.rows.map(r=>`<tr data-entry="${esc(r.entry_id)}" class="${esc(r.class)} ${esc(r.state)} ${esc(r.visibility)}"><td class="seq" title="${esc(r.entry_id)}">${r.parent_seq==null?r.seq:`${r.parent_seq}·${r.seq}`}</td><td class="time">${time(r.at_ms)}</td><td class="class"><span class="pill">${esc(r.class)}</span></td><td class="layer">${esc(r.layer)}</td><td class="actor">${esc(r.actor)}</td><td class="kind">${esc(r.kind)}</td><td class="producer">${esc(r.producer)}</td><td class="state">${esc(r.state)}</td><td class="turn">${r.turn_id??'—'}/${r.step_index??'—'}</td><td class="duration">${duration(r.duration_ms)}</td><td class="summary" title="${esc(r.summary)}">${esc(r.summary)}</td></tr>`).join('');
window.__trajectory=data.rows;rows.querySelectorAll('tr').forEach(tr=>tr.onclick=()=>inspect(tr.dataset.entry,tr));drawOverview(data.rows);if(selected!=null)focusEvent(selected,false);if(follow)ledger.scrollTop=ledger.scrollHeight}
function selector(entry){return `[data-entry="${CSS.escape(entry)}"]`}function inspect(entry,tr){rows.querySelector('.selected')?.classList.remove('selected');track.querySelector('.selected')?.classList.remove('selected');tr?.classList.add('selected');track.querySelector(selector(entry))?.classList.add('selected');selected=entry;const r=window.__trajectory.find(x=>x.entry_id===entry);if(!r)return;$('hint').style.display='none';$('details').textContent=JSON.stringify(r,null,2)}
function focusEvent(entry,scroll=true){const tr=rows.querySelector(selector(entry));if(!tr)return;follow=false;$('follow').classList.remove('on');$('follow').textContent='tail paused';inspect(entry,tr);if(scroll)tr.scrollIntoView({block:'center'})}
function queryParams(){const p=new URLSearchParams({limit:'5000'});if($('search').value)p.set('search',$('search').value);for(const id of ['layer','actor','class','producer','visibility'])if($(id).value)p.set(id,$(id).value);return p}
function rootSeq(r){return r.parent_seq??r.seq}function mergeRows(...groups){const byId=new Map;for(const group of groups)for(const row of group)byId.set(row.entry_id,row);return [...byId.values()].sort((a,b)=>rootSeq(a)-rootSeq(b)||(a.parent_seq!=null)-(b.parent_seq!=null)||a.seq-b.seq)}
async function fetchPage(params){const endpoint=new URL('api/trajectory',window.location.href);endpoint.search=params;const res=await fetch(endpoint);if(!res.ok)throw Error(await res.text());return await res.json()}
async function load(){clearTimeout(timer);try{const data=await fetchPage(queryParams());latestData=data;if(!olderRows.length)hasEarlier=data.hasEarlier;data.rows=mergeRows(olderRows,data.rows);draw(data);$('health').textContent='● live';$('health').style.color='var(--green)'}catch(e){$('health').textContent='● '+e.message;$('health').style.color='var(--red)'}timer=setTimeout(load,1000)}
async function loadEarlier(){const visible=window.__trajectory??[];if(!visible.length||!hasEarlier)return;const oldHeight=ledger.scrollHeight,oldTop=ledger.scrollTop,p=queryParams();p.set('before',String(rootSeq(visible[0])));$('older').disabled=true;try{const page=await fetchPage(p);olderRows=mergeRows(page.rows,olderRows);hasEarlier=page.hasEarlier;if(latestData){latestData.rows=mergeRows(olderRows,latestData.rows);draw(latestData);ledger.scrollTop=oldTop+(ledger.scrollHeight-oldHeight)}}catch(e){$('health').textContent='● '+e.message;$('health').style.color='var(--red)'}$('older').disabled=!hasEarlier}
function resetWindow(){olderRows=[];hasEarlier=false;load()}
$('older').onclick=loadEarlier;$('follow').onclick=()=>{follow=!follow;$('follow').classList.toggle('on',follow);$('follow').textContent=follow?'tail follow':'tail paused'};$('refresh').onclick=load;for(const id of ['layer','actor','class','producer','visibility'])$(id).onchange=resetWindow;let debounce;$('search').oninput=()=>{clearTimeout(debounce);debounce=setTimeout(resetWindow,180)};ledger.onscroll=()=>{if(ledger.scrollHeight-ledger.scrollTop-ledger.clientHeight>80){follow=false;$('follow').classList.remove('on');$('follow').textContent='tail paused'}};load();
</script></body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut cache = TrajectoryCache::default();
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
        let mut cache = TrajectoryCache::default();
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
            timeline_path: path,
            sidebands_dir: dir.path().join("sidebands"),
            cache: Arc::new(Mutex::new(TrajectoryCache::default())),
        };

        let response = query_cached(
            &state,
            TrajectoryQuery {
                after: Some(0),
                limit: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(response.matching_count, 2);
        assert_eq!(response.rows.len(), 1);
        assert_eq!(response.rows[0].seq, 1);
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
            timeline_path: path,
            sidebands_dir: dir.path().join("sidebands"),
            cache: Arc::new(Mutex::new(TrajectoryCache::default())),
        };

        let response = query_cached(
            &state,
            TrajectoryQuery {
                before: Some(2),
                limit: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(response.rows[0].seq, 1);
        assert!(response.has_earlier);
        assert_eq!(response.first_seq, Some(1));
    }

    #[test]
    fn query_rejects_ambiguous_bidirectional_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.jsonl");
        std::fs::write(&path, "").unwrap();
        let state = AppState {
            session_id: "session".into(),
            actor_ref: "main".into(),
            timeline_path: path,
            sidebands_dir: dir.path().join("sidebands"),
            cache: Arc::new(Mutex::new(TrajectoryCache::default())),
        };
        let error = query_cached(
            &state,
            TrajectoryQuery {
                after: Some(1),
                before: Some(2),
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
    fn page_exposes_timeline_overview_and_four_dimension_filters() {
        assert!(PAGE.contains("id=\"track\""));
        assert!(PAGE.contains(">INPUT</span>"));
        assert!(PAGE.contains(">MODEL</span>"));
        assert!(PAGE.contains(">TOOLS</span>"));
        assert!(PAGE.contains("id=\"layer\""));
        assert!(PAGE.contains("id=\"actor\""));
        assert!(PAGE.contains("id=\"class\""));
        assert!(PAGE.contains("id=\"producer\""));
        assert!(PAGE.contains("<option>governance</option>"));
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
            timeline_path: path,
            sidebands_dir: dir.path().join("sidebands"),
            cache: Arc::new(Mutex::new(TrajectoryCache::default())),
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
                    input_refs: vec![chat_state::TimelineRangeRef {
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
                input_refs: vec![chat_state::TimelineRangeRef {
                    timeline_id: "session".into(),
                    first_seq: 0,
                    last_seq: 0,
                }],
                route: chat_state::SidebandRoute {
                    model: "model".into(),
                    backend: "responses".into(),
                },
                initiator_ref: "t:session/1".into(),
                executor: "main".into(),
                output_schema: Some(serde_json::json!({"type": "object"})),
            }),
            chat_state::SidebandEventKind::Attempt(chat_state::SidebandAttempt {
                attempt_no: 1,
                feedback: None,
            }),
            chat_state::SidebandEventKind::Result(chat_state::SidebandResult {
                raw_output: r#"{"decision":"allow","reason":"safe"}"#.into(),
                structured_output: Some(serde_json::json!({"decision": "allow", "reason": "safe"})),
                usage: chat_state::SidebandUsage::default(),
                finish: "stop".into(),
                source_event_seqs: [0, 1],
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
            timeline_path: path,
            sidebands_dir: dir.path().join("sidebands"),
            cache: Arc::new(Mutex::new(TrajectoryCache::default())),
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
        assert!(response.rows.iter().all(|row| row.parent_seq == Some(1)));
        assert_eq!(response.first_seq, Some(1));
        assert_eq!(response.last_seq, Some(1));
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
            timeline_path: state.timeline_path.clone(),
            sidebands_dir: state.sidebands_dir.clone(),
            cache: Arc::new(Mutex::new(TrajectoryCache::default())),
        };
        let error = query_cached(&tampered_state, TrajectoryQuery::default()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("does not match its parent spawn")
        );
    }

    #[tokio::test]
    async fn server_rejects_non_loopback_bind_addresses() {
        let error = serve("missing", "0.0.0.0:0".parse().unwrap(), |_| {})
            .await
            .unwrap_err();
        assert!(error.to_string().contains("loopback"));
    }
}
