//! Local-only Trajectory query server.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Html;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

const LEDGER_PREFIX_PROBE_BYTES: u64 = 4096;

#[derive(Clone)]
struct AppState {
    session_id: String,
    actor_ref: String,
    #[cfg(test)]
    session_dir: PathBuf,
    sessions_root: PathBuf,
    cache: Arc<Mutex<SessionTrajectoryCache>>,
}

#[derive(Default)]
struct SessionTrajectoryCache {
    session_dir: PathBuf,
    offset: u64,
    prefix_probe: Vec<u8>,
    timeline: chat_state::Timeline,
    projector: chat_state::TrajectoryProjector,
    sidebands: BTreeMap<String, SidebandCache>,
    workflows: BTreeMap<String, WorkflowJournalCache>,
    children: BTreeMap<String, SessionTrajectoryCache>,
}

#[derive(Default)]
struct SidebandCache {
    offset: u64,
    prefix_probe: Vec<u8>,
    events: Vec<chat_state::SidebandEvent>,
}

#[derive(Default)]
struct WorkflowJournalCache {
    offset: u64,
    entries: Vec<workflow::JournalEntry>,
    prefix_probe: Vec<u8>,
}

#[derive(Debug, Default, Deserialize)]
struct TrajectoryQuery {
    after: Option<u64>,
    before: Option<u64>,
    entry: Option<String>,
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
    open_workflows: Vec<String>,
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
    super::storage::open_regular_nofollow(&timeline_path, "Trajectory Timeline ledger").map_err(
        |error| {
            anyhow::anyhow!(
                "session '{}' has no readable Timeline v7 ledger at {}: {error}",
                session_id,
                timeline_path.display()
            )
        },
    )?;

