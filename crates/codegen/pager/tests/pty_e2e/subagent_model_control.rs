//! One real parent tool → Shell coordinator → child actor → child view chain for
//! `set_subagent_model`.
//!
//! A real background `spawn_subagent` produces the child from an actual parent
//! TODO. The child's first provider Step is held at its terminal barrier while
//! the parent queues the sampling control; the parent row may therefore only
//! show the typed `queued` admission. Releasing that Step closes it, the child
//! applies the control at the closed-Step boundary, and its next provider
//! request carries the requested model/effort.
//!
//! Transient `Dispatching`, rejection, no-op, supersede and durable-failure
//! matrices stay in the Pager tracker and Shell actor module tests. This case
//! only proves the first real display chain from the verification plan.
#![cfg(unix)]

use super::common::*;
use chat_state::{SubagentEvent, Timeline, TimelineEventKind, ToolEvent};
use pager_pty_harness::{InferenceEndpoint as Endpoint, InferenceRequestMatcher as Matcher};
use serde_json::Value;
use shell::extensions::notification::{
    ControlDomain, ControlPhase, ControlStateUpdate, ControlTarget, SessionUpdate as GrowUpdate,
};

const PARENT_PROMPT: &str = "SUBAGENT_SAMPLING_CONTROL_PARENT";
const SWITCH_PROMPT: &str = "SUBAGENT_SAMPLING_CONTROL_SWITCH";
const PARENT_STEP_ID: &str = "parent-sampling-control-step";
const SPAWN_CALL: &str = "call_parent_spawn_sampling_child";
const SET_CALL: &str = "call_parent_set_subagent_sampling";
const PARENT_SETTLED: &str = "PARENT_INITIAL_TURN_SETTLED";
const PARENT_QUEUED: &str = "PARENT_CONTROL_QUEUED";
const PARENT_CHILD_COMPLETED: &str = "PARENT_CHILD_COMPLETED";
const CHILD_FIRST_TOOL: &str = "call_child_first_step";
const CHILD_AFTER_SWITCH: &str = "CHILD_AFTER_SWITCH";
const CHILD_DESCRIPTION: &str = "Hold one blocked Step for sampling control";
const CHILD_TASK_PROMPT: &str = "Hold your first provider Step until the parent releases it.";
const TARGET_MODEL: &str = "mock/target";
const TARGET_EFFORT: &str = "high";
const SWITCH_NOTICE: &str = "Sampling switched to mock/target (high)";
const PENDING_TRANSITION: &str = "model child→target (high)";
const WAIT: Duration = Duration::from_secs(60);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "real PTY; requires a parent-designated freshly built PAGER_BINARY"]
async fn parent_set_subagent_model_queues_then_child_applies_at_its_closed_step() {
    use serde_json::json;

    let content = ContentController::start_with_models(vec![
        MockModel::new("parent-wire-model"),
        MockModel::new("child-wire-model"),
        MockModel::new("target-wire-model"),
    ])
    .await
    .expect("loopback mock provider");
    content
        .seed_llm_config()
        .expect("seed isolated mock provider");
    // Explicit catalog: parent and child take separate wire models/endpoints so
    // concurrent requests cannot be mistaken for one another, and the target
    // model declares the canonical effort the control may request.
    std::fs::write(
        content.home().join(".grow/config.toml"),
        format!(
            r#"[models]
default = "mock/parent"

[provider.mock.options]
base_url = "{}"

[provider.mock.models.parent]
model = "parent-wire-model"
api_backend = "responses"
context_window = 128000

[provider.mock.models.child]
model = "child-wire-model"
api_backend = "chat_completions"
context_window = 128000

[provider.mock.models.target]
model = "target-wire-model"
api_backend = "chat_completions"
context_window = 128000
reasoning_efforts = [{{ value = "high", label = "High" }}]

[subagents]
enabled = true
permission_mode = "always-approve"
"#,
            content.url()
        ),
    )
    .expect("write isolated provider and subagent config");

    let project = tempfile::tempdir().expect("isolated project directory");
    git2::Repository::init(project.path()).expect("initialize isolated project");

    // The parent delegates through its real TODO; the child is a real
    // background subagent on its own wire model and endpoint.
    let _parent_todo = content.expect_response(
        "parent creates the delegation TODO",
        Matcher::foreground(Endpoint::Responses),
        ScriptedResponse::sse(sse::responses_api_reasoning_then_tool_call_events(
            "Track the delegated child",
            "call_parent_sampling_todo",
            "todo_write",
            &json!({"todos": [{
                "id": PARENT_STEP_ID,
                "content": "Track the delegated child sampling control",
                "status": "in_progress"
            }]})
            .to_string(),
            "parent-wire-model",
        )),
    );
    let _parent_spawn = content.expect_response(
        "parent delegates one background child",
        Matcher::foreground(Endpoint::Responses),
        ScriptedResponse::sse(sse::responses_api_reasoning_then_tool_call_events(
            "Delegate the blocked child",
            SPAWN_CALL,
            "spawn_subagent",
            &json!({
                "parent_step_id": PARENT_STEP_ID,
                "prompt": CHILD_TASK_PROMPT,
                "description": CHILD_DESCRIPTION,
                "subagent_type": "general-purpose",
                "background": true,
                "capability_mode": "all",
                "model": "mock/child"
            })
            .to_string(),
            "parent-wire-model",
        )),
    );
    // The child's first provider Step never crosses its terminal event while
    // the control is being queued: the Step stays open, so the selection can
    // only apply later, at the closed-Step boundary.
    // ScriptedResponse gates its last SSE event. The sampler closes on
    // finish_reason, so gate that event rather than a later [DONE] trailer.
    let mut first_step_events = chat_completions_tool_call_events_with_id(
        CHILD_FIRST_TOOL,
        "run_terminal_command",
        &json!({
            "command": "printf 'child first step closed\\n'",
            "description": "Close the child's first Step"
        })
        .to_string(),
    );
    assert_eq!(
        first_step_events
            .pop()
            .expect("Chat Completions trailer")
            .data,
        "[DONE]"
    );
    let mut child_first_step = content.expect_response_blocked(
        "child first provider Step stays open",
        Matcher::foreground(Endpoint::ChatCompletions),
        ScriptedResponse::sse(first_step_events),
    );
    let _parent_settled = content.expect_response(
        "parent initial Turn settles",
        Matcher::foreground(Endpoint::Responses),
        ScriptedResponse::sse(sse::responses_api_script_exact(
            PARENT_SETTLED,
            "parent-wire-model",
        )),
    );

    let binary = designated_binary();
    let diagnostic_log = std::env::var_os("GROW_SUBAGENT_CONTROL_EVIDENCE_DIR")
        .map(|directory| PathBuf::from(directory).join("cli-diagnostics.log"));
    let mut environment = Vec::new();
    if let Some(log) = diagnostic_log.as_ref() {
        environment.push(EnvOp::set_os(
            std::ffi::OsStr::new("GROW_LOG_FILE"),
            log.as_os_str(),
        ));
        environment.push(EnvOp::set(
            "RUST_LOG",
            "warn,pager::app::acp_handler::session_notification=debug,pager::app::session=debug,acp_transport::gateway=debug",
        ));
    }
    let mut pager = PtyHarness::spawn_with_content_env_ops_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &["--permission-mode", "always-approve", "--trust"],
        &environment,
        Some(project.path()),
    )
    .expect("spawn real isolated Pager");
    pager
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome screen");
    pager
        .inject_keys(format!("{PARENT_PROMPT}\r").as_bytes())
        .expect("submit the parent prompt");

    tokio::time::timeout(WAIT, child_first_step.wait_blocked())
        .await
        .expect("the child's first provider Step must reach its open barrier");
    pager
        .wait_for_text(PARENT_SETTLED, WAIT)
        .expect("parent initial Turn settles while the child Step stays open");
    // Locate the root before waiting for its terminal. The generic whole-screen
    // idle helper also matches the deliberately busy child's response label.
    let root_dir = session_dir_for_call(&content, SPAWN_CALL);
    let root_timeline = root_dir.join("timeline.jsonl");
    wait_for_parent_turn_end(&mut pager, &root_timeline, PARENT_PROMPT);

    // Only the actual spawn fact is trusted for the child identity.
    let root_before = std::fs::read_to_string(&root_timeline).expect("root Timeline");
    let spawn = spawn_for_tool_call(&root_before, SPAWN_CALL);
    let child_session_id = spawn.child_session_id;
    let child_id = spawn.subagent_id;
    assert_eq!(spawn.parent_step_id.as_deref(), Some(PARENT_STEP_ID));
    assert_eq!(spawn.effective_model_id, "mock/child");
    let parent_model_changes = model_change_count(&root_before);
    // A fresh child inherits this parent's cwd. handle_request creates its
    // SessionInfo with the spawned child session id and persists it through
    // session_dir, alongside the root in sessions/{cwd}/{session_id}.
    let child_dir = root_dir
        .parent()
        .expect("root cwd session directory")
        .join(&child_session_id);
    let child_summary = shell::session::persistence::read_summary_in_session_dir(&child_dir)
        .expect("typed summary for the actual child session");
    assert_eq!(child_summary.info.id.0.as_ref(), child_session_id);
    assert_eq!(child_summary.info.cwd, spawn.child_cwd);
    assert_eq!(
        Path::new(&child_summary.info.cwd)
            .canonicalize()
            .expect("canonical child cwd"),
        project
            .path()
            .canonicalize()
            .expect("canonical project cwd")
    );
    assert!(
        child_dir.join("timeline.jsonl").is_file(),
        "actual child journal"
    );

    // The next parent Turn is scripted only now that the real id is known.
    let _parent_control = content.expect_response(
        "parent queues the child sampling control",
        Matcher::foreground(Endpoint::Responses),
        ScriptedResponse::sse(sse::responses_api_reasoning_then_tool_call_events(
            "Switch the child's sampling",
            SET_CALL,
            "set_subagent_model",
            &json!({
                "subagent_id": child_id,
                "model": TARGET_MODEL,
                "effort": TARGET_EFFORT
            })
            .to_string(),
            "parent-wire-model",
        )),
    );
    let _parent_queued = content.expect_response(
        "parent records the queued admission",
        Matcher::foreground(Endpoint::Responses),
        ScriptedResponse::sse(sse::responses_api_script_exact(
            PARENT_QUEUED,
            "parent-wire-model",
        )),
    );
    pager
        .inject_keys(format!("{SWITCH_PROMPT}\r").as_bytes())
        .expect("submit the switch prompt");
    pager
        .wait_for_text(PARENT_QUEUED, WAIT)
        .expect("parent Turn after the queued tool result");
    wait_for_parent_turn_end(&mut pager, &root_timeline, SWITCH_PROMPT);

    let parent_collapsed = pager.screen_contents();
    save_evidence("parent-group-collapsed.txt", &parent_collapsed);
    let root_control = std::fs::read_to_string(&root_timeline).expect("root control Timeline");
    let child_control =
        std::fs::read_to_string(child_dir.join("timeline.jsonl")).expect("child control Timeline");
    parsed_timeline(&root_control);
    parsed_timeline(&child_control);
    save_evidence("parent-control-canonical.jsonl", &root_control);
    save_evidence("child-control-canonical.jsonl", &child_control);
    save_evidence(
        "parent-control-events.json",
        &serde_json::to_string_pretty(&completed_tools(&root_control, SET_CALL))
            .expect("serialize exact typed control completion"),
    );
    assert_single_queued_result(&root_control, &child_id);

    // Completed members follow the ordinary activity-group fold contract.
    // Reveal this group's real row through the existing user interaction.
    pager.inject_keys(b"\t").expect("focus parent scrollback");
    pager
        .wait_for_text("Ctrl+e:", WAIT)
        .expect("parent scrollback owns the group interaction");
    let (row, col) = locate_screen_text(&pager.screen_contents(), "Ran 1 tool")
        .expect("the queued control's activity group");
    let expand = format!(
        "{}{}{}{}",
        sgr_mouse(0, row, col, 'M'),
        sgr_mouse(0, row, col, 'm'),
        sgr_mouse(0, row, col, 'M'),
        sgr_mouse(0, row, col, 'm')
    );
    pager
        .inject_keys(expand.as_bytes())
        .expect("double-click the control activity group");
    pager
        .wait_for_text("Queued · model mock/target · effort high", WAIT)
        .expect("expanded group reveals the queued control row");
    let parent_screen = pager.screen_contents();
    save_evidence("parent-queued.txt", &parent_screen);
    assert_eq!(
        occurrences(&parent_screen, "set_subagent_model → subagent"),
        1,
        "the control keeps one parent row\n{parent_screen}"
    );
    assert!(
        parent_screen.contains("Queued · model mock/target · effort high"),
        "the parent row shows the typed admission, not an application\n{parent_screen}"
    );
    assert!(
        !parent_screen.contains("Applied"),
        "queued admission must never render as applied\n{parent_screen}"
    );
    assert!(
        !parent_screen.contains("Sampling switched to"),
        "the parent view must not claim the child's switch\n{parent_screen}"
    );
    assert_eq!(
        model_change_count(&std::fs::read_to_string(&root_timeline).expect("root Timeline")),
        parent_model_changes,
        "the root selection must not change"
    );
    assert!(
        request_models(&content, "/v1/responses")
            .iter()
            .all(|model| model == "parent-wire-model"),
        "every parent request keeps the parent wire model"
    );

    // The Tasks pane exposes the same real active child even when its spawn
    // activity belongs to the previous parent Turn. Use its normal view button.
    let task_row = locate_screen_text(&parent_screen, "Hold one blocked Step")
        .expect("the actual spawned child in Tasks")
        .0;
    assert_eq!(occurrences(&parent_screen, "[↗]"), 1, "one active child");
    let (row, col) =
        locate_screen_text(&parent_screen, "[↗]").expect("the actual child's Tasks view button");
    assert_eq!(row, task_row, "view button belongs to the spawned child");
    let open = format!(
        "{}{}",
        sgr_mouse(0, row, col + 1, 'M'),
        sgr_mouse(0, row, col + 1, 'm')
    );
    pager
        .inject_keys(open.as_bytes())
        .expect("open the actual child through its Tasks view button");
    pager
        .wait_for_text(PENDING_TRANSITION, WAIT)
        .unwrap_or_else(|error| {
            save_evidence("child-pending-failed.txt", &pager.screen_contents());
            panic!("child view shows the pending control: {error}");
        });
    save_evidence("child-pending.txt", &pager.screen_contents());
    assert!(
        pager.contains_text("mock/child"),
        "the child view keeps its original selection\n{}",
        pager.screen_contents()
    );
    let child_before =
        std::fs::read_to_string(child_dir.join("timeline.jsonl")).expect("child Timeline");
    let original_selection = model_changes(&child_before)
        .pop()
        .expect("child's canonical initial model selection");
    assert_eq!(
        original_selection["to_model_id"].as_str(),
        Some("mock/child")
    );
    assert!(original_selection["to_reasoning_effort"].is_null());
    assert_eq!(
        switches_to(&child_before, TARGET_MODEL),
        0,
        "the child selection must not change while its Step stays open"
    );

    // Release the child Step; its tool batch closes, the control applies, and
    // the next provider Step uses the target selection.
    let _child_next = content.expect_response(
        "child next provider Step uses the target selection",
        Matcher::foreground(Endpoint::ChatCompletions),
        ScriptedResponse::sse(sse::chat_completion_script_exact(
            CHILD_AFTER_SWITCH,
            "target-wire-model",
        )),
    );
    let _parent_child_completed = content.expect_response(
        "parent receives the completed child notification",
        Matcher::foreground(Endpoint::Responses),
        ScriptedResponse::sse(sse::responses_api_script_exact(
            PARENT_CHILD_COMPLETED,
            "parent-wire-model",
        )),
    );
    child_first_step.release();
    pager
        .wait_for_text(CHILD_AFTER_SWITCH, WAIT)
        .expect("child continues on its next provider Step");
    pager
        .wait_for_text(SWITCH_NOTICE, WAIT)
        .expect("one ordinary Sampling control notice in the child view");
    let child_screen = pager.screen_contents();
    save_evidence("child-applied.txt", &child_screen);
    assert_eq!(
        occurrences(&child_screen, SWITCH_NOTICE),
        1,
        "exactly one visible switch notice\n{child_screen}"
    );

    assert_eq!(
        request_models(&content, "/v1/chat/completions"),
        vec![
            "child-wire-model".to_owned(),
            "target-wire-model".to_owned()
        ],
        "the blocked Step keeps the child model and only the next Step uses the target"
    );
    let switched_request = content
        .requests()
        .into_iter()
        .filter(|entry| entry.path == "/v1/chat/completions")
        .filter_map(|entry| entry.body)
        .find(|body| body["model"] == "target-wire-model")
        .expect("child request after the switch");
    assert_eq!(
        switched_request["reasoning_effort"].as_str(),
        Some(TARGET_EFFORT),
        "the next child request carries the requested effort: {switched_request}"
    );

    // Canonical child evidence: exactly one durable model.changed for the new
    // target and one durable Applied Sampling receipt in the same host intent.
    let child_after =
        std::fs::read_to_string(child_dir.join("timeline.jsonl")).expect("child Timeline");
    let switches = switches_to(&child_after, TARGET_MODEL);
    assert_eq!(
        switches, 1,
        "one durable model.changed for the applied target"
    );
    let switch_data = model_changes(&child_after)
        .into_iter()
        .find(|data| data["to_model_id"].as_str() == Some(TARGET_MODEL))
        .expect("model.changed payload for the target");
    let switch_intent: shell::session::ControlIntent =
        serde_json::from_value(switch_data["control_intent"].clone())
            .expect("canonical typed host-owned control intent");
    assert_eq!(switch_intent.generation, 0);
    assert_eq!(
        switch_intent.sequence, 0,
        "the first host operation sequence is legal"
    );
    assert!(
        switch_intent.client_id.starts_with("native-sampling:"),
        "tool control uses the actor's host namespace: {switch_intent:?}"
    );
    assert_eq!(
        switch_data["to_reasoning_effort"].as_str(),
        Some(TARGET_EFFORT)
    );

    // Replay through the production checksum/history projector. Control
    // terminal receipts live in the unified journal, never updates.jsonl.
    let receipts = control_receipts(&content, &child_session_id);
    let applied: Vec<_> = receipts
        .iter()
        .filter(|update| update.phase == ControlPhase::Applied)
        .collect();
    assert_eq!(
        applied.len(),
        1,
        "one durable Applied Sampling receipt: {receipts:#?}"
    );
    let applied = applied[0];
    assert_eq!(applied.domain, ControlDomain::Sampling);
    let target = ControlTarget::Sampling {
        model_id: TARGET_MODEL.into(),
        reasoning_effort: Some(TARGET_EFFORT.into()),
    };
    assert_eq!(applied.current, target);
    assert_eq!(applied.desired.as_ref(), Some(&target));
    assert_eq!(applied.message.as_deref(), Some(SWITCH_NOTICE));
    assert_eq!(applied.intent.as_ref(), Some(&switch_intent));
    assert!(!applied.snapshot, "terminal receipt is not hydration");

    // The parent's single terminal result reports admission for the same id.
    let root_after = std::fs::read_to_string(&root_timeline).expect("root Timeline");
    let results = completed_tools(&root_after, SET_CALL);
    assert_eq!(
        results.len(),
        1,
        "one parent tool result for the control call"
    );
    assert_single_queued_result(&root_after, &child_id);
    assert_eq!(
        model_change_count(&root_after),
        parent_model_changes,
        "the root selection must stay unchanged"
    );

    save_evidence("parent-canonical.jsonl", &root_after);
    save_evidence("child-pending-canonical.jsonl", &child_before);
    save_evidence("child-applied-canonical.jsonl", &child_after);
    save_evidence(
        "control-replay.json",
        &serde_json::to_string_pretty(&receipts).expect("serialize verified replay receipts"),
    );
    let wire_summary: Vec<_> = content
        .requests()
        .into_iter()
        .filter(|entry| entry.path == "/v1/responses" || entry.path == "/v1/chat/completions")
        .map(|entry| {
            let body = entry.body.expect("provider JSON body");
            json!({
                "path": entry.path,
                "model": body["model"].as_str().expect("provider wire model"),
                "reasoning_effort": body.get("reasoning_effort"),
            })
        })
        .collect();
    save_evidence(
        "verified-summary.json",
        &serde_json::to_string_pretty(&json!({
            "child_id": child_id,
            "child_session_id": child_session_id,
            "parent_model_changes_before": parent_model_changes,
            "parent_model_changes_after": model_change_count(&root_after),
            "parent_control_completions": results.len(),
            "target_model_changes": switches,
            "applied_receipts": 1,
            "switch_intent": switch_intent,
            "wire_requests": wire_summary,
        }))
        .expect("serialize verified summary"),
    );

    // Global Ctrl-Q closes the process from a child view. PtyHarness::quit
    // sends plain q (which can just close the child) and may fall back to kill.
    pager.inject_keys(b"\x11").expect("arm global quit");
    pager.update(Duration::from_millis(100));
    pager.inject_keys(b"\x11").expect("confirm global quit");
    assert_eq!(
        pager
            .wait_for_exit_and_drain(Duration::from_secs(15), Duration::from_millis(100))
            .expect("clean process exit from child view"),
        0
    );
    save_evidence("process-exit.txt", "0\n");
}

