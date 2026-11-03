// Per-test-case module for the `pty_e2e` integration test crate.
#[allow(unused_imports)]
use super::common::*;

const DONE_SENTINEL: &str = "EDIT_MERGE_PAR_DONE";

const FIXTURE: &str = "parallel_fix.py";

/// PTY: TWO search_replace calls to the same file issued in ONE model turn
/// coalesce into a single Edit row with the summed diffstat, regardless of the
/// order their completions land in.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "PTY e2e; run with cargo test -p pager --test pty_e2e -- --ignored"]
async fn edit_merge_parallel_pty() {
    let content = ContentController::start_with_models(vec![
        MockModel::new("test-model").with_api_backend("responses"),
    ])
    .await
    .expect("start content");
    let workspace = content.home().join("project");
    std::fs::create_dir_all(&workspace).expect("create project workspace");
    git2::Repository::init(&workspace).expect("initialize isolated project");
    content.seed_llm_config().expect("seed mock LLM config");
    let config_path = content.home().join(".grow/config.toml");
    let config = std::fs::read_to_string(&config_path)
        .expect("read mock LLM config")
        .replace("default = \"mock/mock\"", "default = \"mock/test-model\"")
        .replace(
            "api_backend = \"chat_completions\"",
            "api_backend = \"responses\"",
        )
        .replace(
            "[provider.mock.models.mock]",
            "[provider.mock.models.test-model]",
        )
        .replace("name = \"Mock\"", "name = \"test-model\"");
    std::fs::write(config_path, config).expect("configure Responses backend");
    let target = workspace.join(FIXTURE);
    std::fs::write(
        &target,
        "# parallel merge fixture\n\
         first = \"one\"\n\
         second = \"two\"\n\
         third = \"three\"\n\
         fourth = \"four\"\n\
         fifth = \"five\"\n\
         sixth = \"six\"\n\
         seventh = \"seven\"\n\
         eighth = \"eight\"\n\
         ninth = \"nine\"\n\
         tenth = \"ten\"\n",
    )
    .expect("write fixture");
    let abs = dunce::canonicalize(&target).unwrap_or(target.clone());

    // Non-overlapping 1:1 replacements so both calls succeed against the
    // same starting file whatever order the shell runs them in.
    let args_a = json!({
        "file_path": abs.to_string_lossy(),
        "old_string": "second = \"two\"",
        "new_string": "second = \"two\"  # PAR_EDIT_A",
    })
    .to_string();
    let args_b = json!({
        "file_path": abs.to_string_lossy(),
        "old_string": "ninth = \"nine\"",
        "new_string": "ninth = \"nine\"  # PAR_EDIT_B",
    })
    .to_string();
    let parallel_turn = enqueue_parallel_tool_turn(
        &content,
        &[
            ("call_par_1", "search_replace", args_a),
            ("call_par_2", "search_replace", args_b),
        ],
    );
    content.set_response(DONE_SENTINEL);

    let binary = pager_binary().expect("resolve pager binary");
    let mut harness = PtyHarness::spawn_with_content_in_dir(
        &binary,
        DEFAULT_ROWS,
        DEFAULT_COLS,
        &content,
        &["--permission-mode", "always-approve", "--trust"],
        Some(&workspace),
    )
    .expect("spawn pager");

    harness
        .wait_for_text(WELCOME_SCREEN_SENTINEL, WELCOME_TIMEOUT)
        .expect("welcome");
    harness
        .inject_keys(&PROMPT.as_bytes()[..1])
        .expect("begin prompt from welcome");
    harness
        .wait_until("first character in agent composer", WELCOME_TIMEOUT, |h| {
            h.contains_text("Enter:send") && composer_holds(h, "❯ g")
        })
        .expect("agent composer ready after welcome promotion");
    harness
        .inject_keys(&PROMPT.as_bytes()[1..])
        .expect("finish prompt");
    harness
        .wait_until("full prompt in agent composer", WELCOME_TIMEOUT, |h| {
            composer_holds(h, PROMPT)
        })
        .expect("full prompt rendered before submit");
    harness.inject_keys(b"\r").expect("submit prompt");
    harness
        .wait_for_text(DONE_SENTINEL, Duration::from_secs(90))
        .unwrap_or_else(|_| {
            panic!(
                "expected settled transcript; got:\n{}",
                harness.screen_contents()
            )
        });
    parallel_turn.assert_satisfied();

    let requests = content.requests();
    let agent_requests: Vec<_> = requests
        .iter()
        .filter(|request| {
            request.method == "POST"
                && request.body.as_ref().is_some_and(|body| {
                    body["tools"]
                        .as_array()
                        .is_some_and(|tools| !tools.is_empty())
                })
        })
        .collect();
    let responses_requests: Vec<_> = agent_requests
        .iter()
        .copied()
        .filter(|request| request.path == "/v1/responses")
        .collect();
    assert_eq!(
        agent_requests.len(),
        2,
        "the parallel tool turn and its settling turn must be the only primary requests with tool definitions; requests: {requests:#?}"
    );
    assert_eq!(
        responses_requests.len(),
        agent_requests.len(),
        "primary requests with tool definitions must use Responses, without counting sideband requests: {requests:#?}"
    );
    assert_eq!(
        responses_requests[0].body.as_ref().unwrap()["model"],
        "test-model",
        "the scripted parallel tool turn reached the Responses endpoint"
    );

    let request_summary = || {
        responses_requests
            .iter()
            .enumerate()
            .map(|(index, request)| {
                let body = request.body.as_ref().expect("Responses request body");
                let tools = body["tools"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|tool| {
                        tool["name"]
                            .as_str()
                            .or_else(|| tool["function"]["name"].as_str())
                            .unwrap_or("?")
                    })
                    .collect::<Vec<_>>();
                let input = body["input"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|item| {
                        let output = item["output"]
                            .as_str()
                            .or_else(|| item["content"].as_str())
                            .map(|text| text.chars().take(500).collect::<String>());
                        format!(
                            "type={} call_id={} name={} output={:?}",
                            item["type"].as_str().unwrap_or("?"),
                            item["call_id"].as_str().unwrap_or("?"),
                            item["name"].as_str().unwrap_or("?"),
                            output
                        )
                    })
                    .collect::<Vec<_>>();
                format!(
                    "request[{index}] model={} tools={tools:?} input={input:#?}",
                    body["model"].as_str().unwrap_or("?")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let diagnostic = request_summary();
    let followup_input = responses_requests
        .iter()
        .find_map(|request| {
            let input = request.body.as_ref()?["input"].as_array()?;
            input
                .iter()
                .any(|item| item["type"] == "function_call_output")
                .then_some(input)
        })
        .expect("follow-up Responses input with tool outputs");
    for call_id in ["call_par_1", "call_par_2"] {
        assert!(
            followup_input.iter().any(|item| {
                item["type"] == "function_call_output" && item["call_id"] == call_id
            }),
            "follow-up Responses request must carry output for {call_id}; requests:\n{diagnostic}"
        );
    }
    let final_file = std::fs::read_to_string(&target).expect("read edited fixture");
    assert!(
        final_file.contains("second = \"two\"  # PAR_EDIT_A")
            && final_file.contains("ninth = \"nine\"  # PAR_EDIT_B"),
        "both parallel tool calls must have changed the file before the UI merge assertion; file:\n{final_file}\nrequests:\n{diagnostic}"
    );

    // One merged row summing both calls' diffstats.
    harness
        .wait_for_text(&format!("Edit {FIXTURE} +2/-2"), Duration::from_secs(20))
        .unwrap_or_else(|_| {
            panic!(
                "expected one merged `Edit {FIXTURE} +2/-2` row; got:\n{}\nResponses requests:\n{diagnostic}",
                harness.screen_contents(),
            )
        });
    let screen = harness.screen_contents();
    let edit_rows = screen
        .lines()
        .filter(|l| l.contains(&format!("Edit {FIXTURE}")))
        .count();
    assert_eq!(
        edit_rows, 1,
        "parallel same-file edits must render as ONE Edit row\nscreen:\n{screen}"
    );
    assert!(
        !screen.contains("+1/-1"),
        "per-call diffstats must not survive the merge\nscreen:\n{screen}"
    );
    assert!(
        final_file.contains("second = \"two\"  # PAR_EDIT_A"),
        "first parallel edit must be present in the file:\n{final_file}"
    );
    assert!(
        final_file.contains("ninth = \"nine\"  # PAR_EDIT_B"),
        "second parallel edit must be present in the file:\n{final_file}"
    );

    assert!(
        !harness.contains_text("panicked"),
        "pager panicked\nscreen:\n{}",
        harness.screen_contents()
    );

    harness.quit().expect("clean quit");
}
