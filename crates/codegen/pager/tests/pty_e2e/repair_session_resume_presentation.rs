//! Ctrl+D cold resume: Behavior, bounded history pages, and settled usage.
//!
//! The history case reuses the synthetic SessionSpec fixture, then drives Plan
//! and later pages only through the PTY. The usage case settles 100, suspends
//! on a real tool wait, and completes with 50 in a fresh process.
#[allow(unused_imports)]
use super::common::*;
use shell::session::storage::{JsonlStorageAdapter, PersistedData, SessionUpdate, StorageAdapter};
use shell::session::testkit::synth::{self, SessionSpec};

const HISTORY_TURNS: usize = 300;
const TAIL_TURNS: usize = 140;
const ARTIFACT_DIR: &str = "/tmp/grow-repair-session-resume-presentation";
const WHEEL_UP: &[u8] = b"\x1b[<64;40;12M";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run pty_e2e_persistence with --ignored --test-threads=1"]
async fn ctrl_d_resume_keeps_plan_and_usage_across_history_pages() {
    let content = ContentController::start()
        .await
        .expect("start isolated mock provider");
    content
        .seed_llm_config()
        .expect("seed isolated mock provider");
    let project = tempfile::tempdir().expect("create project");
    git2::Repository::init(project.path()).expect("init isolated project git");
    let project_cwd = dunce::canonicalize(project.path()).expect("canonical project cwd");
    let grow_home = content.home().join(".grow");
    let spec = SessionSpec {
        turns: HISTORY_TURNS,
        acu_per_turn: 1,
        catalog_commands: 1,
        catalog_desc_len: 16,
        agent_chunks_per_turn: 1,
        agent_chunk_len: 24,
        rewind_points: 0,
        files_per_rewind: 0,
        file_content_len: 0,
    };
    let (info, directory) =
        synth::replay::prepare_legacy_session(&grow_home, &project_cwd, &spec).await;
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
    JsonlStorageAdapter::with_root(grow_home.clone())
        .update_current_model_and_agent(
            &info,
            &shell::agent::models::ModelId::new("mock/mock"),
            None,
            None,
        )
        .await
        .expect("select mock model for fixture");
    let early = sequence_marker(1);
    let middle = sequence_marker(HISTORY_TURNS / 2);
    let latest = sequence_marker(HISTORY_TURNS);
    let timeline_path = directory.join("timeline.jsonl");
    let binary = pager_binary().expect("resolve pager binary");
    let instrumentation_log = content.home().join("resume-presentation-phases.jsonl");
    let log_path = instrumentation_log
        .to_str()
        .expect("UTF-8 instrumentation path");
    let child_env = [
        ("GROW_INSTRUMENTATION", "log"),
        ("GROW_INSTRUMENTATION_LOG", log_path),
    ];
    eprintln!(
        "[resume-presentation] fixture_turns={HISTORY_TURNS} tail_turns={TAIL_TURNS} timeline={}",
        timeline_path.display()
    );

    let mut first = PtyHarness::spawn_with_content_env_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &["--continue", "--trust"],
        &child_env,
        Some(&project_cwd),
    )
    .expect("spawn fixture session");
    first
        .wait_until_stable(
            "recent fixture page",
            RESUME_TIMEOUT,
            Duration::from_millis(400),
            |screen| {
                screen.contains_text(&latest)
                    && screen.contains_text("grow · Mock")
                    && !screen.contains_text("Loading session")
            },
        )
        .expect("latest fixture marker on the resumed recent page");
    assert!(
        !first.full_text().contains(&early),
        "initial load printed history outside the recent page\nfull:\n{}",
        first.full_text()
    );
    first
        .inject_keys(b"/plan\r")
        .expect("select Plan through the prompt");
    first
        .wait_until_stable(
            "plan drafting selected",
            Duration::from_secs(20),
            Duration::from_millis(300),
            |screen| {
                screen.contains_text("plan · drafting")
                    && screen.contains_text("Behavior switched to plan")
            },
        )
        .unwrap_or_else(|_| {
            panic!(
                "Plan was not selected through the UI\nscreen:\n{}",
                first.screen_contents()
            )
        });
    for index in 0..TAIL_TURNS {
        let marker = format!("TAILMARK_{index:03}");
        content.set_response(marker.clone());
        first
            .inject_keys(format!("tail {index}\r").as_bytes())
            .expect("submit tail turn");
        first
            .wait_for_text(&marker, Duration::from_secs(30))
            .unwrap_or_else(|_| {
                panic!(
                    "tail turn {index} did not render\nscreen:\n{}",
                    first.screen_contents()
                )
            });
        if index % 20 == 0 {
            eprintln!("[resume-presentation] tail turn {index} visible");
        }
    }
    let last_tail = format!("TAILMARK_{:03}", TAIL_TURNS - 1);
    confirm_ctrl_d(&mut first, "first");
    write_cast(&first, "history-first.cast");
    drop(first);

    assert_current_plan_drafting(&load_persisted(&grow_home, &info).await);
    copy_log(&instrumentation_log, "history-first-phases.jsonl");
    std::fs::write(&instrumentation_log, b"").expect("reset instrumentation log");
    let mut resumed = PtyHarness::spawn_with_content_env_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &["--continue", "--trust"],
        &child_env,
        Some(&project_cwd),
    )
    .expect("spawn cold resume");
    resumed
        .wait_until_stable(
            "resumed plan tail",
            RESUME_TIMEOUT,
            Duration::from_millis(400),
            |screen| {
                screen.contains_text("plan · drafting")
                    && screen.contains_text(&last_tail)
                    && !screen.contains_text("Loading session")
            },
        )
        .unwrap_or_else(|_| {
            panic!(
                "resumed screen lost Plan or the live tail\nscreen:\n{}",
                resumed.screen_contents()
            )
        });
    let opened = resumed.screen_contents();
    assert!(
        !opened.contains("Behavior switched to plan") && !opened.contains("/plan"),
        "recent page still exposes the historical mode event\nscreen:\n{opened}"
    );
    assert!(
        !resumed.full_text().contains(&early) && !resumed.full_text().contains(&middle),
        "cold resume painted older fixture markers before paging"
    );
    let persisted = load_persisted(&grow_home, &info).await;
    assert_current_plan_drafting(&persisted);
    let ledger_before_paging = settled_attempts(&persisted.timeline_events);
    assert!(
        !ledger_before_paging.is_empty(),
        "live turns left no settled usage to compare across paging"
    );
    let recorded_before = recorded_usage(&opened);
    let requests_before = content.request_count();
    let reads_before = history_page_reads(&instrumentation_log);
    assert!(
        !reads_before.is_empty(),
        "resume did not record session.history_page.read"
    );
    assert!(
        reads_before
            .iter()
            .all(|rows| matches!(rows, Some(rows) if *rows <= 128)),
        "initial history read was missing or exceeded 128 rows: {reads_before:?}"
    );

    resumed.inject_keys(b"\t").expect("focus scrollback");
    resumed.update(Duration::from_millis(200));
    for _ in 0..4 {
        resumed
            .inject_keys(WHEEL_UP)
            .expect("wheel toward older history");
        resumed.update(Duration::from_millis(150));
    }
    page_until(&mut resumed, &middle, 80);
    page_until(&mut resumed, &early, 80);
    assert_ne!(
        history_page_reads(&instrumentation_log).len(),
        reads_before.len(),
        "reaching older markers did not read another bounded history page"
    );
    // Simple mode suppresses bare-letter scrollback bindings, so `G` would
    // type into the prompt instead of jumping to the live tail. Enable the
    // vim scrollback nav through the real `/vim-mode` command, refocus the
    // scrollback, then return to live with `G`.
    resumed
        .inject_keys(b"/vim-mode\r")
        .expect("enable vim scrollback nav");
    resumed.update(Duration::from_millis(200));
    resumed.inject_keys(b"\t").expect("refocus scrollback");
    resumed.inject_keys(b"G").expect("return to live tail");
    resumed
        .wait_for_text(&last_tail, Duration::from_secs(20))
        .expect("live tail after paging");
    assert!(
        resumed.contains_text("plan · drafting"),
        "paging changed the live Behavior label\nscreen:\n{}",
        resumed.screen_contents()
    );
    let persisted = load_persisted(&grow_home, &info).await;
    assert_current_plan_drafting(&persisted);
    assert_eq!(
        settled_attempts(&persisted.timeline_events),
        ledger_before_paging,
        "reading history changed settled attempts or their turn owners"
    );
    assert_eq!(content.request_count(), requests_before);
    assert_eq!(
        recorded_usage(&resumed.screen_contents()),
        recorded_before,
        "visible recorded usage changed after paging"
    );

    confirm_ctrl_d(&mut resumed, "second");
    write_cast(&resumed, "history-resumed.cast");
    drop(resumed);
    std::fs::write(&instrumentation_log, b"").expect("reset instrumentation before third resume");
    let mut again = PtyHarness::spawn_with_content_env_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &["--continue", "--trust"],
        &child_env,
        Some(&project_cwd),
    )
    .expect("spawn second cold resume");
    again
        .wait_until_stable(
            "second resume",
            RESUME_TIMEOUT,
            Duration::from_millis(400),
            |screen| {
                screen.contains_text("plan · drafting")
                    && screen.contains_text(&last_tail)
                    && !screen.contains_text("Behavior switched to plan")
                    && !screen.contains_text("Loading session")
            },
        )
        .unwrap_or_else(|_| {
            panic!(
                "second resume did not keep Plan on the live tail\nscreen:\n{}",
                again.screen_contents()
            )
        });
    let persisted = load_persisted(&grow_home, &info).await;
    assert_current_plan_drafting(&persisted);
    assert_eq!(
        settled_attempts(&persisted.timeline_events),
        ledger_before_paging
    );
    let recorded_again = recorded_usage(&again.screen_contents());
    assert_eq!(recorded_again, recorded_before);
    std::fs::write(
        artifact_path("history-ledger.txt"),
        format!(
            "settled_attempts={ledger_before_paging:?}\nrecorded_tokens={}\nrecorded_cache={}\n",
            recorded_again.tokens, recorded_again.cache
        ),
    )
    .expect("write ledger summary");
    copy_log(&instrumentation_log, "history-third-resume-phases.jsonl");
    confirm_ctrl_d(&mut again, "third");
    write_cast(&again, "history-third.cast");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run pty_e2e_persistence with --ignored --test-threads=1"]
