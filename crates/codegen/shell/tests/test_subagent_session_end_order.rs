//! Tier-3 regression: a completed child ends through its own session actor.
//!
//! Under the old ordering the runner appended the child's canonical result
//! before requesting shutdown, so the child's SessionEnd hook lifecycle was
//! rejected by the closed-Timeline protection (`SubagentTimelineEnded`) and
//! never ran. This test asserts the fixed invariant end-to-end through the
//! real subagent runner: the child's SessionEnd hook executes and is recorded
//! exactly once before its canonical result, the result is the child
//! Timeline's last fact, the parent terminal references exactly that result,
//! and no child reports a non-durable session-end lifecycle.
//!
//! Requires a freshly built GROW_BINARY; no external provider is contacted:
//! ```bash
//! cargo test -p shell --test test_subagent_session_end_order -- --ignored
//! ```

use std::path::Path;

use acp_transport::protocol as acp;
use test_support::sse::chat_completions_reasoning_then_tool_call_events;
use test_support::*;

fn script_parent_todo(server: &MockInferenceServer, model: &str) -> InferenceExpectation {
    server.expect_response(
        "create-parent-todo-before-delegation",
        InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
        ScriptedResponse::sse(chat_completions_reasoning_then_tool_call_events(
            "Track the delegated task.",
            "call_parent_todo",
            "todo_write",
            &serde_json::json!({"todos": [{
                "id": "task-step", "content": "Complete the delegated task", "status": "in_progress"
            }]})
            .to_string(),
            model,
        )),
    )
}

fn script_spawn_child(server: &MockInferenceServer, model: &str) -> InferenceExpectation {
    let arguments = serde_json::json!({
        "parent_step_id": "task-step",
        "description": "Check child terminal order",
        "prompt": "Return a short completion message.",
        "subagent_type": "general-purpose",
        "background": false,
        "model": "mock/child",
    });
    server.expect_response(
        "spawn-child-for-terminal-order",
        InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
        ScriptedResponse::sse(chat_completions_reasoning_then_tool_call_events(
            "Delegating the task.",
            "call_child_terminal_order",
            "spawn_subagent",
            &arguments.to_string(),
            model,
        )),
    )
}

/// Locate the one session under `grow_home/sessions` whose Timeline carries a
/// canonical `SubagentResult` (the child); the parent never records one.
fn load_child_timeline(grow_home: &Path) -> (String, chat_state::Timeline) {
    let sessions_root = grow_home.join("sessions");
    let mut found: Vec<(String, chat_state::Timeline)> = Vec::new();
    for cwd_entry in std::fs::read_dir(&sessions_root)
        .expect("read sessions root")
        .flatten()
    {
        // Regular index files live next to per-cwd directories here; only
        // descend into directories, but still fail on genuine read errors.
        if !cwd_entry.file_type().expect("cwd entry file_type").is_dir() {
            continue;
        }
        for id_entry in std::fs::read_dir(cwd_entry.path())
            .expect("read sessions cwd dir")
            .flatten()
        {
            if !id_entry.path().join("timeline.jsonl").is_file() {
                continue;
            }
            let id = id_entry.file_name().to_string_lossy().into_owned();
            let Some(timeline) = shell::session::storage::load_timeline_by_id_at(&id, grow_home)
                .expect("candidate Timeline must load")
            else {
                continue;
            };
            if timeline.events().iter().any(|event| {
                matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::SubagentResult(_)
                )
            }) {
                found.push((id, timeline));
            }
        }
    }
    assert_eq!(
        found.len(),
        1,
        "exactly one child Timeline with a canonical result must exist"
    );
    found.pop().expect("checked length above")
}