    let listener = tokio::net::TcpListener::bind(bind).await?;
    let local = listener.local_addr()?;
    let token = uuid::Uuid::now_v7().simple().to_string();
    let state = AppState {
        session_id: session_id.to_owned(),
        actor_ref: session_actor_ref(&session_dir, session_id)?,
        #[cfg(test)]
        session_dir: session_dir.clone(),
        sessions_root: crate::util::grow_home::grow_home().join("sessions"),
        cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
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

async fn index(
    headers: HeaderMap,
) -> Result<(HeaderMap, Html<&'static str>), (StatusCode, String)> {
    require_local_host(&headers)?;
    Ok((response_security_headers(), Html(PAGE)))
}

async fn query_trajectory(
    State(state): State<AppState>,
    Query(query): Query<TrajectoryQuery>,
    headers: HeaderMap,
) -> Result<(HeaderMap, Json<TrajectoryResponse>), (StatusCode, String)> {
    require_local_host(&headers)?;
    if query.after.is_some() as u8 + query.before.is_some() as u8 + query.entry.is_some() as u8 > 1
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "after, before, and entry are mutually exclusive".into(),
        ));
    }
    let response = tokio::task::spawn_blocking(move || query_cached(&state, query))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
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
    let resolver =
        super::storage::relocation::RelocationView::load_for_sessions_root(&state.sessions_root)?;
    let session_dir = match resolver.find_persisted_session_dir(&state.session_id)? {
        Some(path) => path,
        None => {
            #[cfg(test)]
            {
                state.session_dir.clone()
            }
            #[cfg(not(test))]
            {
                anyhow::bail!(
                    "session '{}' disappeared from local storage",
                    state.session_id
                )
            }
        }
    };
    let mut visited = BTreeSet::from([state.session_id.clone()]);
    cache.refresh_tree(&session_dir, &state.session_id, &resolver, &mut visited)?;
    let mut all_rows = Vec::new();
    cache.collect_rows(
        &state.session_id,
        &state.actor_ref,
        None,
        &[],
        &mut all_rows,
    )?;
    all_rows.sort_by(|left, right| left.nesting_path.cmp(&right.nesting_path));
    if let Some(collision) = all_rows
        .windows(2)
        .find(|pair| pair[0].nesting_path == pair[1].nesting_path)
    {
        anyhow::bail!(
            "Trajectory entries '{}' and '{}' share causal path {:?}",
            collision[0].entry_id,
            collision[1].entry_id,
            collision[0].nesting_path
        );
    }
    let focus_root = query
        .entry
        .as_deref()
        .map(|entry_id| {
            all_rows
                .iter()
                .find(|row| row.entry_id == entry_id)
                .map(root_seq)
                .ok_or_else(|| anyhow::anyhow!("Trajectory entry '{entry_id}' was not found"))
        })
        .transpose()?;
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
    let matches_query = |row: &chat_state::TrajectoryRow| {
        query.after.is_none_or(|after| root_seq(row) > after)
            && query.before.is_none_or(|before| root_seq(row) < before)
            && layer.is_none_or(|value| dimension_matches(&row.layer, value))
            && actor.is_none_or(|value| dimension_matches(&row.actor, value))
            && class.is_none_or(|value| row.class == value)
            && producer.is_none_or(|value| dimension_matches(&row.producer, value))
            && visibility.is_none_or(|value| visibility_name(row.visibility) == value)
            && search.as_ref().is_none_or(|needle| {
                format!(
                    "{} {} {} {} {} {} {} {} {} {} {} {} {} {}",
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
                    row.details,
                )
                .to_lowercase()
                .contains(needle)
            })
    };
    let matching = all_rows
        .iter()
        .filter(|row| focus_root.map_or_else(|| matches_query(row), |root| root_seq(row) == root))
        .collect::<Vec<_>>();
    let matching_count = matching.len();
    let root_sequences = matching
        .iter()
        .map(|row| root_seq(row))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let root_count = root_sequences.len();
    let (start, end) = if query.after.is_some() {
        (0, root_count.min(limit))
    } else {
        (root_count.saturating_sub(limit), root_count)
    };
    let has_earlier = focus_root.map_or_else(
        || query.after.is_none() && start > 0,
        |root| all_rows.iter().any(|row| root_seq(row) < root),
    );
    let selected_roots = root_sequences[start..end]
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let rows = matching
        .into_iter()
        .filter(|row| selected_roots.contains(&root_seq(row)))
        .cloned()
        .collect::<Vec<_>>();
    let first_seq = root_sequences.get(start).copied();
    let last_seq = end
        .checked_sub(1)
        .and_then(|index| root_sequences.get(index).copied());
    Ok(TrajectoryResponse {
        session_id: state.session_id.clone(),
        schema_version: chat_state::TRAJECTORY_SCHEMA_VERSION,
        event_count: cache.event_count(),
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
        open_workflows: cache
            .timeline
            .open_workflow_run_ids()
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
    let summary = super::persistence::read_summary_in_session_dir(session_dir)?;
    let actor = match summary.session_kind.as_deref() {
        Some(kind) if kind.starts_with("subagent") => format!("subagent:{session_id}"),
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

fn root_seq(row: &chat_state::TrajectoryRow) -> u64 {
    *row.nesting_path
        .first()
        .expect("Trajectory rows always have a non-empty nesting path")
}

impl SessionTrajectoryCache {
    fn refresh_tree(
        &mut self,
        session_dir: &Path,
        timeline_id: &str,
        resolver: &super::storage::relocation::RelocationView,
        visited: &mut BTreeSet<String>,
    ) -> anyhow::Result<()> {
        if !self.session_dir.as_os_str().is_empty() && self.session_dir != session_dir {
            *self = Self::default();
        }
        self.session_dir = session_dir.to_owned();
        self.refresh(&session_dir.join(super::storage::TIMELINE_FILE))?;
        self.refresh_sidebands(&session_dir.join("sidebands"))?;
        for sideband_id in self.sidebands.keys() {
            if !visited.insert(sideband_id.clone()) {
                anyhow::bail!(
                    "Timeline identity '{sideband_id}' is linked more than once in the Trajectory tree"
                );
            }
        }
        self.refresh_workflows(timeline_id, visited)?;

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
            let child_dir = resolver.find_persisted_session_dir(&spawn.child_session_id)?;
            let Some(child_dir) = child_dir else {
                if terminal.is_some_and(terminal_requires_child) {
                    anyhow::bail!(
                        "terminal subagent '{}' requires missing child Timeline '{}'",
                        spawn.subagent_id,
                        spawn.child_session_id
                    );
                }
                continue;
            };
            let summary = super::persistence::read_summary_in_session_dir(&child_dir)?;
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
            let child_timeline_path = child_dir.join(super::storage::TIMELINE_FILE);
            match std::fs::symlink_metadata(&child_timeline_path) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                    anyhow::bail!(
                        "child Timeline is not a regular file: {}",
                        child_timeline_path.display()
                    );
                }
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
            child.refresh_tree(&child_dir, &spawn.child_session_id, resolver, visited)?;
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

    fn refresh(&mut self, path: &Path) -> anyhow::Result<()> {
        let mut file = super::storage::open_regular_nofollow(path, "Trajectory Timeline ledger")?;
        let file_len = file.metadata()?.len();
        if file_len < self.offset
            || !ledger_prefix_matches(self.offset, &self.prefix_probe, &mut file)?
        {
            let session_dir = std::mem::take(&mut self.session_dir);
            *self = Self::default();
            self.session_dir = session_dir;
        }
        let (bytes, complete_len) = read_ledger_batch(&mut file, self.offset, path)?;
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
        refresh_ledger_prefix_probe(self.offset, &mut self.prefix_probe, &mut file)?;
        Ok(())
    }

    fn refresh_sidebands(&mut self, directory: &Path) -> anyhow::Result<()> {
        match std::fs::symlink_metadata(directory) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                anyhow::bail!(
                    "Trajectory sidebands path is not a regular directory: {}",
                    directory.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.sidebands.clear();
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        }
        let mut entries = std::fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        let mut seen = BTreeSet::new();
        for entry in entries {
            if !entry.file_type()?.is_dir() {
                anyhow::bail!(
                    "unsupported entry in Trajectory sidebands directory: {}",
                    entry.path().display()
                );
            }
            let sideband_id = entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("Trajectory sideband id is not valid UTF-8"))?;
            chat_state::validate_sideband_id(&sideband_id)?;
            let path = entry.path().join(super::storage::TIMELINE_FILE);
            match std::fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                    anyhow::bail!(
                        "Trajectory sideband ledger is not a regular file: {}",
                        path.display()
                    );
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    let children =
                        std::fs::read_dir(entry.path())?.collect::<Result<Vec<_>, _>>()?;
                    if children.is_empty()
                        || children.iter().all(|child| {
                            child.file_name().to_string_lossy()
                                == format!("{}.lock", super::storage::TIMELINE_FILE)
                        })
                    {
                        continue;
                    }
                    anyhow::bail!(
                        "Trajectory sideband {sideband_id} directory has no Timeline ledger"
                    );
                }
                Err(error) => return Err(error.into()),
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

    fn refresh_workflows(
        &mut self,
        timeline_id: &str,
        visited: &mut BTreeSet<String>,
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
                    private,
                    ..
                }) => Some((
                    run_id.clone(),
                    (event.seq.get(), name.clone(), objective.clone(), *private),
                )),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        let directory = self.session_dir.join("workflows");
        if !spawns.is_empty() {
            let metadata = std::fs::symlink_metadata(&directory)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                anyhow::bail!(
                    "Trajectory workflows path is not a regular directory: {}",
                    directory.display()
                );
            }
        }
        let mut seen = BTreeSet::new();
        for (run_id, (spawn_seq, name, objective, private)) in &spawns {
            if !visited.insert(run_id.clone()) {
                anyhow::bail!(
                    "Workflow identity '{run_id}' is linked more than once in the Trajectory tree"
                );
            }
            super::workflow::store::validate_run_id(run_id)?;
            let run_dir = directory.join(run_id);
            let run_metadata = match std::fs::symlink_metadata(&run_dir) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            if run_metadata.file_type().is_symlink() || !run_metadata.is_dir() {
                anyhow::bail!(
                    "Workflow run directory is not a regular directory: {}",
                    run_dir.display()
                );
            }
            match std::fs::symlink_metadata(run_dir.join("cleared")) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                    continue;
                }
                Ok(_) => anyhow::bail!("Workflow cleared marker is not a regular file"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            let manifest_path = run_dir.join("state.json");
            let manifest = match super::workflow::store::read_bounded_nofollow(
                &manifest_path,
                super::workflow::store::MAX_WORKFLOW_MANIFEST_BYTES,
            ) {
                Ok(bytes) => {
                    serde_json::from_slice::<super::workflow::store::WorkflowRunManifest>(&bytes)?
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    match std::fs::symlink_metadata(run_dir.join("journal.jsonl")) {
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
                || manifest.state.private != *private
                || manifest.state.journal_path.as_deref() != Some(expected_journal.as_str())
            {
                anyhow::bail!("Workflow manifest does not match spawn t:{timeline_id}/{spawn_seq}");
            }
            let journal_path = run_dir.join("journal.jsonl");
            let journal = self.workflows.entry(run_id.clone()).or_default();
            match std::fs::symlink_metadata(&journal_path) {
                Ok(_) => {
                    journal.refresh(&journal_path)?;
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
            chat_state::SidebandTimeline::from_events(sideband.events.clone())?.validate_parent(
                parent_timeline_id,
                &self.timeline,
                parent_seq,
                spawn,
            )?;
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
            let mut row = projected.clone();
            row.entry_id = format!("t:{timeline_id}/{}", row.seq);
            row.parent_entry_id = parent_entry_id.map(str::to_owned);
            row.nesting_path = path_prefix
                .iter()
                .copied()
                .chain(std::iter::once(row.seq))
                .collect();
            let event_index = usize::try_from(row.seq)
                .map_err(|_| anyhow::anyhow!("Timeline {timeline_id} seq exceeds usize"))?;
            let event = self.timeline.events().get(event_index).ok_or_else(|| {
                anyhow::anyhow!("Trajectory projector outran Timeline {timeline_id}")
            })?;
            row.actor = match &event.kind {
                chat_state::TimelineEventKind::Workflow(event) => {
                    format!("workflow:{}", workflow_run_id(event))
                }
                _ => actor_ref.to_owned(),
            };
            let workflow_parent = match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) => spawn.workflow_run_id.as_deref().and_then(|run_id| {
                    self.workflow_agent_parent(timeline_id, run_id, &spawn.subagent_id, path_prefix)
                }),
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(end)) => {
                    subagent_workflows.get(&end.subagent_id).and_then(|run_id| {
                        self.workflow_agent_parent(
                            timeline_id,
                            run_id,
                            &end.subagent_id,
                            path_prefix,
                        )
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
        for entry in &journal.entries {
            rows.push(workflow_row(entry, run_id, parent_entry_id, path_prefix));
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
            journal.entries.iter().find(|entry| {
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
        let attempt_times = sideband
            .events
            .iter()
            .filter_map(|event| {
                matches!(event.kind, chat_state::SidebandEventKind::Attempt(_))
                    .then_some((event.seq, event.at_ms))
            })
            .collect::<BTreeMap<_, _>>();
        for event in &sideband.events {
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
                .map(|workflow| workflow.entries.len())
                .sum::<usize>()
            + self
                .sidebands
                .values()
                .map(|sideband| sideband.events.len())
                .sum::<usize>()
            + self.children.values().map(Self::event_count).sum::<usize>()
    }
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

fn workflow_row(
    entry: &workflow::JournalEntry,
    run_id: &str,
    parent_entry_id: &str,
    path_prefix: &[u64],
) -> chat_state::TrajectoryRow {
    let failed = entry
        .result
        .get(workflow::journal::HOST_ERROR_KEY)
        .and_then(serde_json::Value::as_str);
    let result = serde_json::to_string(&entry.result).unwrap_or_else(|_| "null".into());
    chat_state::TrajectoryRow {
        entry_id: format!("t:{run_id}/{}", entry.seq),
        seq: entry.seq,
        parent_entry_id: Some(parent_entry_id.to_owned()),
        nesting_path: path_prefix.iter().copied().chain([0, entry.seq]).collect(),
        at_ms: i64::try_from(entry.at_ms).unwrap_or(i64::MAX),
        layer: "tool.result".into(),
        actor: format!("workflow:{run_id}"),
        class: "message".into(),
        producer: format!("workflow-host:{}", entry.kind),
        kind: format!("workflow.host_call.{}", entry.kind),
        state: if failed.is_some() {
            "failed".into()
        } else {
            "completed".into()
        },
        visibility: chat_state::SurfaceVisibility::LogOnly,
        turn_id: None,
        step_index: None,
        correlation_id: Some(entry.req_hash.clone()),
        duration_ms: None,
        summary: failed.map_or_else(
            || format!("{} · {}", entry.kind, crate::util::truncate(&result, 220)),
            |error| format!("{} · {}", entry.kind, crate::util::truncate(error, 220)),
        ),
        details: serde_json::to_value(entry).unwrap_or(serde_json::Value::Null),
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
    for entry in &journal.entries {
        if entry.kind != "spawn_agent"
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
    fn refresh(&mut self, path: &Path) -> anyhow::Result<()> {
        let mut file = super::storage::open_regular_nofollow(path, "Trajectory sideband ledger")?;
        let file_len = file.metadata()?.len();
        if file_len < self.offset
            || !ledger_prefix_matches(self.offset, &self.prefix_probe, &mut file)?
        {
            *self = Self::default();
        }
        let (bytes, complete_len) = read_ledger_batch(&mut file, self.offset, path)?;
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
        refresh_ledger_prefix_probe(self.offset, &mut self.prefix_probe, &mut file)?;
        Ok(())
    }
}

impl WorkflowJournalCache {
    fn refresh(&mut self, path: &Path) -> anyhow::Result<()> {
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > workflow::journal::MAX_JOURNAL_BYTES
        {
            anyhow::bail!("invalid Workflow journal: {}", path.display());
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options.open(path)?;
        let opened = file.metadata()?;
        if !opened.is_file() || opened.len() > workflow::journal::MAX_JOURNAL_BYTES {
            anyhow::bail!("Workflow journal changed during open: {}", path.display());
        }
        if opened.len() < self.offset || !self.prefix_matches(&mut file)? {
            *self = Self::default();
        }
        file.seek(std::io::SeekFrom::Start(self.offset))?;
        let mut bytes = Vec::new();
        let remaining = workflow::journal::MAX_JOURNAL_BYTES.saturating_sub(self.offset);
        (&mut file)
            .take(remaining.saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > remaining {
            anyhow::bail!("Workflow journal exceeds the byte limit");
        }
        let complete_len = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        let mut entries = self.entries.clone();
        for line in bytes[..complete_len].split(|byte| *byte == b'\n') {
            if line.is_empty() {
                continue;
            }
            if entries.len() >= workflow::journal::MAX_JOURNAL_ENTRIES {
                anyhow::bail!("Workflow journal exceeds the entry limit");
            }
            let entry = serde_json::from_slice::<workflow::JournalEntry>(line)?;
            let expected = u64::try_from(entries.len())?;
            if entry.seq != expected {
                anyhow::bail!(
                    "Workflow journal is not dense: expected {expected}, found {}",
                    entry.seq
                );
            }
            validate_workflow_journal_entry(&entry)?;
            entries.push(entry);
        }
        self.entries = entries;
        self.offset = self.offset.saturating_add(complete_len as u64);
        self.refresh_prefix_probe(&mut file)?;
        Ok(())
    }

    fn prefix_matches(&self, file: &mut std::fs::File) -> anyhow::Result<bool> {
        if self.prefix_probe.is_empty() {
            return Ok(true);
        }
        let start = self.offset.saturating_sub(self.prefix_probe.len() as u64);
        file.seek(std::io::SeekFrom::Start(start))?;
        let mut actual = vec![0; self.prefix_probe.len()];
        file.read_exact(&mut actual)?;
        Ok(actual == self.prefix_probe)
    }

    fn refresh_prefix_probe(&mut self, file: &mut std::fs::File) -> anyhow::Result<()> {
        const PROBE_BYTES: u64 = 4096;
        let start = self.offset.saturating_sub(PROBE_BYTES);
        file.seek(std::io::SeekFrom::Start(start))?;
        self.prefix_probe.clear();
        file.take(self.offset.saturating_sub(start))
            .read_to_end(&mut self.prefix_probe)?;
        Ok(())
    }
}

fn sideband_row(
    event: &chat_state::SidebandEvent,
    parent_entry_id: &str,
    path_prefix: &[u64],
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
        entry_id: format!("t:{}/{}", event.sideband_id, event.seq),
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

fn ledger_prefix_matches(
    offset: u64,
    expected: &[u8],
    file: &mut std::fs::File,
) -> anyhow::Result<bool> {
    if expected.is_empty() {
        return Ok(true);
    }
    let Some(start) = offset.checked_sub(expected.len() as u64) else {
        return Ok(false);
    };
    file.seek(std::io::SeekFrom::Start(start))?;
    let mut actual = vec![0; expected.len()];
    match file.read_exact(&mut actual) {
        Ok(()) => Ok(actual == expected),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn refresh_ledger_prefix_probe(
    offset: u64,
    probe: &mut Vec<u8>,
    file: &mut std::fs::File,
) -> anyhow::Result<()> {
    let start = offset.saturating_sub(LEDGER_PREFIX_PROBE_BYTES);
    file.seek(std::io::SeekFrom::Start(start))?;
    let mut next = Vec::new();
    file.take(offset.saturating_sub(start))
        .read_to_end(&mut next)?;
    *probe = next;
    Ok(())
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
table{width:100%;min-width:1280px;border-collapse:collapse;table-layout:fixed}thead{position:sticky;top:0;background:#151920;z-index:2}th{text-align:left;color:var(--muted);font-weight:500;padding:9px 8px;border-bottom:1px solid var(--line)}td{padding:8px;border-bottom:1px solid #1b2028;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}tr{cursor:pointer}tbody tr.event-row{height:34px}tbody tr.spacer{cursor:default}tbody tr.spacer td{padding:0;border:0}tbody tr:hover,tbody tr.selected{background:#151b24}.seq{width:108px;color:#657083}.time{width:92px}.class{width:92px}.layer{width:112px}.actor{width:154px}.kind{width:150px}.producer{width:116px}.state{width:88px}.turn{width:72px}.duration{width:82px;text-align:right}.summary{width:auto}.message .kind{color:var(--accent)}.audit .kind{color:var(--green)}.auxiliary .kind{color:#c4b5fd}.failed,.cancelled{color:var(--red)}.retrying{color:var(--yellow)}.shadowed{opacity:.48}.pill{padding:2px 6px;border:1px solid var(--line);border-radius:999px}
@media(max-width:900px){main{grid-template-columns:1fr}.inspector{display:none}.stats{display:none}.turn{display:none}}
</style></head><body>
<header><span class="brand">GROW / TRAJECTORY</span><span class="session" id="session">loading…</span><div class="stats"><span id="counts"></span><span id="position"></span><span class="live" id="health">● live</span></div></header>
<div class="controls"><input id="search" placeholder="Search coordinates, kind, id, payload…"><select id="layer"><option value="">all layers</option><option>system</option><option>user</option><option>assistant</option><option>tool</option><option>plugin</option><option>meta</option></select><select id="actor"><option value="">all actors</option><option>main</option><option>subagent</option><option>workflow</option><option>sideband</option></select><select id="class"><option value="">all classes</option><option>message</option><option>lifecycle</option><option>governance</option><option>audit</option><option>auxiliary</option></select><select id="producer"><option value="">all producers</option><option>core</option><option>model</option><option>tool</option><option>workflow-host</option><option>hook</option><option>plugin</option><option>skill</option><option>mcp</option><option>user</option></select><select id="visibility"><option value="">all surfaces</option><option value="current">current</option><option value="shadowed">shadowed</option><option value="log_only">log only</option></select><button id="older">load earlier</button><button class="follow on" id="follow">tail follow</button><button id="refresh">refresh</button></div>
<div class="overview"><div class="lane-labels"><span>INPUT</span><span>MODEL</span><span>TOOLS</span></div><div class="track" id="track"></div></div>
<main><div class="ledger" id="ledger"><table><thead><tr><th class="seq">seq</th><th class="time">time</th><th class="class">class</th><th class="layer">layer</th><th class="actor">actor</th><th class="kind">kind</th><th class="producer">producer</th><th class="state">state</th><th class="turn">turn/step</th><th class="duration">duration</th><th class="summary">summary</th></tr></thead><tbody id="rows"></tbody></table></div><aside class="inspector"><h3>Event inspector</h3><div class="empty" id="hint">Select an event to inspect its canonical payload and four-dimensional identity.</div><pre id="details"></pre></aside></main>
<script>
const $=id=>document.getElementById(id), rows=$('rows'), ledger=$('ledger'), track=$('track'),ROW_HEIGHT=34,OVERSCAN=20;function hashEntry(){if(!location.hash)return null;try{return decodeURIComponent(location.hash.slice(1))}catch{return null}}let follow=true,selected=hashEntry(),deepLinkPending=selected!=null,timer,latestData=null,olderRows=[],hasEarlier=false,displayRows=[],renderQueued=false;
function esc(v){return String(v??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]))}
function time(ms){return new Date(ms).toLocaleTimeString([], {hour12:false})}function duration(ms){return ms==null?'—':ms<1000?ms+' ms':(ms/1000).toFixed(2)+' s'}
function lane(r){if(r.layer.startsWith('tool'))return['tools',2];if(r.layer==='assistant'||r.producer.startsWith('model')||r.kind.startsWith('request.')||r.kind.startsWith('step.'))return['model',1];return['input',0]}
function drawOverview(items){if(!items.length){track.innerHTML='';return}const starts=items.map(r=>r.at_ms-(r.duration_ms??0)),ends=items.map(r=>r.at_ms),min=Math.min(...starts),max=Math.max(...ends,min+1),domain=max-min;const marks=items.filter(r=>r.kind==='turn.started').map(r=>`<i class="turn-mark" style="left:${((r.at_ms-min)/domain)*100}%"></i>`).join('');const spans=items.map(r=>{const [laneKind,n]=lane(r),start=r.at_ms-(r.duration_ms??0),left=((start-min)/domain)*100,width=Math.max(.12,((Math.max(r.duration_ms??0,1))/domain)*100);return `<button class="span ${laneKind} ${esc(r.state)} ${esc(r.visibility)}" data-entry="${esc(r.entry_id)}" style="--lane:${n};--left:${left}%;--width:${width}%" title="${esc(r.entry_id)} ${esc(r.kind)} · ${esc(r.summary)}"></button>`}).join('');track.innerHTML=marks+spans;track.querySelectorAll('.span').forEach(span=>span.onclick=()=>focusEvent(span.dataset.entry))}
function rowMarkup(r){const depth=Math.max(0,r.nesting_path.length-1),parent=r.parent_entry_id==null?'':` ← ${esc(r.parent_entry_id)}`;return `<tr data-entry="${esc(r.entry_id)}" class="event-row ${esc(r.class)} ${esc(r.state)} ${esc(r.visibility)}"><td class="seq" title="${esc(r.entry_id)}${parent}"><span style="padding-left:${depth*12}px">${depth?'↳ ':''}${r.nesting_path.join('·')}</span></td><td class="time">${time(r.at_ms)}</td><td class="class"><span class="pill">${esc(r.class)}</span></td><td class="layer">${esc(r.layer)}</td><td class="actor">${esc(r.actor)}</td><td class="kind">${esc(r.kind)}</td><td class="producer">${esc(r.producer)}</td><td class="state">${esc(r.state)}</td><td class="turn">${r.turn_id??'—'}/${r.step_index??'—'}</td><td class="duration">${duration(r.duration_ms)}</td><td class="summary" title="${esc(r.summary)}">${esc(r.summary)}</td></tr>`}
function renderLedger(){renderQueued=false;const viewport=Math.max(1,ledger.clientHeight),start=Math.max(0,Math.floor(ledger.scrollTop/ROW_HEIGHT)-OVERSCAN),end=Math.min(displayRows.length,Math.ceil((ledger.scrollTop+viewport)/ROW_HEIGHT)+OVERSCAN),top=start*ROW_HEIGHT,bottom=(displayRows.length-end)*ROW_HEIGHT;rows.innerHTML=`<tr class="spacer"><td colspan="11" style="height:${top}px"></td></tr>`+displayRows.slice(start,end).map(rowMarkup).join('')+`<tr class="spacer"><td colspan="11" style="height:${bottom}px"></td></tr>`;rows.querySelectorAll('tr.event-row').forEach(tr=>tr.onclick=()=>inspect(tr.dataset.entry,tr));if(selected!=null)rows.querySelector(selector(selected))?.classList.add('selected')}
function queueRender(){if(!renderQueued){renderQueued=true;requestAnimationFrame(renderLedger)}}
function draw(data){$('session').textContent=data.sessionId;$('counts').textContent=`${data.eventCount} events · ${data.currentSurfaceItems} surface · ${data.matchingCount} matched`;$('position').textContent=data.activeTurn==null?(data.openWorkflows.length?`${data.openWorkflows.length} workflow active`:'idle'):`turn ${data.activeTurn} / step ${data.activeStep??'—'}`;$('older').disabled=!hasEarlier;displayRows=data.rows;window.__trajectory=displayRows;renderLedger();drawOverview(displayRows);if(follow){ledger.scrollTop=Math.max(0,displayRows.length*ROW_HEIGHT-ledger.clientHeight);renderLedger()}if(selected!=null){focusEvent(selected,deepLinkPending);deepLinkPending=false}}
function selector(entry){return `[data-entry="${CSS.escape(entry)}"]`}function inspect(entry,tr){rows.querySelector('.selected')?.classList.remove('selected');track.querySelector('.selected')?.classList.remove('selected');tr?.classList.add('selected');track.querySelector(selector(entry))?.classList.add('selected');selected=entry;history.replaceState(null,'',`#${encodeURIComponent(entry)}`);const r=displayRows.find(x=>x.entry_id===entry);if(!r)return;$('hint').style.display='none';$('details').textContent=JSON.stringify(r,null,2)}
function focusEvent(entry,scroll=true){const index=displayRows.findIndex(row=>row.entry_id===entry);if(index<0)return;follow=false;$('follow').classList.remove('on');$('follow').textContent='tail paused';if(scroll){ledger.scrollTop=Math.max(0,index*ROW_HEIGHT-ledger.clientHeight/2);renderLedger()}inspect(entry,rows.querySelector(selector(entry)))}
function queryParams(){const p=new URLSearchParams({limit:'5000'});if(deepLinkPending&&selected)p.set('entry',selected);if($('search').value)p.set('search',$('search').value);for(const id of ['layer','actor','class','producer','visibility'])if($(id).value)p.set(id,$(id).value);return p}
function rootSeq(r){return r.nesting_path[0]}function comparePath(a,b){for(let i=0;i<Math.min(a.length,b.length);i++)if(a[i]!==b[i])return a[i]-b[i];return a.length-b.length}function mergeRows(...groups){const byId=new Map;for(const group of groups)for(const row of group)byId.set(row.entry_id,row);return [...byId.values()].sort((a,b)=>comparePath(a.nesting_path,b.nesting_path))}
async function fetchPage(params){const endpoint=new URL('api/trajectory',window.location.href);endpoint.search=params;const res=await fetch(endpoint);if(!res.ok)throw Error(await res.text());return await res.json()}
async function load(){clearTimeout(timer);try{const focusing=deepLinkPending&&selected!=null,data=await fetchPage(queryParams());if(focusing)olderRows=mergeRows(data.rows,olderRows);latestData=data;if(!olderRows.length||focusing)hasEarlier=data.hasEarlier;data.rows=mergeRows(olderRows,data.rows);draw(data);$('health').textContent='● live';$('health').style.color='var(--green)'}catch(e){$('health').textContent='● '+e.message;$('health').style.color='var(--red)'}timer=setTimeout(load,1000)}
async function loadEarlier(){const visible=window.__trajectory??[];if(!visible.length||!hasEarlier)return;const oldHeight=ledger.scrollHeight,oldTop=ledger.scrollTop,p=queryParams();p.set('before',String(rootSeq(visible[0])));$('older').disabled=true;try{const page=await fetchPage(p);olderRows=mergeRows(page.rows,olderRows);hasEarlier=page.hasEarlier;if(latestData){latestData.rows=mergeRows(olderRows,latestData.rows);draw(latestData);ledger.scrollTop=oldTop+(ledger.scrollHeight-oldHeight)}}catch(e){$('health').textContent='● '+e.message;$('health').style.color='var(--red)'}$('older').disabled=!hasEarlier}
function resetWindow(){olderRows=[];hasEarlier=false;load()}
$('older').onclick=loadEarlier;$('follow').onclick=()=>{follow=!follow;$('follow').classList.toggle('on',follow);$('follow').textContent=follow?'tail follow':'tail paused'};$('refresh').onclick=load;for(const id of ['layer','actor','class','producer','visibility'])$(id).onchange=resetWindow;let debounce;$('search').oninput=()=>{clearTimeout(debounce);debounce=setTimeout(resetWindow,180)};ledger.onscroll=()=>{queueRender();if(ledger.scrollHeight-ledger.scrollTop-ledger.clientHeight>80){follow=false;$('follow').classList.remove('on');$('follow').textContent='tail paused'}};window.onhashchange=()=>{const entry=hashEntry();if(entry){deepLinkPending=true;focusEvent(entry)}};load();
</script></body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;

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
            child_cwd: child_cwd.into(),
            worktree_path: None,
            effective_model_id: "model".into(),
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
        let first_len = std::fs::metadata(&path).unwrap().len();

        let mut cache = SessionTrajectoryCache::default();
        cache.refresh(&path).unwrap();
        assert_eq!(cache.timeline.surface()[0].text_content(), "alpha");

        write_timeline(&path, &second);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), first_len);
        cache.refresh(&path).unwrap();
        assert_eq!(cache.timeline.events().len(), 1);
        assert_eq!(cache.timeline.surface()[0].text_content(), "bravo");
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
        assert!(error.to_string().contains("not a regular file"));
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
        let mut cache = WorkflowJournalCache::default();
        cache.refresh(&path).unwrap();
        assert_eq!(cache.entries[0].result, serde_json::json!("aa"));

        write(&entry("bb"));
        cache.refresh(&path).unwrap();
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.entries[0].result, serde_json::json!("bb"));
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
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.offset, committed_offset);
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
                    private: false,
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
        assert_eq!(response.rows[1].entry_id, "t:wf_debug/0");
        assert_eq!(
            response.rows[1].parent_entry_id.as_deref(),
            Some("t:session/0")
        );
        assert_eq!(response.rows[1].nesting_path, [0, 0, 0]);
        assert_eq!(response.rows[1].actor, "workflow:wf_debug");
        assert_eq!(response.rows[2].entry_id, "t:wf_debug/1");

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
            session_dir: dir.path().to_owned(),
            sessions_root: dir.path().join("sessions"),
            cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
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
            session_dir: dir.path().to_owned(),
            sessions_root: dir.path().join("sessions"),
            cache: Arc::new(Mutex::new(SessionTrajectoryCache::default())),
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
        assert!(PAGE.contains("OVERSCAN=20"));
        assert!(PAGE.contains("parent_entry_id"));
        assert!(PAGE.contains("nesting_path"));
        assert!(PAGE.contains("history.replaceState"));
        assert!(PAGE.contains("p.set('entry',selected)"));
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
        assert_eq!(response.first_seq, Some(1));
        assert_eq!(response.last_seq, Some(1));
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
        let child_spawn = root
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Spawned(subagent_spawn(
                    "worker",
                    "child-session",
                    "/child",
                )),
            ))
            .unwrap();
        root.append(
            sampling_types::ConversationItem::assistant("root continued"),
            chat_state::MessageCause::Assistant,
        )
        .unwrap();
        write_timeline(&root_dir.join(super::super::storage::TIMELINE_FILE), &root);

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
        let grandchild_spawn = child
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Spawned(subagent_spawn(
                    "nested-worker",
                    "grandchild-session",
                    "/grandchild",
                )),
            ))
            .unwrap();
        child
            .append(
                sampling_types::ConversationItem::assistant("child continued"),
                chat_state::MessageCause::Assistant,
            )
            .unwrap();
        write_child_session(
            &sessions_root,
            "/child",
            "child-session",
            "root-session",
            &child,
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
        write_child_session(
            &sessions_root,
            "/grandchild",
            "grandchild-session",
            "child-session",
            &grandchild,
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
                (
                    "t:child-session/3",
                    Some("t:root-session/1"),
                    &[1, 3][..],
                    "subagent:child-session",
                ),
                ("t:root-session/2", None, &[2][..], "main"),
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
        assert_eq!(filtered.first_seq, Some(1));
        assert_eq!(filtered.last_seq, Some(1));

        let focused = query_cached(
            &state,
            TrajectoryQuery {
                entry: Some("t:grandchild-session/1".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(focused.rows.len(), 7);
        assert_eq!(focused.first_seq, Some(1));
        assert_eq!(focused.last_seq, Some(1));
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
        let error = serve("missing", "0.0.0.0:0".parse().unwrap(), |_| {})
            .await
            .unwrap_err();
        assert!(error.to_string().contains("loopback"));
    }
}
