//! Exercise a real process exit during a running root tool and automatic
//! recovery from the same session in a fresh Pager process.
#[allow(unused_imports)]
use super::common::*;

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run the owning pty_e2e_* Cargo test with --ignored"]
async fn ctrl_d_suspends_running_root_and_continue_resumes_without_replay() {
    let content = ContentController::start()
        .await
        .expect("start mock content");
    content
        .seed_llm_config()
        .expect("seed isolated mock provider");

    let project = tempfile::tempdir().expect("create project directory");
    git2::Repository::init(project.path()).expect("init test workspace");
    let marker = project.path().join("command-runs.txt");
    let pid_file = project.path().join("command-child.pid");
    let marker_arg = shell_quote(&marker);
    let pid_arg = shell_quote(&pid_file);
    let command = format!(
        "printf 'started\\n' >> {marker}; /bin/sleep 20 & sleep_pid=$!; printf '%s\\n' \"$sleep_pid\" > {pid}; wait \"$sleep_pid\"; printf 'finished\\n' >> {marker}; echo COMMAND_FINISHED",
        marker = marker_arg,
        pid = pid_arg,
    );
    let args = json!({
        "command": command,
        "description": "suspend while command is running"
    })
    .to_string();
    let _tool_turn = expect_tool_turn(&content, "call_suspend_root", "run_terminal_command", args);
    content.set_chunk_delay(Some(Duration::from_millis(75)));
    content.set_response("RECOVERED_ROOT_RESULT");

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
    const USER_PROMPT: &str = "SUSPEND_PTY_USER_ONCE";
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
            "root command did not start; screen:\n{}\nrequests:\n{}",
            first.screen_contents(),
            dump_non_system_messages(&content.request_bodies())
        )
    });
    first
        .inject_keys(b"\x04\x04")
        .expect("suspend during the running root command");
    let exit = first
        .wait_for_exit_and_drain(Duration::from_secs(15), Duration::from_secs(1))
        .expect("wait for confirmed suspension exit");
    assert_eq!(
        exit,
        0,
        "confirmed suspension should exit normally, got {exit:?}; screen:\n{}",
        first.screen_contents()
    );
    let command_pid = std::fs::read_to_string(&pid_file)
        .expect("running command recorded its child process")
        .trim()
        .parse::<libc::pid_t>()
        .expect("valid command child pid");
    assert!(command_pid > 1, "invalid command child pid {command_pid}");
    assert!(
        poll_for(Duration::from_secs(5), || {
            (!process_alive(command_pid)).then_some(())
        })
        .is_some(),
        "confirmed suspension must terminate the command process tree (pid {command_pid})"
    );
    let session_path = session_dir_with_prompt(&content, USER_PROMPT);
    let timeline_path = session_path.join("timeline.jsonl");
    let before = std::fs::read_to_string(&timeline_path).expect("read suspended Timeline");
    assert_eq!(
        interrupted_command_settlement_count(&before, "call_suspend_root"),
        1,
        "suspension must durably settle the running command as interrupted"
    );
    assert_eq!(
        interrupted_tool_result_count(&before, "call_suspend_root"),
        1,
        "the model must receive the persisted interrupted command result"
    );
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
    .expect("spawn cold --continue process");
    resumed
        .wait_for_text("RECOVERED_ROOT_RESULT", RESUME_TIMEOUT)
        .unwrap_or_else(|_| {
            panic!(
                "root did not automatically resume after --continue; screen:\n{}\nrequests:\n{}",
                resumed.screen_contents(),
                dump_non_system_messages(&content.request_bodies())
            )
        });

    let same_session_path = session_dir(&content, &mut resumed);
    assert_eq!(
        session_path, same_session_path,
        "--continue must restore the same durable session"
    );
    let initial_turns = started_turn_ids(&before);
    assert!(
        !initial_turns.is_empty(),
        "initial user turn was not recorded"
    );
    let after = poll_for(Duration::from_secs(45), || {
        resumed.update(Duration::from_millis(30));
        std::fs::read_to_string(&timeline_path)
            .ok()
            .filter(|timeline| turn_has_ended(timeline, &initial_turns[0]))
    })
    .expect("automatic recovery must durably finish the original Turn");
    assert_eq!(
        interrupted_command_settlement_count(&after, "call_suspend_root"),
        1,
        "resume must not duplicate the interrupted command settlement"
    );
    assert_eq!(
        interrupted_tool_result_count(&after, "call_suspend_root"),
        1,
        "resume must not duplicate the interrupted tool result"
    );
    let resumed_turns = started_turn_ids(&after);
    assert!(
        !initial_turns.is_empty(),
        "initial user turn was not recorded"
    );
    assert_eq!(
        resumed_turns, initial_turns,
        "resume must preserve the original Turn identity without starting another user turn"
    );
    assert!(
        turn_has_ended(&after, &initial_turns[0]),
        "automatic recovery should finish the original Turn"
    );
    assert_eq!(
        started_prompt_count(&after, USER_PROMPT),
        1,
        "the accepted Turn prompt must not be duplicated during resume"
    );
    assert_eq!(
        consumed_user_input_count(&after, USER_PROMPT),
        1,
        "the accepted User message must not be duplicated during resume"
    );
    let runs = std::fs::read_to_string(&marker).expect("read command marker");
    assert_eq!(
        runs.lines().filter(|line| *line == "started").count(),
        1,
        "the completed command must not be executed a second time"
    );
    assert_eq!(
        runs.lines().filter(|line| *line == "finished").count(),
        0,
        "a command interrupted for suspension must not continue or be replayed"
    );
    assert!(
        !resumed.contains_text("panicked"),
        "Pager panicked during resume:\n{}",
        resumed.screen_contents()
    );
    resumed.quit().expect("quit resumed Pager");

    let session_id = session_path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .expect("session directory name is UTF-8");
    let export_parent = tempfile::tempdir().expect("create export parent");
    let export_path = export_parent.path().join("root-export");
    let grow_home = content.home().join(".grow");
    let export = std::process::Command::new(&binary)
        .args(["export", session_id])
        .arg(&export_path)
        .env("GROW_HOME", &grow_home)
        .current_dir(project.path())
        .output()
        .expect("run read-only CLI export against the isolated session");
    assert!(
        export.status.success(),
        "export failed (status {}): stdout:\n{}\nstderr:\n{}",
        export.status,
        String::from_utf8_lossy(&export.stdout),
        String::from_utf8_lossy(&export.stderr)
    );
    let exported_transcript = std::fs::read_to_string(export_path.join("transcript.md"))
        .expect("read exported root transcript");
    assert!(
        exported_transcript.contains(USER_PROMPT),
        "export omitted the original input:\n{exported_transcript}"
    );
    assert!(
        exported_transcript.contains("RECOVERED_ROOT_RESULT"),
        "export omitted the completed recovery response:\n{exported_transcript}"
    );
}

