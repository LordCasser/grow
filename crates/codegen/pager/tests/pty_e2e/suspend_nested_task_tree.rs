//! Real Pager process suspension and cold recovery across a root, completed A,
//! active B, and B's grandchild. Foreground calls fix provider request order.
#[allow(unused_imports)]
use super::common::*;

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run the owning pty_e2e_* Cargo test with --ignored"]
async fn ctrl_d_suspends_and_cold_resumes_root_child_grandchild_tree() {
    run_nested_tree_recovery(false, false).await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run the owning pty_e2e_* Cargo test with --ignored"]
async fn cold_resume_after_partial_leaf_release_does_not_replay_checkpoint() {
    run_nested_tree_recovery(true, false).await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run the owning pty_e2e_* Cargo test with --ignored"]
async fn concurrent_cold_resume_claims_one_root_without_replaying_tree_work() {
    run_nested_tree_recovery(false, true).await;
}

async fn run_nested_tree_recovery(partial_leaf_release: bool, concurrent_resume: bool) {
    let content = ContentController::start()
        .await
        .expect("start mock content");
    content
        .seed_llm_config()
        .expect("seed isolated mock provider");
    let config_path = content.home().join(".grow/config.toml");
    let mut config = std::fs::read_to_string(&config_path).expect("read mock configuration");
    config.push_str(
        "\n[subagents]\nenabled = true\nmax_depth = 3\npermission_mode = \"always-approve\"\n",
    );
    std::fs::write(config_path, config)
        .expect("enable the nested test's explicit child capability");

    let project = tempfile::tempdir().expect("create project directory");
    std::fs::create_dir_all(project.path().join(".git")).expect("create .git");
    let marker = project.path().join("nested-command-runs.txt");
    let pid_file = project.path().join("nested-command-child.pid");
    let marker_arg = shell_quote(&marker);
    let pid_arg = shell_quote(&pid_file);
    let command = format!(
        "printf 'started\\n' >> {marker}; /bin/sleep 20 & sleep_pid=$!; printf '%s\\n' \"$sleep_pid\" > {pid}; wait \"$sleep_pid\"; printf 'finished\\n' >> {marker}; echo NESTED_COMMAND_FINISHED",
        marker = marker_arg,
        pid = pid_arg,
    );
    let root_task_a = json!({
        "prompt": "Complete this task immediately without tools.",
        "description": "Complete child A before child B starts",
        "subagent_type": "general-purpose",
        "background": false,
        "capability_mode": "all"
    })
    .to_string();
    let root_task_b = json!({
        "prompt": "Delegate this task to one child and wait for it to finish.",
        "description": "Wait for child B",
        "subagent_type": "general-purpose",
        "background": false,
        "capability_mode": "all"
    })
    .to_string();
    let child_b_task = json!({
        "prompt": "Delegate this task to one grandchild and wait for it to finish.",
        "description": "Wait for B's grandchild",
        "subagent_type": "general-purpose",
        "background": false,
        "capability_mode": "all"
    })
    .to_string();
    let grandchild_command = json!({
        "command": command,
        "description": "Run B's bounded child command"
    })
    .to_string();
    let _root_child_a =
        expect_tool_turn(&content, "call_root_child_a", "spawn_subagent", root_task_a);
    let _child_a_completion = content.expect_agent_turn("child A completion", "CHILD_A_COMPLETED");
    let _root_child_b =
        expect_tool_turn(&content, "call_root_child_b", "spawn_subagent", root_task_b);
    let _child_b_grandchild = expect_tool_turn(
        &content,
        "call_b_grandchild",
        "spawn_subagent",
        child_b_task,
    );
    let _grandchild_command = expect_tool_turn(
        &content,
        "call_grandchild_command",
        "run_terminal_command",
        grandchild_command,
    );
    content.set_response("NESTED_TREE_RESUMED_RESULT");

    let binary = pager_binary().expect("resolve Pager binary");
    let spawn_args = ["--permission-mode", "always-approve", "--trust"];
    let mut first = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &spawn_args,
        Some(project.path()),
    )
    .expect("spawn first Pager process");
    first
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome screen");
    const USER_PROMPT: &str = "SUSPEND_NESTED_TREE_ONCE";
    first
        .inject_keys(format!("{USER_PROMPT}\r").as_bytes())
        .expect("submit one user turn");

    poll_for(Duration::from_secs(45), || {
        first.update(Duration::from_millis(30));
        std::fs::read_to_string(&marker)
            .ok()
            .filter(|contents| contents.lines().any(|line| line == "started") && pid_file.exists())
    })
    .unwrap_or_else(|| {
        panic!(
            "grandchild command did not start; screen:\n{}\nrequests:\n{}\ntool names: {:?}",
            first.screen_contents(),
            dump_non_system_messages(&content.request_bodies()),
            content
                .request_bodies()
                .iter()
                .map(|body| {
                    body.get("tools")
                        .and_then(serde_json::Value::as_array)
                        .map(|tools| {
                            tools
                                .iter()
                                .filter_map(|tool| {
                                    tool.pointer("/function/name")
                                        .or_else(|| tool.get("name"))
                                        .and_then(serde_json::Value::as_str)
                                        .map(str::to_owned)
                                })
                                .collect::<Vec<_>>()
                        })
                })
                .collect::<Vec<_>>()
        )
    });

    let root_dir = session_dir_for_prompt(&content, USER_PROMPT);
    let root_timeline_path = root_dir.join("timeline.jsonl");
    let root_before = std::fs::read_to_string(&root_timeline_path).expect("root Timeline");
    let root_session_id = root_dir
        .file_name()
        .and_then(|name| name.to_str())
        .expect("root session directory name")
        .to_owned();
    let (child_a_session_id, child_a_security_parent) =
        spawn_for_tool_call(&root_before, "call_root_child_a").expect("child A spawn fact");
    assert_eq!(
        child_a_security_parent, root_session_id,
        "child A must retain the root as its direct security parent"
    );
    let child_a_dir = session_dir_by_id(&content, &child_a_session_id);
    let child_a_timeline_path = child_a_dir.join("timeline.jsonl");
    let child_a_before = std::fs::read_to_string(&child_a_timeline_path).expect("child A Timeline");
    assert!(
        child_a_before.contains("CHILD_A_COMPLETED"),
        "child A must finish before child B's long command starts"
    );
    assert_eq!(
        subagent_terminal_count(&root_before, &child_a_session_id),
        1,
        "child A must have one canonical terminal before B starts"
    );
    assert_eq!(
        subagent_terminal_with_result_count(&root_before, &child_a_session_id),
        1,
        "child A terminal must retain its canonical result reference"
    );
    assert_eq!(
        tool_result_count(&root_before, "call_root_child_a"),
        1,
        "root must have received child A's canonical result before B starts"
    );
    let (child_b_session_id, child_b_security_parent) =
        spawn_for_tool_call(&root_before, "call_root_child_b").expect("child B spawn fact");
    assert_eq!(
        child_b_security_parent, root_session_id,
        "child B must retain the root as its direct security parent"
    );
    let child_b_dir = session_dir_by_id(&content, &child_b_session_id);
    let child_b_timeline_path = child_b_dir.join("timeline.jsonl");
    let child_b_before = std::fs::read_to_string(&child_b_timeline_path).expect("child B Timeline");
    // Lifecycle facts are stored on the lifecycle root Timeline, including
    // nested spawns. security_parent_session_id identifies the direct parent.
    let (grandchild_session_id, grandchild_security_parent) =
        spawn_for_tool_call(&root_before, "call_b_grandchild")
            .expect("grandchild spawn fact on lifecycle root");
    let grandchild_subagent_id = subagent_id_for_tool_call(&root_before, "call_b_grandchild")
        .expect("grandchild subagent identity on lifecycle root");
    assert_eq!(
        grandchild_security_parent, child_b_session_id,
        "grandchild must retain child B as its direct security parent"
    );
    let grandchild_dir = session_dir_by_id(&content, &grandchild_session_id);
    let grandchild_timeline_path = grandchild_dir.join("timeline.jsonl");
    let grandchild_before =
        std::fs::read_to_string(&grandchild_timeline_path).expect("grandchild Timeline");
    assert!(
        grandchild_before.contains("call_grandchild_command"),
        "grandchild command call must be durably admitted before suspension"
    );
    let root_turns_before = started_turn_ids(&root_before);
    assert_eq!(
        root_turns_before.len(),
        1,
        "fixture should create one root Turn"
    );
    let child_a_turns_before = started_turn_ids(&child_a_before);
    let child_b_turns_before = started_turn_ids(&child_b_before);
    let grandchild_turns_before = started_turn_ids(&grandchild_before);
    assert_eq!(
        child_a_turns_before.len(),
        1,
        "fixture should create one child A Turn"
    );
    assert!(
        turn_has_ended(&child_a_before, &child_a_turns_before[0]),
        "child A's Turn must be complete before child B starts"
    );
    assert_eq!(
        child_b_turns_before.len(),
        1,
        "fixture should create one child B Turn"
    );
    assert_eq!(
        grandchild_turns_before.len(),
        1,
        "fixture should create one grandchild Turn"
    );

    first
        .inject_keys(b"\x04\x04")
        .expect("suspend the root and its nested child tree with two confirmed Ctrl+D presses");
    let exit = first
        .wait_for_exit_and_drain(Duration::from_secs(15), Duration::from_secs(1))
        .unwrap_or_else(|error| {
            let log = std::fs::read_to_string(content.home().join(".grow/logs/unified.jsonl"))
                .unwrap_or_default();
            let tail = log.lines().rev().take(40).collect::<Vec<_>>();
            let facts = [&root_timeline_path, &child_b_timeline_path, &grandchild_timeline_path]
                .map(|path| {
                    let raw = std::fs::read_to_string(path).unwrap_or_default();
                    let events = raw.lines().filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                        .filter(|event| matches!(event["type"].as_str(), Some("runtime" | "turn")))
                        .collect::<Vec<_>>();
                    format!("{}: {}", path.display(), serde_json::to_string(&events).unwrap())
                });
            panic!(
                "tree suspension did not confirm: {error:#}\nlast runtime logs:\n{}\ncausal facts:\n{}",
                tail.into_iter().rev().collect::<Vec<_>>().join("\n"), facts.join("\n")
            )
        });
    assert_eq!(
        exit,
        0,
        "suspended tree should exit normally, got {exit:?}; screen:\n{}",
        first.screen_contents()
    );
    let command_pid = std::fs::read_to_string(&pid_file)
        .expect("running grandchild command recorded its child process")
        .trim()
        .parse::<libc::pid_t>()
        .expect("valid grandchild command child pid");
    assert!(command_pid > 1, "invalid command child pid {command_pid}");
    assert!(
        poll_for(Duration::from_secs(5), || {
            (!process_alive(command_pid)).then_some(())
        })
        .is_some(),
        "confirmed suspension must terminate the grandchild process tree (pid {command_pid})"
    );
    let root_suspended =
        std::fs::read_to_string(&root_timeline_path).expect("suspended root Timeline");
    let grandchild_suspended =
        std::fs::read_to_string(&grandchild_timeline_path).expect("suspended grandchild Timeline");
    assert_eq!(
        interrupted_command_settlement_count(&grandchild_suspended, "call_grandchild_command"),
        1,
        "tree suspension must durably settle the grandchild command as interrupted"
    );
    assert_eq!(
        interrupted_tool_result_count(&grandchild_suspended, "call_grandchild_command"),
        1,
        "the grandchild model must receive the interrupted command result"
    );
    assert_eq!(
        json_field_count(&root_suspended, "origin_tool_call_id", "call_b_grandchild"),
        1,
        "root lifecycle Timeline must keep one grandchild Spawn fact"
    );
    drop(first);
    let partial_release_instance = if partial_leaf_release {
        Some(append_partial_leaf_release(
            &root_timeline_path,
            &child_b_timeline_path,
            &grandchild_timeline_path,
            &grandchild_session_id,
        ))
    } else {
        None
    };

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
    .expect("spawn cold --continue process");
    if concurrent_resume {
        // Both processes share the same isolated HOME, cwd, ContentController,
        // and durable session owner. Pump each PTY in short slices so neither
        // child can block the other's output while writer ownership resolves.
        let mut competing = PtyHarness::spawn_with_content_in_dir(
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
        .expect("spawn competing cold --continue process");
        let deadline = Instant::now() + RESUME_TIMEOUT;
        let mut completed = false;
        while Instant::now() < deadline {
            resumed.update(Duration::from_millis(10));
            competing.update(Duration::from_millis(10));
            let contents = std::fs::read_to_string(&root_timeline_path).unwrap_or_default();
            if contents.contains("NESTED_TREE_RESUMED_RESULT")
                && turn_has_ended(&contents, &root_turns_before[0])
            {
                completed = true;
                break;
            }
        }
        assert!(
            completed,
            "neither concurrent --continue process completed the original root Turn; first screen:\n{}\nsecond screen:\n{}\nrequests:\n{}",
            resumed.screen_contents(),
            competing.screen_contents(),
            dump_non_system_messages(&content.request_bodies())
        );
        let render_deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < render_deadline
            && !resumed.contains_text("NESTED_TREE_RESUMED_RESULT")
            && !competing.contains_text("NESTED_TREE_RESUMED_RESULT")
        {
            resumed.update(Duration::from_millis(10));
            competing.update(Duration::from_millis(10));
        }
        assert!(
            resumed.contains_text("NESTED_TREE_RESUMED_RESULT")
                || competing.contains_text("NESTED_TREE_RESUMED_RESULT"),
            "at least one process must attach to and render the resumed root result; first screen:\n{}\nsecond screen:\n{}",
            resumed.screen_contents(),
            competing.screen_contents()
        );
        // Tear down both processes without asking either to create another
        // suspension operation after the shared root has completed.
        drop(competing);
        drop(resumed);
    } else {
        resumed
            .wait_for_text("NESTED_TREE_RESUMED_RESULT", RESUME_TIMEOUT)
            .unwrap_or_else(|_| {
                panic!(
                    "nested tree did not resume automatically; screen:\n{}\nrequests:\n{}",
                    resumed.screen_contents(),
                    dump_non_system_messages(&content.request_bodies())
                )
            });
        poll_for(RESUME_TIMEOUT, || {
            resumed.update(Duration::from_millis(30));
            let contents = std::fs::read_to_string(&root_timeline_path).ok()?;
            (contents.contains("NESTED_TREE_RESUMED_RESULT")
                && turn_has_ended(&contents, &root_turns_before[0]))
            .then_some(())
        })
        .expect("root must finish its original turn after the child tree resumes");
        resumed.quit().expect("quit resumed Pager");
    }

    assert_eq!(
        root_dir,
        session_dir_for_prompt(&content, USER_PROMPT),
        "root session identity must remain stable"
    );
    assert_eq!(
        child_a_dir,
        session_dir_by_id(&content, &child_a_session_id),
        "child A session identity must remain stable"
    );
    assert_eq!(
        child_b_dir,
        session_dir_by_id(&content, &child_b_session_id),
        "child B session identity must remain stable"
    );
    assert_eq!(
        grandchild_dir,
        session_dir_by_id(&content, &grandchild_session_id),
        "grandchild session identity must remain stable"
    );
    let root_after = std::fs::read_to_string(&root_timeline_path).expect("resumed root Timeline");
    let child_a_after =
        std::fs::read_to_string(&child_a_timeline_path).expect("resumed child A Timeline");
    let child_b_after =
        std::fs::read_to_string(&child_b_timeline_path).expect("resumed child B Timeline");
    let grandchild_after =
        std::fs::read_to_string(&grandchild_timeline_path).expect("resumed grandchild Timeline");
    assert_eq!(
        started_turn_ids(&root_after),
        started_turn_ids(&root_before),
        "root resume must retain its original Turn"
    );
    assert_eq!(
        consumed_user_input_count(&root_after, USER_PROMPT),
        1,
        "the root user input must not be duplicated during resume"
    );
    assert_eq!(
        started_turn_ids(&child_a_after),
        child_a_turns_before,
        "child A must not start a second Turn during B recovery"
    );
    assert!(turn_has_ended(&child_a_after, &child_a_turns_before[0]));
    assert_eq!(
        subagent_terminal_count(&root_after, &child_a_session_id),
        1,
        "child A must not execute or terminalize twice"
    );
    assert_eq!(
        subagent_terminal_with_result_count(&root_after, &child_a_session_id),
        1,
        "child A's canonical result reference must remain unique after resume"
    );
    assert_eq!(
        started_turn_ids(&child_b_after),
        child_b_turns_before,
        "child B resume must retain its original Turn; before={}, after={}",
        turn_identity_facts(&child_b_before),
        turn_identity_facts(&child_b_after),
    );
    assert_eq!(
        started_turn_ids(&grandchild_after),
        grandchild_turns_before,
        "grandchild resume must retain its original Turn"
    );
    assert!(turn_has_ended(&child_b_after, &child_b_turns_before[0]));
    assert!(turn_has_ended(
        &grandchild_after,
        &grandchild_turns_before[0]
    ));
    if partial_release_instance.is_some() {
        assert!(
            turn_has_ended_with_outcome(
                &grandchild_after,
                &grandchild_turns_before[0],
                "interrupted"
            ),
            "a child whose execution was already released must use ordinary cold interruption recovery"
        );
    }
    assert_eq!(
        spawn_for_tool_call(&root_after, "call_root_child_a")
            .map(|(session_id, _)| session_id)
            .as_deref(),
        Some(child_a_session_id.as_str()),
        "root must not spawn a replacement A child"
    );
    assert_eq!(
        spawn_for_tool_call(&root_after, "call_root_child_b")
            .map(|(session_id, _)| session_id)
            .as_deref(),
        Some(child_b_session_id.as_str()),
        "root must not spawn a replacement B child"
    );
    assert_eq!(
        spawn_for_tool_call(&root_after, "call_b_grandchild")
            .map(|(session_id, _)| session_id)
            .as_deref(),
        Some(grandchild_session_id.as_str()),
        "child must not spawn a replacement grandchild"
    );
    assert_eq!(
        json_field_count(&root_after, "origin_tool_call_id", "call_root_child_a"),
        1,
        "A child Spawn fact must remain unique"
    );
    assert_eq!(
        json_field_count(&root_after, "origin_tool_call_id", "call_root_child_b"),
        1,
        "B child Spawn fact must remain unique"
    );
    assert_eq!(
        json_field_count(&root_after, "origin_tool_call_id", "call_b_grandchild"),
        1,
        "grandchild Spawn fact must remain unique"
    );
    assert_eq!(
        tool_result_count(&root_after, "call_root_child_a"),
        1,
        "root must consume child A's result exactly once"
    );
    assert_eq!(
        subagent_terminal_with_result_count(&root_after, &child_b_session_id),
        1,
        "child B must publish one canonical result reference after recovery"
    );
    assert_eq!(
        tool_result_count(&root_after, "call_root_child_b"),
        1,
        "root must consume child B's result exactly once"
    );
    assert_eq!(
        subagent_terminal_with_result_count(&root_after, &grandchild_session_id),
        1,
        "grandchild must publish one canonical result reference after recovery"
    );
    assert_eq!(
        tool_result_count(&child_b_after, "call_b_grandchild"),
        1,
        "child must consume the original grandchild result exactly once"
    );
    assert!(
        subagent_completion_receipt_consumed(&child_b_after, &grandchild_subagent_id),
        "the restored ToolResult must consume the matching completion receipt so it cannot open a second Turn"
    );
    assert_eq!(
        interrupted_tool_result_count(&grandchild_after, "call_grandchild_command"),
        1,
        "grandchild must retain one interrupted command result after recovery"
    );
    assert_eq!(
        interrupted_command_settlement_count(&grandchild_after, "call_grandchild_command"),
        1,
        "grandchild command settlement must not be duplicated during recovery"
    );
    let runs = std::fs::read_to_string(&marker).expect("read command marker");
    assert_eq!(
        runs.lines().filter(|line| *line == "started").count(),
        1,
        "the interrupted grandchild command must not be replayed"
    );
    assert_eq!(
        runs.lines().filter(|line| *line == "finished").count(),
        0,
        "a command interrupted for suspension must not continue or be replayed"
    );
    if !partial_leaf_release && !concurrent_resume {
        let export_parent = tempfile::tempdir().expect("create tree export parent");
        let export_path = export_parent.path().join("tree-export");
        let export = std::process::Command::new(&binary)
            .args(["export", &root_session_id])
            .arg(&export_path)
            .env("GROW_HOME", content.home().join(".grow"))
            .current_dir(project.path())
            .output()
            .expect("export the settled tree with the real CLI");
        assert!(
            export.status.success(),
            "tree export failed ({}): {}",
            export.status,
            String::from_utf8_lossy(&export.stderr)
        );
        let root_export = std::fs::read_to_string(export_path.join("transcript.md"))
            .expect("read exported root transcript");
        assert!(root_export.contains(USER_PROMPT));
        assert!(root_export.contains("NESTED_TREE_RESUMED_RESULT"));
    }
}

fn append_partial_leaf_release(
    root_path: &Path,
    parent_path: &Path,
    leaf_path: &Path,
    leaf_session_id: &str,
) -> String {
    use chat_state::session_runtime::{RuntimePhase, SessionRuntimeEvent};

    let root = read_timeline(root_path);
    let root_runtime = root.runtime_lifecycle().clone();
    assert_eq!(root_runtime.phase, RuntimePhase::Suspended);
    let operation_id = root_runtime
        .operation_id
        .clone()
        .expect("suspension operation identity");
    let instance_id = uuid::Uuid::new_v4().to_string();
    append_runtime_events(
        root_path,
        &[
            SessionRuntimeEvent::RestorePrepared {
                operation_id: operation_id.clone(),
                instance_id: instance_id.clone(),
            },
            SessionRuntimeEvent::Resumed {
                operation_id: operation_id.clone(),
                instance_id: instance_id.clone(),
            },
        ],
    );

    let parent = read_timeline(parent_path);
    let parent_runtime = parent.runtime_lifecycle();
    assert_eq!(parent_runtime.phase, RuntimePhase::Suspended);
    assert_eq!(
        parent_runtime.operation_id.as_deref(),
        Some(operation_id.as_str())
    );
    let leaf = read_timeline(leaf_path);
    let leaf_runtime = leaf.runtime_lifecycle();
    assert_eq!(leaf_runtime.phase, RuntimePhase::Suspended);
    assert_eq!(
        leaf_runtime.operation_id.as_deref(),
        Some(operation_id.as_str())
    );
    let leaf_checkpoint = leaf_runtime
        .checkpoint_seq
        .expect("leaf suspension checkpoint sequence");
    assert!(root_runtime.tree.iter().any(|node| {
        node.session_id == leaf_session_id && node.checkpoint_seq == leaf_checkpoint
    }));
    append_runtime_events(
        leaf_path,
        &[
            SessionRuntimeEvent::RestorePrepared {
                operation_id: operation_id.clone(),
                instance_id: instance_id.clone(),
            },
            SessionRuntimeEvent::Resumed {
                operation_id: operation_id.clone(),
                instance_id: instance_id.clone(),
            },
            SessionRuntimeEvent::ExecutionReleased {
                operation_id,
                instance_id: instance_id.clone(),
            },
        ],
    );
    let root_after = read_timeline(root_path);
    let leaf_after = read_timeline(leaf_path);
    assert_eq!(
        root_after.runtime_lifecycle().phase,
        RuntimePhase::Restoring
    );
    assert_eq!(leaf_after.runtime_lifecycle().phase, RuntimePhase::Running);
    assert_eq!(
        read_timeline(parent_path).runtime_lifecycle().phase,
        RuntimePhase::Suspended,
        "the intermediate child remains behind the restore gate"
    );
    instance_id
}

fn read_timeline(path: &Path) -> chat_state::Timeline {
    let contents = std::fs::read_to_string(path).expect("read Timeline before fault injection");
    let events = contents
        .lines()
        .map(|line| serde_json::from_str(line).expect("valid Timeline event JSON"))
        .collect();
    chat_state::Timeline::from_events(events).expect("valid typed Timeline before fault injection")
}

fn append_runtime_events(path: &Path, events: &[chat_state::session_runtime::SessionRuntimeEvent]) {
    use std::io::Write as _;

    let mut timeline = read_timeline(path);
    let previous_len = timeline.events().len();
    for event in events {
        timeline
            .record(chat_state::TimelineEventKind::Runtime(event.clone()))
            .expect("runtime fault-injection event follows the typed state machine");
    }
    let appended = &timeline.events()[previous_len..];
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("open Timeline for fault injection");
    for event in appended {
        writeln!(
            file,
            "{}",
            serde_json::to_string(event).expect("serialize typed event")
        )
        .expect("append typed Timeline event");
    }
}

fn turn_has_ended_with_outcome(contents: &str, turn_id: &str, outcome: &str) -> bool {
    contents
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .any(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("turn")
                && event
                    .pointer("/event/state")
                    .and_then(serde_json::Value::as_str)
                    == Some("ended")
                && event
                    .pointer("/event/id")
                    .and_then(serde_json::Value::as_str)
                    == Some(turn_id)
                && event
                    .pointer("/event/outcome")
                    .and_then(serde_json::Value::as_str)
                    == Some(outcome)
        })
}

fn spawn_for_tool_call(contents: &str, call_id: &str) -> Option<(String, String)> {
    contents
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("subagent")
                && event
                    .pointer("/event/state")
                    .and_then(serde_json::Value::as_str)
                    == Some("spawned")
        })
        .find_map(|event| {
            let spawn = event.pointer("/event/event")?;
            (spawn
                .get("origin_tool_call_id")
                .and_then(serde_json::Value::as_str)
                == Some(call_id))
            .then(|| {
                Some((
                    spawn.get("child_session_id")?.as_str()?.to_owned(),
                    spawn
                        .get("security_parent_session_id")?
                        .as_str()?
                        .to_owned(),
                ))
            })
            .flatten()
        })
}

