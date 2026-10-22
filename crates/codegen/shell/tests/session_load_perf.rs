//! End-to-end measurement of why resuming a large session is slow: the time
//! spent before historical text arrives and before the load response.
//!
//! The pager resumes via `session/load`; streamed history can render before
//! the response enables normal input. The shell
//! answers by (1) `load_light` (Timeline Surface; rewind points now load lazily) and
//! (2) `replay_session_updates`, which derives ordered notifications from the
//! same verified journal cut and forwards each as a `session/update`. All of that
//! happens while the client waits; both tests drive the real production code.
//!
//! * [`phase_breakdown_real_functions`] drives the exact load-path functions
//!   (`load_session_without_updates`, `load_updates_for_replay_at`) and attributes
//!   wall-clock to rewind load, chat+summary load, and updates read+parse+filter,
//!   then prints a per-`sessionUpdate`-kind byte breakdown of `updates.jsonl`.
//! * [`full_session_load_e2e`] stands up a real `MvpAgent` over in-process ACP
//!   pipes (via [`load_session_via_agent`]); times `session/load` end-to-end,
//!   counts replayed notifications and historical bytes, and dumps load-window
//!   `instrumentation_timer!` events.
//!
//! Session data (both tests): a synthetic session from the shared
//! [`synth`](shell::session::testkit::synth) generator (redundant
//! `available_commands_update` + big rewind snapshots; size knobs via
//! `GROW_PERF_*`), or a real session dir via `GROW_PERF_SESSION_SRC=<session-dir>`.
//! `GROW_PERF_SESSION_FORMAT=modern` upgrades the synthetic fixture with real
//! response admission and payload-free candidate anchors. The legacy comparison
//! binary uses `GROW_PERF_MISSING_PROJECTION_EVERY=16` for projection gaps; the new
//! journal has no projection cache to omit or repair.
//!
//! Run (needs the `test-support` feature; on by default under Bazel):
//!   cargo test -p shell --features test-support --test session_load_perf -- --nocapture
//!   cargo test -p shell --features test-support --test session_load_perf full_session_load_e2e -- --ignored --nocapture

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use acp_transport::protocol as acp;
use tempfile::TempDir;

use shell::session::info::Info;
use shell::session::storage::{JsonlStorageAdapter, StorageAdapter, load_updates_for_replay_at};
use shell::session::testkit::e2e::load_session_via_agent;
use shell::session::testkit::synth::{self, SessionSpec};