/// A background command is represented in Tasks by its terminal state. Cold
/// continuation must recover that state from CommandsSettled rather than
/// resurrecting the interrupted command as running.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run the owning pty_e2e_* Cargo test with --ignored"]
async fn ctrl_d_resume_restores_interrupted_command_as_terminal_in_tasks() {
    let content = ContentController::start()
        .await
        .expect("start mock content");
    content
        .seed_llm_config()
        .expect("seed isolated mock provider");

    let project = tempfile::tempdir().expect("create project directory");
    std::fs::create_dir_all(project.path().join(".git")).expect("create .git");
    let marker = project.path().join("task-command-runs.txt");
    let pid_file = project.path().join("task-command.pid");
    let command = format!(
        "printf 'started\\n' >> {}; /bin/sleep 120 & sleep_pid=$!; printf '%s\\n' \"$sleep_pid\" > {}; wait \"$sleep_pid\"; printf 'finished\\n' >> {}",
        shell_quote(&marker),
        shell_quote(&pid_file),
        shell_quote(&marker)
    );
    let args = json!({
        "command": command,
        "description": "cold resume terminal state regression"
    })
    .to_string();
    let _tool_turn = expect_tool_turn(
        &content,
        "call_resume_terminal",
        "run_terminal_command",
        args,
    );
    content.set_response("BACKGROUND_COMMAND_ACCEPTED");

    let binary = pager_binary().expect("resolve Pager binary");
    let mut first = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &["--permission-mode", "always-approve", "--trust"],
        Some(project.path()),
    )
    .expect("spawn first Pager process");
    first
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome screen");
    const USER_PROMPT: &str = "SUSPEND_COMMAND_TERMINAL_TASK";
    first
        .inject_keys(format!("{USER_PROMPT}\r").as_bytes())
        .expect("submit prompt");
    poll_for(Duration::from_secs(15), || {
        first.update(Duration::from_millis(30));
        std::fs::read_to_string(&marker)
            .ok()
            .filter(|text| text.contains("started") && pid_file.exists())
    })
    .expect("foreground command must have started before backgrounding");
    let command_started = Instant::now();
    assert!(
        poll_for(Duration::from_secs(5), || {
            first.update(Duration::from_millis(30));
            (command_started.elapsed() >= Duration::from_millis(3_200)).then_some(())
        })
        .is_some(),
        "command did not remain in the foreground for at least 3.2 seconds"
    );
    first
        .inject_keys(b"\x02")
        .expect("send foreground command to background");
    first
        .wait_for_text("BACKGROUND_COMMAND_ACCEPTED", Duration::from_secs(45))
        .unwrap_or_else(|_| {
            panic!(
                "backgrounding did not release the command call; screen:\n{}\nrequests:\n{}",
                first.screen_contents(),
                dump_non_system_messages(&content.request_bodies())
            )
        });
    const TASK_ROW: &str = "Task cold resume terminal state regression";
    first.inject_keys(b"\x07").expect("open running Tasks pane");
    first
        .wait_for_text(TASK_ROW, Duration::from_secs(10))
        .expect("command must appear in running Tasks before suspension");
    first.inject_keys(b"\x07").expect("close Tasks pane");
    first
        .inject_keys(b"\x04\x04")
        .expect("suspend via Ctrl+D twice");
    assert_eq!(
        first
            .wait_for_exit_and_drain(Duration::from_secs(15), Duration::from_secs(1))
            .expect("wait for suspension"),
        0
    );
    let command_pid = std::fs::read_to_string(&pid_file)
        .expect("command child pid")
        .trim()
        .parse::<libc::pid_t>()
        .expect("valid command child pid");
    assert!(
        poll_for(Duration::from_secs(5), || (!process_alive(command_pid))
            .then_some(()))
        .is_some(),
        "suspension must stop the command"
    );

    let session_path = session_dir_with_prompt(&content, USER_PROMPT);
    let timeline_path = session_path.join("timeline.jsonl");
    let suspended = std::fs::read_to_string(&timeline_path).expect("read suspended Timeline");
    assert_eq!(
        interrupted_command_settlement_count(&suspended, "call_resume_terminal"),
        1
    );
    let settlement = parsed_timeline(&suspended)
        .find_map(|event| {
            let is_settlement = event.get("type").and_then(serde_json::Value::as_str)
                == Some("runtime")
                && event
                    .pointer("/event/kind")
                    .and_then(serde_json::Value::as_str)
                    == Some("commands_settled");
            is_settlement
                .then(|| {
                    event
                        .pointer("/event/commands")
                        .and_then(serde_json::Value::as_array)
                })
                .flatten()
                .and_then(|commands| {
                    commands.iter().find(|command| {
                        command.get("call_id").and_then(serde_json::Value::as_str)
                            == Some("call_resume_terminal")
                    })
                })
                .cloned()
        })
        .expect("durable CommandsSettled must contain the interrupted command");
    assert_eq!(
        settlement
            .get("is_backgrounded")
            .and_then(serde_json::Value::as_bool),
        Some(true),
        "the command was explicitly backgrounded before suspension"
    );
    let start_time: std::time::SystemTime = serde_json::from_value(
        settlement
            .get("start_time")
            .expect("settlement records actual command start time")
            .clone(),
    )
    .expect("decode actual command start time");
    let end_time: std::time::SystemTime = serde_json::from_value(
        settlement
            .get("end_time")
            .expect("settlement records actual command end time")
            .clone(),
    )
    .expect("decode actual command end time");
    let execution_elapsed = end_time
        .duration_since(start_time)
        .expect("actual command end follows actual start");
    assert!(
        execution_elapsed >= Duration::from_secs(3),
        "durable execution bounds must include the foreground interval before manual backgrounding; got {execution_elapsed:?}"
    );
    drop(first);

    content.set_response("BACKGROUND_COMMAND_RESUMED");
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
    resumed
        .wait_for_text("BACKGROUND_COMMAND_RESUMED", RESUME_TIMEOUT)
        .unwrap_or_else(|_| {
            panic!(
                "resume did not finish; screen:\n{}\nrequests:\n{}",
                resumed.screen_contents(),
                dump_non_system_messages(&content.request_bodies())
            )
        });
    assert_eq!(session_path, session_dir(&content, &mut resumed));
    resumed.inject_keys(b"\x07").expect("open Tasks pane");
    resumed
        .wait_for_text("No running tasks.", Duration::from_secs(5))
        .unwrap_or_else(|_| {
            panic!(
                "interrupted command must be absent from running-only Tasks after resume:\n{}",
                resumed.screen_contents()
            )
        });
    resumed
        .inject_keys(b"h")
        .expect("show completed task history");
    resumed
        .wait_for_text(TASK_ROW, Duration::from_secs(10))
        .unwrap_or_else(|_| {
            panic!(
                "interrupted command must be present as terminal in Tasks history:\n{}",
                resumed.screen_contents()
            )
        });
    let tasks_screen = resumed.screen_contents();
    let terminal_row = tasks_screen
        .lines()
        .find(|line| line.contains(TASK_ROW))
        .expect("terminal task row is visible");
    let expected_elapsed = pager_render::util::format_duration(execution_elapsed);
    assert!(
        terminal_row
            .split_whitespace()
            .any(|token| token == expected_elapsed),
        "Tasks row must display the full recorded execution interval {expected_elapsed}: {terminal_row:?}"
    );
    assert!(
        terminal_row
            .split_whitespace()
            .any(|token| matches!(token, "✗" | "x")),
        "Tasks history must mark the command as unsuccessful: {terminal_row:?}"
    );
    eprintln!("Interrupted Tasks row: {terminal_row}");
    eprintln!("Recorded process elapsed: {execution_elapsed:?}");
    assert!(
        !resumed.contains_text("1 command still running"),
        "the interrupted command must not return as running:\n{}",
        resumed.screen_contents()
    );
    let after = std::fs::read_to_string(&timeline_path).expect("read resumed Timeline");
    assert_eq!(
        interrupted_command_settlement_count(&after, "call_resume_terminal"),
        1,
        "resume must preserve the original command settlement"
    );
    let runs = std::fs::read_to_string(&marker).expect("read command marker");
    assert_eq!(
        runs.lines().filter(|line| *line == "started").count(),
        1,
        "command must not replay"
    );
    assert_eq!(
        runs.lines().filter(|line| *line == "finished").count(),
        0,
        "interrupted command must not finish"
    );
    resumed.quit().expect("quit resumed Pager");
}