#[tokio::test]
#[ignore = "requires a freshly built grow binary"]
async fn child_session_end_precedes_canonical_result_through_real_runner() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.set_response("Finished the requested work.");
            let workdir = git_workdir();
            let mut sandbox = TestSandbox::builder().mock_url(server.url()).build();
            let grow_home = sandbox.grow_home().to_path_buf();

            let state_dir = tempfile::TempDir::new().expect("create state dir");
            let marker = state_dir.path().join("session_end_runs.txt");
            let script_path = sandbox.home().join("session_end_order_hook.sh");
            std::fs::write(
                &script_path,
                format!(
                    "#!/bin/sh\n\
                     printf 'ended\\n' >> {marker}\n\
                     cat > /dev/null\n\
                     exit 0\n",
                    marker = marker.display(),
                ),
            )
            .expect("write hook script");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
                    .expect("chmod hook script");
            }
            let hooks_dir = grow_home.join("hooks");
            std::fs::create_dir_all(&hooks_dir).expect("write hooks dir");
            std::fs::write(
                hooks_dir.join("session_end.json"),
                serde_json::json!({
                    "hooks": {
                        "session_end": [{
                            "hooks": [{
                                "type": "command",
                                "command": format!("sh {}", script_path.display()),
                                "timeout": 30
                            }]
                        }]
                    }
                })
                .to_string(),
            )
            .expect("write hook config");

            let diagnostic_dir = tempfile::tempdir().expect("create diagnostic dir");
            let diagnostic_log = diagnostic_dir.path().join("grow-session-end-order.log");
            sandbox.set_env("GROW_LOG_FILE", &diagnostic_log);
            std::fs::write(
                grow_home.join("config.toml"),
                format!(
                    r#"[models]
default = "mock/parent"
[provider.mock]
api_backend = "chat_completions"
[provider.mock.options]
base_url = "{}"
env_key = "GROW_API_KEY"
[provider.mock.models.parent]
model = "parent-wire"
context_window = 200000
agent_type = "grow-build"
[provider.mock.models.child]
model = "child-wire"
context_window = 200000
agent_type = "grow-build"
"#,
                    server.url()
                ),
            )
            .expect("write mock provider config");

            let client =
                GrowStdioClient::spawn_with_sandbox(&server, workdir.workspace(), sandbox).await;
            client.initialize_with_timeout().await;
            let session_id = client
                .create_session_with_timeout(workdir.workspace())
                .await;
            let parent_todo = script_parent_todo(&server, "parent-wire");
            let spawn = script_spawn_child(&server, "parent-wire");
            let result = client
                .prompt_with_timeout(&session_id, "Delegate this small task.")
                .await
                .unwrap();
            assert_eq!(result.stop_reason, acp::StopReason::EndTurn);
            parent_todo.assert_satisfied();
            spawn.assert_satisfied();

            let (child_id, child_timeline) = load_child_timeline(&grow_home);
            let events = child_timeline.events();
            let result_positions: Vec<usize> = events
                .iter()
                .enumerate()
                .filter(|(_, event)| {
                    matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::SubagentResult(_)
                    )
                })
                .map(|(index, _)| index)
                .collect();
            assert_eq!(
                result_positions.len(),
                1,
                "the child result must be canonical exactly once"
            );
            let result_index = result_positions[0];
            let result_seq = events[result_index].seq.get();
            assert_eq!(
                result_index,
                events.len() - 1,
                "the child result must be the child Timeline's last fact"
            );

            let mut session_end_occurrences = events.iter().enumerate().filter(|(_, event)| {
                matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                        event: chat_state::HookEventType::SessionEnd,
                        ..
                    })
                )
            });
            let (hook_index, hook_event) = session_end_occurrences
                .next()
                .expect("the child SessionEnd occurrence must be recorded");
            assert!(
                session_end_occurrences.next().is_none(),
                "the child SessionEnd must be recorded exactly once"
            );
            assert!(
                hook_index < result_index,
                "the child SessionEnd must precede its canonical result"
            );
            let occurrence_id = match &hook_event.kind {
                chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                    occurrence_id,
                    ..
                }) => occurrence_id.clone(),
                _ => unreachable!("filtered above"),
            };
            let completed_index = events
                .iter()
                .position(|event| {
                    matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Hook(
                            chat_state::HookEvent::Completed {
                                occurrence_id: completed_id,
                                ..
                            }
                        ) if completed_id == &occurrence_id
                    )
                })
                .expect("the child SessionEnd occurrence must complete");
            assert!(
                completed_index < result_index,
                "the child SessionEnd aggregate must precede its canonical result"
            );

            let parent_timeline =
                shell::session::storage::load_timeline_by_id_at(session_id.0.as_ref(), &grow_home)
                    .expect("read parent Timeline")
                    .expect("parent session exists");
            let terminals: Vec<_> = parent_timeline
                .events()
                .iter()
                .filter(|event| {
                    matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(
                            _
                        ))
                    )
                })
                .collect();
            assert_eq!(
                terminals.len(),
                1,
                "the parent terminal must be committed exactly once"
            );
            match &terminals[0].kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(
                    terminal,
                )) => {
                    let range = terminal
                        .result_ref
                        .as_ref()
                        .expect("the parent terminal must reference the child result");
                    assert_eq!(range.timeline_id, child_id);
                    assert_eq!(range.first_seq, result_seq);
                    assert_eq!(range.last_seq, result_seq);
                }
                _ => unreachable!("filtered above"),
            }

            let marker_text = std::fs::read_to_string(&marker).unwrap_or_default();
            assert!(
                marker_text
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .count()
                    >= 1,
                "the child SessionEnd hook must execute externally; marker: {marker_text:?}"
            );
            let log = std::fs::read_to_string(&diagnostic_log).unwrap_or_default();
            assert!(
                !log.contains("session-end hook lifecycle was not durable"),
                "the child session-end lifecycle must be durable:\n{log}"
            );
        })
        .await;
}

