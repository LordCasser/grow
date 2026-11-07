//! Real provider → Tools/Shell → Pager presentation → later ordinary input.
//! No ACP handler or tool result is replaced by a test acknowledgement.
#![cfg(unix)]

use super::common::*;
use serde_json::Value;

const CALL: &str = "call_pty_advisory_late_reply";
const QUESTION: &str = "ADVISORY_ROUTE: which label should I use?";
const NOTES_QUESTION: &str = "ADVISORY_NOTES: any additional context?";
const RECOMMENDATION: &str = "/new";
const ANSWER: &str = "/quit";
const NOTES: &str = "/new LATE_REPLY_NOTES_LITERAL";
const ORIGINAL_DONE: &str = "ADVISORY_ORIGINAL_TURN_DONE";
const REPLY_DONE: &str = "ADVISORY_REAL_REPLY_TURN_DONE";
const SETTING: &str = "Allow non-blocking questions";
const NO_REQUEST_WINDOW: Duration = Duration::from_millis(900);
const WAIT: Duration = Duration::from_secs(30);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "real PTY; requires parent-designated freshly built PAGER_BINARY"]
async fn advisory_continues_then_reply_is_one_new_literal_input_and_replays_read_only() {
    let content = isolated_content().await;
    let project = tempfile::tempdir().expect("isolated project");
    git2::Repository::init(project.path()).expect("initialize isolated project");
    let ask = expect_tool_turn(&content, CALL, "ask_user_question", advisory_arguments());
    let mut continuation =
        content.expect_agent_turn_blocked("unanswered advisory continuation", ORIGINAL_DONE);
    let binary = designated_binary();
    let mut pager = spawn(&binary, &content, project.path(), false);
    pager
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome");
    pager
        .inject_keys(b"ASK_ADVISORY_PTY\r")
        .expect("submit actual user prompt");

    tokio::time::timeout(WAIT, continuation.wait_blocked())
        .await
        .expect("provider received continuation without a human answer");
    ask.assert_satisfied();
    reveal_advisory_card(&mut pager);
    pager
        .wait_for_text(QUESTION, WAIT)
        .expect("actual question is visible");
    pager
        .wait_for_text("agent recommendation: /new", WAIT)
        .expect("exact recommendation is visible");
    assert!(
        !pager.contains_text("Answered — read-only"),
        "presentation must not claim a human answer"
    );
    let before_reply = foreground_bodies(&content);
    assert_eq!(
        before_reply.len(),
        2,
        "one ask and one unanswered continuation"
    );
    assert_one_result_per_request(&before_reply, CALL);
    let result = tool_result_text(before_reply.last().unwrap(), CALL);
    assert!(
        result.contains("the user has NOT answered"),
        "recommendation is not a human answer: {result}"
    );
    assert!(
        result.contains(QUESTION) && result.contains(RECOMMENDATION),
        "recommendation retains its question: {result}"
    );

    // The continuation is still running. Advisory presentation must leave
    // the ordinary composer usable; the later editor borrows this draft.
    pager
        .inject_keys(b" ")
        .expect("focus ordinary composer from expanded card");
    pager.update(Duration::from_millis(50));
    pager
        .inject_keys(b"ADVISORY_COMPOSER_DRAFT")
        .expect("type independent composer draft");
    pager
        .wait_for_text("ADVISORY_COMPOSER_DRAFT", WAIT)
        .expect("composer remains usable during original Turn");
    assert!(composer_holds(&pager, "ADVISORY_COMPOSER_DRAFT"));
    continuation.release();
    pager
        .wait_for_text(ORIGINAL_DONE, WAIT)
        .expect("normal final body");
    pager
        .wait_for_turn_idle(WAIT)
        .expect("original Turn ended in the UI");
    let directory = poll_for(WAIT, || {
        pager.update(Duration::from_millis(20));
        session_with_call(&content, CALL)
    })
    .expect("canonical source session");
    let source = directory.file_name().unwrap().to_str().unwrap();
    let stable_id = format!("question-reply:{source}:{CALL}");
    let original_end = poll_for(WAIT, || {
        pager.update(Duration::from_millis(20));
        original_turn_end(&events(&directory), CALL)
    })
    .expect("canonical original Turn end before clicking Reply");
    assert_single_settlement(&events(&directory), CALL);

    // The original Turn has durably ended before the real Reply action.
    // QuestionView must stash and restore the independent composer draft.
    let original_sessions = session_directories(&content);
    record_checkpoint(
        &pager,
        &content,
        Some(&directory),
        "advisory_original_ended",
    );
    let reply_turn = content.expect_agent_turn("actual later human input", REPLY_DONE);
    if std::env::var_os("GROW_PTY_REPLY_MOUSE_PROBE").is_some() {
        // Optional bounded diagnostic: exercise the visible Reply body with
        // the product's existing text-selection setting, not the tool header.
        double_click_text(&mut pager, "Reply with your own answer");
    } else {
        activate_reply_action(&mut pager, "Reply with your own answer");
    }
    pager.wait_for_text("/quit", WAIT).unwrap_or_else(|error| {
        record_checkpoint(
            &pager,
            &content,
            Some(&directory),
            "advisory_initial_reply_unknown",
        );
        panic!(
            "real QuestionView option: {error}\n{}",
            pager.screen_contents()
        );
    });
    record_checkpoint(&pager, &content, Some(&directory), "advisory_editor_open");
    pager
        .wait_for_text("unselect", WAIT)
        .expect("QuestionView navigation owns keys");
    // Two questions let the first changed selection and the second real
    // freeform note travel together through QuestionView's existing editor.
    pager
        .inject_keys(keys::DOWN)
        .expect("choose a different option");
    pager
        .inject_keys(keys::ENTER)
        .expect("accept /quit as label data and advance");
    pager.update(Duration::from_millis(50));
    record_checkpoint(
        &pager,
        &content,
        Some(&directory),
        "advisory_second_question",
    );
    pager
        .wait_for_text(NOTES_QUESTION, WAIT)
        .expect("second question editor");
    pager
        .inject_keys(b"z")
        .expect("open existing Other notes input");
    pager
        .wait_for_text("back", WAIT)
        .expect("QuestionView Other input owns keys");
    record_checkpoint(&pager, &content, Some(&directory), "advisory_other_input");
    pager
        .inject_keys(format!("\x1b[200~{NOTES}\x1b[201~").as_bytes())
        .expect("paste slash-style notes as data");
    pager
        .wait_for_text("LATE_REPLY_NOTES_LITERAL", WAIT)
        .expect("real edited notes visible");
    pager
        .inject_keys(keys::ENTER)
        .expect("submit edited question reply");
    pager.update(Duration::from_millis(50));
    record_checkpoint(
        &pager,
        &content,
        Some(&directory),
        "advisory_reply_submitted",
    );
    pager.wait_for_text(REPLY_DONE, WAIT).unwrap_or_else(|error| {
        record_checkpoint(&pager, &content, Some(&directory), "advisory_reply_body_timeout");
        if std::env::var_os("GROW_PTY_REPLY_NAV_PROBE").is_some() {
            let counts = || {
                let input_events = events(&directory).iter().filter(|event| event["type"] == "input").count();
                let rpc = std::fs::read_to_string(content.home().join(".grow/pty-admission-rpc.log")).expect("isolated admission RPC log");
                let queries = rpc.lines().filter(|line| line.contains("sending \"ext_method\" request:") && line.contains(&stable_id)).count();
                (foreground_bodies(&content).len(), input_events, queries)
            };
            let before = counts();
            eprintln!("mouse nav probe before: provider/input/query={before:?} marker_visible={}", pager.contains_text(REPLY_DONE));
            let screen = pager.screen_contents();
            let (row, col) = locate_screen_text(&screen, "Worked for").expect("visible Worked row for transcript focus");
            let select = format!("{}{}", sgr_mouse(0, row, col, 'M'), sgr_mouse(0, row, col, 'm'));
            pager.inject_keys(select.as_bytes()).expect("single click Worked row to focus transcript");
            pager.update(Duration::from_millis(100));
            record_checkpoint(&pager, &content, Some(&directory), "advisory_reply_nav_click");
            let after_click = counts();
            eprintln!("mouse nav probe click: provider/input/query={after_click:?} marker_visible={}", pager.contains_text(REPLY_DONE));
            assert_eq!(after_click, before, "single focus click must not create a provider request, canonical input event, or admission query");
            for step in 1..=2 {
                pager.inject_keys(b"\x1b[6~").expect("one PageDown navigation probe");
                pager.update(Duration::from_millis(250));
                record_checkpoint(&pager, &content, Some(&directory), &format!("advisory_reply_nav_page_down_{step}"));
                let after = counts();
                eprintln!("mouse nav probe PageDown {step}: provider/input/query={after:?} marker_visible={}", pager.contains_text(REPLY_DONE));
                assert_eq!(after, before, "PageDown must not create a provider request, canonical input event, or admission query");
            }
        }
        panic!("new human Turn response: {error}\n{}", pager.screen_contents());
    });
    pager.wait_for_turn_idle(WAIT).expect("reply Turn ended");
    reply_turn.assert_satisfied();
    assert!(
        composer_holds(&pager, "ADVISORY_COMPOSER_DRAFT"),
        "question editor restores composer draft\n{}",
        pager.screen_contents()
    );

    let bodies = foreground_bodies(&content);
    assert_eq!(
        bodies.len(),
        3,
        "late answer creates exactly one new provider request"
    );
    let human = user_text(bodies.last().unwrap());
    for text in [
        QUESTION,
        NOTES_QUESTION,
        RECOMMENDATION,
        ANSWER,
        NOTES,
        source,
        CALL,
    ] {
        assert!(
            human.contains(text),
            "real user input lost {text:?}: {human}"
        );
    }
    assert!(
        human.contains("My answer: /quit"),
        "selection must be attributed to the human: {human}"
    );
    assert!(
        human.contains("You continued with your recommendation: /new"),
        "recommendation retains agent provenance: {human}"
    );
    assert_one_result_per_request(&bodies, CALL);
    let accepted = poll_for(WAIT, || {
        pager.update(Duration::from_millis(20));
        let recorded = events(&directory);
        reply_submission(&directory, &recorded, &stable_id).map(|input| (recorded, input))
    })
    .expect("stable reply input was durably submitted");
    assert_reply_lifecycle(&accepted.0, &accepted.1, original_end, &stable_id);
    assert_single_settlement(&accepted.0, CALL);
    record_checkpoint(
        &pager,
        &content,
        Some(&directory),
        "advisory_reply_admitted",
    );
    // A new Turn follows the transcript tail; navigate back to the original
    // card without invoking Reply or issuing another admission query.
    show_original_card_from_prompt(&mut pager);
    pager
        .wait_for_text("Answered — read-only", WAIT)
        .unwrap_or_else(|error| {
            record_checkpoint(
                &pager,
                &content,
                Some(&directory),
                "advisory_reply_unconfirmed",
            );
            panic!(
                "canonical accepted card: {error}\n{}",
                pager.screen_contents()
            );
        });
    record_checkpoint(&pager, &content, Some(&directory), "advisory_live_answered");
    assert_no_new_requests(&mut pager, &content, 3);
    assert_eq!(
        session_directories(&content),
        original_sessions,
        "slash-style answer data must not create a session"
    );
    write_cast_if_requested(&pager, "advisory_reply_main.cast");
    // Discard only the test's unsent composer draft, then end the process
    // cleanly so --continue has an authoritative retired writer to replay.
    pager
        .inject_keys(b" ")
        .expect("focus restored ordinary composer");
    pager.update(Duration::from_millis(50));
    pager
        .inject_keys(b"\x15")
        .expect("clear test composer draft");
    quit_cleanly(&mut pager);
    drop(pager);

    let mut replay = spawn(&binary, &content, project.path(), true);
    replay
        .wait_for_text(REPLY_DONE, RESUME_TIMEOUT)
        .expect("same session replay");
    replay
        .wait_for_turn_idle(WAIT)
        .expect("replayed session idle");
    show_original_card_from_prompt(&mut replay);
    // A cold card queries its stable identity through the real Reply action.
    // The canonical answer must prevent reopening an editable question.
    if !replay.contains_text("Answered — read-only") {
        activate_reply_action(&mut replay, "Reply with your own answer");
        // Selecting the same card shortly after expanding it can fold it.
        // Restore visibility only; this must not issue another Reply query.
        replay.update(Duration::from_millis(50));
        reveal_advisory_card(&mut replay);
    }
    replay
        .wait_for_text("Answered — read-only", WAIT)
        .unwrap_or_else(|error| {
            record_checkpoint(
                &replay,
                &content,
                Some(&directory),
                "advisory_cold_reply_unconfirmed",
            );
            panic!(
                "cold canonical answer is read-only: {error}\n{}",
                replay.screen_contents()
            );
        });
    record_checkpoint(
        &replay,
        &content,
        Some(&directory),
        "advisory_cold_answered",
    );
    double_click_text(&mut replay, "Answered — read-only");
    replay
        .inject_keys(b" ")
        .expect("focus ordinary composer after read-only card click");
    replay.update(Duration::from_millis(50));
    replay
        .inject_keys(b"READ_ONLY_COMPOSER_PROBE")
        .expect("read-only click leaves ordinary input usable");
    replay
        .wait_for_text("READ_ONLY_COMPOSER_PROBE", WAIT)
        .expect("ordinary composer probe");
    assert!(
        composer_holds(&replay, "READ_ONLY_COMPOSER_PROBE"),
        "answered card must not reopen a question editor\n{}",
        replay.screen_contents()
    );
    replay
        .inject_keys(b"\x15")
        .expect("clear unsent replay probe");
    assert_no_new_requests(&mut replay, &content, 3);
    let after = events(&directory);
    let replay_input =
        reply_submission(&directory, &after, &stable_id).expect("same durable reply after replay");
    assert_eq!(
        accepted.1, replay_input,
        "replay retains the original input identity"
    );
    assert_reply_lifecycle(&after, &replay_input, original_end, &stable_id);
    assert_single_settlement(&after, CALL);
    assert_eq!(
        session_with_call(&content, CALL).as_ref(),
        Some(&directory),
        "slash-style label must not create a new session"
    );
    write_cast_if_requested(&replay, "advisory_reply_continue.cast");
    quit_cleanly(&mut replay);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "real PTY; requires parent-designated freshly built PAGER_BINARY"]