/// Capture the pre-storage-change wire DTOs separately from the end-to-end
/// history oracle. These are presentation payloads, not executable facts.
#[test]
#[ignore = "explicit capture/verification of the storage-change wire fixture"]
fn history_wire_golden() {
    let fixture_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let input: Vec<serde_json::Value> = serde_json::from_slice(
        &std::fs::read(fixture_directory.join("history-wire-input.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        input.len(),
        46,
        "review the family coverage when changing the fixture"
    );
    let mut output = Vec::with_capacity(input.len());
    for case in input {
        let notification = case["notification"].clone();
        let normalized = match case["channel"].as_str().unwrap() {
            "grow" => serde_json::to_value(
                serde_json::from_value::<shell::extensions::notification::SessionNotification>(
                    notification.clone(),
                )
                .unwrap_or_else(|error| panic!("{}: {error}", case["family"])),
            )
            .unwrap(),
            "acp" => serde_json::to_value(
                serde_json::from_value::<acp::SessionNotification>(notification.clone())
                    .unwrap_or_else(|error| panic!("{}: {error}", case["family"])),
            )
            .unwrap(),
            channel => panic!("unknown fixture channel: {channel}"),
        };
        assert_eq!(
            normalized, notification,
            "{}: fixture must use the actual canonical wire shape without dropped fields",
            case["family"],
        );
        output.push(serde_json::json!({
            "family": case["family"],
            "channel": case["channel"],
            "notification": normalized,
        }));
    }
    let golden = fixture_directory.join("history-wire-golden.json");
    if std::env::var_os("GROW_HISTORY_WIRE_CAPTURE").is_some() {
        std::fs::write(
            &golden,
            serde_json::to_string_pretty(&output).unwrap() + "\n",
        )
        .unwrap();
    } else {
        let expected: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(&golden).unwrap()).unwrap();
        assert_eq!(
            output, expected,
            "historical presentation wire payload changed"
        );
    }
}

// ───────────────────────── session spec ─────────────────────────

/// Small defaults over the shared [`SessionSpec`]; scale/override via
/// `GROW_PERF_*` (e.g. `GROW_PERF_TURNS`,
/// `GROW_PERF_SCALE`), or point `GROW_PERF_SESSION_SRC` at a real session dir.
fn perf_spec() -> SessionSpec {
    SessionSpec::from_env_prefixed(
        "GROW_PERF",
        SessionSpec {
            turns: 12,
            acu_per_turn: 2,
            catalog_commands: 8,
            catalog_desc_len: 128,
            agent_chunks_per_turn: 2,
            agent_chunk_len: 512,
            rewind_points: 2,
            files_per_rewind: 2,
            file_content_len: 512,
        },
    )
}

fn modern_history() -> bool {
    match std::env::var("GROW_PERF_SESSION_FORMAT")
        .as_deref()
        .unwrap_or("legacy")
    {
        "legacy" => false,
        "modern" => true,
        value => panic!("unsupported GROW_PERF_SESSION_FORMAT: {value}"),
    }
}

// ───────────────────────── session setup ─────────────────────────

/// Prepare a session on disk under `root` for working dir `cwd`. With
/// `GROW_PERF_SESSION_SRC` set, copy a real session over a registered stub
/// (keeping our `summary.json`); otherwise synthesize one via
/// [`synth::prepare_session`].
async fn prepare_session(root: &Path, cwd: &Path, spec: &SessionSpec) -> (Info, PathBuf) {
    let Ok(src) = std::env::var("GROW_PERF_SESSION_SRC") else {
        // Conservative ASCII JSON bounds for this fixture, checked before the
        // shared generator allocates its whole output. Keep performance runs
        // from accidentally producing multi-GB rewind/catalog snapshots.
        const BUDGET: u128 = 256 * 1024 * 1024;
        let fields = [
            spec.turns,
            spec.acu_per_turn,
            spec.catalog_commands,
            spec.catalog_desc_len,
            spec.agent_chunks_per_turn,
            spec.agent_chunk_len,
            spec.rewind_points,
            spec.files_per_rewind,
            spec.file_content_len,
        ];
        assert!(
            fields.iter().all(|&value| value as u128 <= BUDGET),
            "fixture parameter exceeds 256 MiB budget"
        );
        let updates = spec.turns as u128
            * (4096
                + spec.acu_per_turn as u128
                    * (1024
                        + spec.catalog_commands as u128 * (spec.catalog_desc_len as u128 + 1024))
                + spec.agent_chunks_per_turn as u128 * (spec.agent_chunk_len as u128 + 1024));
        let rewind = spec.rewind_points as u128
            * (1024 + spec.files_per_rewind as u128 * (spec.file_content_len as u128 * 2 + 2048));
        let response_bytes = if modern_history() {
            spec.agent_chunks_per_turn as u128 * spec.agent_chunk_len as u128
        } else {
            spec.agent_chunk_len as u128
        };
        let timeline = spec.turns as u128 * (response_bytes + 12288) + 8192;
        assert!(
            updates + rewind + timeline <= BUDGET,
            "synthetic fixture exceeds 256 MiB budget"
        );
        let result = if modern_history() {
            synth::replay::prepare_legacy_session(root, cwd, spec).await
        } else {
            synth::prepare_session(root, cwd, spec).await
        };
        JsonlStorageAdapter::with_root(root.to_path_buf())
            .update_current_model_and_agent(
                &result.0,
                &shell::agent::models::ModelId::new("test/test-model"),
                None,
                None,
            )
            .await
            .expect("set explicit fixture model");
        if modern_history() {
            let missing_every = std::env::var("GROW_PERF_MISSING_PROJECTION_EVERY")
                .map(|value| {
                    value
                        .parse::<usize>()
                        .expect("valid missing projection interval")
                })
                .unwrap_or(0);
            let route = std::env::var("GROW_INFERENCE_BASE_URL").ok().map(|url| {
                sampling_types::model_image_input_key_from_parts(
                    "test-model",
                    &sampling_types::ApiBackend::ChatCompletions,
                    &url,
                    &Default::default(),
                )
            });
            synth::write_admitted_history(
                &result.0,
                &result.1,
                spec,
                "test/test-model",
                missing_every,
                route
                    .as_ref()
                    .filter(|_| std::env::var_os("GROW_PERF_OMIT_ROUTE_BASELINE").is_none())
                    .map(|route| ("test-model", route)),
            );
        } else {
            append_perf_timeline(root, &result.0, spec).await;
        }
        return result;
    };

    let adapter = JsonlStorageAdapter::with_root(root.to_path_buf());
    let id = uuid::Uuid::new_v4().to_string();
    let info = Info {
        id: synth::sid(&id),
        cwd: cwd.to_string_lossy().to_string(),
    };
    adapter
        .init_session(&info, shell::agent::models::ModelId::new("test-model"))
        .await
        .expect("init_session");
    let dir = synth::locate_session_dir(root, &id);
    for name in ["timeline.jsonl", "rewind_points.jsonl"] {
        let from = Path::new(&src).join(name);
        if from.exists() {
            std::fs::copy(&from, dir.join(name)).unwrap();
        }
    }
    eprintln!("[perf] using REAL session copied from {src}");
    (info, dir)
}

/// Add a small, valid canonical history to the synthetic fixture only. The
/// shared synthesizer intentionally remains an updates/rewind generator.
async fn append_perf_timeline(root: &Path, info: &Info, spec: &SessionSpec) {
    use sampling_types::ConversationItem;
    let adapter = JsonlStorageAdapter::with_root(root.to_path_buf());
    let mut timeline = chat_state::Timeline::from_seed(vec![ConversationItem::system(
        "stable synthetic system context",
    )])
    .expect("valid system head");
    for event in timeline.events() {
        adapter
            .append_timeline_event_durable(info, event)
            .await
            .unwrap();
    }
    for turn_no in 0..spec.turns {
        let turn = chat_state::TurnId(turn_no as u64);
        let prompt = format!("synthetic historical prompt {turn_no}");
        let started = timeline
            .record(chat_state::TimelineEventKind::Turn(
                chat_state::TurnEvent::Started {
                    prompt_id: "fixture-prompt".into(),
                    id: turn,
                    input_ids: Vec::new(),
                    identity: chat_state::TurnIdentity {
                        goal_definition_revision: None,
                        origin: "user".into(),
                        turn_kind: "internal".into(),
                        goal_id: None,
                        stage_id: None,
                    },
                    model_id: "test/test-model".into(),
                    input_message_count: timeline.surface().len(),
                    prompt_index: turn_no,
                    prompt_text: prompt.clone(),
                    input_kind: chat_state::TurnInputKind::Prompt,
                    redirect_kind: None,
                },
            ))
            .unwrap();
        adapter
            .append_timeline_event_durable(info, &started)
            .await
            .unwrap();
        let mut user = ConversationItem::user(prompt);
        user.set_prompt_index(turn_no);
        let message = timeline
            .append(user, chat_state::MessageCause::User)
            .unwrap();
        adapter
            .append_timeline_event_durable(info, &message)
            .await
            .unwrap();
        let answer = timeline
            .append(
                ConversationItem::assistant(format!(
                    "synthetic historical answer {turn_no}: {}",
                    "x".repeat(spec.agent_chunk_len)
                )),
                chat_state::MessageCause::Assistant,
            )
            .unwrap();
        adapter
            .append_timeline_event_durable(info, &answer)
            .await
            .unwrap();
        let ended = timeline
            .record(chat_state::TimelineEventKind::Turn(
                chat_state::TurnEvent::Ended {
                    id: turn,
                    outcome: "completed".into(),
                    duration_ms: 1,
                    tool_count: 0,
                    terminal: chat_state::TurnTerminal {
                        source: chat_state::TurnTerminalSource::Host,
                        stop_reason: "end_turn".into(),
                        completion_kind: "completed".into(),
                    },
                    cancellation_category: None,
                    details: None,
                },
            ))
            .unwrap();
        adapter
            .append_timeline_event_durable(info, &ended)
            .await
            .unwrap();
    }
}

/// Re-create the rewind file after the isolation step deletes it (synthetic
/// case). For a real session copy we cannot regenerate; leave it absent.
fn generate_or_restore_rewind(path: &Path, spec: &SessionSpec) {
    if std::env::var("GROW_PERF_SESSION_SRC").is_ok() {
        return;
    }
    synth::write_rewind_jsonl(path, spec);
}

// ───────────────────────── physical journal stats ─────────────────────────

/// Per-kind statistics for the generated/loaded physical Timeline journal.
#[derive(Default)]
struct KindStats {
    count: BTreeMap<String, u64>,
    bytes: BTreeMap<String, u64>,
}

fn file_size_mb(path: &Path) -> f64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) as f64 / 1e6
}

