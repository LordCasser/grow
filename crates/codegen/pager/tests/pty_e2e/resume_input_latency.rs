//! Manual PTY timing probe for the full `--continue` path.
use super::common::*;
use shell::session::storage::{JsonlStorageAdapter, StorageAdapter};
use shell::session::testkit::synth::{self, SessionSpec};

fn print_phase_summary(path: &Path, turns: usize) {
    let contents = std::fs::read_to_string(path).expect("read child instrumentation log");
    let mut phases = std::collections::BTreeMap::<String, (usize, u128, u64)>::new();
    for line in contents.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let fields = value.get("fields").unwrap_or(&value);
        if fields.get("event").and_then(|v| v.as_str()) != Some("timing") {
            continue;
        }
        let Some(name) = fields.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        if !name.starts_with("pager.") && !name.starts_with("session.") {
            continue;
        }
        let Some(elapsed_us) = fields.get("elapsed_us").and_then(|v| v.as_u64()) else {
            continue;
        };
        let (count, sum, max) = phases.entry(name.to_owned()).or_default();
        *count += 1;
        *sum += u128::from(elapsed_us);
        *max = (*max).max(elapsed_us);
    }
    for (name, (count, sum_us, max_us)) in phases {
        eprintln!(
            "[resume-phase] turns={turns} name={name} count={count} sum_ms={:.1} max_ms={:.1}",
            sum_us as f64 / 1000.0,
            max_us as f64 / 1000.0,
        );
    }
}

fn sequence_marker(text: &str) -> Option<String> {
    let marker = text.get(text.find("GROW_SEQ_")?..)?;
    let digits = marker.strip_prefix("GROW_SEQ_")?.get(..6)?;
    digits
        .bytes()
        .all(|byte| byte.is_ascii_digit())
        .then(|| format!("GROW_SEQ_{digits}"))
}

fn trace_marker_output_ms(
    cast_path: &Path,
    last_marker: &str,
) -> (Option<(String, f64)>, Option<f64>) {
    let contents = std::fs::read_to_string(cast_path).expect("read pre-key PTY cast");
    let mut tail = String::new();
    let mut first_received_at = None;
    let mut last_received_at = None;
    let tail_capacity = last_marker.len() + 64;
    for line in contents.lines().skip(1) {
        let event: serde_json::Value = serde_json::from_str(line).expect("parse PTY cast event");
        let elapsed = event
            .get(0)
            .and_then(serde_json::Value::as_f64)
            .expect("cast event timestamp");
        tail.push_str(
            event
                .get(2)
                .and_then(serde_json::Value::as_str)
                .expect("cast event output"),
        );
        if first_received_at.is_none()
            && let Some(marker) = sequence_marker(&tail)
        {
            first_received_at = Some((marker, elapsed * 1000.0));
        }
        if last_received_at.is_none() && tail.contains(last_marker) {
            last_received_at = Some(elapsed * 1000.0);
        }
        if tail.len() > tail_capacity {
            let mut start = tail.len() - tail_capacity;
            while !tail.is_char_boundary(start) {
                start += 1;
            }
            tail.drain(..start);
        }
    }
    (first_received_at, last_received_at)
}

fn replay_completion_timer(allow_legacy_proxy: bool) -> &'static str {
    if allow_legacy_proxy {
        "pager.replay.end_batch"
    } else {
        "pager.replay.finalize"
    }
}

fn configured_turns(defaults: &[usize]) -> Vec<usize> {
    let Ok(value) = std::env::var("GROW_PTY_RESUME_TURNS") else {
        return defaults.to_vec();
    };
    let turns = value
        .split(',')
        .map(|part| {
            part.trim()
                .parse::<usize>()
                .expect("GROW_PTY_RESUME_TURNS must be comma-separated positive integers")
        })
        .collect::<Vec<_>>();
    assert!(!turns.is_empty(), "GROW_PTY_RESUME_TURNS cannot be empty");
    assert!(
        turns.iter().all(|turn| *turn > 0),
        "turn counts must be positive"
    );
    assert!(
        turns
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == turns.len(),
        "GROW_PTY_RESUME_TURNS cannot repeat a turn count"
    );
    turns
}