/// A paused Goal survives cold runtime restoration, and an explicit restart
/// resumes the original Goal without another ordinary user message.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run the owning pty_e2e_* Cargo test with --ignored"]
async fn ctrl_d_cold_resume_restart_resumes_paused_goal() {
    let content = ContentController::start()
        .await
        .expect("start mock content");
    content
        .seed_llm_config()
        .expect("seed isolated mock provider");
    content.set_response("GOAL_SESSION_READY");

    let project = tempfile::tempdir().expect("create project directory");
    git2::Repository::init(project.path()).expect("init test workspace");
    let binary = pager_binary().expect("resolve Pager binary");
    let mut first = PtyHarness::spawn_with_content_env_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &["--permission-mode", "always-approve", "--trust"],
        &[("GROW_GOAL", "1")],
        Some(project.path()),
    )
    .expect("spawn first Pager process with Goal enabled");
    first
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome screen");
    const USER_PROMPT: &str = "GOAL_COLD_RESUME_SESSION_SEED";
    first
        .inject_keys(format!("{USER_PROMPT}\r").as_bytes())
        .expect("create a session with one ordinary user turn");
    first
        .wait_for_text("GOAL_SESSION_READY", Duration::from_secs(45))
        .expect("seed user turn completes");

    let mut held_goal_turn = content.expect_agent_turn_blocked(
        "hold first Goal continuation until paused",
        "GOAL_FIRST_CONTINUATION_HELD",
    );
    first
        .inject_keys(b"/goal set cold resume goal restart fixture\r")
        .expect("create isolated Goal through /goal set");
    first
        .wait_for_text("Goal created and activated", Duration::from_secs(15))
        .unwrap_or_else(|_| {
            panic!(
                "/goal set did not activate Goal; screen:\n{}",
                first.screen_contents()
            )
        });
    tokio::time::timeout(Duration::from_secs(15), held_goal_turn.wait_blocked())
        .await
        .expect("first Goal continuation reaches the provider");
    first
        .wait_for_text("GOAL_FIRST_CONTINUATION_HELD", Duration::from_secs(15))
        .expect("hold the first model turn while the Goal is active");

    first
        .inject_keys(b"/goal pause\r")
        .expect("pause the active Goal with /goal pause");
    first
        .wait_for_text("User paused current Goal", Duration::from_secs(15))
        .unwrap_or_else(|_| {
            panic!(
                "/goal pause did not pause the Goal; screen:\n{}",
                first.screen_contents()
            )
        });
    first
        .inject_keys(b"\x04\x04")
        .expect("suspend the paused Goal with Ctrl+D twice");
    assert_eq!(
        first
            .wait_for_exit_and_drain(Duration::from_secs(15), Duration::from_secs(1))
            .expect("wait for confirmed suspension exit"),
        0,
        "paused Goal session should suspend normally"
    );
    held_goal_turn.release();
    let session_path = session_dir_with_prompt(&content, USER_PROMPT);
    let timeline_path = session_path.join("timeline.jsonl");
    let suspended = std::fs::read_to_string(&timeline_path).expect("read suspended Timeline");
    let goal_continuation_count = |timeline: &str| {
        parsed_timeline(timeline)
            .filter(|event| {
                event.get("type").and_then(serde_json::Value::as_str) == Some("turn")
                    && event
                        .pointer("/event/state")
                        .and_then(serde_json::Value::as_str)
                        == Some("started")
                    && event
                        .pointer("/event/identity/origin")
                        .and_then(serde_json::Value::as_str)
                        == Some("goal_continuation")
            })
            .count()
    };
    let suspended_goal_turns = goal_continuation_count(&suspended);
    assert!(
        suspended_goal_turns >= 1,
        "the active Goal must have started an internally-owned continuation before pause"
    );
    drop(first);

    content.set_response("GOAL_RESTART_AUTOCONTINUED");
    let mut resumed = PtyHarness::spawn_with_content_env_in_dir(
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
        &[("GROW_GOAL", "1")],
        Some(project.path()),
    )
    .expect("cold --continue with Goal enabled");
    resumed
        .wait_for_text(WELCOME_SCREEN_SENTINEL, RESUME_TIMEOUT)
        .unwrap_or_else(|_| {
            panic!(
                "cold --continue did not restore the session; screen:\n{}",
                resumed.screen_contents()
            )
        });
    assert_eq!(session_path, session_dir(&content, &mut resumed));
    let before_restart = poll_for(RESUME_TIMEOUT, || {
        resumed.update(Duration::from_millis(30));
        let contents = std::fs::read_to_string(&timeline_path).ok()?;
        parsed_timeline(&contents)
            .any(|event| {
                event
                    .pointer("/event/kind")
                    .and_then(serde_json::Value::as_str)
                    == Some("execution_released")
            })
            .then_some(contents)
    })
    .expect("the cold runtime must release execution before restart");
    assert_eq!(
        goal_continuation_count(&before_restart),
        suspended_goal_turns,
        "a paused Goal must remain idle until the user explicitly restarts it"
    );

    let _complete_restarted_goal = expect_tool_turn(
        &content,
        "call_goal_resume_complete",
        "update_goal",
        json!({
            "status": "complete",
            "completion_report": "Results: the resumed Goal continuation ran. Evidence: this scripted provider turn reached update_goal after /goal restart. Verification: the PTY fixture observed the goal_continuation Timeline entry. Limitations: this fixture verifies lifecycle recovery only."
        })
        .to_string(),
    );
    resumed
        .wait_for_text("Ctrl+x:commands", RESUME_TIMEOUT)
        .expect("the resumed Agent composer is ready for input");
    resumed
        .inject_keys(b"/goal restart")
        .expect("type restart into the resumed composer");
    resumed
        .wait_for_text("❯ /goal restart", Duration::from_secs(5))
        .expect("the composer received the restart command");
    resumed.inject_keys(b"\r").expect("submit Goal restart");
    resumed
        .wait_for_text("Goal restarted", Duration::from_secs(15))
        .unwrap_or_else(|_| {
            panic!(
                "/goal restart was not accepted; screen:\n{}",
                resumed.screen_contents()
            )
        });
    let after_restart = poll_for(RESUME_TIMEOUT, || {
        resumed.update(Duration::from_millis(30));
        let contents = std::fs::read_to_string(&timeline_path).ok()?;
        let goal_completed = parsed_timeline(&contents).any(|event| {
            event
                .pointer("/event/snapshot/goal/status")
                .and_then(serde_json::Value::as_str)
                == Some("complete")
        });
        (goal_completed && goal_continuation_count(&contents) > suspended_goal_turns)
            .then_some(contents)
    })
    .unwrap_or_else(|| {
        panic!(
            "restarted Goal did not continue and complete without a user prompt; screen:
{}",
            resumed.screen_contents()
        )
    });
    eprintln!(
        "Goal continuation turns: {suspended_goal_turns} before restart, {} after restart",
        goal_continuation_count(&after_restart)
    );
    assert!(
        goal_continuation_count(&after_restart) > suspended_goal_turns,
        "/goal restart must start a new turn with origin=goal_continuation"
    );
    assert_eq!(
        started_prompt_count(&after_restart, USER_PROMPT),
        1,
        "cold resume and Goal restart must not duplicate or require an ordinary user prompt"
    );
    assert!(
        !resumed.contains_text("panicked"),
        "Pager panicked during Goal restart:\n{}",
        resumed.screen_contents()
    );
    resumed.quit().expect("quit resumed Pager");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run the owning pty_e2e_* Cargo test with --ignored"]