async fn settings_saved_false_forces_next_same_alive_and_cold_ask_to_wait_for_real_answer() {
    let content = isolated_content().await;
    let project = tempfile::tempdir().expect("isolated project");
    git2::Repository::init(project.path()).expect("initialize isolated project");
    let setup = content.expect_agent_turn(
        "establish live session before Settings",
        "SETTINGS_LIVE_SESSION_READY",
    );
    let binary = designated_binary();
    let mut pager = spawn(&binary, &content, project.path(), false);
    pager
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome");
    pager
        .inject_keys(b"SETTINGS_START\r")
        .expect("establish actual session");
    pager
        .wait_for_text("SETTINGS_LIVE_SESSION_READY", WAIT)
        .expect("setup response");
    pager.wait_for_turn_idle(WAIT).expect("setup Turn ended");
    setup.assert_satisfied();

    pager.inject_keys(keys::F2).expect("actual Settings entry");
    pager
        .wait_for_text("Appearance", WAIT)
        .expect("Settings modal");
    pager.inject_keys(b"/").expect("Settings search");
    inject_keys_paced(&mut pager, b"Allow non-blocking questions");
    pager
        .wait_for_text(SETTING, WAIT)
        .expect("Agent preference row");
    pager
        .inject_keys(keys::ENTER)
        .expect("commit Settings filter");
    pager
        .inject_keys(b" ")
        .expect("save preference false through Settings");
    poll_for(WAIT, || {
        pager.update(Duration::from_millis(20));
        let cfg = saved_config(&content);
        cfg.get("toolset")
            .and_then(|table| table.get("ask_user_question"))
            .and_then(|table| table.get("allow_non_blocking"))
            .and_then(toml::Value::as_bool)
            .is_some_and(|allowed| !allowed)
            .then_some(())
    })
    .expect("Settings effect durably saved false");
    let cfg = saved_config(&content);
    assert_eq!(
        cfg["toolset"]["ask_user_question"]["timeout_enabled"].as_bool(),
        Some(false)
    );
    assert_eq!(
        cfg["toolset"]["ask_user_question"]["timeout_secs"].as_integer(),
        Some(71)
    );
    assert_eq!(
        cfg["advisory_pty_fixture"]["preserve_me"].as_str(),
        Some("raw field survives")
    );
    pager.inject_keys(keys::ESC).expect("close Settings");
    pager
        .wait_for_text("SETTINGS_LIVE_SESSION_READY", WAIT)
        .expect("returned to same live session");

    run_forced_blocking(
        &mut pager,
        &content,
        "call_pty_forced_alive",
        "FORCED_ALIVE",
        "SETTINGS_ALIVE_ANSWERED",
    )
    .await;
    let directory =
        session_with_call(&content, "call_pty_forced_alive").expect("live canonical session");
    quit_cleanly(&mut pager);
    drop(pager);

    let mut replay = spawn(&binary, &content, project.path(), true);
    replay
        .wait_for_text("SETTINGS_ALIVE_ANSWERED", RESUME_TIMEOUT)
        .expect("same isolated session restored");
    replay
        .wait_for_turn_idle(WAIT)
        .expect("restored session idle");
    assert_eq!(
        saved_config(&content)["toolset"]["ask_user_question"]["allow_non_blocking"].as_bool(),
        Some(false)
    );
    run_forced_blocking(
        &mut replay,
        &content,
        "call_pty_forced_cold",
        "FORCED_COLD",
        "SETTINGS_COLD_ANSWERED",
    )
    .await;
    assert_eq!(
        session_with_call(&content, "call_pty_forced_cold").as_ref(),
        Some(&directory)
    );
    for call in ["call_pty_forced_alive", "call_pty_forced_cold"] {
        assert_single_settlement(&events(&directory), call);
    }
    write_cast_if_requested(&replay, "advisory_settings_continue.cast");
    quit_cleanly(&mut replay);
}