fn typed_replay_stats(
    updates: &[agent_client_protocol::schema::v1::SessionUpdate],
) -> serde_json::Value {
    use agent_client_protocol::schema::v1::{ContentBlock, SessionUpdate};
    let mut notification_types = std::collections::BTreeMap::<&'static str, usize>::new();
    let mut block_types = std::collections::BTreeMap::<&'static str, usize>::new();
    let mut text_bytes = 0usize;
    let mut message_blocks = 0usize;
    let mut canonical_turns = 0usize;
    let mut assistant_message_notifications = 0usize;
    for update in updates {
        let kind = match update {
            SessionUpdate::UserMessageChunk(chunk) => {
                message_blocks += 1;
                match &chunk.content {
                    ContentBlock::Text(text) => {
                        text_bytes += text.text.len();
                        *block_types.entry("text").or_default() += 1;
                    }
                    _ => *block_types.entry("non_text").or_default() += 1,
                }
                "user_message_chunk"
            }
            SessionUpdate::AgentMessageChunk(chunk) => {
                assistant_message_notifications += 1;
                message_blocks += 1;
                match &chunk.content {
                    ContentBlock::Text(text) => {
                        text_bytes += text.text.len();
                        if text.text.lines().any(|line| line.starts_with("GROW_SEQ_")) {
                            canonical_turns += 1;
                        }
                        *block_types.entry("text").or_default() += 1;
                    }
                    _ => *block_types.entry("non_text").or_default() += 1,
                }
                "agent_message_chunk"
            }
            SessionUpdate::AvailableCommandsUpdate(_) => "available_commands_update",
            SessionUpdate::ToolCall(_) => "tool_call",
            SessionUpdate::ToolCallUpdate(_) => "tool_call_update",
            _ => "other",
        };
        *notification_types.entry(kind).or_default() += 1;
    }
    serde_json::json!({
        "notifications": updates.len(),
        "notification_types": notification_types,
        "canonical_turns_by_fixture_sequence_marker": canonical_turns,
        "assistant_message_notifications": assistant_message_notifications,
        "message_blocks": message_blocks,
        "block_types": block_types,
        "text_bytes": text_bytes,
    })
}

fn rfc3339_order_key(timestamp: &str) -> Option<String> {
    let timestamp = timestamp.strip_suffix('Z')?;
    let (whole, fraction) = timestamp.split_once('.').unwrap_or((timestamp, ""));
    if whole.len() != 19 || !whole.is_ascii() || fraction.len() > 9 {
        return None;
    }
    if !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(format!("{whole}.{:0<9}Z", fraction))
}

fn retained_trace_prefix(
    path: &Path,
    completion_timer: &str,
) -> (Vec<serde_json::Value>, String, String) {
    let contents = std::fs::read_to_string(path).expect("read child instrumentation log");
    let rows = contents
        .lines()
        .filter_map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .map(|value| (value, line))
        })
        .collect::<Vec<_>>();
    let completion_index = rows
        .iter()
        .position(|(value, _)| {
            value
                .get("fields")
                .unwrap_or(value)
                .get("name")
                .and_then(|v| v.as_str())
                == Some(completion_timer)
        })
        .unwrap_or_else(|| panic!("loading trace must include {completion_timer}"));
    let completion_timestamp = rows[completion_index]
        .0
        .get("timestamp")
        .and_then(|value| value.as_str())
        .expect("completion event timestamp")
        .to_owned();
    let boundary_key =
        rfc3339_order_key(&completion_timestamp).expect("completion timestamp must be RFC3339 UTC");
    let mut prefix = Vec::new();
    let mut jsonl = String::new();
    for (index, (value, line)) in rows.into_iter().enumerate() {
        let timestamp_key = value
            .get("timestamp")
            .and_then(|value| value.as_str())
            .and_then(rfc3339_order_key);
        let before_boundary = timestamp_key
            .as_ref()
            .is_some_and(|timestamp| timestamp < &boundary_key)
            || (timestamp_key.as_ref() == Some(&boundary_key) && index <= completion_index);
        // Timestamp clipping excludes later rows at or beyond the boundary,
        // while retaining the completion row itself for boundary cost analysis.
        if before_boundary || (index <= completion_index && timestamp_key.is_none()) {
            prefix.push(value);
            jsonl.push_str(line);
            jsonl.push('\n');
        }
    }
    (prefix, jsonl, completion_timestamp)
}