async fn ctrl_d_resume_keeps_split_turn_usage() {
    let content = ContentController::start()
        .await
        .expect("start isolated mock provider");
    content
        .seed_llm_config()
        .expect("seed isolated mock provider");
    let project = tempfile::tempdir().expect("create project");
    git2::Repository::init(project.path()).expect("init isolated project git");
    let marker = project.path().join("usage-command.txt");
    let command = format!(
        "printf 'started\\n' >> {marker}; /bin/sleep 30",
        marker = shell_quote(&marker)
    );
    let args = json!({
        "command": command,
        "description": "wait so Ctrl+D can suspend this turn"
    })
    .to_string();
    // The failed run's ledger was not kept, so this fixture does not name the
    // auxiliary purpose. An unmatched chat call otherwise reports the fixed
    // default text as prompt 10 plus 7 completion tokens, and that 17 is what
    // the prompt ledger folded. Keep the same text and settle 0.
    let mut auxiliary = content.expect_response(
        "non-target auxiliary settlement",
        pager_pty_harness::InferenceRequestMatcher::auxiliary(
            pager_pty_harness::InferenceEndpoint::ChatCompletions,
        ),
        ScriptedResponse::sse(with_usage(
            sse::chat_completion_script_exact(
                "Hello from the pty_harness mock inference server.",
                "test-model",
            ),
            0,
            0,
        )),
    );
    let mut settled_100 = content.expect_agent_turn_with_responses(
        "settled 100 then tool wait",
        ScriptedResponse::sse(with_usage(
            sse::responses_api_reasoning_then_tool_call_events(
                "",
                "call_usage_split",
                "run_terminal_command",
                &args,
                "test-model",
            ),
            70,
            30,
        )),
        ScriptedResponse::sse(with_usage(
            sse::chat_completions_reasoning_then_tool_call_events(
                "",
                "call_usage_split",
                "run_terminal_command",
                &args,
                "test-model",
            ),
            70,
            30,
        )),
    );
    let mut settled_50 = content.expect_agent_turn_with_responses(
        "settled 50 after resume",
        ScriptedResponse::sse(with_usage(
            sse::responses_api_script_exact("USAGE_AFTER_RESUME_50", "test-model"),
            40,
            10,
        )),
        ScriptedResponse::sse(with_usage(
            sse::chat_completion_script_exact("USAGE_AFTER_RESUME_50", "test-model"),
            40,
            10,
        )),
    );

    let binary = pager_binary().expect("resolve pager binary");
    let mut first = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &["--permission-mode", "always-approve", "--trust"],
        Some(project.path()),
    )
    .expect("spawn usage session");
    first
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome");
    first
        .inject_keys(b"split the turn usage\r")
        .expect("submit the usage turn");
    poll_for(Duration::from_secs(45), || {
        first.update(Duration::from_millis(30));
        std::fs::read_to_string(&marker)
            .ok()
            .filter(|text| text.contains("started"))
    })
    .unwrap_or_else(|| {
        panic!(
            "tool wait did not start\nscreen:\n{}",
            first.screen_contents()
        )
    });
    let session = session_dir(&content, &mut first);
    let diagnostics = SplitTurnDiagnostics {
        content: &content,
        session: session.clone(),
    };
    let session_id = session
        .file_name()
        .and_then(|name| name.to_str())
        .expect("session id")
        .to_owned();
    let grow_home = content.home().join(".grow");
    let suspended = load_persisted_id(&grow_home, &session_id).await;
    let (turn_id, prompt_id, identity) = single_started_turn(&suspended.timeline_events);
    let first_attempts = settled_attempts(&suspended.timeline_events);
    assert_eq!(
        first_attempts.len(),
        1,
        "the waiting turn must have only the first 100-token settlement"
    );
    assert_eq!(first_attempts[0].tokens, 100);
    assert_eq!(first_attempts[0].owner.turn_id, turn_id);
    assert_eq!(first_attempts[0].owner.prompt_id, prompt_id);
    assert!(
        completed_turns(&suspended.updates).is_empty(),
        "the original turn must still be open before the second 50-token response"
    );
    settled_100.wait_satisfied().await;
    tokio::time::timeout(Duration::from_secs(30), auxiliary.wait_satisfied())
        .await
        .expect("non-target auxiliary chat completion was not consumed");
    auxiliary.assert_satisfied();
    confirm_ctrl_d(&mut first, "usage-first");
    write_cast(&first, "usage-first.cast");
    drop(first);

    let mut resumed = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &[
            "--continue",
            "--permission-mode",
            "always-approve",
            "--trust",
        ],
        Some(project.path()),
    )
    .expect("resume suspended usage turn");
    resumed
        .wait_for_text("USAGE_AFTER_RESUME_50", RESUME_TIMEOUT)
        .unwrap_or_else(|_| {
            panic!(
                "resumed turn did not finish\nscreen:\n{}",
                resumed.screen_contents()
            )
        });
    settled_50.wait_satisfied().await;
    let persisted = wait_for_split_turn(
        &grow_home,
        &diagnostics.session,
        &session_id,
        &content,
        &mut resumed,
        &turn_id,
        &prompt_id,
        &identity,
    )
    .await;
    let shown = recorded_usage(&resumed.screen_contents());
    write_split_usage_ledger(&persisted, Some(&shown));
    assert_eq!(
        shown.tokens, "150",
        "visible lifetime is not the settled 150"
    );
    preserve_split_turn_diagnostics(&diagnostics.session, diagnostics.content);
    confirm_ctrl_d(&mut resumed, "usage-second");
    write_cast(&resumed, "usage-resumed.cast");
    drop(resumed);

    let mut again = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &["--continue", "--trust"],
        Some(project.path()),
    )
    .expect("resume completed usage session");
    again
        .wait_for_text("USAGE_AFTER_RESUME_50", RESUME_TIMEOUT)
        .expect("completed result remains visible");
    let again_data = load_persisted_id(&grow_home, &session_id).await;
    assert!(
        split_turn_is_complete(&again_data, &turn_id, &prompt_id, &identity),
        "second resume changed the original turn's 100+50 settlement: attempts={:?} completed={:?}",
        settled_attempts(&again_data.timeline_events),
        completed_turns(&again_data.updates)
    );
    assert_eq!(recorded_usage(&again.screen_contents()).tokens, "150");
    confirm_ctrl_d(&mut again, "usage-third");
    write_cast(&again, "usage-third.cast");
}