async fn run_forced_blocking(
    pager: &mut PtyHarness,
    content: &ContentController,
    call: &str,
    question: &str,
    done: &str,
) {
    let ask = expect_tool_turn(
        content,
        call,
        "ask_user_question",
        json!({
            "blocking": false,
            "questions": [{"question": question, "recommended_option": "Suggested", "options": [
                {"label": "Suggested", "description": "Agent suggestion"},
                {"label": "Human choice", "description": "Actual user selection"}
            ]}]
        })
        .to_string(),
    );
    let after_answer =
        content.expect_agent_turn(format!("only after real blocking answer {call}"), done);
    let before = foreground_bodies(content).len();
    pager
        .inject_keys(format!("ASK_{question}\r").as_bytes())
        .expect("ask in existing session");
    pager
        .wait_for_text(question, WAIT)
        .expect("actual blocking QuestionView causal barrier");
    pager
        .wait_for_text("Human choice", WAIT)
        .expect("blocking options are rendered");
    ask.assert_satisfied();
    assert_eq!(
        foreground_bodies(content).len(),
        before + 1,
        "ask reached the UI before observing no continuation"
    );
    assert_no_new_requests(pager, content, before + 1);
    assert!(
        !pager.contains_text("reply to change"),
        "saved false must use blocking presentation\n{}",
        pager.screen_contents()
    );
    pager.inject_keys(keys::DOWN).expect("choose actual answer");
    pager
        .inject_keys(keys::ENTER)
        .expect("real blocking answer");
    pager
        .wait_for_text(done, WAIT)
        .expect("provider continues after real answer");
    pager
        .wait_for_turn_idle(WAIT)
        .expect("blocking Turn finished");
    after_answer.assert_satisfied();
    let bodies = foreground_bodies(content);
    assert_eq!(bodies.len(), before + 2);
    let result = tool_result_text(bodies.last().unwrap(), call);
    assert!(
        result.contains("User has answered") && result.contains("Human choice"),
        "real human result: {result}"
    );
    assert_one_result_per_request(&bodies, call);
}