fn wait_for_parent_turn_end(pager: &mut PtyHarness, timeline_path: &Path, prompt: &str) {
    pager
        .wait_until(
            "canonical parent Turn ended and prompt idle",
            WAIT,
            |harness| {
                let contents = std::fs::read_to_string(timeline_path).expect("parent Timeline");
                let timeline = parsed_timeline(&contents);
                let turn = timeline
                    .events()
                    .iter()
                    .find_map(|event| match &event.kind {
                        TimelineEventKind::Turn(chat_state::TurnEvent::Started {
                            id,
                            prompt_text,
                            ..
                        }) if prompt_text == prompt => Some(id),
                        _ => None,
                    });
                let ended = timeline.events().iter().any(|event| {
                    matches!(&event.kind,
                    TimelineEventKind::Turn(chat_state::TurnEvent::Ended { id, outcome, .. })
                    if Some(id) == turn && outcome == "completed")
                });
                ended && !harness.contains_text("Ctrl+c:cancel")
            },
        )
        .expect("parent Turn settles while its child Step stays open");
}

fn save_evidence(name: &str, contents: &str) {
    let Some(directory) = std::env::var_os("GROW_SUBAGENT_CONTROL_EVIDENCE_DIR") else {
        return;
    };
    let directory = PathBuf::from(directory);
    assert!(
        directory.is_absolute() && directory.starts_with("/tmp"),
        "test evidence must use an explicit /tmp directory"
    );
    std::fs::create_dir_all(&directory).expect("create requested test evidence directory");
    std::fs::write(directory.join(name), contents).expect("save requested test evidence");
}