async fn ctrl_d_cold_resume_keeps_the_original_explicit_skill_snapshot() {
    skill_snapshot_survives_suspend_and_surface_replay(false).await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run the owning pty_e2e_* Cargo test with --ignored"]
async fn minimal_cold_resume_keeps_the_original_explicit_skill_snapshot() {
    skill_snapshot_survives_suspend_and_surface_replay(true).await;
}

#[cfg(unix)]
async fn skill_snapshot_survives_suspend_and_surface_replay(minimal: bool) {
    const USER_PROMPT: &str = "/test-skill inspect SKILL_SOURCE_FREEZE_ONCE";
    const ORIGINAL_BODY: &str = "ORIGINAL_SKILL_GUIDANCE_SNAPSHOT";
    const UPDATED_BODY: &str = "UPDATED_SKILL_GUIDANCE_AFTER_SUSPEND";

    let content = ContentController::start()
        .await
        .expect("start mock content");
    content
        .seed_llm_config()
        .expect("seed isolated mock provider");

    let project = tempfile::tempdir().expect("create project directory");
    git2::Repository::init(project.path()).expect("git init project");
    let skill_file = project.path().join(".grow/skills/test-skill/SKILL.md");
    std::fs::create_dir_all(skill_file.parent().unwrap()).expect("create skill directory");
    let write_skill = |body: &str| {
        std::fs::write(
            &skill_file,
            format!(
                "---\nname: test-skill\ndescription: PTY source freeze test.\nuser-invocable: true\ndisable-model-invocation: true\n---\n\n{body}\n"
            ),
        )
        .expect("write workspace Skill");
    };
    write_skill(ORIGINAL_BODY);

    let marker = project.path().join("skill-command-runs.txt");
    let pid_file = project.path().join("skill-command-child.pid");
    let command = format!(
        "printf 'started\\n' >> {marker}; /bin/sleep 20 & sleep_pid=$!; printf '%s\\n' \"$sleep_pid\" > {pid}; wait \"$sleep_pid\"; printf 'finished\\n' >> {marker}; echo SKILL_COMMAND_FINISHED",
        marker = shell_quote(&marker),
        pid = shell_quote(&pid_file),
    );
    let args = json!({
        "command": command,
        "description": "suspend after explicit Skill guidance is accepted"
    })
    .to_string();
    let _command_turn = expect_tool_turn(
        &content,
        "call_skill_suspend_command",
        "run_terminal_command",
        args,
    );
    content.set_chunk_delay(Some(Duration::from_millis(75)));
    content.set_response("SKILL_RESUMED_RESULT");

    let binary = pager_binary().expect("resolve Pager binary");
    let mut spawn_args = vec!["--permission-mode", "always-approve", "--trust"];
    if minimal {
        spawn_args.push("--minimal");
    }
    let mut first = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &spawn_args,
        Some(project.path()),
    )
    .expect("spawn first Pager process");
    if minimal {
        first.set_respond_to_queries(true);
        wait_minimal_ready(&mut first);
    } else {
        first
            .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
            .expect("welcome screen");
    }
    first
        .inject_keys(b"/test-skill")
        .expect("promote Welcome to a session and discover workspace Skills");
    first
        .wait_for_text("PTY source freeze test.", Duration::from_secs(20))
        .expect("the real Shell advertised the Skill before dispatch");
    first
        .inject_keys(format!("{}\r", USER_PROMPT.trim_start_matches("/test-skill")).as_bytes())
        .expect("submit explicit Skill prompt");

    poll_for(Duration::from_secs(45), || {
        first.update(Duration::from_millis(30));
        std::fs::read_to_string(&marker)
            .ok()
            .filter(|contents| contents.lines().any(|line| line == "started") && pid_file.exists())
    })
    .unwrap_or_else(|| {
        panic!(
            "the command did not start after loading the explicit Skill; screen:\n{}\nrequests:\n{}",
            first.screen_contents(),
            dump_non_system_messages(&content.request_bodies())
        )
    });

    let initial_requests = content.request_bodies();
    assert!(
        initial_requests
            .iter()
            .any(|request| request.to_string().contains(ORIGINAL_BODY)),
        "the first accepted provider request must contain the prepared original Skill: {initial_requests:#?}"
    );
    let session_path = session_dir_with_prompt(&content, USER_PROMPT);
    let timeline_path = session_path.join("timeline.jsonl");
    let before = std::fs::read_to_string(&timeline_path).expect("read pre-suspension Timeline");
    let (skill_call_id, skill_body) = committed_user_skill(&before);
    assert_eq!(
        skill_body,
        // The on-disk body ends in a newline; argument appending preserves it.
        format!("{ORIGINAL_BODY}\n\n\n**ARGUMENTS:** inspect SKILL_SOURCE_FREEZE_ONCE")
    );
    assert_eq!(committed_user_skill_count(&before), 1);
    assert_eq!(
        tool_lifecycle_count(&before, &skill_call_id, "load_skill", "started"),
        1
    );
    assert_eq!(
        tool_lifecycle_count(&before, &skill_call_id, "load_skill", "completed"),
        1
    );

    first
        .inject_keys(b"\x04\x04")
        .expect("suspend during the running command");
    let exit = first
        .wait_for_exit_and_drain(Duration::from_secs(15), Duration::from_secs(1))
        .expect("wait for confirmed suspension exit");
    assert_eq!(exit, 0, "confirmed suspension should exit normally");
    let command_pid = std::fs::read_to_string(&pid_file)
        .expect("running command recorded its child process")
        .trim()
        .parse::<libc::pid_t>()
        .expect("valid child pid");
    assert!(
        poll_for(Duration::from_secs(5), || {
            (!process_alive(command_pid)).then_some(())
        })
        .is_some()
    );
    let suspended = std::fs::read_to_string(&timeline_path).expect("read suspended Timeline");
    assert_eq!(
        interrupted_command_settlement_count(&suspended, "call_skill_suspend_command"),
        1
    );
    drop(first);

    // The cold process sees a different on-disk catalog. Recovery must keep
    // using the already committed Skill item and input identity.
    write_skill(UPDATED_BODY);
    let resume_log = project.path().join("resume-debug.log");
    let mut resume_args = vec![
        "--continue",
        "--permission-mode",
        "always-approve",
        "--trust",
        "--debug-file",
        resume_log.to_str().expect("UTF-8 fixture path"),
    ];
    if minimal {
        resume_args.push("--minimal");
    }
    let mut resumed = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &resume_args,
        Some(project.path()),
    )
    .expect("spawn cold --continue process");
    if minimal {
        resumed.set_respond_to_queries(true);
    }
    resumed
        .wait_for_text("SKILL_RESUMED_RESULT", RESUME_TIMEOUT)
        .unwrap_or_else(|_| {
            panic!(
                "the original Turn did not resume; screen:\n{}\nrequests:\n{}\nresume log:\n{}",
                resumed.screen_contents(),
                dump_non_system_messages(&content.request_bodies()),
                std::fs::read_to_string(&resume_log).unwrap_or_default(),
            )
        });

    let resumed_requests = content.request_bodies();
    assert!(
        resumed_requests
            .iter()
            .skip(initial_requests.len())
            .any(|request| { request.to_string().contains(ORIGINAL_BODY) }),
        "a post-resume provider request must receive the committed Skill snapshot: {resumed_requests:#?}"
    );
    assert!(
        resumed_requests
            .iter()
            .all(|request| !request.to_string().contains(UPDATED_BODY)),
        "cold resume must not reread the modified Skill source: {resumed_requests:#?}"
    );

    let after = poll_for(Duration::from_secs(45), || {
        resumed.update(Duration::from_millis(30));
        std::fs::read_to_string(&timeline_path)
            .ok()
            .filter(|contents| turn_has_ended(contents, &started_turn_ids(&before)[0]))
    })
    .expect("resume should end the original Turn");
    assert_eq!(session_path, session_dir_with_prompt(&content, USER_PROMPT));
    let initial_turns = started_turn_ids(&before);
    assert_eq!(started_turn_ids(&after), initial_turns);
    assert_eq!(started_prompt_count(&after, USER_PROMPT), 1);
    assert_eq!(consumed_user_input_count(&after, USER_PROMPT), 1);
    assert_eq!(committed_user_skill_count(&after), 1);
    assert_eq!(
        tool_lifecycle_count(&after, &skill_call_id, "load_skill", "started"),
        1
    );
    assert_eq!(
        tool_lifecycle_count(&after, &skill_call_id, "load_skill", "completed"),
        1
    );
    assert_eq!(
        interrupted_command_settlement_count(&after, "call_skill_suspend_command"),
        1
    );
    let runs = std::fs::read_to_string(&marker).expect("read command marker");
    assert_eq!(runs.lines().filter(|line| *line == "started").count(), 1);
    assert_eq!(runs.lines().filter(|line| *line == "finished").count(), 0);
    resumed
        .inject_keys(b"\x04\x04")
        .expect("suspend completed Skill session");
    assert_eq!(
        resumed
            .wait_for_exit_and_drain(Duration::from_secs(15), Duration::from_secs(1))
            .unwrap(),
        0
    );

    let session_id = session_path.file_name().unwrap().to_str().unwrap();
    let export_parent = tempfile::tempdir().expect("create Skill export parent");
    let export_path = export_parent.path().join("skill-export");
    let exported = std::process::Command::new(&binary)
        .args(["export", session_id])
        .arg(&export_path)
        .envs(content.sandbox().env())
        .current_dir(project.path())
        .output()
        .expect("export the original Skill snapshot");
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stderr)
    );
    let markdown = std::fs::read_to_string(export_path.join("transcript.md")).unwrap();
    assert!(
        markdown.contains(ORIGINAL_BODY),
        "export must retain the prepared snapshot: {markdown}"
    );
    assert!(!markdown.contains(UPDATED_BODY));

    let requests_before_headless = content.request_bodies().len();
    let output = tokio::time::timeout(
        Duration::from_secs(45),
        tokio::process::Command::new(&binary)
            .args([
                "--continue",
                "--permission-mode",
                "always-approve",
                "--trust",
                "--output-format",
                "streaming-json",
                "-p",
                "Observe the completed history without loading another Skill",
            ])
            .envs(content.sandbox().env())
            .current_dir(project.path())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("headless cold continuation must terminate")
    .expect("run headless continuation");
    assert!(
        output.status.success(),
        "headless failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let streamed = String::from_utf8(output.stdout).unwrap();
    let historical_results = streamed
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|line| {
            line.get("toolCallId").and_then(serde_json::Value::as_str) == Some(&skill_call_id)
                && line
                    .pointer("/rawOutput/value/prepared_body")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|body| body.contains(ORIGINAL_BODY))
        })
        .count();
    assert_eq!(
        historical_results, 0,
        "headless noReplay must not emit old Skill calls as current output: {streamed}"
    );
    assert!(streamed.contains("SKILL_RESUMED_RESULT"), "{streamed}");
    assert!(!streamed.contains(UPDATED_BODY));
    let headless_requests = content.request_bodies();
    assert!(
        headless_requests
            .iter()
            .skip(requests_before_headless)
            .any(|request| { request.to_string().contains(ORIGINAL_BODY) }),
        "headless cold continuation must use the same committed Skill context: {headless_requests:#?}"
    );
    assert!(
        headless_requests
            .iter()
            .all(|request| !request.to_string().contains(UPDATED_BODY))
    );
    let final_timeline = std::fs::read_to_string(&timeline_path).unwrap();
    assert_eq!(committed_user_skill_count(&final_timeline), 1);
    assert_eq!(
        std::fs::read_to_string(&marker)
            .unwrap()
            .lines()
            .filter(|line| *line == "started")
            .count(),
        1
    );
}