fn subagent_id_for_tool_call(contents: &str, call_id: &str) -> Option<String> {
    contents
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("subagent")
                && event
                    .pointer("/event/state")
                    .and_then(serde_json::Value::as_str)
                    == Some("spawned")
        })
        .find_map(|event| {
            let spawn = event.pointer("/event/event")?;
            (spawn
                .get("origin_tool_call_id")
                .and_then(serde_json::Value::as_str)
                == Some(call_id))
            .then(|| spawn.get("subagent_id")?.as_str().map(str::to_owned))
            .flatten()
        })
}

fn subagent_completion_receipt_consumed(contents: &str, subagent_id: &str) -> bool {
    let events = contents
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .collect::<Vec<_>>();
    let receipt_ids = events
        .iter()
        .filter(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("notification")
                && event
                    .pointer("/event/state")
                    .and_then(serde_json::Value::as_str)
                    == Some("received")
                && event
                    .pointer("/event/source/kind")
                    .and_then(serde_json::Value::as_str)
                    == Some("subagent_completed")
                && event
                    .pointer("/event/source/subagent_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(subagent_id)
        })
        .filter_map(|event| event.pointer("/event/id")?.as_str())
        .collect::<Vec<_>>();
    receipt_ids.len() == 1
        && events.iter().any(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("notification")
                && event
                    .pointer("/event/state")
                    .and_then(serde_json::Value::as_str)
                    == Some("consumed")
                && event
                    .pointer("/event/notification_ids")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(receipt_ids[0])))
        })
}