fn designated_binary() -> PathBuf {
    // Never let pager_binary() silently build or pick an old default artifact.
    let binary = PathBuf::from(
        std::env::var_os("PAGER_BINARY")
            .expect("parent must designate the final built binary via PAGER_BINARY"),
    );
    assert!(
        binary.is_absolute(),
        "final binary identity must be an absolute path"
    );
    let binary = std::fs::canonicalize(binary).expect("final binary exists");
    let metadata = std::fs::metadata(&binary).expect("final binary metadata");
    let mut file = std::fs::File::open(&binary).expect("read binary identity");
    let mut hash = blake3::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let length = std::io::Read::read(&mut file, &mut buffer).expect("hash final binary");
        if length == 0 {
            break;
        }
        hash.update(&buffer[..length]);
    }
    let identity = serde_json::json!({
        "path": binary,
        "bytes": metadata.len(),
        "mtime_ns": metadata.modified().expect("mtime")
            .duration_since(std::time::UNIX_EPOCH).expect("binary mtime since epoch")
            .as_nanos().to_string(),
        "blake3": hash.finalize().to_hex().to_string(),
    });
    save_evidence(
        "binary-identity.json",
        &serde_json::to_string_pretty(&identity).expect("serialize binary identity"),
    );
    eprintln!(
        "subagent-model-control PTY binary={} bytes={} mtime={:?} blake3={}",
        binary.display(),
        metadata.len(),
        metadata.modified().expect("mtime"),
        hash.finalize()
    );
    binary
}