/// The cancellation scenario needs two isolated providers: the parent talks
/// to a scripted mock, the child's model is a *different* mock server whose
/// foreground terminal events are held so the child stays genuinely live in
/// its provider stream until the parent cancels it. Holding the child server
/// does not hold the parent's turns.
struct SplitProviders {
    parent: MockInferenceServer,
    child: MockInferenceServer,
}

impl SplitProviders {
    async fn start() -> Self {
        let parent = MockInferenceServer::start().await.unwrap();
        let child = MockInferenceServer::start().await.unwrap();
        assert_ne!(parent.url(), child.url());
        child.hold_agent_completions();
        Self { parent, child }
    }

    fn write_config(&self, grow_home: &Path) {
        std::fs::write(
            grow_home.join("config.toml"),
            format!(
                r#"[models]
default = "mock/parent"
[provider.mock]
api_backend = "chat_completions"
[provider.mock.options]
base_url = "{}"
env_key = "GROW_API_KEY"
[provider.mock.models.parent]
model = "parent-wire"
context_window = 200000
agent_type = "grow-build"
[provider.child]
api_backend = "chat_completions"
[provider.child.options]
base_url = "{}"
env_key = "GROW_API_KEY"
[provider.child.models.live]
model = "child-wire"
context_window = 200000
agent_type = "grow-build"
"#,
                self.parent.url(),
                self.child.url()
            ),
        )
        .expect("write split mock provider config");
    }
}

/// Install the same session_end hook shape as the completion test. The
/// returned `TempDir` keeps the marker's directory alive; the hook appends
/// one line per execution to `marker`.
fn install_session_end_hook(grow_home: &Path) -> (tempfile::TempDir, std::path::PathBuf) {
    let state_dir = tempfile::TempDir::new().expect("create state dir");
    let marker = state_dir.path().join("session_end_runs.txt");
    let script_path = grow_home.join("session_end_order_hook.sh");
    std::fs::write(
        &script_path,
        format!(
            "#!/bin/sh\n\
             printf 'ended\\n' >> {marker}\n\
             cat > /dev/null\n\
             exit 0\n",
            marker = marker.display(),
        ),
    )
    .expect("write hook script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod hook script");
    }
    let hooks_dir = grow_home.join("hooks");
    std::fs::create_dir_all(&hooks_dir).expect("write hooks dir");
    std::fs::write(
        hooks_dir.join("session_end.json"),
        serde_json::json!({
            "hooks": {
                "session_end": [{
                    "hooks": [{
                        "type": "command",
                        "command": format!("sh {}", script_path.display()),
                        "timeout": 30
                    }]
                }]
            }
        })
        .to_string(),
    )
    .expect("write hook config");
    (state_dir, marker)
}