fn subagent_terminal_count(contents: &str, child_session_id: &str) -> usize {
    count_subagent_terminal_facts(contents, child_session_id, false)
}

fn subagent_terminal_with_result_count(contents: &str, child_session_id: &str) -> usize {
    count_subagent_terminal_facts(contents, child_session_id, true)
}

fn count_subagent_terminal_facts(
    contents: &str,
    child_session_id: &str,
    require_result_ref: bool,
) -> usize {
    fn is_terminal(
        value: &serde_json::Value,
        child_session_id: &str,
        require_result_ref: bool,
    ) -> bool {
        match value {
            serde_json::Value::Object(object) => {
                let terminal = object.get("state").and_then(serde_json::Value::as_str)
                    == Some("ended")
                    && object.get("event").is_some_and(|event| {
                        event
                            .get("child_session_id")
                            .and_then(serde_json::Value::as_str)
                            == Some(child_session_id)
                            && (!require_result_ref
                                || event
                                    .get("result_ref")
                                    .is_some_and(|reference| !reference.is_null()))
                    });
                terminal
                    || object
                        .values()
                        .any(|value| is_terminal(value, child_session_id, require_result_ref))
            }
            serde_json::Value::Array(values) => values
                .iter()
                .any(|value| is_terminal(value, child_session_id, require_result_ref)),
            _ => false,
        }
    }

    contents
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| is_terminal(event, child_session_id, require_result_ref))
        .count()
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn session_dir_by_id(content: &ContentController, session_id: &str) -> PathBuf {
    let sessions = content.home().join(".grow").join("sessions");
    std::fs::read_dir(&sessions)
        .expect("sessions directory")
        .flatten()
        .map(|cwd_dir| cwd_dir.path().join(session_id))
        .find(|path| path.is_dir())
        .unwrap_or_else(|| {
            panic!(
                "no session directory for {session_id} under {}",
                sessions.display()
            )
        })
}