fn parsed_timeline(contents: &str) -> Timeline {
    shell::session::testkit::decode_timeline_fixture(contents)
        .expect("valid checksummed canonical Timeline")
}

fn model_changes(contents: &str) -> Vec<Value> {
    parsed_timeline(contents)
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::Observation(observation)
                if observation.scope == "model" && observation.name == "changed" =>
            {
                Some(observation.data.clone().expect("model.changed data"))
            }
            _ => None,
        })
        .collect()
}

fn model_change_count(contents: &str) -> usize {
    model_changes(contents).len()
}

fn switches_to(contents: &str, model: &str) -> usize {
    model_changes(contents)
        .iter()
        .filter(|data| data["to_model_id"].as_str() == Some(model))
        .count()
}

fn completed_tools(contents: &str, call_id: &str) -> Vec<ToolEvent> {
    parsed_timeline(contents)
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::Tool(
                tool @ ToolEvent::Completed {
                    call_id: actual, ..
                },
            ) if actual == call_id => Some(tool.clone()),
            _ => None,
        })
        .collect()
}

fn assert_single_queued_result(contents: &str, child_id: &str) {
    use sampling_types::ConversationItem;
    use tools::implementations::grow_build::set_subagent_model::{
        SetSubagentModelOutput, SubagentModelStatus,
    };

    let timeline = parsed_timeline(contents);
    let results: Vec<_> = timeline
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::Messages(message) => Some(&message.items),
            _ => None,
        })
        .flatten()
        .filter_map(|item| match item {
            ConversationItem::ToolResult(result) if result.tool_call_id == SET_CALL => Some(result),
            _ => None,
        })
        .collect();
    save_evidence(
        "parent-control-result.json",
        &serde_json::to_string_pretty(&results).expect("serialize canonical control result"),
    );
    assert_eq!(
        results.len(),
        1,
        "one canonical model-visible control result"
    );
    let output: SetSubagentModelOutput =
        serde_json::from_str(&results[0].content).expect("typed parent admission output");
    assert_eq!(output.subagent_id, child_id);
    assert!(matches!(output.status, SubagentModelStatus::Queued));
}

