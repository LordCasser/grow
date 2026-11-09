//! Exercise the model-facing spawn tool through a real child provider request.
//! Requires a freshly built GROW_BINARY; no external provider is contacted.

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

async fn run_effort_case(effort: &str, expected_error: Option<&str>) {
    let server = MockInferenceServer::start().await.unwrap();
    server.set_response("Finished the requested work.");
    let workdir = git_workdir();
    let sandbox = TestSandbox::builder().mock_url(server.url()).build();
    std::fs::write(
        sandbox.grow_home().join("config.toml"),
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
reasoning_efforts = ["none", "high", {{ value = "max", default = true }}]
"#,
            server.url()
        ),
    )
    .unwrap();
    let client = GrowStdioClient::spawn_with_sandbox(&server, workdir.workspace(), sandbox).await;
    client.initialize_with_timeout().await;
    let session_id = client
        .create_session_with_timeout(workdir.workspace())
        .await;
    let arguments = serde_json::json!({
        "parent_step_id": "task-step",
        "description": "Check child effort",
        "prompt": "Return a short completion message.",
        "subagent_type": "general-purpose",
        "background": false,
        "model": "mock/child",
        "effort": effort,
    });
    let parent_todo = script_parent_todo(&server, "parent-wire");
    let spawn = server.expect_response(
        "spawn-with-effort",
        InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
        ScriptedResponse::sse(chat_completions_reasoning_then_tool_call_events(
            "Delegating the task.",
            "call_child_effort",
            "spawn_subagent",
            &arguments.to_string(),
            "parent-wire",
        )),
    );
    let result = client
        .prompt_with_timeout(&session_id, "Delegate this small task.")
        .await
        .unwrap();
    assert_eq!(result.stop_reason, acp::StopReason::EndTurn);
    parent_todo.assert_satisfied();
    spawn.assert_satisfied();
    let bodies = server.request_bodies();
    let child_requests: Vec<_> = bodies
        .iter()
        .filter(|body| body["model"] == "child-wire")
        .collect();
    if let Some(error) = expected_error {
        assert!(
            child_requests.is_empty(),
            "invalid effort issued a child request: {child_requests:?}"
        );
        assert!(
            serde_json::to_string(&bodies).unwrap().contains(error),
            "parent did not receive {error:?}"
        );
    } else {
        assert!(
            !child_requests.is_empty(),
            "no child provider request: {}",
            server.request_log_summary()
        );
        for body in child_requests {
            assert_eq!(
                body["reasoning_effort"], effort,
                "child request used a different effort: {body}"
            );
        }
    }
}

#[tokio::test]
#[ignore = "requires a freshly built grow binary"]
async fn subagent_tool_effort_reaches_provider_and_rejects_invalid_selections() {
    tokio::task::LocalSet::new()
        .run_until(async {
            run_effort_case("high", None).await;
            run_effort_case("none", None).await;
            run_effort_case("low", Some("is not offered by model")).await;
            run_effort_case("bogus", Some("Invalid subagent reasoning effort")).await;
        })
        .await;
}