fn session_dir_for_prompt(content: &ContentController, prompt: &str) -> PathBuf {
    let sessions = content.home().join(".grow").join("sessions");
    for cwd_dir in std::fs::read_dir(&sessions)
        .expect("sessions directory")
        .flatten()
    {
        let Ok(entries) = std::fs::read_dir(cwd_dir.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let timeline = entry.path().join("timeline.jsonl");
            if std::fs::read_to_string(timeline).is_ok_and(|contents| contents.contains(prompt)) {
                return entry.path();
            }
        }
    }
    panic!("no session Timeline contains prompt {prompt:?}");
}

fn consumed_user_input_count(contents: &str, prompt: &str) -> usize {
    let wrapped_prompt = format!("<user_query>\n{prompt}\n</user_query>");
    contents
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("input")
                && event
                    .pointer("/event/state")
                    .and_then(serde_json::Value::as_str)
                    == Some("consumed")
                && event
                    .pointer("/event/item/type")
                    .and_then(serde_json::Value::as_str)
                    == Some("user")
                && event
                    .pointer("/event/item/content")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|parts| {
                        parts.iter().any(|part| {
                            part.get("type").and_then(serde_json::Value::as_str) == Some("text")
                                && part.get("text").and_then(serde_json::Value::as_str)
                                    == Some(wrapped_prompt.as_str())
                        })
                    })
        })
        .count()
}