/// `(len, content_hash)` fingerprint of a file, for asserting it is byte-for-byte
/// unchanged across an operation (zero-data-loss guard). Missing file → `(0, 0)`.
fn file_fingerprint(path: &Path) -> (u64, u64) {
    use std::hash::{Hash, Hasher};
    let Ok(bytes) = std::fs::read(path) else {
        return (0, 0);
    };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    (bytes.len() as u64, hasher.finish())
}

/// Counts physical bytes once. This is an outside-timing diagnostic; replay
/// correctness uses the production projector and full ACP content equality.
fn timeline_kind_breakdown(path: &Path) -> KindStats {
    let mut stats = KindStats::default();
    let contents = std::fs::read_to_string(path).expect("read physical Timeline fixture");
    shell::session::testkit::decode_timeline_fixture(&contents)
        .expect("the statistics describe a checksummed, valid Timeline journal");
    for line in contents.lines() {
        let record: serde_json::Value = serde_json::from_str(line).unwrap();
        let fact = &record["fact"];
        let kind = match fact["kind"].as_str().unwrap() {
            "domain" => format!("domain/{}", fact["event"]["type"].as_str().unwrap()),
            "presentation" => {
                let content = &fact["event"]["content"];
                match content["kind"].as_str().unwrap() {
                    "grow" => format!(
                        "presentation/{}",
                        content["data"]["sessionUpdate"].as_str().unwrap()
                    ),
                    kind => format!("presentation/{kind}"),
                }
            }
            _ => unreachable!("the production codec rejected unknown physical facts"),
        };
        *stats.count.entry(kind.clone()).or_default() += 1;
        *stats.bytes.entry(kind).or_default() += line.len() as u64 + 1;
    }
    stats
}