#[tokio::test]
#[ignore = "requires a freshly built grow binary"]
async fn subagent_resume_tool_patches_model_and_effort_at_the_provider() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let server = MockInferenceServer::start().await.unwrap();
            server.set_response("Finished the requested work.");
            let workdir = git_workdir();
            let sandbox = TestSandbox::builder().mock_url(server.url()).build();
            let grow_home = sandbox.grow_home().to_path_buf();
            std::fs::write(
                grow_home.join("config.toml"),
                format!(
                    r#"[models]
default = "mock/parent"
allowed_models = ["mock/parent", "mock/child", "mock/alternate", "mock/limited", "mock/hidden", "mock/tiny"]
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
reasoning_efforts = ["none", "high", {{ value = "max", default = true }}]
[provider.mock.models.alternate]
model = "alternate-wire"
context_window = 200000
agent_type = "grow-build"
reasoning_efforts = ["none", "high", {{ value = "max", default = true }}]
[provider.mock.models.limited]
model = "limited-wire"
context_window = 200000
agent_type = "grow-build"
reasoning_efforts = ["none"]
[provider.mock.models.hidden]
hidden = true
context_window = 200000
[provider.mock.models.blocked]
context_window = 200000
[provider.mock.models.tiny]
model = "tiny-wire"
context_window = 1
reasoning_efforts = ["none", "high"]
"#,
                    server.url()
                ),
            )
            .unwrap();
            let client =
                GrowStdioClient::spawn_with_sandbox(&server, workdir.workspace(), sandbox).await;
            client.initialize_with_timeout().await;
            let session_id = client
                .create_session_with_timeout(workdir.workspace())
                .await;
            let parent_todo = script_parent_todo(&server, "parent-wire");
            let source = server.expect_response(
                "spawn-resume-source",
                InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
                ScriptedResponse::sse(chat_completions_reasoning_then_tool_call_events(
                    "Creating the resume source.",
                    "call_resume_source",
                    "spawn_subagent",
                    &serde_json::json!({
                        "parent_step_id": "task-step",
        "description": "Create resume source",
                        "prompt": "Return a short source completion.",
                        "subagent_type": "general-purpose",
                        "background": false,
                        "model": "mock/child",
                        "effort": "high",
                    })
                    .to_string(),
                    "parent-wire",
                )),
            );
            client
                .prompt_with_timeout(&session_id, "Delegate the source task.")
                .await
                .unwrap();
            parent_todo.assert_satisfied();
            source.assert_satisfied();
            let source_id = server
                .request_bodies()
                .iter()
                .filter_map(|body| body["messages"].as_array())
                .flatten()
                .filter_map(|message| message["content"].as_str())
                .flat_map(str::lines)
                .find_map(|line| line.trim().strip_prefix("subagent_id: ").map(str::to_owned))
                .expect("completed source tool result must expose its real resume ID");
            let source_path = std::fs::read_dir(grow_home.join("sessions"))
                .unwrap()
                .filter_map(Result::ok)
                .map(|entry| entry.path().join(&source_id).join("timeline.jsonl"))
                .find(|path| path.is_file())
                .expect("source must have a durable Timeline");
            let source_bytes = std::fs::read(&source_path).unwrap();
            for (name, selection, wire_model, effort, expected_error) in [
                ("omitted", serde_json::json!({}), "child-wire", "high", None),
                ("null", serde_json::json!({"model":null,"effort":null}), "child-wire", "high", None),
                ("model", serde_json::json!({"model":"mock/alternate"}), "alternate-wire", "high", None),
                ("effort", serde_json::json!({"effort":"none"}), "child-wire", "none", None),
                ("both", serde_json::json!({"model":"mock/alternate","effort":"none"}), "alternate-wire", "none", None),
                ("invalid", serde_json::json!({"effort":"bogus"}), "", "", Some("Invalid subagent reasoning effort")),
                ("unsupported", serde_json::json!({"model":"mock/limited"}), "", "", Some("is not offered by model")),
                ("unknown", serde_json::json!({"model":"mock/unknown"}), "", "", Some("Unknown Task.model ID")),
                ("hidden", serde_json::json!({"model":"mock/hidden"}), "", "", Some("Unknown Task.model ID")),
                ("blocked", serde_json::json!({"model":"mock/blocked"}), "", "", Some("Unknown Task.model ID")),
                ("small", serde_json::json!({"model":"mock/tiny"}), "", "", Some("no closed Surface range")),
            ] {
                let mut arguments = serde_json::json!({
                    "parent_step_id": "task-step",
        "description": "Continue source task",
                    "prompt": "Return a short resumed completion.",
                    "subagent_type": "general-purpose",
                    "background": false,
                    "resume_from": source_id,
                });
                arguments.as_object_mut().unwrap().extend(selection.as_object().unwrap().clone());
                let scripted = server.expect_response(
                    format!("resume-{name}"),
                    InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
                    ScriptedResponse::sse(chat_completions_reasoning_then_tool_call_events(
                        "Continuing the source.",
                        &format!("call_resume_{name}"),
                        "spawn_subagent",
                        &arguments.to_string(),
                        "parent-wire",
                    )),
                );
                let before = server.request_bodies().len();
                let result = client.prompt_with_timeout(&session_id, &format!("Continue the source: {name}.")).await.unwrap();
                assert_eq!(result.stop_reason, acp::StopReason::EndTurn);
                scripted.assert_satisfied();
                let bodies = server.request_bodies()[before..].to_vec();
                let child_requests: Vec<_> = bodies.iter().filter(|body| body["model"] != "parent-wire").collect();
                if let Some(error) = expected_error {
                    assert!(child_requests.is_empty(), "{name}: {child_requests:?}");
                    let call_id = format!("call_resume_{name}");
                    let current_error = bodies.iter()
                        .filter_map(|body| body["messages"].as_array())
                        .flatten()
                        .find(|message| message["tool_call_id"] == call_id)
                        .and_then(|message| message["content"].as_str())
                        .expect("the current failed resume must return its own tool result");
                    assert!(current_error.contains(error), "{name}: expected {error:?}, got {current_error}");
                } else {
                    assert!(!child_requests.is_empty(), "{name}: {}", server.request_log_summary());
                    for body in child_requests {
                        assert_eq!(body["model"], wire_model, "{name}: {body}");
                        assert_eq!(body["reasoning_effort"], effort, "{name}: {body}");
                    }
                }
                assert_eq!(std::fs::read(&source_path).unwrap(), source_bytes, "{name} changed source Timeline");
            }
        })
        .await;
}