async fn isolated_content() -> ContentController {
    let content = ContentController::start().await.expect("loopback provider");
    content.seed_llm_config().expect("isolated provider config");
    let config = content.home().join(".grow/config.toml");
    let mut text = std::fs::read_to_string(&config).expect("seeded provider config");
    text.push_str("\n[toolset.ask_user_question]\ntimeout_enabled = false\ntimeout_secs = 71\n\n[advisory_pty_fixture]\npreserve_me = \"raw field survives\"\n");
    std::fs::write(config, text).expect("seed only isolated timeout and raw-field controls");
    content
}

fn advisory_arguments() -> String {
    json!({"blocking": false, "questions": [
        {"question": QUESTION, "recommended_option": RECOMMENDATION, "options": [
            {"label": RECOMMENDATION, "description": "Agent recommendation, literal label"},
            {"label": ANSWER, "description": "Different human answer, literal label"}
        ]},
        {"question": NOTES_QUESTION, "recommended_option": "Keep notes brief", "options": [
            {"label": "Keep notes brief", "description": "Agent recommendation"},
            {"label": "More context", "description": "An alternative"}
        ]}
    ]})
    .to_string()
}

fn spawn(binary: &Path, content: &ContentController, project: &Path, resume: bool) -> PtyHarness {
    let mut args = vec!["--no-leader", "--trust"];
    if resume {
        args.push("--continue");
    }
    let rpc_log = content.home().join(".grow/pty-admission-rpc.log");
    let mut environment = vec![
        EnvOp::remove("GROW_ASK_USER_QUESTION_ALLOW_NON_BLOCKING"),
        EnvOp::remove("GROW_ASK_USER_QUESTION_TIMEOUT_ENABLED"),
        EnvOp::remove("GROW_ASK_USER_QUESTION_TIMEOUT_SECS"),
    ];
    if std::env::var_os("GROW_PTY_CAST_DIR").is_some() {
        environment.push(EnvOp::set_os(
            std::ffi::OsStr::new("GROW_LOG_FILE"),
            rpc_log.as_os_str(),
        ));
        environment.push(EnvOp::set("RUST_LOG", "acp_transport::gateway=debug"));
    }
    PtyHarness::spawn_with_content_env_ops_in_dir(
        binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        content,
        &args,
        &environment,
        Some(project),
    )
    .expect("spawn real isolated Pager")
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
    eprintln!(
        "advisory PTY binary={} bytes={} mtime={:?} blake3={}",
        binary.display(),
        metadata.len(),
        metadata.modified().expect("mtime"),
        hash.finalize()
    );
    binary
}