// Startup can persist an empty Session before the view dispatches its input.
// Directory order is not the identity of the Session whose Turn we suspend.
fn session_dir_with_prompt(content: &ContentController, prompt: &str) -> PathBuf {
    let sessions = content.home().join(".grow/sessions");
    let mut matches = Vec::new();
    for cwd in std::fs::read_dir(&sessions)
        .expect("sessions directory")
        .flatten()
    {
        let Ok(entries) = std::fs::read_dir(cwd.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            if std::fs::read_to_string(entry.path().join("timeline.jsonl"))
                .is_ok_and(|timeline| started_prompt_count(&timeline, prompt) == 1)
            {
                matches.push(entry.path());
            }
        }
    }
    assert_eq!(
        matches.len(),
        1,
        "expected one durable Turn for {prompt:?}: {matches:?}"
    );
    matches.pop().unwrap()
}

fn committed_user_skill(contents: &str) -> (String, String) {
    parsed_timeline(contents)
        .find_map(|event| {
            if event.get("type")?.as_str()? != "input"
                || event.pointer("/event/state")?.as_str()? != "consumed"
            {
                return None;
            }
            let skills = event.pointer("/event/skill_loads")?.as_array()?;
            if skills.len() != 1 {
                return None;
            }
            let skill = skills.first()?;
            if skill.get("source").and_then(serde_json::Value::as_str) != Some("user") {
                return None;
            }
            Some((
                skill.get("call_id")?.as_str()?.to_owned(),
                skill.get("prepared_body")?.as_str()?.to_owned(),
            ))
        })
        .unwrap_or_else(|| panic!("one committed explicit user Skill item; Timeline:\n{contents}"))
}

