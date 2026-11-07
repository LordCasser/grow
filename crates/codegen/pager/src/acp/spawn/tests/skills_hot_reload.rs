//! AC06 exercises the in-process runtime through actual config/file events.
use super::*;
use acp_transport::{AcpAgentTx, AcpClientMessage, acp_send};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use test_support::{MockInferenceServer, ScriptedResponse, SseEvent};

struct Worker {
    tx: Option<AcpAgentTx>,
    cancel: CancellationToken,
    guard: AgentShutdownGuard,
    commands: Arc<Mutex<Vec<acp::AvailableCommand>>>,
    drain: tokio::task::JoinHandle<()>,
}

impl Worker {
    fn tx(&self) -> &AcpAgentTx {
        self.tx.as_ref().unwrap()
    }

    async fn wait_command(&self, name: &str, description: Option<&str>) {
        tokio::time::timeout(Duration::from_secs(12), async {
            loop {
                let matches = self.commands.lock().unwrap().iter().any(|command| {
                    command.name == name
                        && description.is_none_or(|text| command.description == text)
                });
                if matches == description.is_some() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("slash {name} did not become {description:?}"));
    }

    async fn stop(mut self) {
        self.cancel.cancel();
        self.tx.take();
        let handle = self.guard.thread.take().unwrap();
        assert_eq!(
            join_agent_thread(handle, Duration::from_secs(30)),
            JoinOutcome::Joined
        );
        self.drain.abort();
        let _ = self.drain.await;
        assert_all_watchers_retired();
    }
}

fn skill(root: &Path, name: &str, description: &str) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("SKILL.md");
    std::fs::write(
        &path,
        format!("---\nname: {name}\ndescription: {description}\n---\nAC06_BODY_{name}\n"),
    )
    .unwrap();
    path
}

fn write_config(paths: &[String], disabled: &[&str], ignore: &[String]) {
    let config = json!({
        "features": {"mcp_recursive_config_watch": false},
        "skills": {"paths": paths, "disabled": disabled, "ignore": ignore}
    });
    std::fs::write(
        shell::util::grow_home::grow_home().join("config.toml"),
        toml::to_string(&config).unwrap(),
    )
    .unwrap();
}

fn paths(root: &Path) -> Vec<String> {
    vec![
        dunce::canonicalize(root)
            .unwrap()
            .to_string_lossy()
            .into_owned(),
    ]
}

async fn start_worker(server: &MockInferenceServer) -> Worker {
    diagnostics::tls::install_ring_provider_once();
    let raw: toml::Value = toml::from_str(&format!(
        "[models]\ndefault = 'test/test-model'\n\
         [provider.test]\napi_backend = 'chat_completions'\n\
         [provider.test.options]\nbase_url = '{}'\napi_key = 'test'\n\
         [provider.test.models.test-model]\ncontext_window = 128000\n",
        server.url()
    ))
    .unwrap();
    let mut config = AgentConfig::new_from_toml_cfg(&raw).unwrap();
    config.default_model_override = None;
    config.models.default = Some("test/test-model".into());
    config.remote_settings = Some(Default::default());
    let cancel = CancellationToken::new();
    let spawned = spawn_shell(config, &cancel, None).await.unwrap();
    let commands = Arc::new(Mutex::new(Vec::new()));
    let captured = commands.clone();
    let mut rx = spawned.channel.rx;
    let drain = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            match message {
                AcpClientMessage::SessionNotification(notification) => {
                    if let acp::SessionUpdate::AvailableCommandsUpdate(update) =
                        notification.request.update
                    {
                        *captured.lock().unwrap() = update.available_commands;
                    }
                    let _ = notification.response_tx.send(Ok(()));
                }
                AcpClientMessage::RequestPermission(permission) => {
                    let option = permission
                        .request
                        .options
                        .iter()
                        .find(|option| matches!(option.kind, acp::PermissionOptionKind::AllowOnce))
                        .expect("the fixture can grant the read-only skill call once");
                    let response = acp::RequestPermissionResponse::new(
                        acp::RequestPermissionOutcome::Selected(
                            acp::SelectedPermissionOutcome::new(option.option_id.clone()),
                        ),
                    );
                    let _ = permission.response_tx.send(Ok(response));
                }
                _ => {}
            }
        }
    });
    let tx = spawned.channel.tx;
    let _: acp::InitializeResponse = tokio::time::timeout(
        Duration::from_secs(10),
        acp_send(acp::InitializeRequest::new(acp::ProtocolVersion::V1), &tx),
    )
    .await
    .unwrap()
    .unwrap();
    Worker {
        tx: Some(tx),
        cancel: cancel.clone(),
        guard: AgentShutdownGuard::new(cancel, Some(spawned.thread_handle)),
        commands,
        drain,
    }
}