fn print_kind_breakdown(label: &str, stats: &KindStats) {
    let total: u64 = stats.bytes.values().sum();
    eprintln!(
        "\n[perf] {label}: Timeline journal composition ({:.1} MB total):",
        total as f64 / 1e6
    );
    eprintln!(
        "  {:<32} {:>8} {:>10} {:>7}",
        "physical fact kind", "count", "MB", "%"
    );
    let mut rows: Vec<(&String, &u64)> = stats.bytes.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1));
    for (kind, bytes) in rows {
        let count = stats.count.get(kind).copied().unwrap_or(0);
        let pct = if total > 0 {
            *bytes as f64 / total as f64 * 100.0
        } else {
            0.0
        };
        eprintln!(
            "  {:<32} {:>8} {:>10.1} {:>6.1}%",
            kind,
            count,
            *bytes as f64 / 1e6,
            pct
        );
    }
}

// ───────────────────────── TEST 1: phase breakdown ─────────────────────────

/// Attribute the pre-render load cost to its real phases using the exact
/// production functions, isolating rewind-point load from everything else.
///
/// `#[ignore]`: this is a measurement tool (generates tens of MB, ~3 s), and its
/// only correctness assertion is covered by the unit tests. Run explicitly with
/// `--ignored` (optionally `GROW_PERF_SESSION_SRC=...`) to get the numbers.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "perf measurement tool; run with --ignored"]
async fn phase_breakdown_real_functions() {
    let root = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    let spec = perf_spec();

    let (info, dir) = prepare_session(root.path(), cwd.path(), &spec).await;

    let timeline_path = dir.join("timeline.jsonl");
    let rewind_path = dir.join("rewind_points.jsonl");
    eprintln!(
        "\n[perf] session dir: {}\n[perf]   timeline.jsonl      = {:.1} MB\n[perf]   rewind_points.jsonl = {:.1} MB",
        dir.display(),
        file_size_mb(&timeline_path),
        file_size_mb(&rewind_path),
    );

    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());

    // Phase A: load_light core (summary + Timeline fold), what mvp_agent's
    // `load_light` blocks on before replay.
    let t = Instant::now();
    let light = adapter
        .load_session_without_updates(&info)
        .await
        .expect("load_session_without_updates");
    let full_load_light = t.elapsed();
    drop(light);

    // Lazy rewind path: the deferred cost moved here. The picker only needs
    // a cheap metadata scan; an actual rewind triggers the full content load.
    // Both read the same file that `load_light` no longer touches.
    use workspace::session::file_state::FileStateTracker;
    let t = Instant::now();
    let lazy_metas = FileStateTracker::with_lazy_file(
        std::fs::File::open(&rewind_path).expect("open rewind fixture"),
        rewind_path.clone(),
    )
    .get_rewind_point_metas()
    .await
    .expect("scan rewind fixture metadata");
    let lazy_metas_scan = t.elapsed();
    let t = Instant::now();
    let lazy_points = FileStateTracker::with_lazy_file(
        std::fs::File::open(&rewind_path).expect("open rewind fixture"),
        rewind_path.clone(),
    )
    .get_rewind_points()
    .await
    .expect("load complete rewind fixture");
    let lazy_full_load = t.elapsed();
    let num_rewind = lazy_points.len();
    assert_eq!(
        lazy_metas.len(),
        num_rewind,
        "picker metadata scan must see every rewind point"
    );

    // Phase A': isolate rewind cost by deleting the rewind file and re-measuring.
    // The delta is the rewind-point deserialization (full file-content snapshots).
    std::fs::remove_file(&rewind_path).ok();
    let t = Instant::now();
    let _light2 = adapter
        .load_session_without_updates(&info)
        .await
        .expect("load_session_without_updates (no rewind)");
    let load_light_no_rewind = t.elapsed();
    // restore for downstream/manual reruns
    generate_or_restore_rewind(&rewind_path, &spec);

    let rewind_cost = full_load_light.saturating_sub(load_light_no_rewind);

    // Phase B: updates replay parse. The typed `load_updates_for_replay_at`
    // reads the whole file, typed-parses every line, and applies rewind
    // filtering; production now streams via `stream_replay_updates_at`, so this
    // measures the materialize-all parse cost.
    let t = Instant::now();
    let replayed = load_updates_for_replay_at(info.id.0.as_ref(), root.path())
        .expect("load_updates_for_replay_at")
        .unwrap_or_default();
    let updates_parse = t.elapsed();

    let stats = timeline_kind_breakdown(&timeline_path);
    print_kind_breakdown("phase_breakdown", &stats);

    eprintln!("\n[perf] ===== PRE-RENDER LOAD PHASE BREAKDOWN (real production fns) =====");
    eprintln!("  rewind_points (on disk)      : {num_rewind}");
    eprintln!("  rewind_points loaded in load : 0 (deferred → lazy)");
    eprintln!("  updates replayed (acp)       : {}", replayed.len());
    eprintln!("  ----------------------------------------------------------------");
    eprintln!(
        "  load_light (summary+chat)        : {:>8.1} ms",
        full_load_light.as_secs_f64() * 1e3
    );
    eprintln!(
        "    └─ rewind in load_light (now)  : {:>8.1} ms",
        rewind_cost.as_secs_f64() * 1e3
    );
    eprintln!(
        "    └─ summary + chat only         : {:>8.1} ms",
        load_light_no_rewind.as_secs_f64() * 1e3
    );
    eprintln!(
        "  lazy rewind: picker metas scan   : {:>8.1} ms (on /rewind open)",
        lazy_metas_scan.as_secs_f64() * 1e3
    );
    eprintln!(
        "  lazy rewind: full content load   : {:>8.1} ms (on rewind execute)",
        lazy_full_load.as_secs_f64() * 1e3
    );
    eprintln!(
        "  updates read+parse+filter        : {:>8.1} ms",
        updates_parse.as_secs_f64() * 1e3
    );
    eprintln!("  ----------------------------------------------------------------");
    eprintln!(
        "  TOTAL pre-render parse work      : {:>8.1} ms",
        (full_load_light + updates_parse).as_secs_f64() * 1e3
    );
    eprintln!("================================================================\n");

    assert!(
        !stats.bytes.is_empty(),
        "expected a non-empty Timeline journal"
    );
}