#[tokio::test]
#[ignore = "requires a freshly built grow binary"]
async fn subagent_resume_inherits_a_parent_selected_route_with_unset_effort() {
    fn model_observations(timeline: &chat_state::Timeline) -> Vec<&serde_json::Value> {
        timeline
            .events()
            .iter()
            .filter_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Observation(observation)
                    if observation.scope == "model" && observation.name == "changed" =>
                {
                    Some(observation.data.as_ref().expect("model selection data"))
                }
                _ => None,
            })
            .collect()
    }

    tokio::task::LocalSet::new()
        .run_until(async {
            // Expectations can distinguish foreground from auxiliary requests,
            // but cannot match a model. Give each model its own provider endpoint
            // so the parent's request can never consume the child's control script.
            let parent_server = MockInferenceServer::start_with_models(vec![
                MockModelEntry::with_agent_type("parent-wire", "grow-build"),
            ])
            .await
            .unwrap();
            let source_server = MockInferenceServer::start_with_models(vec![
                MockModelEntry::with_agent_type("source-wire", "grow-build"),
            ])
            .await
            .unwrap();
            let switched_server = MockInferenceServer::start_with_models(vec![
                MockModelEntry::with_agent_type("switched-wire", "grow-build"),
            ])
            .await
            .unwrap();
            parent_server.set_response("The delegated task completed.");
            source_server.set_response("Unexpected request on the birth route.");
            switched_server.set_response("Completed on the tool-selected route.");
            let workdir = git_workdir();
            let sandbox = TestSandbox::builder()
                .mock_url(parent_server.url())
                .build();
            let grow_home = sandbox.grow_home().to_path_buf();
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
reasoning_efforts = ["none", "high", {{ value = "max", default = true }}]
[provider.source]
api_backend = "chat_completions"
[provider.source.options]
base_url = "{}"
env_key = "GROW_API_KEY"
[provider.source.models.child]
model = "source-wire"
context_window = 200000
agent_type = "grow-build"
reasoning_efforts = ["none", "high", {{ value = "max", default = true }}]
[provider.switched]
api_backend = "chat_completions"
[provider.switched.options]
base_url = "{}"
env_key = "GROW_API_KEY"
[provider.switched.models.final]
model = "switched-wire"
context_window = 200000
agent_type = "grow-build"
"#,
                    parent_server.url(),
                    source_server.url(),
                    switched_server.url(),
                ),
            )
            .unwrap();
            let client = GrowStdioClient::spawn_with_sandbox(
                &parent_server,
                workdir.workspace(),
                sandbox,
            )
            .await;
            client.initialize_with_timeout().await;
            let parent_id = client
                .create_session_with_timeout(workdir.workspace())
                .await;
            let parent_todo = script_parent_todo(&parent_server, "parent-wire");
            let spawn_source = parent_server.expect_response(
                "parent-spawns-source-for-model-control",
                InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
                ScriptedResponse::sse(chat_completions_reasoning_then_tool_call_events(
                    "Delegating the source task.",
                    "call_composed_source",
                    "spawn_subagent",
                    &serde_json::json!({
                        "parent_step_id": "task-step",
        "description": "Create model-switch source",
                        "prompt": "Read the requested fixture and complete the task.",
                        "subagent_type": "general-purpose",
                        "background": true,
                        "model": "source/child",
                        "effort": "high",
                    })
                    .to_string(),
                    "parent-wire",
                )),
            );
            let fixture = workdir.workspace().join("sampling-control-fixture.txt");
            std::fs::write(&fixture, "Close the current child Step after the parent queues control.").unwrap();
            let mut source_boundary = source_server.expect_response_blocked(
                "running-source-keeps-its-current-provider-route",
                InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
                ScriptedResponse::sse(chat_completions_reasoning_then_tool_call_events(
                    "Reading the fixture before continuing.",
                    "call_source_boundary",
                    "read_file",
                    &serde_json::json!({"target_file":fixture}).to_string(),
                    "source-wire",
                )),
            );
            let first = client
                .prompt_with_timeout(&parent_id, "Delegate a background child and keep it running.")
                .await
                .unwrap();
            assert_eq!(first.stop_reason, acp::StopReason::EndTurn);
            parent_todo.assert_satisfied();
            spawn_source.assert_satisfied();
            tokio::time::timeout(std::time::Duration::from_secs(30), source_boundary.wait_blocked()).await.unwrap();
            let parent_before = shell::session::storage::load_timeline_by_id_at(parent_id.0.as_ref(), &grow_home)
                .unwrap().unwrap();
            let source_spawn = parent_before.events().iter().find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(spawn))
                    if spawn.origin_tool_call_id.as_deref() == Some("call_composed_source") => Some(spawn),
                _ => None,
            }).expect("real Task must persist the source Spawn");
            assert_eq!(source_spawn.effective_model_id, "source/child");
            assert_eq!(source_spawn.reasoning_effort, Some(sampling_types::ReasoningEffort::High));
            let source_id = source_spawn.subagent_id.clone();
            let switch_source = parent_server.expect_response(
                "parent-selects-running-child-model-with-unset-effort",
                InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
                ScriptedResponse::sse(chat_completions_reasoning_then_tool_call_events(
                    "Selecting the final model for the running child.",
                    "call_source_switch_to_unset",
                    "set_subagent_model",
                    &serde_json::json!({"subagent_id":source_id,"model":"switched/final"}).to_string(),
                    "parent-wire",
                )),
            );
            let controlled = client.prompt_with_timeout(&parent_id, "Switch that running child to switched/final.").await.unwrap();
            assert_eq!(controlled.stop_reason, acp::StopReason::EndTurn);
            switch_source.assert_satisfied();
            let source_requests = source_server.request_bodies();
            assert_eq!(source_requests.len(), 1, "control cannot restart the admitted source request: {source_requests:?}");
            assert_eq!(source_requests[0]["model"], "source-wire");
            assert_eq!(source_requests[0]["reasoning_effort"], "high");
            assert!(switched_server.request_bodies().is_empty(), "queued does not apply before the child closes its Step");
            let control_bodies = parent_server.request_bodies();
            let queued = control_bodies.iter().filter_map(|body| body["messages"].as_array()).flatten()
                .find(|message| message["tool_call_id"] == "call_source_switch_to_unset")
                .and_then(|message| message["content"].as_str())
                .expect("the parent must receive the native tool's target admission receipt");
            assert_eq!(serde_json::from_str::<serde_json::Value>(queued).unwrap(), serde_json::json!({"subagent_id":source_id,"status":"queued"}));
            source_boundary.release();
            let wait_source = parent_server.expect_response(
                "parent-awaits-the-controlled-child",
                InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
                ScriptedResponse::sse(chat_completions_reasoning_then_tool_call_events(
                    "Waiting for the controlled source to finish.",
                    "call_composed_source_wait",
                    "wait_subagent",
                    &serde_json::json!({"subagent_ids":[source_id],"timeout_ms":30000}).to_string(),
                    "parent-wire",
                )),
            );
            client.prompt_with_timeout(&parent_id, "Wait for the controlled child to finish.").await.unwrap();
            wait_source.assert_satisfied();
            source_boundary.assert_satisfied();
            let switched_requests = switched_server.request_bodies();
            assert_eq!(switched_requests.len(), 1, "the source's next provider request must use the selected endpoint: {switched_requests:?}");
            assert_eq!(switched_requests[0]["model"], "switched-wire");
            assert!(switched_requests[0].get("reasoning_effort").is_none(), "model-only selection must clear the old effort: {}", switched_requests[0]);
            assert!(!switched_requests[0]["messages"].to_string().contains("call_source_switch_to_unset"), "the parent tool exchange remains in the parent");
            let source_dir = std::fs::read_dir(grow_home.join("sessions")).unwrap()
                .filter_map(Result::ok)
                .map(|entry| entry.path().join(&source_spawn.child_session_id))
                .find(|path| path.join("timeline.jsonl").is_file())
                .expect("source must have a canonical session directory");
            let source_path = source_dir.join("timeline.jsonl");
            let source_bytes = std::fs::read(&source_path).unwrap();
            let source_timeline = shell::session::storage::load_timeline_by_id_at(&source_spawn.child_session_id, &grow_home)
                .unwrap().unwrap();
            let source_selections = model_observations(&source_timeline);
            let source_selection = source_selections.last().expect("source model-control selection must be durable");
            assert_eq!(source_selection["from_model_id"], "source/child");
            assert_eq!(source_selection["from_reasoning_effort"], "high");
            assert_eq!(source_selection["to_model_id"], "switched/final");
            assert_eq!(source_selection["to_provider_model"], "switched-wire");
            assert_eq!(source_selection["to_reasoning_effort"], serde_json::Value::Null);
            let expected_transport = sampling_types::model_image_input_key_from_parts(
                "switched-wire",
                &sampling_types::ApiBackend::ChatCompletions,
                &switched_server.url(),
                &Default::default(),
            );
            assert_eq!(source_selection["to_model_transport"], serde_json::to_value(&expected_transport).unwrap());
            let resume_arguments = serde_json::json!({
                "parent_step_id": "task-step",
        "description": "Continue switched source",
                "prompt": "Continue the source conversation and complete this follow-up.",
                "subagent_type": "general-purpose",
                "background": false,
                "resume_from": source_id,
            });
            assert!(resume_arguments.get("model").is_none() && resume_arguments.get("effort").is_none());
            let resume = parent_server.expect_response(
                "parent-resumes-the-tool-selected-source",
                InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
                ScriptedResponse::sse(chat_completions_reasoning_then_tool_call_events(
                    "Continuing the source's final selection.",
                    "call_composed_resume",
                    "spawn_subagent",
                    &resume_arguments.to_string(),
                    "parent-wire",
                )),
            );
            let before_resume = switched_requests.len();
            let second = client
                .prompt_with_timeout(&parent_id, "Continue that source without overriding model or effort.")
                .await
                .unwrap();
            assert_eq!(second.stop_reason, acp::StopReason::EndTurn);
            resume.assert_satisfied();
            let final_requests = switched_server.request_bodies();
            let derived_requests = &final_requests[before_resume..];
            assert_eq!(derived_requests.len(), 1, "resumed child must issue one request on the source's final route: {derived_requests:?}");
            assert_eq!(derived_requests[0]["model"], "switched-wire");
            assert!(derived_requests[0].get("reasoning_effort").is_none(), "resume must retain unset effort: {}", derived_requests[0]);
            assert_eq!(source_server.request_bodies().len(), source_requests.len(), "resume cannot return to the source's birth route");
            assert!(parent_server.request_bodies().iter()
                .filter(|body| body["tools"].as_array().is_some_and(|tools| !tools.is_empty()))
                .all(|body| body["model"] == "parent-wire" && body["reasoning_effort"] == "max"),
                "child control must leave its parent's route and effort unchanged");
            let parent_after = shell::session::storage::load_timeline_by_id_at(parent_id.0.as_ref(), &grow_home)
                .unwrap().unwrap();
            let derived_spawn = parent_after.events().iter().find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(spawn))
                    if spawn.origin_tool_call_id.as_deref() == Some("call_composed_resume") => Some(spawn),
                _ => None,
            }).expect("real resume Task must persist the derived Spawn");
            assert_eq!(derived_spawn.resumed_from.as_deref(), Some(source_id.as_str()));
            assert_eq!(derived_spawn.effective_model_id, "switched/final");
            assert_eq!(derived_spawn.reasoning_effort, None);
            assert_eq!(derived_spawn.model_transport_key, expected_transport);
            let derived_timeline = shell::session::storage::load_timeline_by_id_at(&derived_spawn.child_session_id, &grow_home)
                .unwrap().unwrap();
            let derived_selections = model_observations(&derived_timeline);
            assert_eq!(derived_selections.len(), 1, "the derived baseline must use the final selection without replaying source controls");
            let baseline = derived_selections[0];
            assert_eq!(baseline["from_model_id"], "switched/final");
            assert_eq!(baseline["to_model_id"], "switched/final");
            assert_eq!(baseline["from_provider_model"], "switched-wire");
            assert_eq!(baseline["to_provider_model"], "switched-wire");
            assert_eq!(baseline["from_reasoning_effort"], serde_json::Value::Null);
            assert_eq!(baseline["to_reasoning_effort"], serde_json::Value::Null);
            assert_eq!(baseline["to_model_transport"], source_selection["to_model_transport"]);
            assert_eq!(std::fs::read(&source_path).unwrap(), source_bytes, "derived resume must not change the source Timeline");
            let unchanged_source = parent_after.events().iter().find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(spawn))
                    if spawn.subagent_id == source_id => Some(spawn),
                _ => None,
            }).unwrap();
            assert_eq!(serde_json::to_value(unchanged_source).unwrap(), serde_json::to_value(source_spawn).unwrap(), "source birth facts must remain immutable after its model control and derived resume");
        })
        .await;
}
