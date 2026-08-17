//! Local-only Trajectory query server.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct AppState {
    session_id: String,
    session_dir: PathBuf,
}

#[derive(Debug, Default, Deserialize)]
struct TrajectoryQuery {
    after: Option<u64>,
    category: Option<String>,
    visibility: Option<String>,
    search: Option<String>,
    limit: Option<usize>,
}

#[derive(Serialize)]
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
    last_seq: Option<u64>,
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
            "session '{}' has no Timeline v2 ledger at {}",
            session_id,
            timeline_path.display()
        );
    }

    let state = AppState {
        session_id: session_id.to_owned(),
        session_dir,
    };
    let app = Router::new()
        .route("/", get(index))
        .route("/api/trajectory", get(query_trajectory))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let local = listener.local_addr()?;
    let url = format!("http://{local}");
    on_ready(&url);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(PAGE)
}

async fn query_trajectory(
    State(state): State<AppState>,
    Query(query): Query<TrajectoryQuery>,
) -> Result<Json<TrajectoryResponse>, (StatusCode, String)> {
    let path = state.session_dir.join(super::storage::TIMELINE_FILE);
    let timeline = tokio::task::spawn_blocking(move || read_timeline(&path))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    let snapshot = timeline.trajectory();
    let search = query.search.as_deref().map(str::to_lowercase);
    let category = query.category.as_deref().filter(|value| !value.is_empty());
    let visibility = query
        .visibility
        .as_deref()
        .filter(|value| !value.is_empty());
    let mut rows = snapshot
        .rows
        .into_iter()
        .filter(|row| query.after.is_none_or(|after| row.seq > after))
        .filter(|row| category.is_none_or(|category| row.category == category))
        .filter(|row| {
            visibility.is_none_or(|visibility| {
                serde_json::to_value(row.visibility)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .as_deref()
                    == Some(visibility)
            })
        })
        .filter(|row| {
            search.as_ref().is_none_or(|needle| {
                format!(
                    "{} {} {} {} {} {}",
                    row.category,
                    row.name,
                    row.state,
                    row.summary,
                    row.correlation_id.as_deref().unwrap_or_default(),
                    row.details
                )
                .to_lowercase()
                .contains(needle)
            })
        })
        .collect::<Vec<_>>();
    let matching_count = rows.len();
    let limit = query.limit.unwrap_or(2_000).clamp(1, 10_000);
    if rows.len() > limit {
        rows.drain(..rows.len() - limit);
    }
    let last_seq = rows.last().map(|row| row.seq);
    Ok(Json(TrajectoryResponse {
        session_id: state.session_id,
        schema_version: snapshot.schema_version,
        event_count: snapshot.event_count,
        current_surface_items: snapshot.current_surface_items,
        active_turn: snapshot.active_turn,
        active_step: snapshot.active_step,
        open_requests: snapshot.open_requests,
        open_tools: snapshot.open_tools,
        matching_count,
        last_seq,
        rows,
    }))
}