fn show_original_card_from_prompt(pager: &mut PtyHarness) {
    // Tab can complete an existing composer draft. Select the visible reply
    // response once so transcript navigation has explicit key ownership.
    let screen = pager.screen_contents();
    let (row, col) = locate_screen_text(&screen, REPLY_DONE)
        .expect("visible reply response before transcript navigation");
    let select = format!(
        "{}{}",
        sgr_mouse(0, row, col, 'M'),
        sgr_mouse(0, row, col, 'm')
    );
    pager
        .inject_keys(select.as_bytes())
        .expect("focus transcript on visible reply response");
    pager
        .wait_for_text("Ctrl+e:", WAIT)
        .expect("transcript navigation owns keys");
    // PrevTurn first selects the current Turn's prompt when starting on its
    // response; the second action selects the preceding Turn's prompt.
    for _ in 0..2 {
        pager
            .inject_keys(b"\x1b[1;2D")
            .expect("previous Turn using simple-mode Shift+Left navigation");
        pager.update(Duration::from_millis(50));
    }
    reveal_advisory_card(pager);
}

fn reveal_advisory_card(pager: &mut PtyHarness) {
    pager.update(Duration::from_millis(50));
    if pager.contains_text("agent recommendation:") {
        return;
    }
    // Collapsed rendering is one header followed by one advisory summary.
    // Expand that actual tool row, without invoking the Reply body action.
    pager
        .wait_for_text("advisory — agent continues", WAIT)
        .expect("collapsed suggestion card");
    let screen = pager.screen_contents();
    let (summary_row, _) = locate_screen_text(&screen, "advisory — agent continues").unwrap();
    assert!(
        summary_row > 0,
        "tool header must be visible above summary\n{screen}"
    );
    let select = format!(
        "{}{}",
        sgr_mouse(0, summary_row - 1, 6, 'M'),
        sgr_mouse(0, summary_row - 1, 6, 'm')
    );
    pager
        .inject_keys(select.as_bytes())
        .expect("select actual tool header once");
    pager.update(Duration::from_millis(50));
    pager
        .inject_keys(keys::RIGHT)
        .expect("expand selected tool using existing keyboard action");
    pager
        .wait_for_text("agent recommendation:", WAIT)
        .expect("expanded actual card");
}