fn sequence_marker(turn: usize) -> String {
    format!("GROW_SEQ_{turn:06}")
}

fn confirm_ctrl_d(harness: &mut PtyHarness, label: &str) {
    harness
        .inject_keys(b"\x04\x04")
        .unwrap_or_else(|error| panic!("{label} Ctrl+D injection failed: {error}"));
    let exit = harness
        .wait_for_exit_and_drain(Duration::from_secs(20), Duration::from_secs(1))
        .unwrap_or_else(|error| panic!("{label} did not exit: {error}"));
    assert_eq!(exit, 0, "{label} Ctrl+D exit status");
}

fn page_until(harness: &mut PtyHarness, needle: &str, max_pages: usize) {
    for _ in 0..max_pages {
        if harness.contains_text(needle) {
            return;
        }
        harness.inject_keys(keys::PGUP).expect("page up");
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(800) {
            harness.update(Duration::from_millis(40));
            if harness.contains_text(needle) {
                return;
            }
        }
    }
    panic!(
        "PageUp never reached {needle}\nscreen:\n{}",
        harness.screen_contents()
    );
}

fn with_usage(mut events: Vec<SseEvent>, prompt: u64, completion: u64) -> Vec<SseEvent> {
    for event in &mut events {
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&event.data) else {
            continue;
        };
        let mut changed = false;
        if value.get("usage").is_some() {
            value["usage"] = json!({
                "prompt_tokens": prompt,
                "completion_tokens": completion,
                "total_tokens": prompt + completion
            });
            changed = true;
        }
        if value.pointer("/response/usage").is_some() {
            value["response"]["usage"] = json!({
                "input_tokens": prompt,
                "output_tokens": completion,
                "total_tokens": prompt + completion,
                "input_tokens_details": { "cached_tokens": 0 },
                "output_tokens_details": { "reasoning_tokens": 0 }
            });
            changed = true;
        }
        if changed {
            event.data = value.to_string();
        }
    }
    events
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SettledAttempt {
    tokens: u64,
    owner: chat_state::UsagePromptOwner,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompletedTurn {
    prompt_id: String,
    identity: Option<chat_state::TurnIdentity>,
    total_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedUsage {
    tokens: String,
    cache: String,
}

async fn load_persisted(grow_home: &Path, info: &shell::session::info::Info) -> PersistedData {
    JsonlStorageAdapter::with_root(grow_home.to_path_buf())
        .load_session(info)
        .await
        .expect("load verified session")
}

async fn load_persisted_id(grow_home: &Path, session_id: &str) -> PersistedData {
    let storage = JsonlStorageAdapter::with_root(grow_home.to_path_buf());
    let info = storage
        .list_sessions(None)
        .await
        .expect("list sessions")
        .into_iter()
        .find(|summary| summary.info.id.0.as_ref() == session_id)
        .unwrap_or_else(|| panic!("session {session_id} is not in the isolated home"))
        .info;
    storage
        .load_session(&info)
        .await
        .expect("load verified session")
}

fn assert_current_plan_drafting(data: &PersistedData) {
    let state = &data
        .control_snapshot
        .as_ref()
        .expect("latest control snapshot")
        .behavior
        .state;
    assert!(
        matches!(
            state,
            shell::session::behavior::BehaviorState::Plan(
                shell::session::behavior::PlanPhase::Drafting
            )
        ),
        "latest control behavior is {state:?}, not Plan Drafting"
    );
}

fn settled_attempts(events: &[chat_state::TimelineEvent]) -> Vec<SettledAttempt> {
    events
        .iter()
        .filter_map(|event| {
            let chat_state::TimelineEventKind::Observation(observation) = &event.kind else {
                return None;
            };
            if observation.scope != "sampling_usage" || observation.name != "attempt_settled" {
                return None;
            }
            let data = observation
                .data
                .as_ref()
                .expect("attempt settlement has no payload");
            let usage = data
                .get("usage")
                .expect("attempt settlement has no usage object");
            let prompt = usage
                .get("prompt_tokens")
                .and_then(serde_json::Value::as_u64)
                .expect("prompt_tokens");
            let completion = usage
                .get("completion_tokens")
                .and_then(serde_json::Value::as_u64)
                .expect("completion_tokens");
            let owner = serde_json::from_value(
                data.get("prompt_owner")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            )
            .unwrap_or_else(|error| panic!("attempt prompt owner: {error}; payload={data}"));
            Some(SettledAttempt {
                tokens: prompt + completion,
                owner,
            })
        })
        .collect()
}

fn started_turns(
    events: &[chat_state::TimelineEvent],
) -> Vec<(chat_state::TurnId, String, chat_state::TurnIdentity)> {
    events
        .iter()
        .filter_map(|event| {
            let chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started {
                id,
                prompt_id,
                identity,
                ..
            }) = &event.kind
            else {
                return None;
            };
            Some((*id, prompt_id.clone(), identity.clone()))
        })
        .collect()
}

fn single_started_turn(
    events: &[chat_state::TimelineEvent],
) -> (chat_state::TurnId, String, chat_state::TurnIdentity) {
    let mut started = started_turns(events);
    assert_eq!(started.len(), 1, "expected exactly one admitted user turn");
    started.remove(0)
}

fn completed_turns(updates: &[SessionUpdate]) -> Vec<CompletedTurn> {
    updates
        .iter()
        .filter_map(|update| {
            let SessionUpdate::Grow(notification) = update else {
                return None;
            };
            let shell::extensions::notification::SessionUpdate::TurnCompleted {
                prompt_id,
                identity,
                usage,
                ..
            } = &notification.update
            else {
                return None;
            };
            Some(CompletedTurn {
                prompt_id: prompt_id.clone(),
                identity: identity.clone(),
                total_tokens: usage.as_ref().map(|usage| usage.totals.total_tokens),
            })
        })
        .collect()
}

fn split_turn_is_complete(
    data: &PersistedData,
    turn_id: &chat_state::TurnId,
    prompt_id: &str,
    identity: &chat_state::TurnIdentity,
) -> bool {
    let started = started_turns(&data.timeline_events);
    if started.len() != 1 || &started[0].0 != turn_id || started[0].1 != prompt_id {
        return false;
    }
    let attempts = settled_attempts(&data.timeline_events);
    let completed = completed_turns(&data.updates);
    attempts.len() == 2
        && attempts.iter().all(|attempt| {
            &attempt.owner.turn_id == turn_id && attempt.owner.prompt_id == prompt_id
        })
        && attempts[0].tokens == 100
        && attempts[1].tokens == 50
        && completed.len() == 1
        && completed[0].prompt_id == prompt_id
        && completed[0].identity.as_ref() == Some(identity)
        && completed[0].total_tokens == Some(150)
}

async fn wait_for_split_turn(
    grow_home: &Path,
    session_dir: &Path,
    session_id: &str,
    content: &ContentController,
    harness: &mut PtyHarness,
    turn_id: &chat_state::TurnId,
    prompt_id: &str,
    identity: &chat_state::TurnIdentity,
) -> PersistedData {
    let started = Instant::now();
    let mut latest = None;
    while started.elapsed() < Duration::from_secs(20) {
        harness.update(Duration::from_millis(50));
        let data = load_persisted_id(grow_home, session_id).await;
        if split_turn_is_complete(&data, turn_id, prompt_id, identity) {
            return data;
        }
        latest = Some(data);
    }
    let data = latest.expect("usage session was loaded");
    preserve_split_turn_diagnostics(session_dir, content);
    write_split_usage_ledger(&data, None);
    panic!(
        "completed usage was not the original turn's 100 then 50\nattempts={:?}\ncompleted={:?}\nscreen:\n{}",
        settled_attempts(&data.timeline_events),
        completed_turns(&data.updates),
        harness.screen_contents()
    );
}

fn recorded_usage(screen: &str) -> RecordedUsage {
    let line = screen
        .lines()
        .find(|line| line.contains("recorded tokens"))
        .unwrap_or_else(|| panic!("recorded usage is absent\nscreen:\n{screen}"));
    let index = line.find("recorded tokens").expect("recorded tokens");
    let tokens = line[..index]
        .split_whitespace()
        .next_back()
        .unwrap_or_else(|| panic!("recorded token count missing in {line}"))
        .to_owned();
    assert!(
        !tokens.is_empty() && !tokens.starts_with('≥'),
        "recorded token count is blank or only a lower bound: {line}"
    );
    let cache = line[index + "recorded tokens".len()..]
        .split(['·', '│'])
        .map(str::trim)
        .find(|part| part.contains("cache"))
        .unwrap_or_else(|| panic!("recorded cache item missing in {line}"))
        .to_owned();
    RecordedUsage { tokens, cache }
}

fn history_page_reads(path: &Path) -> Vec<Option<usize>> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    contents
        .lines()
        .filter(|line| line.contains("session.history_page.read"))
        .map(|line| {
            line.split("rows").nth(1).and_then(|rest| {
                rest.chars()
                    .skip_while(|ch| !ch.is_ascii_digit())
                    .take_while(|ch| ch.is_ascii_digit())
                    .collect::<String>()
                    .parse::<usize>()
                    .ok()
            })
        })
        .collect()
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn artifact_path(name: &str) -> PathBuf {
    std::fs::create_dir_all(ARTIFACT_DIR).expect("create artifact dir");
    Path::new(ARTIFACT_DIR).join(name)
}

fn write_cast(harness: &PtyHarness, name: &str) {
    harness
        .write_cast(&artifact_path(name))
        .unwrap_or_else(|error| panic!("write cast {name}: {error}"));
}

fn copy_log(source: &Path, name: &str) {
    if let Ok(contents) = std::fs::read(source) {
        std::fs::write(artifact_path(name), contents).expect("copy instrumentation log");
    }
}

struct SplitTurnDiagnostics<'a> {
    content: &'a ContentController,
    session: PathBuf,
}

