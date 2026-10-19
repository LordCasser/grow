//! Exercise a real process exit during a running root tool and automatic
//! recovery from the same session in a fresh Pager process.
#[allow(unused_imports)]
use super::common::*;

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run the owning pty_e2e_* Cargo test with --ignored"]
async fn ctrl_c_suspends_running_root_and_continue_resumes_without_replay() {
    let content = ContentController::start()
        .await
        .expect("start mock content");
    content
        .seed_llm_config()
        .expect("seed isolated mock provider");

    let project = tempfile::tempdir().expect("create project directory");
    std::fs::create_dir_all(project.path().join(".git")).expect("create .git");
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
        .inject_keys(b"\x03")
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
    let session_path = session_dir(&content, &mut first);
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

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run the owning pty_e2e_* Cargo test with --ignored"]
async fn ctrl_c_cold_resume_keeps_the_original_explicit_skill_snapshot() {
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
    let session_path = session_dir(&content, &mut first);
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
        .inject_keys(b"\x03")
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
    let mut resume_args = vec![
        "--continue",
        "--permission-mode",
        "always-approve",
        "--trust",
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
                "the original Turn did not resume; screen:\n{}\nrequests:\n{}",
                resumed.screen_contents(),
                dump_non_system_messages(&content.request_bodies())
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
    assert_eq!(session_path, session_dir(&content, &mut resumed));
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
        .inject_keys(b"\x03")
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

fn committed_user_skill(contents: &str) -> (String, String) {
    let skill = parsed_timeline(contents)
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
        .expect("one committed explicit user Skill item");
    skill
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

fn parsed_timeline(contents: &str) -> impl Iterator<Item = serde_json::Value> + '_ {
    contents
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
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