fn activate_reply_action(pager: &mut PtyHarness, text: &str) {
    pager
        .wait_for_text(text, WAIT)
        .expect("visible Reply keyboard affordance");
    let screen = pager.screen_contents();
    let (row, col) = locate_screen_text(&screen, text).expect("visible Reply row");
    let select = format!(
        "{}{}",
        sgr_mouse(0, row, col, 'M'),
        sgr_mouse(0, row, col, 'm')
    );
    pager
        .inject_keys(select.as_bytes())
        .expect("select visible advisory Reply card");
    pager.update(Duration::from_millis(50));
    pager
        .inject_keys(b"r")
        .expect("invoke the visible card's existing Reply keyboard action");
}

fn double_click_text(pager: &mut PtyHarness, text: &str) {
    pager.wait_for_text(text, WAIT).unwrap_or_else(|error| {
        panic!(
            "visible action {text:?}: {error}\n{}",
            pager.screen_contents()
        )
    });
    let screen = pager.screen_contents();
    let (row, col) = locate_screen_text(&screen, text).expect("action coordinates");
    double_click(pager, row, col);
}

fn double_click(pager: &mut PtyHarness, row: u16, col: u16) {
    let click = format!(
        "{}{}{}{}",
        sgr_mouse(0, row, col, 'M'),
        sgr_mouse(0, row, col, 'm'),
        sgr_mouse(0, row, col, 'M'),
        sgr_mouse(0, row, col, 'm')
    );
    pager
        .inject_keys(click.as_bytes())
        .expect("actual SGR mouse double-click");
}

fn foreground_bodies(content: &ContentController) -> Vec<Value> {
    // Match the existing mock server's foreground classifier, including the
    // BYOK path where diagnostic x-grow-* headers are absent.
    content
        .requests()
        .into_iter()
        .filter(|entry| {
            entry.path == "/v1/chat/completions"
                && if entry.header("x-grow-turn-idx").is_some() {
                    true
                } else if entry.header("x-grow-req-id").is_some() {
                    false
                } else {
                    entry
                        .body
                        .as_ref()
                        .and_then(|body| body["tools"].as_array())
                        .is_some_and(|tools| tools.len() >= 2)
                }
        })
        .filter_map(|entry| entry.body)
        .collect()
}

fn tool_result_text(body: &Value, call: &str) -> String {
    body["messages"]
        .as_array()
        .expect("chat-completions messages")
        .iter()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == call)
        .expect("actual tool result")["content"]
        .as_str()
        .expect("model tool text")
        .to_owned()
}