impl Drop for SplitTurnDiagnostics<'_> {
    fn drop(&mut self) {
        preserve_split_turn_diagnostics(&self.session, self.content);
    }
}

fn write_split_usage_ledger(data: &PersistedData, shown: Option<&RecordedUsage>) {
    let shown = shown
        .map(|usage| format!("{usage:?}"))
        .unwrap_or_else(|| "screen usage not captured".to_owned());
    std::fs::write(
        artifact_path("usage-ledger.txt"),
        format!(
            "attempts={:?}\ncompleted={:?}\nshown={shown}\n",
            settled_attempts(&data.timeline_events),
            completed_turns(&data.updates),
        ),
    )
    .expect("write usage ledger");
}

fn preserve_split_turn_diagnostics(session: &Path, content: &ContentController) {
    if std::fs::create_dir_all(ARTIFACT_DIR).is_err() {
        return;
    }
    let root = Path::new(ARTIFACT_DIR);
    for name in ["timeline.jsonl", "updates.jsonl"] {
        if let Ok(bytes) = std::fs::read(session.join(name)) {
            let _ = std::fs::write(root.join(format!("usage-{name}")), bytes);
        }
    }
    copy_tree(&session.join("sidebands"), &root.join("usage-sidebands"));
    let mut lines = String::new();
    for (index, entry) in content.requests().iter().enumerate() {
        let req_id = entry.header("x-grow-req-id").unwrap_or("-");
        let turn = entry.header("x-grow-turn-idx").unwrap_or("-");
        lines.push_str(&format!(
            "{index} {} {} req={req_id} turn={turn}\n",
            entry.method, entry.path
        ));
    }
    let _ = std::fs::write(root.join("usage-requests.txt"), lines);
}

fn copy_tree(source: &Path, destination: &Path) {
    let Ok(entries) = std::fs::read_dir(source) else {
        return;
    };
    if std::fs::create_dir_all(destination).is_err() {
        return;
    }
    for entry in entries.flatten() {
        let path = entry.path();
        let target = destination.join(entry.file_name());
        if path.is_dir() {
            copy_tree(&path, &target);
        } else if let Ok(bytes) = std::fs::read(&path) {
            let _ = std::fs::write(target, bytes);
        }
    }
}