/// Bounded poll; refuses to pass on a timeout instead of relying on sleeps.
async fn wait_until(description: &str, mut check: impl FnMut() -> bool) {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        while !check() {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for: {description}"));
}

/// The tool-result text one provider request carried back for `call_id`.
fn tool_result_text(server: &MockInferenceServer, call_id: &str) -> String {
    server
        .request_bodies()
        .iter()
        .filter_map(|body| body["messages"].as_array())
        .flatten()
        .filter(|message| message["role"] == "tool" && message["tool_call_id"] == call_id)
        .find_map(|message| message["content"].as_str())
        .unwrap_or_else(|| {
            panic!(
                "missing real tool result for {call_id}: {}",
                server.request_log_summary()
            )
        })
        .to_owned()
}

#[tokio::test]
#[ignore = "requires a freshly built grow binary"]
async fn cancelled_child_session_end_precedes_cancelled_result_through_real_runner() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let providers = SplitProviders::start().await;
            let workdir = git_workdir();
            let mut sandbox = TestSandbox::builder()
                .mock_url(providers.parent.url())
                .build();
            let grow_home = sandbox.grow_home().to_path_buf();
            providers.write_config(&grow_home);
            let (_marker_dir, marker) = install_session_end_hook(&grow_home);

            let diagnostic_dir = tempfile::tempdir().expect("create diagnostic dir");
            let diagnostic_log = diagnostic_dir.path().join("grow-session-end-cancel.log");
            sandbox.set_env("GROW_LOG_FILE", &diagnostic_log);

            let client =
                GrowStdioClient::spawn_with_sandbox(&providers.parent, workdir.workspace(), sandbox)
                    .await;
            client.initialize_with_timeout().await;
            let session_id = client
                .create_session_with_timeout(workdir.workspace())
                .await;

            // Turn 1: parent todo + background spawn of a live child on the
            // held child provider. The turn after the tool result ends with
            // the parent's fixed fallback response.
            providers.parent.set_response("Delegated a live child.");
            let parent_todo = script_parent_todo(&providers.parent, "parent-wire");
            let spawn_arguments = serde_json::json!({
                "parent_step_id": "task-step",
                "description": "Own a live child that the parent cancels",
                "prompt": "Stay in your provider stream until the parent cancels you.",
                "subagent_type": "general-purpose",
                "background": true,
                "model": "child/live",
            });
            let spawn = providers.parent.expect_response(
                "spawn-live-child-for-cancellation",
                InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
                ScriptedResponse::sse(chat_completions_reasoning_then_tool_call_events(
                    "Delegating a live child.",
                    "call_spawn_cancel",
                    "spawn_subagent",
                    &spawn_arguments.to_string(),
                    "parent-wire",
                )),
            );
            let result = client
                .prompt_with_timeout(&session_id, "Delegate a live child for cancellation.")
                .await
                .unwrap();
            assert_eq!(result.stop_reason, acp::StopReason::EndTurn);
            parent_todo.assert_satisfied();
            spawn.assert_satisfied();

            // The background spawn tool result carries the real child ID; no
            // hard-coded ID and no assumption the child already finished.
            let child_id = tool_result_text(&providers.parent, "call_spawn_cancel")
                .lines()
                .find_map(|line| line.trim().strip_prefix("subagent_id: "))
                .expect("background spawn must return the actual child ID")
                .to_owned();

            // Wait until the child is genuinely live: its own provider has a
            // chat-completion request, its Timeline exists, and it has not
            // produced a canonical result yet.
            wait_until("the child must enter its provider stream", || {
                providers.child.has_chat_completion_request()
            })
            .await;
            wait_until("the child Timeline must be persisted", || {
                shell::session::storage::load_timeline_by_id_at(&child_id, &grow_home)
                    .ok()
                    .flatten()
                    .is_some()
            })
            .await;
            let child_before = shell::session::storage::load_timeline_by_id_at(&child_id, &grow_home)
                .expect("candidate Timeline must load")
                .expect("the child Timeline exists");
            assert!(
                !child_before.events().iter().any(|event| {
                    matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::SubagentResult(_)
                    )
                }),
                "the child must be live without a canonical result when cancelled"
            );

            // Turn 2: the parent itself cancels the live child through the
            // real runner and waits for the actual terminal delivery.
            providers
                .parent
                .set_response("Cancelled the child and observed its terminal delivery.");
            let kill = providers.parent.expect_response(
                "cancel-live-child",
                InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
                ScriptedResponse::sse(chat_completions_reasoning_then_tool_call_events(
                    "Cancelling the live child.",
                    "call_kill_child",
                    "kill_command_or_subagent",
                    &serde_json::json!({ "task_id": child_id }).to_string(),
                    "parent-wire",
                )),
            );
            let wait = providers.parent.expect_response(
                "wait-cancelled-child",
                InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
                ScriptedResponse::sse(chat_completions_reasoning_then_tool_call_events(
                    "Waiting for the cancelled child.",
                    "call_wait_child",
                    "wait_subagent",
                    &serde_json::json!({
                        "subagent_ids": [child_id],
                        "timeout_ms": 30000
                    })
                    .to_string(),
                    "parent-wire",
                )),
            );
            let result = client
                .prompt_with_timeout(&session_id, "Cancel the delegated child and wait for it.")
                .await
                .unwrap();
            assert_eq!(result.stop_reason, acp::StopReason::EndTurn);
            kill.assert_satisfied();
            wait.assert_satisfied();
            let waited: serde_json::Value =
                serde_json::from_str(&tool_result_text(&providers.parent, "call_wait_child"))
                    .expect("wait_subagent returns JSON");
            assert_eq!(
                waited["reason"], "all_finished",
                "wait must observe the real terminal delivery: {waited}"
            );
            assert_eq!(waited["subagents"][0]["subagent_id"], child_id);
            assert_eq!(
                waited["subagents"][0]["status"], "cancelled",
                "the runner must deliver a cancelled terminal: {waited}"
            );

            // Child Timeline: one cancelled canonical result as the last
            // fact, preceded exactly once by its own SessionEnd lifecycle.
            let (child_timeline_id, child_timeline) = load_child_timeline(&grow_home);
            assert_eq!(child_timeline_id, child_id);
            let events = child_timeline.events();
            let result_positions: Vec<usize> = events
                .iter()
                .enumerate()
                .filter(|(_, event)| {
                    matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::SubagentResult(_)
                    )
                })
                .map(|(index, _)| index)
                .collect();
            assert_eq!(
                result_positions.len(),
                1,
                "the cancelled child result must be canonical exactly once"
            );
            let result_index = result_positions[0];
            let result_seq = events[result_index].seq.get();
            assert_eq!(
                result_index,
                events.len() - 1,
                "the cancelled result must be the child Timeline's last fact"
            );
            match &events[result_index].kind {
                chat_state::TimelineEventKind::SubagentResult(result) => {
                    assert_eq!(result.subagent_id, child_id);
                    assert_eq!(
                        result.outcome,
                        chat_state::SubagentOutcome::Cancelled,
                        "the child result must carry the cancelled outcome"
                    );
                }
                _ => unreachable!("filtered above"),
            }

            let mut session_end_occurrences = events.iter().enumerate().filter(|(_, event)| {
                matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                        event: chat_state::HookEventType::SessionEnd,
                        ..
                    })
                )
            });
            let (hook_index, hook_event) = session_end_occurrences
                .next()
                .expect("the cancelled child SessionEnd occurrence must be recorded");
            assert!(
                session_end_occurrences.next().is_none(),
                "the cancelled child SessionEnd must be recorded exactly once"
            );
            assert!(
                hook_index < result_index,
                "the cancelled child SessionEnd must precede its canonical result"
            );
            let occurrence_id = match &hook_event.kind {
                chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Triggered {
                    occurrence_id,
                    ..
                }) => occurrence_id.clone(),
                _ => unreachable!("filtered above"),
            };
            let completed_index = events
                .iter()
                .position(|event| {
                    matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Hook(
                            chat_state::HookEvent::Completed {
                                occurrence_id: completed_id,
                                ..
                            }
                        ) if completed_id == &occurrence_id
                    )
                })
                .expect("the cancelled child SessionEnd occurrence must complete");
            assert!(
                completed_index < result_index,
                "the cancelled child SessionEnd aggregate must precede its canonical result"
            );

            // Parent Timeline: exactly one Ended terminal, cancelled, with an
            // exact reference to the child's cancelled result.
            let parent_timeline =
                shell::session::storage::load_timeline_by_id_at(session_id.0.as_ref(), &grow_home)
                    .expect("read parent Timeline")
                    .expect("parent session exists");
            let terminals: Vec<_> = parent_timeline
                .events()
                .iter()
                .filter(|event| {
                    matches!(
                        &event.kind,
                        chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(
                            _
                        ))
                    )
                })
                .collect();
            assert_eq!(
                terminals.len(),
                1,
                "the parent terminal must be committed exactly once"
            );
            match &terminals[0].kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(
                    terminal,
                )) => {
                    assert_eq!(terminal.subagent_id, child_id);
                    assert_eq!(
                        terminal.outcome,
                        chat_state::SubagentOutcome::Cancelled,
                        "the parent terminal must carry the cancelled outcome"
                    );
                    let range = terminal
                        .result_ref
                        .as_ref()
                        .expect("the parent terminal must reference the cancelled child result");
                    assert_eq!(range.timeline_id, child_id);
                    assert_eq!(range.first_seq, result_seq);
                    assert_eq!(range.last_seq, result_seq);
                }
                _ => unreachable!("filtered above"),
            }

            // External evidence: the hook actually executed and no
            // closed-Timeline rejection was logged for the child.
            let marker_text = std::fs::read_to_string(&marker).unwrap_or_default();
            assert!(
                marker_text
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .count()
                    >= 1,
                "the cancelled child SessionEnd hook must execute externally; marker: {marker_text:?}"
            );
            let log = std::fs::read_to_string(&diagnostic_log).unwrap_or_default();
            assert!(
                !log.contains("session-end hook lifecycle was not durable")
                    && !log.contains("SubagentTimelineEnded"),
                "the cancelled child session-end lifecycle must be durable:\n{log}"
            );
        })
        .await;
}