async fn new_session(worker: &Worker, cwd: &Path) -> acp::SessionId {
    shell::agent::folder_trust::grant_folder_trust(cwd);
    let session: acp::NewSessionResponse = tokio::time::timeout(
        Duration::from_secs(15),
        acp_send(acp::NewSessionRequest::new(cwd.to_path_buf()), worker.tx()),
    )
    .await
    .unwrap()
    .unwrap();
    session.session_id
}

async fn assert_callable(
    worker: &Worker,
    server: &MockInferenceServer,
    session: &acp::SessionId,
    name: &str,
    callable: bool,
) {
    let id = format!("ac06_{}", server.request_count());
    let chunk = |delta: Value, finish: Value| {
        json!({
            "id": &id, "object": "chat.completion.chunk", "created": 1, "model": "test-model",
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]
        })
    };
    let mut expectation = server.expect_response(
        &id,
        test_support::InferenceRequestMatcher::foreground(
            test_support::InferenceEndpoint::ChatCompletions,
        ),
        ScriptedResponse::sse(vec![
            SseEvent::data(
                chunk(
                    json!({"role": "assistant", "tool_calls": [{
                        "index": 0, "id": &id, "type": "function", "function": {
                            "name": "load_skill", "arguments": json!({"name": name}).to_string()
                        }
                    }]}),
                    Value::Null,
                )
                .to_string(),
            ),
            SseEvent::data(chunk(json!({}), json!("tool_calls")).to_string()),
            SseEvent::data("[DONE]"),
        ]),
    );
    let _: acp::PromptResponse = tokio::time::timeout(
        Duration::from_secs(30),
        acp_send(
            acp::PromptRequest::new(
                session.clone(),
                vec![acp::ContentBlock::Text(acp::TextContent::new(format!(
                    "AC06 probe {id}"
                )))],
            ),
            worker.tx(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    expectation.wait_received().await;
    let result = server
        .request_bodies()
        .into_iter()
        .flat_map(|body| body["messages"].as_array().cloned().unwrap_or_default())
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == id)
        .unwrap_or_else(|| panic!("no actual load_skill result for {id}"));
    let text = result["content"].to_string();
    assert_eq!(
        text.contains(&format!("AC06_BODY_{name}")),
        callable,
        "{result}"
    );
    if !callable {
        assert!(
            text.contains("disabled")
                || text.contains("not found")
                || text.contains("Unknown")
                || text.contains("unknown")
                || text.contains("unavailable"),
            "{result}"
        );
    }
}

fn event_count(event: &SkillsWatcherEvent) -> usize {
    SKILLS_WATCHER_EVENTS
        .lock()
        .unwrap()
        .iter()
        .filter(|observed| *observed == event)
        .count()
}

async fn wait_event(event: SkillsWatcherEvent, count: usize) {
    tokio::time::timeout(Duration::from_secs(12), async {
        while event_count(&event) < count {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "missing watcher event {event:?}: {:?}",
            *SKILLS_WATCHER_EVENTS.lock().unwrap()
        )
    });
}

fn assert_all_watchers_retired() {
    let events = SKILLS_WATCHER_EVENTS.lock().unwrap();
    for event in events.iter() {
        if let SkillsWatcherEvent::Active(paths) = event {
            let active = events
                .iter()
                .filter(|e| **e == SkillsWatcherEvent::Active(paths.clone()))
                .count();
            let retired = events
                .iter()
                .filter(|e| **e == SkillsWatcherEvent::Retired(paths.clone()))
                .count();
            assert_eq!(active, retired, "unretired native watcher: {events:?}");
        }
    }
}

fn gate(
    fail: bool,
) -> (
    tokio::sync::oneshot::Receiver<()>,
    std::sync::mpsc::Sender<()>,
    ConfigWatcherRegistrationGateGuard,
) {
    let gate = arm_config_watcher_registration_gate(
        shell::util::grow_home::grow_home(),
        WatcherRegistrationKind::Skills,
    );
    CONFIG_WATCHER_REGISTRATION_GATE
        .get()
        .unwrap()
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .fail = fail;
    gate
}

fn isolated_user_home() -> (test_support::EnvGuard, test_support::EnvGuard) {
    let home = shell::util::grow_home::grow_home().join("user-home");
    std::fs::create_dir_all(home.join(".grow/skills")).unwrap();
    (
        test_support::EnvGuard::set("HOME", &home),
        test_support::EnvGuard::set("USERPROFILE", &home),
    )
}

fn user_skills_root() -> PathBuf {
    dirs::home_dir().unwrap().join(".grow/skills")
}

async fn server() -> MockInferenceServer {
    let server = MockInferenceServer::start().await.unwrap();
    server.set_response("AC06 probe complete");
    server
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn config_only_disabled_and_ignore_refresh_callable_and_slash() {
    if crate::test_util::run_with_isolated_grow_home(concat!(
        module_path!(),
        "::config_only_disabled_and_ignore_refresh_callable_and_slash"
    )) {
        return;
    }
    let _home = isolated_user_home();
    let server = server().await;
    skill(&user_skills_root(), "ac06-disabled", "AC06 enabled");
    let ignored = skill(&user_skills_root(), "ac06-ignored", "AC06 retained");
    write_config(&[], &[], &[]);
    let cwd = tempfile::tempdir().unwrap();
    let worker = start_worker(&server).await;
    let session = new_session(&worker, cwd.path()).await;
    worker
        .wait_command("ac06-disabled", Some("AC06 enabled"))
        .await;
    worker
        .wait_command("ac06-ignored", Some("AC06 retained"))
        .await;
    assert_callable(&worker, &server, &session, "ac06-disabled", true).await;
    wait_event(SkillsWatcherEvent::Active(vec![]), 1).await;
    let registrations = event_count(&SkillsWatcherEvent::Started(vec![]));

    // No discovery files change from here through re-enable.
    write_config(&[], &["ac06-disabled"], &[]);
    worker.wait_command("ac06-disabled", None).await;
    assert_callable(&worker, &server, &session, "ac06-disabled", false).await;
    write_config(
        &[],
        &["ac06-disabled"],
        &[ignored.parent().unwrap().to_string_lossy().into_owned()],
    );
    worker.wait_command("ac06-ignored", None).await;
    assert_callable(&worker, &server, &session, "ac06-ignored", false).await;
    write_config(&[], &[], &[]);
    worker
        .wait_command("ac06-disabled", Some("AC06 enabled"))
        .await;
    worker
        .wait_command("ac06-ignored", Some("AC06 retained"))
        .await;
    assert_callable(&worker, &server, &session, "ac06-disabled", true).await;
    assert_callable(&worker, &server, &session, "ac06-ignored", true).await;
    assert_eq!(
        event_count(&SkillsWatcherEvent::Started(vec![])),
        registrations,
        "disabled/ignore updates must not rebuild identical paths"
    );
    worker.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn custom_root_creation_modification_and_removal_follow_config() {
    if crate::test_util::run_with_isolated_grow_home(concat!(
        module_path!(),
        "::custom_root_creation_modification_and_removal_follow_config"
    )) {
        return;
    }
    write_config(&[], &[], &[]);
    let _home = isolated_user_home();
    let server = server().await;
    let cwd = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let custom = paths(external.path());
    let worker = start_worker(&server).await;
    let session = new_session(&worker, cwd.path()).await;
    wait_event(SkillsWatcherEvent::Active(vec![]), 1).await;
    write_config(&custom, &[], &[]);
    wait_event(SkillsWatcherEvent::Active(custom.clone()), 1).await;
    wait_event(SkillsWatcherEvent::Retired(vec![]), 1).await;
    skill(external.path(), "ac06-external", "AC06 created");
    worker
        .wait_command("ac06-external", Some("AC06 created"))
        .await;
    assert_callable(&worker, &server, &session, "ac06-external", true).await;
    skill(external.path(), "ac06-external", "AC06 modified");
    worker
        .wait_command("ac06-external", Some("AC06 modified"))
        .await;
    write_config(&[], &[], &[]);
    worker.wait_command("ac06-external", None).await;
    wait_event(SkillsWatcherEvent::Retired(custom), 1).await;
    assert_callable(&worker, &server, &session, "ac06-external", false).await;
    worker.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn initial_registration_preserves_latest_paths() {
    if crate::test_util::run_with_isolated_grow_home(concat!(
        module_path!(),
        "::initial_registration_preserves_latest_paths"
    )) {
        return;
    }
    write_config(&[], &[], &[]);
    let (started, release, _gate) = gate(false);
    let _home = isolated_user_home();
    let server = server().await;
    let worker = start_worker(&server).await;
    started.await.unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let session = new_session(&worker, cwd.path()).await;
    let external = tempfile::tempdir().unwrap();
    let custom = paths(external.path());
    // This existing skill makes slash acknowledgement prove the config update
    // reached the consumer before initial registration is allowed to complete.
    skill(external.path(), "ac06-initial", "AC06 during registration");
    write_config(&custom, &[], &[]);
    worker
        .wait_command("ac06-initial", Some("AC06 during registration"))
        .await;
    release.send(()).unwrap();
    wait_event(SkillsWatcherEvent::Active(custom), 1).await;
    skill(external.path(), "ac06-initial", "AC06 after registration");
    worker
        .wait_command("ac06-initial", Some("AC06 after registration"))
        .await;
    assert_callable(&worker, &server, &session, "ac06-initial", true).await;
    worker.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn replacement_registration_preserves_latest_paths() {
    if crate::test_util::run_with_isolated_grow_home(concat!(
        module_path!(),
        "::replacement_registration_preserves_latest_paths"
    )) {
        return;
    }
    write_config(&[], &[], &[]);
    let _home = isolated_user_home();
    let server = server().await;
    let worker = start_worker(&server).await;
    let cwd = tempfile::tempdir().unwrap();
    new_session(&worker, cwd.path()).await;
    wait_event(SkillsWatcherEvent::Active(vec![]), 1).await;
    let first = tempfile::tempdir().unwrap();
    let latest = tempfile::tempdir().unwrap();
    let first_paths = paths(first.path());
    let latest_paths = paths(latest.path());
    let (started, release, _gate) = gate(false);
    write_config(&first_paths, &[], &[]);
    started.await.unwrap();
    skill(latest.path(), "ac06-latest", "AC06 latest pending");
    write_config(&latest_paths, &[], &[]);
    worker
        .wait_command("ac06-latest", Some("AC06 latest pending"))
        .await;
    release.send(()).unwrap();
    wait_event(SkillsWatcherEvent::Active(latest_paths), 1).await;
    wait_event(SkillsWatcherEvent::Retired(first_paths), 1).await;
    skill(latest.path(), "ac06-latest", "AC06 latest watched");
    worker
        .wait_command("ac06-latest", Some("AC06 latest watched"))
        .await;
    worker.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn failed_replacement_keeps_prior_native_owner() {
    if crate::test_util::run_with_isolated_grow_home(concat!(
        module_path!(),
        "::failed_replacement_keeps_prior_native_owner"
    )) {
        return;
    }
    write_config(&[], &[], &[]);
    let _home = isolated_user_home();
    let server = server().await;
    let worker = start_worker(&server).await;
    let cwd = tempfile::tempdir().unwrap();
    new_session(&worker, cwd.path()).await;
    wait_event(SkillsWatcherEvent::Active(vec![]), 1).await;
    let external = tempfile::tempdir().unwrap();
    let custom = paths(external.path());
    let (started, release, _gate) = gate(true);
    write_config(&custom, &[], &[]);
    started.await.unwrap();
    release.send(()).unwrap();
    // A real event on the old watcher must still reach this resident Session.
    skill(&user_skills_root(), "ac06-old-owner", "AC06 retained owner");
    worker
        .wait_command("ac06-old-owner", Some("AC06 retained owner"))
        .await;
    assert_eq!(event_count(&SkillsWatcherEvent::Active(custom)), 0);
    assert_eq!(event_count(&SkillsWatcherEvent::Retired(vec![])), 0);
    worker.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn initial_failure_retries_latest_paths_after_config_round_trip() {
    if crate::test_util::run_with_isolated_grow_home(concat!(
        module_path!(),
        "::initial_failure_retries_latest_paths_after_config_round_trip"
    )) {
        return;
    }
    let _home = isolated_user_home();
    // A is the startup empty custom-path list. The native user root remains
    // available for a real disk event once a watcher has successfully registered.
    write_config(&[], &[], &[]);
    let (started, release, _gate) = gate(true);
    let server = server().await;
    let worker = start_worker(&server).await;
    started.await.unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let session = new_session(&worker, cwd.path()).await;
    let external = tempfile::tempdir().unwrap();
    let custom = paths(external.path());
    skill(
        external.path(),
        "ac06-round-trip-b",
        "AC06 config B acknowledged",
    );

    // Both acknowledgements come from the resident Session's reload owner;
    // no synthetic ConfigUpdate or manual watch value can bypass the consumer.
    write_config(&custom, &[], &[]);
    worker
        .wait_command("ac06-round-trip-b", Some("AC06 config B acknowledged"))
        .await;
    write_config(&[], &[], &[]);
    worker.wait_command("ac06-round-trip-b", None).await;
    assert_eq!(event_count(&SkillsWatcherEvent::Started(vec![])), 1);
    assert_eq!(event_count(&SkillsWatcherEvent::Active(vec![])), 0);

    // Initial registration returns None with latest A pending. Equality with
    // startup A must not suppress the first actual owner for those paths.
    release.send(()).unwrap();
    wait_event(SkillsWatcherEvent::Active(vec![]), 1).await;
    assert_eq!(event_count(&SkillsWatcherEvent::Started(vec![])), 2);
    assert_eq!(event_count(&SkillsWatcherEvent::Started(custom)), 0);

    skill(
        &user_skills_root(),
        "ac06-round-trip-a",
        "AC06 latest A watched",
    );
    worker
        .wait_command("ac06-round-trip-a", Some("AC06 latest A watched"))
        .await;
    assert_callable(&worker, &server, &session, "ac06-round-trip-a", true).await;
    assert_eq!(
        event_count(&SkillsWatcherEvent::Started(vec![])),
        2,
        "disk events must not cause extra registration attempts"
    );
    worker.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn initial_failure_can_recover_on_later_paths_update() {
    if crate::test_util::run_with_isolated_grow_home(concat!(
        module_path!(),
        "::initial_failure_can_recover_on_later_paths_update"
    )) {
        return;
    }
    write_config(&[], &[], &[]);
    let (started, release, _gate) = gate(true);
    let _home = isolated_user_home();
    let server = server().await;
    let worker = start_worker(&server).await;
    started.await.unwrap();
    let cwd = tempfile::tempdir().unwrap();
    new_session(&worker, cwd.path()).await;
    let external = tempfile::tempdir().unwrap();
    let custom = paths(external.path());
    write_config(&custom, &[], &[]);
    release.send(()).unwrap();
    wait_event(SkillsWatcherEvent::Active(custom), 1).await;
    skill(external.path(), "ac06-recovered", "AC06 recovered");
    worker
        .wait_command("ac06-recovered", Some("AC06 recovered"))
        .await;
    worker.stop().await;
}

async fn stop_during_replacement(gateway_exit: bool) {
    write_config(&[], &[], &[]);
    let _home = isolated_user_home();
    let server = server().await;
    let mut worker = start_worker(&server).await;
    wait_event(SkillsWatcherEvent::Active(vec![]), 1).await;
    let external = tempfile::tempdir().unwrap();
    let custom = paths(external.path());
    let (started, release, _gate) = gate(false);
    write_config(&custom, &[], &[]);
    started.await.unwrap();
    if gateway_exit {
        worker.tx.take();
    } else {
        worker.cancel.cancel();
    }
    // Teardown must retire the old native owner before the new registration
    // is unblocked; otherwise a blocked constructor could stall gateway exit.
    wait_event(SkillsWatcherEvent::Retired(vec![]), 1).await;
    release.send(()).unwrap();
    worker.stop().await;
    assert_eq!(event_count(&SkillsWatcherEvent::Active(custom.clone())), 1);
    assert_eq!(event_count(&SkillsWatcherEvent::Retired(custom)), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn cancellation_during_replacement_retires_all_native_owners() {
    if crate::test_util::run_with_isolated_grow_home(concat!(
        module_path!(),
        "::cancellation_during_replacement_retires_all_native_owners"
    )) {
        return;
    }
    stop_during_replacement(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn gateway_exit_during_replacement_retires_all_native_owners() {
    if crate::test_util::run_with_isolated_grow_home(concat!(
        module_path!(),
        "::gateway_exit_during_replacement_retires_all_native_owners"
    )) {
        return;
    }
    stop_during_replacement(true).await;
}