// ───────────────────────── TEST 2: true e2e ─────────────────────────

/// Counts replayed notifications and records first/last receipt timestamps so
/// we can see how long the client streams history before `load` returns.
#[derive(Default)]
struct LoadCounters {
    count: u64,
    /// `available_commands_update` notifications forwarded during the load.
    /// History replay skips the (thousands of) historical ones, so this stays tiny.
    acu_count: u64,
    first_at: Option<Instant>,
    last_at: Option<Instant>,
    first_history_at: Option<Instant>,
    last_history_at: Option<Instant>,
    history_count: u64,
    history_bytes: usize,
    max_history_gap: Duration,
    /// Optional correctness run, disabled for timing comparisons.
    history: Option<Vec<acp::SessionUpdate>>,
}

struct CountingClient {
    counters: Rc<RefCell<LoadCounters>>,
}

#[async_trait::async_trait(?Send)]
impl acp_transport::AcpClientHandler for CountingClient {
    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        let outcome = args
            .options
            .first()
            .map(|o| {
                acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                    o.option_id.clone(),
                ))
            })
            .unwrap_or(acp::RequestPermissionOutcome::Cancelled);
        Ok(acp::RequestPermissionResponse::new(outcome))
    }

    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        let mut c = self.counters.borrow_mut();
        let now = Instant::now();
        c.count += 1;
        if matches!(args.update, acp::SessionUpdate::AvailableCommandsUpdate(_)) {
            c.acu_count += 1;
        }
        c.first_at.get_or_insert(now);
        c.last_at = Some(now);
        if let acp::SessionUpdate::UserMessageChunk(chunk)
        | acp::SessionUpdate::AgentMessageChunk(chunk) = &args.update
            && let acp::ContentBlock::Text(text) = &chunk.content
            && !text.text.is_empty()
        {
            if c.first_history_at.is_none() {
                tracing::info!(target: shell::instrumentation::TARGET,
                    event = "resume_first_history_received");
            }
            c.first_history_at.get_or_insert(now);
            if let Some(last) = c.last_history_at {
                c.max_history_gap = c.max_history_gap.max(now.duration_since(last));
            }
            c.last_history_at = Some(now);
            c.history_count += 1;
            c.history_bytes += text.text.len();
        }
        if let Some(history) = &mut c.history
            && matches!(
                args.update,
                acp::SessionUpdate::UserMessageChunk(_) | acp::SessionUpdate::AgentMessageChunk(_)
            )
        {
            history.push(args.update);
        }
        Ok(())
    }
}