fn retain_loading_trace(
    turns: usize,
    modern: bool,
    dense_tools: bool,
    source_bytes: &serde_json::Value,
    instrumentation_log: &Path,
    harness: &PtyHarness,
    first_marker_visible: Option<(&str, f64)>,
    last_marker: &str,
    last_marker_visible_ms: Option<f64>,
    completion_observed_ms: f64,
    ready_stable_ms: f64,
    completion_timer: &str,
    legacy_proxy: bool,
    pty_spawn_call_returned_ms: f64,
    slow_progress: Option<&serde_json::Value>,
) {
    let Some(trace_dir) = std::env::var_os("GROW_PTY_RESUME_TRACE_DIR") else {
        return;
    };
    let trace_dir = PathBuf::from(trace_dir);
    std::fs::create_dir_all(&trace_dir).expect("create retained PTY trace directory");
    let label = std::env::var("GROW_PTY_RESUME_TRACE_LABEL").unwrap_or_else(|_| "run".into());
    assert!(
        label
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'),
        "GROW_PTY_RESUME_TRACE_LABEL must contain only ASCII letters, digits, '-' or '_'"
    );
    let run_nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after UNIX epoch")
        .as_nanos();
    let stem = format!(
        "{label}-{}-{turns}-{}-{run_nonce}",
        if modern { "modern" } else { "legacy" },
        std::process::id(),
    );
    let cast_path = trace_dir.join(format!("{stem}-pre-key.cast"));
    harness
        .write_cast(&cast_path)
        .expect("write pre-key PTY cast");

    let (first_marker_output, last_marker_output_ms) =
        trace_marker_output_ms(&cast_path, last_marker);
    let (phases, phase_jsonl, completion_timestamp) =
        retained_trace_prefix(instrumentation_log, completion_timer);
    let raw_phase_path = trace_dir.join(format!("{stem}-pre-key.jsonl"));
    let raw_phase_jsonl =
        std::fs::read(instrumentation_log).expect("read full instrumentation log");
    std::fs::write(&raw_phase_path, raw_phase_jsonl)
        .expect("write retained raw instrumentation JSONL");
    let loading_phase_path = trace_dir.join(format!("{stem}-loading.jsonl"));
    std::fs::write(&loading_phase_path, phase_jsonl).expect("write retained loading JSONL");
    let events = [
        Some(serde_json::json!({"kind":"process_started", "elapsed_ms":0.0})),
        first_marker_output.as_ref().map(|(marker, elapsed_ms)| {
            serde_json::json!({
            "kind":"first_sequence_marker_pty_received", "marker":marker,
            "clock":"pty_received_cast_clock", "elapsed_ms":elapsed_ms
                })
        }),
        last_marker_output_ms.map(|elapsed_ms| {
            serde_json::json!({
                "kind":"last_history_marker_pty_received", "marker":last_marker,
                "clock":"pty_received_cast_clock", "elapsed_ms":elapsed_ms
            })
        }),
        first_marker_visible.map(|(marker, elapsed_ms)| {
            serde_json::json!({
                "kind":"first_sequence_marker_visible", "marker":marker,
                "elapsed_ms":elapsed_ms
            })
        }),
        last_marker_visible_ms.map(|elapsed_ms| {
            serde_json::json!({
                "kind":"last_history_marker_visible", "marker":last_marker, "elapsed_ms":elapsed_ms
            })
        }),
        Some(serde_json::json!({
            "kind":"replay_completion_observed", "timer":completion_timer,
            "legacy_proxy":legacy_proxy, "elapsed_ms":completion_observed_ms
        })),
        Some(serde_json::json!({
            "kind":"ready_prompt_stable", "elapsed_ms":ready_stable_ms
        })),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let trace = serde_json::json!({
        "schema_version": 1,
        "turns": turns,
        "modern": modern,
        "dense_tools": dense_tools,
        "raw_window": "PTY spawn through stable ready prompt, before ready-key injection",
        "clocks": {
            "process_elapsed_origin": "immediately before PTY spawn call",
            "pty_received_cast_clock": "PtyHarness spawn instant; timestamps are PTY chunk receipt, not application write completion",
            "pty_clock_origin_offset_ms_from_process_elapsed": {
                "lower_bound": 0.0,
                "upper_bound": pty_spawn_call_returned_ms,
                "reason": "PtyHarness records spawned_at inside spawn before the call returns"
            }
        },
        "loading_window": if legacy_proxy {
            "instrumentation rows timestamp-clipped through legacy pager.replay.end_batch proxy"
        } else {
            "instrumentation rows timestamp-clipped through pager.replay.finalize"
        },
        "source": source_bytes,
        "slow_replay_progress": slow_progress,
        "loading": {
            "events": events,
            "instrumentation_prefix_through_completion": phases,
            "completion_timestamp": completion_timestamp,
            "instrumentation_file": loading_phase_path.file_name().and_then(|name| name.to_str()),
        },
        "raw_files": {
            "cast": cast_path.file_name().and_then(|name| name.to_str()),
            "instrumentation": raw_phase_path.file_name().and_then(|name| name.to_str()),
            "frames_before_keys": harness.frame_count(),
        },
    });
    std::fs::write(
        trace_dir.join(format!("{stem}-loading.json")),
        serde_json::to_vec_pretty(&trace).expect("serialize retained PTY trace"),
    )
    .expect("write retained PTY loading trace");
}

fn mark_replay_chunk(value: &mut serde_json::Value, marker: &str) -> bool {
    match value {
        serde_json::Value::Object(fields) => {
            if let Some(serde_json::Value::String(text)) = fields.get_mut("text") {
                text.push_str(marker);
                return true;
            }
            fields
                .values_mut()
                .any(|value| mark_replay_chunk(value, marker))
        }
        serde_json::Value::Array(items) => items
            .iter_mut()
            .any(|value| mark_replay_chunk(value, marker)),
        _ => false,
    }
}

async fn append_fixture_timeline(
    root: &Path,
    info: &shell::session::info::Info,
    turns: usize,
    marker: &str,
) {
    use sampling_types::ConversationItem;
    let storage = JsonlStorageAdapter::with_root(root.to_path_buf());
    let mut timeline = chat_state::Timeline::from_seed(vec![ConversationItem::system(
        "synthetic resume benchmark",
    )])
    .expect("valid seed");
    for event in timeline.events() {
        storage
            .append_timeline_event_durable(info, event)
            .await
            .expect("append seed");
    }
    for turn_no in 0..turns {
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
                    model_id: "mock/mock".into(),
                    input_message_count: timeline.surface().len(),
                    prompt_index: turn_no,
                    prompt_text: prompt.clone(),
                    input_kind: chat_state::TurnInputKind::Prompt,
                    redirect_kind: None,
                },
            ))
            .expect("start turn");
        storage
            .append_timeline_event_durable(info, &started)
            .await
            .expect("append turn start");
        let mut user = ConversationItem::user(prompt);
        user.set_prompt_index(turn_no);
        let user = timeline
            .append(user, chat_state::MessageCause::User)
            .expect("append user");
        storage
            .append_timeline_event_durable(info, &user)
            .await
            .expect("persist user");
        let answer = if turn_no + 1 == turns {
            marker.to_owned()
        } else {
            format!("historical answer {turn_no}: {}", "x".repeat(4096))
        };
        let answer = timeline
            .append(
                ConversationItem::assistant(answer),
                chat_state::MessageCause::Assistant,
            )
            .expect("append answer");
        storage
            .append_timeline_event_durable(info, &answer)
            .await
            .expect("persist answer");
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
            .expect("end turn");
        storage
            .append_timeline_event_durable(info, &ended)
            .await
            .expect("persist turn end");
    }
}

