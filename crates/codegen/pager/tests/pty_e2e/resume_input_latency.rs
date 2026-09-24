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
        if !name.starts_with("pager.") && !name.starts_with("session.load_session") {
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

async fn measure_resume(turns: usize, slow_replay_writer: bool, dense_tools: bool) {
    let content = ContentController::start()
        .await
        .expect("start mock provider");
    content
        .seed_llm_config()
        .expect("seed mock provider config");
    let project = tempfile::tempdir().expect("create project");
    std::fs::create_dir_all(project.path().join(".git")).expect("create repository marker");
    let project_cwd = std::fs::canonicalize(project.path()).expect("canonical project cwd");
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
    let (info, directory) = synth::prepare_session(&grow_home, &project_cwd, &spec).await;
    let marker = format!("RESUME_READY_{turns}");
    append_fixture_timeline(&grow_home, &info, turns, &marker).await;
    let replay_started = format!("RESUME_REPLAY_STARTED_{turns}");
    if slow_replay_writer {
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
                        mark_replay_chunk(&mut value, &replay_started),
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
    let line = serde_json::json!({
        "timestamp": 0,
        "method": "session/update",
        "params": {"sessionId": info.id, "update": update},
    });
    use std::io::Write as _;
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(directory.join("updates.jsonl"))
            .expect("open fixture updates"),
        "{line}"
    )
    .expect("append last replay marker");

    let binary = pager_binary().expect("resolve pager binary");
    let instrumentation_log = content.home().join(format!("resume-{turns}-phases.jsonl"));
    let log_path = instrumentation_log
        .to_str()
        .expect("UTF-8 fixture instrumentation path");
    let mut child_env = vec![
        ("GROW_INSTRUMENTATION", "log"),
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
    let mut replay_marker_visible = if dense_tools {
        harness
            .wait_for_text("Loading session", Duration::from_secs(15))
            .expect("show session loading prompt before replay completes");
        assert!(
            !harness.contains_text(&marker),
            "dense replay must still be in progress at the loading prompt"
        );
        None
    } else if slow_replay_writer {
        harness
            .wait_for_text(&replay_started, Duration::from_secs(15))
            .expect("show streamed history replay marker");
        let visible_at = Some(process_started.elapsed());
        assert!(
            !harness.contains_text(&marker),
            "history replay must still be in progress before input measurement"
        );
        visible_at
    } else {
        harness
            .wait_until_stable(
                "resumed history and agent prompt",
                RESUME_TIMEOUT,
                Duration::from_millis(500),
                |h| h.contains_text(&marker) && h.contains_text("grow · Mock"),
            )
            .expect("show final replay marker and stable agent prompt");
        Some(process_started.elapsed())
    };
    let draft = if slow_replay_writer {
        "REPLAY_KEYS:"
    } else {
        "PERF:"
    };
    harness
        .inject_keys(draft.as_bytes())
        .expect("type a draft during history replay");
    harness
        .wait_for_text(draft, Duration::from_secs(10))
        .expect("echo draft during history replay");
    if slow_replay_writer
        && replay_marker_visible.is_none()
        && harness.contains_text(&replay_started)
    {
        replay_marker_visible = Some(process_started.elapsed());
    }
    let mut echoes = Vec::with_capacity(20);
    let mut replay_completed_at_key = harness.contains_text(&marker).then_some(0);
    for count in 1..=20 {
        let echo_started = Instant::now();
        harness.inject_keys(b"Q").expect("type one key");
        harness
            .wait_for_text(
                &format!("{draft}{}", "Q".repeat(count)),
                Duration::from_secs(10),
            )
            .expect("echo one key");
        echoes.push(echo_started.elapsed());
        if slow_replay_writer {
            if replay_marker_visible.is_none() && harness.contains_text(&replay_started) {
                replay_marker_visible = Some(process_started.elapsed());
            }
            if harness.contains_text(&marker) {
                replay_completed_at_key.get_or_insert(count);
            }
        }
    }
    echoes.sort_unstable();
    let p95 = echoes[(echoes.len() * 95).div_ceil(100) - 1];
    print_phase_summary(&instrumentation_log, turns);
    let echo_window_end_ms = process_started.elapsed().as_secs_f64() * 1000.0;
    let replay_marker_visible_ms = replay_marker_visible.map(|at| at.as_secs_f64() * 1000.0);
    eprintln!(
        "[resume-pty] turns={turns} replay_marker_visible_ms={replay_marker_visible_ms:?} echo_window_end_ms={echo_window_end_ms:.1} replay_completed_at_key={replay_completed_at_key:?}",
    );
    if slow_replay_writer {
        assert!(
            p95 <= Duration::from_millis(100),
            "delayed-writer replay key echo p95 must stay <=100ms; got {p95:?}"
        );
        harness
            .wait_until_stable(
                "resumed history and agent prompt",
                RESUME_TIMEOUT,
                Duration::from_millis(500),
                |h| h.contains_text(&marker) && h.contains_text("grow · Mock"),
            )
            .expect("show final replay marker and stable agent prompt");
        assert!(
            replay_completed_at_key.is_none(),
            "history replay completed during key-echo measurement at key {:?}",
            replay_completed_at_key.expect("checked Some case"),
        );
    }
    let history_visible = process_started.elapsed();
    if slow_replay_writer {
        assert!(
            harness.contains_text(draft),
            "input typed during replay must survive SessionLoaded"
        );
    }
    eprintln!(
        "[resume-pty] turns={turns} update_bytes={} history_visible_ms={:.1} echo_p95_ms={:.1} echo_max_ms={:.1} frames={}",
        std::fs::metadata(directory.join("updates.jsonl"))
            .unwrap()
            .len(),
        history_visible.as_secs_f64() * 1000.0,
        p95.as_secs_f64() * 1000.0,
        echoes.last().unwrap().as_secs_f64() * 1000.0,
        harness.frame_count(),
    );
    harness.quit().expect("reap pager");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "manual long-session PTY latency measurement"]
async fn resume_input_latency_128_512_turns() {
    for turns in [128, 512] {
        measure_resume(turns, false, false).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "manual slow-writer PTY replay input measurement"]
async fn resume_input_during_replay_with_slow_writer() {
    measure_resume(512, true, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "manual dense-tool-history PTY latency measurement"]
async fn resume_input_during_dense_tool_replay_with_slow_writer() {
    measure_resume(512, true, true).await;
}