/// Count the complete physical session footprint, excluding symlink targets.
/// Runs outside the measured RPC, like fixture generation and the history oracle.
fn session_directory_stats(directory: &Path) -> (u64, u64) {
    let mut pending = vec![directory.to_path_buf()];
    let (mut bytes, mut files) = (0, 0);
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).expect("read fixture directory") {
            let entry = entry.expect("read fixture entry");
            let metadata = entry.path().symlink_metadata().expect("stat fixture entry");
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                bytes += metadata.len();
                files += 1;
            }
        }
    }
    (bytes, files)
}

/// Parse the production instrumentation JSON log into `(name -> elapsed_ms)`.
fn parse_instrumentation_log(path: &Path) -> Vec<(String, usize, f64)> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out = BTreeMap::<String, (usize, f64)>::new();
    let mut in_load = false;
    for line in contents.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let fields = v.get("fields").unwrap_or(&v);
        match fields.get("event").and_then(|e| e.as_str()) {
            Some("resume_load_start") => {
                in_load = true;
                continue;
            }
            Some("resume_load_end") => break,
            _ if !in_load => continue,
            _ => {}
        }
        if fields.get("event").and_then(|e| e.as_str()) != Some("timing") {
            continue;
        }
        let Some(name) = fields.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        let us = fields
            .get("elapsed_us")
            .and_then(|u| u.as_u64())
            .or_else(|| {
                fields
                    .get("elapsed_ms")
                    .and_then(|m| m.as_u64())
                    .map(|m| m * 1000)
            })
            .unwrap_or(0);
        let (count, ms) = out.entry(name.to_string()).or_default();
        *count += 1;
        *ms += us as f64 / 1000.0;
    }
    out.into_iter()
        .map(|(name, (count, ms))| (name, count, ms))
        .collect()
}

#[test]
fn instrumentation_summary_excludes_fixture_and_post_load_work() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let rows = [
        serde_json::json!({"event":"timing", "name":"phase", "elapsed_us":100000}),
        serde_json::json!({"event":"resume_load_start"}),
        serde_json::json!({"fields":{"event":"timing", "name":"phase", "elapsed_us":2000}}),
        serde_json::json!({"fields":{"event":"timing", "name":"phase", "elapsed_us":3000}}),
        serde_json::json!({"event":"timing", "name":"parent", "elapsed_us":10000}),
        serde_json::json!({"event":"resume_load_end"}),
        serde_json::json!({"event":"timing", "name":"phase", "elapsed_us":200000}),
    ];
    std::fs::write(
        file.path(),
        rows.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    assert_eq!(
        parse_instrumentation_log(file.path()),
        vec![("parent".into(), 1, 10.0), ("phase".into(), 2, 5.0)]
    );
}