fn committed_user_skill_count(contents: &str) -> usize {
    parsed_timeline(contents)
        .filter(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("input")
                && event
                    .pointer("/event/state")
                    .and_then(serde_json::Value::as_str)
                    == Some("consumed")
                && event
                    .pointer("/event/skill_loads")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|skills| !skills.is_empty())
        })
        .count()
}

fn tool_lifecycle_count(contents: &str, call_id: &str, name: &str, state: &str) -> usize {
    parsed_timeline(contents)
        .filter(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("tool")
                && event
                    .pointer("/event/state")
                    .and_then(serde_json::Value::as_str)
                    == Some(state)
                && event
                    .pointer("/event/call_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(call_id)
                && event
                    .pointer("/event/name")
                    .and_then(serde_json::Value::as_str)
                    == Some(name)
        })
        .count()
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

fn parsed_timeline(contents: &str) -> std::vec::IntoIter<serde_json::Value> {
    shell::session::testkit::decode_timeline_fixture(contents)
        .expect("valid checksummed Timeline journal")
        .events()
        .iter()
        .map(|event| serde_json::to_value(event).expect("serialize typed Timeline event"))
        .collect::<Vec<_>>()
        .into_iter()
}

fn started_turn_ids(contents: &str) -> Vec<String> {
    parsed_timeline(contents)
        .filter_map(|event| {
            (event.get("type")?.as_str()? == "turn"
                && event.pointer("/event/state")?.as_str()? == "started")
                .then(|| event.pointer("/event/id")?.as_str().map(str::to_owned))
                .flatten()
        })
        .collect()
}

fn turn_has_ended(contents: &str, turn_id: &str) -> bool {
    parsed_timeline(contents).any(|event| {
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

fn started_prompt_count(contents: &str, prompt: &str) -> usize {
    parsed_timeline(contents)
        .filter(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("turn")
                && event
                    .pointer("/event/state")
                    .and_then(serde_json::Value::as_str)
                    == Some("started")
                && event
                    .pointer("/event/prompt_text")
                    .and_then(serde_json::Value::as_str)
                    == Some(prompt)
        })
        .count()
}

fn consumed_user_input_count(contents: &str, prompt: &str) -> usize {
    let wrapped_prompt = format!("<user_query>\n{prompt}\n</user_query>");
    parsed_timeline(contents)
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

fn interrupted_command_settlement_count(contents: &str, call_id: &str) -> usize {
    parsed_timeline(contents)
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
    parsed_timeline(contents)
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