fn control_receipts(content: &ContentController, session_id: &str) -> Vec<ControlStateUpdate> {
    let mut receipts = Vec::new();
    let emission = shell::session::storage::stream_replay_grow_notifications_at(
        session_id,
        &content.home().join(".grow"),
        |notification| {
            if let GrowUpdate::ControlStateUpdate(update) = notification.update {
                receipts.push(update);
            }
        },
    )
    .expect("strict canonical Grow history replay");
    assert_eq!(emission, shell::session::storage::ReplayEmission::Emitted);
    receipts
}

fn request_models(content: &ContentController, path: &str) -> Vec<String> {
    content
        .requests()
        .into_iter()
        .filter(|entry| entry.path == path)
        .map(|entry| {
            entry.body.expect("provider JSON body")["model"]
                .as_str()
                .expect("provider wire model")
                .to_owned()
        })
        .collect()
}

fn occurrences(screen: &str, needle: &str) -> usize {
    screen.matches(needle).count()
}

fn spawn_for_tool_call(contents: &str, call_id: &str) -> chat_state::SubagentSpawnEvent {
    let spawns: Vec<_> = parsed_timeline(contents)
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn))
                if spawn.origin_tool_call_id.as_deref() == Some(call_id) =>
            {
                Some((**spawn).clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(spawns.len(), 1, "one actual spawn for {call_id}");
    spawns.into_iter().next().expect("actual spawned child")
}

fn session_dir_for_call(content: &ContentController, call_id: &str) -> PathBuf {
    let sessions = content.home().join(".grow/sessions");
    let mut matches = Vec::new();
    for cwd in std::fs::read_dir(&sessions).expect("isolated sessions") {
        let cwd = cwd.expect("cwd directory");
        if !cwd.file_type().expect("cwd entry type").is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(cwd.path()).expect("session directories") {
            let entry = entry.expect("session directory");
            if !entry.file_type().expect("session entry type").is_dir() {
                continue;
            }
            let directory = entry.path();
            let contents = std::fs::read_to_string(directory.join("timeline.jsonl"))
                .expect("root canonical journal");
            if parsed_timeline(&contents).events().iter().any(|event| {
                matches!(&event.kind, TimelineEventKind::Tool(ToolEvent::Started { call_id: actual, .. }) if actual == call_id)
            }) {
                matches.push(directory);
            }
        }
    }
    assert_eq!(matches.len(), 1, "one root owning {call_id}: {matches:?}");
    matches.pop().expect("root session directory")
}