fn json_field_count(contents: &str, field: &str, expected: &str) -> usize {
    fn contains_field(value: &serde_json::Value, field: &str, expected: &str) -> bool {
        match value {
            serde_json::Value::Object(object) => {
                object.get(field).and_then(serde_json::Value::as_str) == Some(expected)
                    || object
                        .values()
                        .any(|value| contains_field(value, field, expected))
            }
            serde_json::Value::Array(values) => values
                .iter()
                .any(|value| contains_field(value, field, expected)),
            _ => false,
        }
    }
    contents
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| contains_field(value, field, expected))
        .count()
}

fn interrupted_command_settlement_count(contents: &str, call_id: &str) -> usize {
    contents
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("runtime")
                && event
                    .pointer("/event/kind")
                    .and_then(serde_json::Value::as_str)
                    == Some("commands_settled")
                && event
                    .pointer("/event/commands")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|commands| {
                        commands.iter().any(|command| {
                            command.get("call_id").and_then(serde_json::Value::as_str)
                                == Some(call_id)
                                && command.get("outcome").and_then(serde_json::Value::as_str)
                                    == Some("interrupted")
                        })
                    })
        })
        .count()
}

fn interrupted_tool_result_count(contents: &str, call_id: &str) -> usize {
    contents
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("messages")
                && event
                    .pointer("/event/cause")
                    .and_then(serde_json::Value::as_str)
                    == Some("tool_result")
                && event
                    .pointer("/event/items")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|items| {
                        items.iter().any(|item| {
                            if item.get("type").and_then(serde_json::Value::as_str)
                                != Some("tool_result")
                                || item.get("tool_call_id").and_then(serde_json::Value::as_str)
                                    != Some(call_id)
                            {
                                return false;
                            }
                            item.get("content")
                                .and_then(serde_json::Value::as_str)
                                .and_then(|content| {
                                    serde_json::from_str::<serde_json::Value>(content).ok()
                                })
                                .is_some_and(|content| {
                                    content.get("status").and_then(serde_json::Value::as_str)
                                        == Some("interrupted")
                                })
                        })
                    })
        })
        .count()
}