fn assert_one_result_per_request(bodies: &[Value], call: &str) {
    for body in bodies {
        let messages = body["messages"].as_array().expect("messages");
        let asks = messages
            .iter()
            .filter(|message| message["role"] == "assistant")
            .flat_map(|message| message["tool_calls"].as_array().into_iter().flatten())
            .filter(|tool| tool["id"] == call)
            .count();
        let results = messages
            .iter()
            .filter(|message| message["role"] == "tool" && message["tool_call_id"] == call)
            .count();
        if asks > 0 {
            assert_eq!(results, 1, "one old call result per request: {body}");
        }
        assert!(results <= 1, "old tool must never settle twice: {body}");
    }
}

fn user_text(body: &Value) -> String {
    body["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|message| message["role"] == "user")
        .map(|message| message["content"].to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_no_new_requests(pager: &mut PtyHarness, content: &ContentController, count: usize) {
    let deadline = Instant::now() + NO_REQUEST_WINDOW;
    while Instant::now() < deadline {
        pager.update(Duration::from_millis(50));
        assert_eq!(
            foreground_bodies(content).len(),
            count,
            "unexpected foreground provider request\n{}",
            dump_non_system_messages(&content.request_bodies())
        );
    }
}

fn saved_config(content: &ContentController) -> toml::Value {
    toml::from_str(
        &std::fs::read_to_string(content.home().join(".grow/config.toml")).expect("saved config"),
    )
    .expect("valid saved TOML")
}

fn events(directory: &Path) -> Vec<Value> {
    let text = std::fs::read_to_string(directory.join("timeline.jsonl"))
        .expect("canonical session journal");
    shell::session::testkit::decode_timeline_fixture(&text)
        .expect("valid checksummed Timeline")
        .events()
        .iter()
        .map(|event| serde_json::to_value(event).expect("Timeline event"))
        .collect()
}

fn session_with_call(content: &ContentController, call: &str) -> Option<PathBuf> {
    let mut found = Vec::new();
    for cwd in std::fs::read_dir(content.home().join(".grow/sessions"))
        .ok()?
        .flatten()
    {
        let Ok(entries) = std::fs::read_dir(cwd.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(text) = std::fs::read_to_string(entry.path().join("timeline.jsonl")) else {
                continue;
            };
            let Ok(timeline) = shell::session::testkit::decode_timeline_fixture(&text) else {
                continue;
            };
            if timeline.events().iter().any(|event| matches!(&event.kind, chat_state::TimelineEventKind::Tool(chat_state::ToolEvent::Started { call_id, .. }) if call_id == call)) {
                found.push(entry.path());
            }
        }
    }
    assert!(
        found.len() <= 1,
        "one actual source session for {call}: {found:?}"
    );
    found.pop()
}

fn session_directories(content: &ContentController) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    for cwd in std::fs::read_dir(content.home().join(".grow/sessions"))
        .expect("isolated sessions directory")
        .flatten()
    {
        let Ok(entries) = std::fs::read_dir(cwd.path()) else {
            continue;
        };
        directories.extend(
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.join("timeline.jsonl").is_file()),
        );
    }
    directories.sort();
    directories
}

fn original_turn_end(events: &[Value], call: &str) -> Option<u64> {
    let turn = events.iter().find(|event| {
        event["type"] == "tool"
            && event["event"]["state"] == "started"
            && event["event"]["call_id"] == call
    })?["event"]["turn"]
        .clone();
    events.iter().find(|event| {
        event["type"] == "turn"
            && event["event"]["state"] == "ended"
            && event["event"]["id"] == turn
    })?["seq"]
        .as_u64()
}

fn assert_single_settlement(events: &[Value], call: &str) {
    assert_eq!(
        events
            .iter()
            .filter(|event| event["type"] == "tool"
                && event["event"]["state"] == "completed"
                && event["event"]["call_id"] == call)
            .count(),
        1,
        "one canonical tool completion"
    );
    let results = events
        .iter()
        .filter(|event| event["type"] == "messages" && event["event"]["cause"] == "tool_result")
        .flat_map(|event| event["event"]["items"].as_array().into_iter().flatten())
        .filter(|item| item["type"] == "tool_result" && item["tool_call_id"] == call)
        .count();
    assert_eq!(results, 1, "one canonical model-visible old tool result");
}

fn reply_submission(directory: &Path, events: &[Value], stable_id: &str) -> Option<String> {
    let mut inputs = Vec::new();
    for event in events
        .iter()
        .filter(|event| event["type"] == "input" && event["event"]["state"] == "submitted")
    {
        let hash = event["event"]["payload_ref"]["blake3"]
            .as_str()
            .expect("canonical payload reference");
        let bytes = std::fs::read(
            directory
                .join("artifacts/inputs")
                .join(format!("{hash}.json")),
        )
        .expect("canonical immutable input payload");
        assert_eq!(
            blake3::hash(&bytes).to_hex().as_str(),
            hash,
            "payload bytes match canonical reference"
        );
        let payload: Value = serde_json::from_slice(&bytes).expect("stored input payload");
        if payload["prompt_id"] == stable_id {
            assert_eq!(
                payload["requested_route"]["route"], "fifo",
                "ordinary literal FIFO reply"
            );
            assert_eq!(
                payload["origin"]["kind"], "user",
                "reply has real human provenance"
            );
            assert!(
                payload["skill_invocations"]
                    .as_array()
                    .is_some_and(Vec::is_empty),
                "slash-style answer data must not load a Skill"
            );
            inputs.push(event["event"]["input_id"].as_str().unwrap().to_owned());
        }
    }
    assert!(
        inputs.len() <= 1,
        "stable question reply Submitted only once: {inputs:?}"
    );
    inputs.pop()
}

fn assert_reply_lifecycle(events: &[Value], input: &str, original_end: u64, stable_id: &str) {
    let submitted = events
        .iter()
        .find(|event| {
            event["type"] == "input"
                && event["event"]["state"] == "submitted"
                && event["event"]["input_id"] == input
        })
        .expect("canonical Submitted");
    assert!(
        submitted["seq"].as_u64().unwrap() > original_end,
        "reply admitted after original Turn ended"
    );
    let turns: Vec<_> = events
        .iter()
        .filter(|event| {
            event["type"] == "turn"
                && event["event"]["state"] == "started"
                && event["event"]["prompt_id"] == stable_id
        })
        .collect();
    assert_eq!(turns.len(), 1, "one distinct later reply Turn");
    assert!(
        events.iter().any(|event| event["type"] == "input"
            && event["event"]["state"] == "consumed"
            && event["event"]["input_ids"]
                .as_array()
                .is_some_and(|ids| ids.iter().any(|id| id == input))),
        "real reply reached canonical input consumption"
    );
}

fn record_checkpoint(
    pager: &PtyHarness,
    content: &ContentController,
    session: Option<&Path>,
    label: &str,
) {
    let Some(directory) = std::env::var_os("GROW_PTY_CAST_DIR").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&directory).expect("create diagnostic artifact directory");
    pager
        .write_cast(&directory.join(format!("{label}.cast")))
        .expect("checkpoint cast");
    std::fs::write(
        directory.join(format!("{label}.screen.txt")),
        pager.screen_contents(),
    )
    .expect("checkpoint screen");
    std::fs::write(
        directory.join(format!("{label}.provider.json")),
        serde_json::to_vec_pretty(&foreground_bodies(content)).unwrap(),
    )
    .expect("checkpoint provider requests");
    for (source, suffix) in [
        (
            content.home().join(".grow/pty-admission-rpc.log"),
            "rpc.log",
        ),
        (
            content.home().join(".grow/logs/unified.jsonl"),
            "unified.jsonl",
        ),
    ] {
        if source.is_file() {
            std::fs::copy(source, directory.join(format!("{label}.{suffix}")))
                .expect("checkpoint isolated runtime log");
        }
    }
    if let Some(session) = session {
        std::fs::copy(
            session.join("timeline.jsonl"),
            directory.join(format!("{label}.timeline.jsonl")),
        )
        .expect("checkpoint canonical journal");
        let recorded = events(session);
        for event in recorded
            .iter()
            .filter(|event| event["type"] == "input" && event["event"]["state"] == "submitted")
        {
            let hash = event["event"]["payload_ref"]["blake3"]
                .as_str()
                .expect("canonical input hash");
            std::fs::copy(
                session
                    .join("artifacts/inputs")
                    .join(format!("{hash}.json")),
                directory.join(format!("{label}.input-{hash}.json")),
            )
            .expect("checkpoint immutable input blob");
        }
    }
    eprintln!(
        "advisory PTY checkpoint={label} artifacts={}",
        directory.display()
    );
}

fn quit_cleanly(pager: &mut PtyHarness) {
    pager.inject_keys(b"\x11").expect("arm quit");
    pager.update(Duration::from_millis(100));
    pager.inject_keys(b"\x11").expect("confirm quit");
    assert_eq!(
        pager
            .wait_for_exit_and_drain(Duration::from_secs(15), Duration::from_millis(100))
            .expect("clean process exit"),
        0
    );
}