async fn measure_resume(
    turns: usize,
    slow_replay_writer: bool,
    dense_tools: bool,
    quit_loading: bool,
    modern: bool,
) {
    let content = ContentController::start()
        .await
        .expect("start mock provider");
    content
        .seed_llm_config()
        .expect("seed mock provider config");
    let project = tempfile::tempdir().expect("create project");
    std::fs::create_dir_all(project.path().join(".git")).expect("create repository marker");
    let project_cwd = dunce::canonicalize(project.path()).expect("canonical project cwd");
    let grow_home = content.home().join(".grow");
    let spec = SessionSpec {
        turns,
        acu_per_turn: 8,
        catalog_commands: 16,
        catalog_desc_len: 96,
        agent_chunks_per_turn: 8,
        agent_chunk_len: 4096,
        rewind_points: 2,
        files_per_rewind: 2,
        file_content_len: 256,
    };
    let (info, directory) =
        synth::replay::prepare_legacy_session(&grow_home, &project_cwd, &spec).await;
    let marker = format!("RESUME_READY_{turns}");
    if modern {
        let route = sampling_types::model_image_input_key_from_parts(
            "mock",
            &sampling_types::ApiBackend::ChatCompletions,
            &content.url(),
            &Default::default(),
        );
        synth::write_admitted_history(
            &info,
            &directory,
            &spec,
            "mock/mock",
            0,
            Some(("mock", &route)),
        );
    } else {
        append_fixture_timeline(&grow_home, &info, turns, &marker).await;
    }
    let replay_started = format!("RESUME_REPLAY_STARTED_{turns}");
    if slow_replay_writer && !modern {
        let updates_path = directory.join("updates.jsonl");
        let updates = std::fs::read_to_string(&updates_path).expect("read fixture updates");
        let mut marked = 0;
        let mut updates = updates
            .lines()
            .map(|line| {
                if line.contains("agent_message_chunk") {
                    let mut value: serde_json::Value =
                        serde_json::from_str(line).expect("parse fixture update");
                    assert!(
                        mark_replay_chunk(
                            &mut value,
                            &format!("\n{replay_started} GROW_SEQ_{marked:06}\n")
                        ),
                        "agent chunk must contain text"
                    );
                    marked += 1;
                    serde_json::to_string(&value).expect("serialize marked fixture update")
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>();
        assert!(marked > 100, "fixture must contain streamed agent chunks");
        if dense_tools {
            let mut interleaved = Vec::with_capacity(updates.len() + turns * 2);
            let mut chunks = 0;
            for line in updates {
                let is_agent_chunk = line.contains("agent_message_chunk");
                interleaved.push(line);
                if !is_agent_chunk {
                    continue;
                }
                chunks += 1;
                if chunks % 8 != 0 {
                    continue;
                }
                let turn_index = chunks / 8 - 1;
                let call_id = format!("resume-tool-{turn_index}");
                for update in [
                    serde_json::json!({
                        "timestamp": chunks,
                        "method": "session/update",
                        "params": {"sessionId": info.id, "update": {
                            "sessionUpdate": "tool_call",
                            "toolCallId": call_id,
                            "title": format!("Historical read {turn_index}"),
                            "kind": "read",
                            "status": "in_progress",
                            "rawInput": {"path": format!("src/module-{turn_index}.rs")}
                        }}
                    }),
                    serde_json::json!({
                        "timestamp": chunks,
                        "method": "session/update",
                        "params": {"sessionId": info.id, "update": {
                            "sessionUpdate": "tool_call_update",
                            "toolCallId": call_id,
                            "status": "completed",
                            "rawOutput": {"text": "historical tool output"}
                        }}
                    }),
                ] {
                    interleaved
                        .push(serde_json::to_string(&update).expect("serialize dense tool update"));
                }
            }
            assert_eq!(
                chunks,
                turns * 8,
                "fixture has eight chunks per historical turn"
            );
            updates = interleaved;
        }
        std::fs::write(&updates_path, format!("{}\n", updates.join("\n")))
            .expect("write marked replay fixture");
    }
    JsonlStorageAdapter::with_root(grow_home.clone())
        .update_current_model_and_agent(
            &info,
            &shell::agent::models::ModelId::new("mock/mock"),
            None,
            None,
        )
        .await
        .expect("select mock model for fixture");
    assert_eq!(
        JsonlStorageAdapter::with_root(grow_home.clone())
            .list_sessions(Some(project_cwd.to_str().expect("UTF-8 test cwd")))
            .await
            .expect("list fixture sessions")
            .len(),
        1,
        "synthetic session must be discoverable before PTY launch"
    );
    let update = agent_client_protocol::schema::v1::SessionUpdate::AgentMessageChunk(
        agent_client_protocol::schema::v1::ContentChunk::new(
            agent_client_protocol::schema::v1::ContentBlock::Text(
                agent_client_protocol::schema::v1::TextContent::new(marker.clone()),
            ),
        ),
    );
    if modern {
        let notification =
            agent_client_protocol::schema::v1::SessionNotification::new(info.id.clone(), update);
        JsonlStorageAdapter::with_root(grow_home.clone())
            .append_update(
                &info,
                &shell::session::storage::SessionUpdate::Acp(Box::new(notification)),
            )
            .await
            .expect("append last replay marker");
    } else {
        let line = serde_json::json!({"timestamp":0,"method":"session/update","params":{"sessionId":info.id,"update":update}});
        use std::io::Write as _;
        writeln!(
            std::fs::OpenOptions::new()
                .append(true)
                .open(directory.join("updates.jsonl"))
                .expect("open temporary legacy fixture"),
            "{line}"
        )
        .unwrap();
        synth::replay::finish_legacy_fixture(&info, &directory);
    }
    // Capture semantic source counts from the real projector output before the
    // optional legacy export rewrites the fixture for the retained old CLI.
    let history_body = {
        let updates =
            shell::session::storage::load_updates_for_replay_at(info.id.0.as_ref(), &grow_home)
                .expect("read typed fixture replay for timing metadata")
                .expect("fixture session exists");
        let mut stats = typed_replay_stats(&updates);
        stats["scope"] = serde_json::json!(if dense_tools && !modern {
            "typed_projector_before_legacy_dense_tool_updates"
        } else {
            "typed_projector_output"
        });
        stats["matches_runtime_notification_input"] = serde_json::json!(!dense_tools || modern);
        stats
    };
    if std::env::var_os("GROW_PTY_LEGACY_COMPARISON").is_some() {
        shell::session::testkit::legacy_comparison::export_legacy_comparison_fixture(
            &info, &directory,
        )
        .expect("export isolated pre-change benchmark fixture");
    }

    let timeline_path = directory.join("timeline.jsonl");
    let updates_path = directory.join("updates.jsonl");
    let timeline_bytes = std::fs::metadata(&timeline_path).map_or(0, |metadata| metadata.len());
    let updates_bytes = std::fs::metadata(&updates_path).map_or(0, |metadata| metadata.len());
    let updates_file_present = updates_path.exists();
    let source_bytes = serde_json::json!({
        "timeline_bytes": timeline_bytes,
        "updates_bytes": updates_bytes,
        "timeline_and_updates_bytes": timeline_bytes + updates_bytes,
        "counted_files": ["timeline.jsonl", "updates.jsonl"],
        "excluded_source_files": ["summary and other session files"],
        "source_file_count": if updates_file_present { 2 } else { 1 },
        "history_body": history_body,
    });

    let binary = pager_binary().expect("resolve pager binary");
    let instrumentation_log = content.home().join(format!("resume-{turns}-phases.jsonl"));
    let log_path = instrumentation_log
        .to_str()
        .expect("UTF-8 fixture instrumentation path");
    let instrumented = std::env::var_os("GROW_PTY_DISABLE_INSTRUMENTATION").is_none();
    eprintln!("[resume-source] turns={turns} instrumentation={instrumented} stats={source_bytes}");
    let mut child_env = vec![
        (
            "GROW_INSTRUMENTATION",
            if instrumented { "log" } else { "off" },
        ),
        ("GROW_INSTRUMENTATION_LOG", log_path),
    ];
    if slow_replay_writer {
        child_env.push(("GROW_TEST_FRAME_WRITE_DELAY_MS", "40"));
    }
    let process_started = Instant::now();
    let mut harness = PtyHarness::spawn_with_content_env_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &["--continue"],
        &child_env,
        Some(&project_cwd),
    )
    .expect("spawn pager");
    let pty_spawn_call_returned_ms = process_started.elapsed().as_secs_f64() * 1000.0;
    let mut slow_progress_trace = None;
    if slow_replay_writer {
        harness
            .wait_for_text("Loading session", Duration::from_secs(15))
            .expect("observe loading before replay completes");
        assert!(
            !harness.contains_text(&marker),
            "fixture must still be loading"
        );
        let loading_observed = Instant::now();
        if quit_loading {
            harness
                .inject_keys(b"\x04\x04")
                .expect("confirm Ctrl+D while loading");
            let exit = harness
                .wait_for_exit_and_drain(Duration::from_secs(15), Duration::from_secs(1))
                .expect("loading Ctrl+D must terminate normally");
            assert_eq!(exit, 0);
            // The second process must cross its own SessionLoaded boundary;
            // history and the model label may be visible before input is ready.
            std::fs::write(&instrumentation_log, b"").unwrap();
            let mut resumed = PtyHarness::spawn_with_content_env_in_dir(
                &binary,
                DEFAULT_ROWS,
                DEFAULT_COLS,
                &content,
                &["--continue"],
                &child_env,
                Some(&project_cwd),
            )
            .expect("resume after exit during loading");
            let legacy_proxy = std::env::var_os("GROW_PTY_END_BATCH_PROXY").is_some();
            let completion_timer = replay_completion_timer(legacy_proxy);
            resumed
                .wait_until_stable(
                    "history after interrupted load",
                    RESUME_TIMEOUT,
                    Duration::from_millis(500),
                    |h| {
                        h.contains_text(&marker)
                            && h.contains_text("grow · Mock")
                            && !h.contains_text("Loading session")
                            && (!instrumented
                                || std::fs::read_to_string(&instrumentation_log)
                                    .is_ok_and(|log| log.contains(completion_timer)))
                    },
                )
                .expect("interrupted load must leave the session recoverable");
            resumed
                .inject_keys(b"\x04\x04")
                .expect("confirm final quit");
            assert_eq!(
                resumed
                    .wait_for_exit_and_drain(Duration::from_secs(15), Duration::from_secs(1))
                    .unwrap(),
                0
            );
            return;
        }
        harness
            .inject_keys(b"LOADING_INPUT_MUST_BE_IGNORED\r")
            .expect("send ordinary input during loading");
        let deadline = Instant::now() + RESUME_TIMEOUT;
        let mut highest = None;
        let mut advances = 0usize;
        let mut first_visible = None;
        let mut last_advance = None;
        let mut visible_maxima = Vec::new();
        let mut max_gap = Duration::ZERO;
        while !harness.contains_text(&marker) {
            assert!(
                Instant::now() < deadline,
                "history did not complete: {}",
                harness.screen_contents()
            );
            harness.update(Duration::from_millis(20));
            let sequence = harness
                .screen_contents()
                .split("GROW_SEQ_")
                .skip(1)
                .filter_map(|part| part.get(..6)?.parse::<usize>().ok())
                .max();
            if let Some(sequence) = sequence
                && highest.is_none_or(|prior| sequence > prior)
            {
                let now = process_started.elapsed();
                first_visible.get_or_insert(now);
                if let Some(prior) = last_advance {
                    max_gap = max_gap.max(now - prior);
                }
                last_advance = Some(now);
                highest = Some(sequence);
                visible_maxima.push((now, sequence));
                advances += 1;
            }
        }
        if let Some(last) = last_advance {
            max_gap = max_gap.max(process_started.elapsed() - last);
        }
        let observed_loading_window = loading_observed.elapsed();
        let history_visible_ms = process_started.elapsed().as_secs_f64() * 1000.0;
        let first_visible_ms = first_visible
            .map(|time| time.as_secs_f64() * 1000.0)
            .unwrap_or(f64::NAN);
        let final_screen = harness.screen_contents();
        slow_progress_trace = Some(serde_json::json!({
            "observed_loading_window_ms": observed_loading_window.as_secs_f64() * 1000.0,
            "first_history_marker_visible_ms": first_visible_ms,
            "history_complete_visible_ms": history_visible_ms,
            "advances": advances,
            "highest_sequence": highest,
            "visible_maxima": visible_maxima.iter().map(|(elapsed, sequence)| serde_json::json!({
                "elapsed_ms": elapsed.as_secs_f64() * 1000.0,
                "sequence": sequence,
            })).collect::<Vec<_>>(),
            "max_content_gap_ms": max_gap.as_secs_f64() * 1000.0,
        }));
        eprintln!(
            "[resume-progress] turns={turns} modern={modern} dense_tools={dense_tools} first_history_ms={first_visible_ms:.1} history_visible_ms={history_visible_ms:.1} advances={advances} highest={highest:?} visible_maxima={visible_maxima:?} max_content_gap_ms={:.1} observed_loading_window_ms={:.1}",
            max_gap.as_secs_f64() * 1000.0,
            observed_loading_window.as_secs_f64() * 1000.0
        );
        assert!(
            advances >= 2
                // A short fixture may finish before a second history paint.
                // Bound the entire observed loading window, not the trivially
                // zero first-to-last gap of a single frame. Larger fixtures
                // always retain the multiple-advances gate.
                || (turns <= 512
                    && advances == 1
                    && observed_loading_window < Duration::from_millis(500)),
            "slow replay must advance or finish a short load promptly; loading_window={observed_loading_window:?}, advances={advances}, highest={highest:?}, visible_maxima={visible_maxima:?}, final_screen:\n{final_screen}"
        );
        assert!(
            max_gap < Duration::from_secs(2),
            "history stalled between visible advances: {max_gap:?}"
        );
        assert!(
            !harness.contains_text("LOADING_INPUT_MUST_BE_IGNORED"),
            "loading keys must not become a draft or prompt"
        );
    }
    let mut first_marker_visible = slow_progress_trace.as_ref().and_then(|progress| {
        let first = progress["visible_maxima"].as_array()?.first()?;
        Some((
            format!("GROW_SEQ_{:06}", first["sequence"].as_u64()?),
            first["elapsed_ms"].as_f64()?,
        ))
    });
    let mut last_marker_visible_ms = slow_progress_trace
        .as_ref()
        .and_then(|progress| progress["history_complete_visible_ms"].as_f64());
    if std::env::var_os("GROW_PTY_RESUME_TRACE_DIR").is_some() {
        harness
            .wait_until("last history marker visible", RESUME_TIMEOUT, |h| {
                let now_ms = process_started.elapsed().as_secs_f64() * 1000.0;
                if let Some(marker) = sequence_marker(&h.screen_contents()) {
                    first_marker_visible.get_or_insert((marker, now_ms));
                }
                if h.contains_text(&marker) {
                    last_marker_visible_ms.get_or_insert(now_ms);
                    true
                } else {
                    false
                }
            })
            .expect("last history marker visible");
    } else {
        harness
            .wait_for_text(&marker, RESUME_TIMEOUT)
            .expect("last history marker visible");
    }
    let history_visible = process_started.elapsed();
    // Historical text can be visible before SessionLoaded finishes its replay
    // finalization. Measure the completion boundary before testing ready input.
    let ready_deadline = Instant::now() + RESUME_TIMEOUT;
    let legacy_proxy = std::env::var_os("GROW_PTY_END_BATCH_PROXY").is_some();
    let completion_timer = replay_completion_timer(legacy_proxy);
    while instrumented
        && !std::fs::read_to_string(&instrumentation_log)
            .unwrap_or_default()
            .contains(completion_timer)
    {
        assert!(
            Instant::now() < ready_deadline,
            "replay completion was not observed: {}",
            harness.screen_contents()
        );
        harness.update(Duration::from_millis(20));
    }
    let ready = process_started.elapsed();
    // Observe the ready view independently of diagnostic timers, before the
    // fixed stability interval. All-history finalization may follow the last
    // visible history marker by a substantial interval.
    harness
        .wait_until("ready session view", RESUME_TIMEOUT, |h| {
            h.contains_text(&marker)
                && h.contains_text("grow · Mock")
                && !h.contains_text("Loading session")
        })
        .expect("full-load ready view");
    let ready_visible = process_started.elapsed();
    harness
        .wait_until_stable(
            "resumed history and agent prompt",
            RESUME_TIMEOUT,
            Duration::from_millis(500),
            |h| {
                h.contains_text(&marker)
                    && h.contains_text("grow · Mock")
                    && !h.contains_text("Loading session")
            },
        )
        .expect("final history and ready prompt");
    if instrumented {
        retain_loading_trace(
            turns,
            modern,
            dense_tools,
            &source_bytes,
            &instrumentation_log,
            &harness,
            first_marker_visible
                .as_ref()
                .map(|(marker, elapsed_ms)| (marker.as_str(), *elapsed_ms)),
            &marker,
            last_marker_visible_ms,
            ready.as_secs_f64() * 1000.0,
            process_started.elapsed().as_secs_f64() * 1000.0,
            completion_timer,
            legacy_proxy,
            pty_spawn_call_returned_ms,
            slow_progress_trace.as_ref(),
        );
    }
    assert_eq!(
        content.request_count(),
        0,
        "resume and ignored loading input must not run inference"
    );
    let draft = "READY_KEYS:";
    harness
        .inject_keys(draft.as_bytes())
        .expect("type after loading");
    harness
        .wait_for_text(draft, Duration::from_secs(10))
        .expect("ready input available");
    let mut echoes = Vec::with_capacity(20);
    for count in 1..=20 {
        let started = Instant::now();
        harness.inject_keys(b"Q").expect("type ready key");
        harness
            .wait_for_text(
                &format!("{draft}{}", "Q".repeat(count)),
                Duration::from_secs(10),
            )
            .expect("echo ready key");
        echoes.push(started.elapsed());
    }
    echoes.sort_unstable();
    let p95 = echoes[(echoes.len() * 95).div_ceil(100) - 1];
    assert!(p95 < Duration::from_millis(100), "ready input p95={p95:?}");
    if instrumented {
        print_phase_summary(&instrumentation_log, turns);
    }
    let ready_ms = if instrumented {
        format!("{:.1}", ready.as_secs_f64() * 1000.0)
    } else {
        "unavailable".to_owned()
    };
    eprintln!(
        "[resume-pty] turns={turns} modern={modern} update_bytes={} history_visible_ms={:.1} ready_ms={ready_ms} ready_visible_ms={:.1} instrumentation={instrumented} ready_echo_p95_ms={:.1} ready_echo_max_ms={:.1} frames={}",
        std::fs::metadata(directory.join("updates.jsonl")).map_or(0, |metadata| metadata.len()),
        history_visible.as_secs_f64() * 1000.0,
        ready_visible.as_secs_f64() * 1000.0,
        p95.as_secs_f64() * 1000.0,
        echoes.last().unwrap().as_secs_f64() * 1000.0,
        harness.frame_count()
    );
    harness
        .inject_keys(b"\x04\x04")
        .expect("confirm ready quit");
    assert_eq!(
        harness
            .wait_for_exit_and_drain(Duration::from_secs(15), Duration::from_secs(1))
            .unwrap(),
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "manual long-session PTY latency measurement"]
async fn resume_input_latency_128_512_turns() {
    for turns in [128, 512] {
        measure_resume(turns, false, false, false, false).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "manual slow-writer PTY replay input measurement"]
async fn resume_history_progress_with_slow_writer() {
    for turns in configured_turns(&[512]) {
        measure_resume(turns, true, false, false, false).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "manual dense-tool-history PTY latency measurement"]
async fn resume_dense_history_progress_with_slow_writer() {
    // At 512 turns the 4 KiB chunks completed in one visible frame in 287 ms
    // (run17), which is too short to pressure replay progress across frames.
    for turns in configured_turns(&[2048]) {
        measure_resume(turns, true, true, false, false).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "manual loading Ctrl+D PTY regression"]
async fn ctrl_d_exits_during_session_loading() {
    measure_resume(512, true, false, true, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "modern loading Ctrl+D exit and durable resume regression"]
async fn ctrl_d_exits_during_modern_session_loading() {
    measure_resume(512, true, false, true, true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "modern admission/projection CLI resume benchmark"]
async fn resume_modern_admission_projection_128_512_turns() {
    for turns in configured_turns(&[128, 512]) {
        measure_resume(turns, false, false, false, true).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "modern admission/projection visible history with a slow terminal writer"]
async fn resume_modern_history_progress_with_slow_writer() {
    for turns in configured_turns(&[512]) {
        measure_resume(turns, true, false, false, true).await;
    }
}