/// True end-to-end: real `MvpAgent` over real ACP pipes. Times `session/load`
/// (what the pager blocks on), counts replayed notifications, and prints the
/// shell's own per-phase instrumentation.
///
/// `#[ignore]` by default because it stands up the full agent; run with
/// `--ignored --nocapture`.
#[tokio::test(flavor = "current_thread")]
#[ignore = "heavy: builds a full MvpAgent and replays a large session; run with --ignored"]
async fn full_session_load_e2e() {
    diagnostics::tls::install_ring_provider_once();

    let server = test_support::MockInferenceServer::start().await.unwrap();

    let grow_home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    let spec = perf_spec();
    let instr_log = grow_home.path().join("instr.jsonl");

    // SAFETY: single-threaded current-thread runtime; set before any agent code
    // reads these process-globals (grow_home()/instrumentation mode are OnceLock).
    unsafe {
        std::env::set_var("GROW_HOME", grow_home.path());
        std::env::set_var("GROW_INSTRUMENTATION", "log");
        std::env::set_var("GROW_INSTRUMENTATION_LOG", &instr_log);
        std::env::set_var("GROW_CLI_CHAT_PROXY_BASE_URL", server.url());
        std::env::set_var("GROW_INFERENCE_BASE_URL", server.url());
        std::env::set_var("GROW_API_KEY", "test-key-for-ci");
    }

    // Install the production instrumentation layer so `instrumentation_timer!`
    // events are written to our temp log file.
    use tracing_subscriber::Registry;
    use tracing_subscriber::prelude::*;
    let _ = tracing_subscriber::registry()
        .with(shell::instrumentation::layer::<Registry>())
        .try_init();

    let (info, dir) = prepare_session(grow_home.path(), cwd.path(), &spec).await;
    let updates_path = dir.join("updates.jsonl");
    let timeline_path = dir.join("timeline.jsonl");
    let rewind_path = dir.join("rewind_points.jsonl");
    let timeline_bytes = std::fs::metadata(&timeline_path).unwrap().len();
    let updates_bytes = std::fs::metadata(&updates_path).map_or(0, |metadata| metadata.len());
    let (directory_bytes, directory_files) = session_directory_stats(&dir);
    eprintln!(
        "\n[perf] e2e session: timeline={:.1} MB rewind={:.1} MB",
        file_size_mb(&timeline_path),
        file_size_mb(&rewind_path)
    );
    let stats = timeline_kind_breakdown(&timeline_path);
    print_kind_breakdown("e2e", &stats);

    // Zero-data-loss guard: a pure load must never rewrite rewind_points.jsonl
    // (it is read lazily, never on the load path). Captured here, asserted after.
    let rewind_path_guard = rewind_path.clone();
    let rewind_fp_before = file_fingerprint(&rewind_path_guard);

    let expected_history = std::env::var_os("GROW_PERF_ASSERT_HISTORY").map(|_| {
        load_updates_for_replay_at(info.id.0.as_ref(), grow_home.path())
            .unwrap()
            .unwrap()
            .into_iter()
            .filter(|update| {
                matches!(
                    update,
                    acp::SessionUpdate::UserMessageChunk(_)
                        | acp::SessionUpdate::AgentMessageChunk(_)
                )
            })
            .collect::<Vec<_>>()
    });

    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let counters = Rc::new(RefCell::new(LoadCounters {
                history: expected_history.as_ref().map(|_| Vec::new()),
                ..Default::default()
            }));
            let client = CountingClient {
                counters: counters.clone(),
            };
            let loaded = load_session_via_agent(
                client,
                "perf-test",
                info.id.clone(),
                cwd.path().to_path_buf(),
                shell::session::testkit::e2e::mock_agent_config(&server.url()),
            )
            .await;
            let load_started = loaded.load_started;
            let load_elapsed = loaded.load_elapsed;
            if let Some(expected) = &expected_history {
                assert_eq!(counters.borrow().history.as_ref().unwrap(), expected,
                    "all historical text must arrive once and in order before the load response");
                if modern_history() {
                    let history = counters.borrow();
                    let markers = history.history.as_ref().unwrap().iter().filter_map(|update| match update {
                        acp::SessionUpdate::AgentMessageChunk(chunk) => match &chunk.content {
                            acp::ContentBlock::Text(text) => Some(text.text.lines().filter(|line| line.starts_with("GROW_SEQ_")).map(str::to_owned).collect::<Vec<_>>()),
                            _ => None,
                        },
                        _ => None,
                    }).flatten().collect::<Vec<_>>();
                    assert_eq!(markers, (1..=spec.turns).map(|turn| format!("GROW_SEQ_{turn:06}")).collect::<Vec<_>>(), "independent fixture sequence oracle rejects missing, duplicated or reordered canonical answers");
                }
            }
            // Keep the connection alive for asynchronous live advertisements.
            let _client_conn = loaded.client_conn;

            // Snapshot at the response barrier. Counts include administrative
            // notifications; a live catalog may already have arrived here.
            let (replay_count, acu_replayed, ttfn, ttln, first_history_ms, last_history_ms, history_count, history_bytes, max_history_gap_ms) = {
                let c = counters.borrow();
                (
                    c.count,
                    c.acu_count,
                    c.first_at
                        .map(|t| t.duration_since(load_started).as_secs_f64() * 1e3)
                        .unwrap_or(0.0),
                    c.last_at
                        .map(|t| t.duration_since(load_started).as_secs_f64() * 1e3)
                        .unwrap_or(0.0),
                    c.first_history_at.map(|t| t.duration_since(load_started).as_secs_f64() * 1e3).unwrap_or(0.0),
                    c.last_history_at.map(|t| t.duration_since(load_started).as_secs_f64() * 1e3).unwrap_or(0.0),
                    c.history_count,
                    c.history_bytes,
                    c.max_history_gap.as_secs_f64() * 1e3,
                )
            };

            // Skipping persisted catalogs is safe only if a live catalog reaches
            // the client. It may arrive before or after the response barrier.
            let readvertised = tokio::time::timeout(Duration::from_secs(10), async {
                while counters.borrow().acu_count == 0 {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .is_ok();

            // Flush the instrumentation writer and read the per-phase log.
            let _ = shell::instrumentation::finalize();
            std::thread::sleep(Duration::from_millis(150));
            if let Some(trace_path) = std::env::var_os("GROW_PERF_TRACE_PATH") {
                std::fs::copy(&instr_log, trace_path).expect("save complete instrumentation trace");
            }
            let mut phases = parse_instrumentation_log(&instr_log);
            phases.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
            assert!(!phases.is_empty(), "load-window instrumentation must be captured");
            if modern_history() && std::env::var_os("GROW_PERF_OMIT_ROUTE_BASELINE").is_none() {
                assert!(!phases.iter().any(|(name, _, _)| name == "session.load.model_route_append"), "fixture route must match the selected runtime route");
            }
            if modern_history() {
                let count = |name: &str| phases.iter().filter(|phase| phase.0 == name).map(|phase| phase.1).sum::<usize>();
                assert_eq!(count("session.projection.typed_canonical"), 0, "there is no persisted canonical projection to construct");
                assert_eq!(count("session.projection.raw_canonical"), 0, "there is no second projection body to reconcile");
                assert_eq!(count("session.load.history_projection"), 1, "one complete history projection per load; actor receipts reuse the validated cut");
            }

            // Replay-skip guard: the historical available_commands_update copies
            // (3197 in the pathological real session, hundreds synthetic) must
            // NOT be replayed.
            let acu_persisted = stats.count.get("available_commands_update").copied().unwrap_or(0);

            eprintln!("\n[perf] ===== END-TO-END session/load (what the pager waits on) =====");
            eprintln!("  total session/load round-trip : {:>9.1} ms", load_elapsed.as_secs_f64() * 1e3);
            eprintln!("  notifications replayed         : {:>9}", replay_count);
            eprintln!("  available_commands_update      : {acu_replayed:>9} replayed / {acu_persisted} on disk");
            eprintln!("  live catalog reached          : {readvertised:>9}");
            eprintln!("  time-to-first notification     : {ttfn:>9.1} ms");
            eprintln!("  time-to-last notification      : {ttln:>9.1} ms");
            eprintln!("  first historical text          : {first_history_ms:>9.1} ms");
            eprintln!("  last historical text           : {last_history_ms:>9.1} ms");
            eprintln!("  historical text chunks / bytes : {history_count} / {history_bytes}");
            eprintln!("  ---- shell-side nested/inclusive timers (not additive) ----");
            if phases.is_empty() {
                eprintln!("  (no instrumentation events captured)");
            } else {
                for (name, count, ms) in &phases {
                    eprintln!("  {name:<48} {ms:>9.1} ms count={count}");
                }
            }
            eprintln!("================================================================\n");
            eprintln!("PERF_RESUME {}", serde_json::json!({
                "format": if modern_history() { "modern" } else { "presentation" },
                "turns": spec.turns,
                "missing_every": std::env::var("GROW_PERF_MISSING_PROJECTION_EVERY").unwrap_or_else(|_| "0".into()),
                "route_seeded": modern_history() && std::env::var_os("GROW_PERF_OMIT_ROUTE_BASELINE").is_none(),
                "timeline_bytes": timeline_bytes, "updates_bytes": updates_bytes,
                "directory_bytes": directory_bytes, "directory_files": directory_files,
                "directory_after_load": session_directory_stats(&dir),
                "load_ms": load_elapsed.as_secs_f64()*1e3, "first_notification_ms": ttfn,
                "first_history_ms": first_history_ms, "last_history_ms": last_history_ms,
                "max_history_gap_ms": max_history_gap_ms,
                "history_count": history_count, "history_bytes": history_bytes,
                "phases": phases.iter().map(|(name, count, ms)| serde_json::json!({"name":name,"count":count,"ms":ms})).collect::<Vec<_>>()
            }));
            if modern_history() {
                assert_eq!(history_count, (spec.turns * 2) as u64, "one user and one canonical answer per turn");
                assert!(!updates_path.exists(), "a load must not create an obsolete updates cache");
                let verified = JsonlStorageAdapter::with_root(grow_home.path().to_path_buf())
                    .load_session_without_updates(&info).await.expect("post-load journal remains fully valid");
                assert_eq!(verified.timeline.admitted_response_views().len(), spec.turns, "all canonical admissions survive load without duplicate facts");
            }

            assert!(replay_count > 0, "expected replayed notifications during load");
            // The lazy rewind file must be byte-for-byte unchanged by a load.
            assert_eq!(
                file_fingerprint(&rewind_path_guard),
                rewind_fp_before,
                "rewind_points.jsonl must be unchanged after a load (zero data loss)"
            );
            // Historical ACUs are skipped even for the small default fixture;
            // one live catalog advertisement can arrive before the response.
            assert!(
                acu_replayed <= 1,
                "historical available_commands_update must be skipped on replay \
                 (replayed {acu_replayed} of {acu_persisted} persisted)"
            );
            // ...but the catalog IS re-advertised to the client after load.
            assert!(
                readvertised,
                "live available_commands_update must reach the client"
            );
        })
        .await;
}