fn tool_result_count(contents: &str, call_id: &str) -> usize {
    contents
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("messages")
                && event
                    .pointer("/event/cause")
                    .and_then(serde_json::Value::as_str)
                    == Some("tool_result")
                && event
                    .pointer("/event/items")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|items| {
                        items.iter().any(|item| {
                            item.get("type").and_then(serde_json::Value::as_str)
                                == Some("tool_result")
                                && item.get("tool_call_id").and_then(serde_json::Value::as_str)
                                    == Some(call_id)
                        })
                    })
        })
        .count()
}

fn started_turn_ids(contents: &str) -> Vec<String> {
    contents
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|event| {
            (event.get("type")?.as_str()? == "turn"
                && event.pointer("/event/state")?.as_str()? == "started")
                .then(|| event.pointer("/event/id")?.as_str().map(str::to_owned))
                .flatten()
        })
        .collect()
}

fn turn_identity_facts(contents: &str) -> String {
    let facts = contents
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|event| {
            let kind = event.get("type")?.as_str()?;
            if kind != "turn" {
                return None;
            }
            let detail = event.get("event")?;
            let state = detail.get("state")?.as_str()?;
            let mut fact = serde_json::Map::new();
            fact.insert("seq".into(), event.get("seq")?.clone());
            fact.insert("state".into(), serde_json::Value::String(state.into()));
            for field in [
                "id",
                "prompt_id",
                "input_ids",
                "identity",
                "prompt_index",
                "prompt_text",
                "input_kind",
                "redirect_kind",
                "outcome",
                "terminal",
            ] {
                if let Some(value) = detail.get(field) {
                    fact.insert(field.into(), value.clone());
                }
            }
            Some(serde_json::Value::Object(fact))
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&facts).expect("serialize Turn identity facts")
}

fn turn_has_ended(contents: &str, turn_id: &str) -> bool {
    contents
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .any(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("turn")
                && event
                    .pointer("/event/state")
                    .and_then(serde_json::Value::as_str)
                    == Some("ended")
                && event
                    .pointer("/event/id")
                    .and_then(serde_json::Value::as_str)
                    == Some(turn_id)
        })
}

fn process_alive(pid: libc::pid_t) -> bool {
    if pid <= 1 {
        return false;
    }
    // SAFETY: signal 0 only checks whether the process still exists.
    unsafe { libc::kill(pid, 0) == 0 }
}