fn read_timeline(path: &Path) -> anyhow::Result<chat_state::Timeline> {
    let bytes = std::fs::read(path)?;
    let complete_len = if bytes.last().is_none_or(|byte| *byte == b'\n') {
        bytes.len()
    } else {
        bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1)
    };
    let mut events = Vec::new();
    for (index, line) in bytes[..complete_len]
        .split(|byte| *byte == b'\n')
        .enumerate()
    {
        if line.is_empty() {
            continue;
        }
        let event = serde_json::from_slice::<chat_state::TimelineEvent>(line)
            .map_err(|error| anyhow::anyhow!("{}:{}: {error}", path.display(), index + 1))?;
        events.push(event);
    }
    chat_state::Timeline::from_events(events).map_err(anyhow::Error::from)
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
.controls{height:52px;display:flex;align-items:center;gap:10px;padding:8px 18px;border-bottom:1px solid var(--line);background:var(--panel)}input,select,button{height:34px;background:#0b0e13;color:var(--text);border:1px solid var(--line);border-radius:6px;padding:0 10px;font:inherit}input{width:min(480px,40vw)}button{cursor:pointer}button:hover{border-color:#4b5563}.follow.on{color:var(--green);border-color:#28623e}
main{height:calc(100vh - 110px);display:grid;grid-template-columns:minmax(0,1fr) 420px}.ledger{overflow:auto}.inspector{border-left:1px solid var(--line);background:#0e1116;overflow:auto;padding:16px}.inspector h3{margin:0 0 12px}.inspector pre{white-space:pre-wrap;word-break:break-word;color:#cbd5e1;line-height:1.5}.empty{color:var(--muted)}
table{width:100%;border-collapse:collapse;table-layout:fixed}thead{position:sticky;top:0;background:#151920;z-index:2}th{text-align:left;color:var(--muted);font-weight:500;padding:9px 8px;border-bottom:1px solid var(--line)}td{padding:8px;border-bottom:1px solid #1b2028;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}tr{cursor:pointer}tbody tr:hover,tbody tr.selected{background:#151b24}.seq{width:62px;color:#657083}.time{width:98px}.scope{width:78px}.kind{width:132px}.state{width:92px}.turn{width:72px}.duration{width:86px;text-align:right}.summary{width:auto}.message .kind{color:var(--accent)}.tool .kind{color:var(--green)}.request .kind{color:#c4b5fd}.failed,.cancelled{color:var(--red)}.retrying{color:var(--yellow)}.shadowed{opacity:.48}.pill{padding:2px 6px;border:1px solid var(--line);border-radius:999px}
@media(max-width:900px){main{grid-template-columns:1fr}.inspector{display:none}.stats{display:none}.turn{display:none}}
</style></head><body>
<header><span class="brand">GROW / TRAJECTORY</span><span class="session" id="session">loading…</span><div class="stats"><span id="counts"></span><span id="position"></span><span class="live" id="health">● live</span></div></header>
<div class="controls"><input id="search" placeholder="Search summary, id, raw details…"><select id="category"><option value="">all categories</option><option>turn</option><option>step</option><option>request</option><option>message</option><option>tool</option><option>compaction</option><option>recovery</option><option>observation</option></select><select id="visibility"><option value="">all surfaces</option><option value="current">current</option><option value="shadowed">shadowed</option><option value="log_only">log only</option></select><button class="follow on" id="follow">tail follow</button><button id="refresh">refresh</button></div>
<main><div class="ledger" id="ledger"><table><thead><tr><th class="seq">seq</th><th class="time">time</th><th class="scope">scope</th><th class="kind">event</th><th class="state">state</th><th class="turn">turn/step</th><th class="duration">duration</th><th class="summary">summary</th></tr></thead><tbody id="rows"></tbody></table></div><aside class="inspector"><h3>Event inspector</h3><div class="empty" id="hint">Select an event to inspect its canonical payload.</div><pre id="details"></pre></aside></main>
<script>
const $=id=>document.getElementById(id), rows=$('rows'), ledger=$('ledger');let follow=true,selected=null,timer;
function esc(v){return String(v??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]))}
function time(ms){return new Date(ms).toLocaleTimeString([], {hour12:false})}function duration(ms){return ms==null?'—':ms<1000?ms+' ms':(ms/1000).toFixed(2)+' s'}
function draw(data){$('session').textContent=data.sessionId;$('counts').textContent=`${data.eventCount} events · ${data.currentSurfaceItems} surface · ${data.matchingCount} matched`;$('position').textContent=data.activeTurn==null?'idle':`turn ${data.activeTurn} / step ${data.activeStep??'—'}`;rows.innerHTML=data.rows.map(r=>`<tr data-seq="${r.seq}" class="${esc(r.category)} ${esc(r.state)} ${esc(r.visibility)}"><td class="seq">${r.seq}</td><td class="time">${time(r.at_ms)}</td><td class="scope"><span class="pill">${esc(r.category)}</span></td><td class="kind">${esc(r.name)}</td><td class="state">${esc(r.state)}</td><td class="turn">${r.turn_id??'—'}/${r.step_index??'—'}</td><td class="duration">${duration(r.duration_ms)}</td><td class="summary" title="${esc(r.summary)}">${esc(r.summary)}</td></tr>`).join('');
window.__trajectory=data.rows;rows.querySelectorAll('tr').forEach(tr=>tr.onclick=()=>inspect(Number(tr.dataset.seq),tr));if(follow)ledger.scrollTop=ledger.scrollHeight}
function inspect(seq,tr){rows.querySelector('.selected')?.classList.remove('selected');tr.classList.add('selected');selected=seq;const r=window.__trajectory.find(x=>x.seq===seq);$('hint').style.display='none';$('details').textContent=JSON.stringify(r,null,2)}
async function load(){clearTimeout(timer);const p=new URLSearchParams({limit:'5000'});if($('search').value)p.set('search',$('search').value);if($('category').value)p.set('category',$('category').value);if($('visibility').value)p.set('visibility',$('visibility').value);try{const res=await fetch('/api/trajectory?'+p);if(!res.ok)throw Error(await res.text());draw(await res.json());$('health').textContent='● live';$('health').style.color='var(--green)'}catch(e){$('health').textContent='● '+e.message;$('health').style.color='var(--red)'}timer=setTimeout(load,1000)}
$('follow').onclick=()=>{follow=!follow;$('follow').classList.toggle('on',follow);$('follow').textContent=follow?'tail follow':'tail paused'};$('refresh').onclick=load;for(const id of ['category','visibility'])$(id).onchange=load;let debounce;$('search').oninput=()=>{clearTimeout(debounce);debounce=setTimeout(load,180)};ledger.onscroll=()=>{if(ledger.scrollHeight-ledger.scrollTop-ledger.clientHeight>80){follow=false;$('follow').classList.remove('on');$('follow').textContent='tail paused'}};load();
</script></body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_ignores_only_an_incomplete_tail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timeline.jsonl");
        let timeline =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::user("hello")])
                .unwrap();
        let line = serde_json::to_string(&timeline.events()[0]).unwrap();
        let mut bytes = format!("{line}\n{{\"version\":").into_bytes();
        bytes.extend_from_slice(&[0xe2, 0x82]);
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(read_timeline(&path).unwrap().events().len(), 1);
    }

    #[tokio::test]
    async fn server_rejects_non_loopback_bind_addresses() {
        let error = serve("missing", "0.0.0.0:0".parse().unwrap(), |_| {})
            .await
            .unwrap_err();
        assert!(error.to_string().contains("loopback"));
    }
}
