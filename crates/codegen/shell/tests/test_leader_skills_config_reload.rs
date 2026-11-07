//! AC06 real leader: edit config.toml, observe existing session slash commands
//! and model load_skill results, then exercise external-root native events.
//! Requires a freshly built GROW_BINARY; no real provider is contacted.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use futures::FutureExt as _;
use serde_json::{Value, json};
use shell::leader::{ClientCapabilities, ClientMode, LeaderClient};
use test_support::leader::LeaderFixture;
use test_support::sse::chat_completions_reasoning_then_tool_call_events;
use test_support::*;

const TIMEOUT: Duration = Duration::from_secs(45);

struct Client {
    ipc: LeaderClient,
    next_id: u64,
    commands: BTreeMap<String, Vec<Value>>,
}
impl Client {
    async fn receive(&mut self) -> Value {
        let wire = self.ipc.recv().await.expect("leader disconnected");
        let value: Value = serde_json::from_str(&wire).unwrap();
        if value["method"] == "session/update"
            && value["params"]["update"]["sessionUpdate"] == "available_commands_update"
        {
            self.commands.insert(
                value["params"]["sessionId"].as_str().unwrap().to_owned(),
                value["params"]["update"]["availableCommands"]
                    .as_array()
                    .unwrap()
                    .clone(),
            );
        }
        value
    }

    async fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.ipc
            .send(json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}).to_string())
            .unwrap();
        tokio::time::timeout(TIMEOUT, async {
            loop {
                let value = self.receive().await;
                if value["id"] == id {
                    assert!(value.get("error").is_none(), "{method}: {value}");
                    return value["result"].clone();
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{method} timed out"))
    }

    async fn commands(&mut self, session: &str, expected: &[(&str, bool)]) {
        tokio::time::timeout(TIMEOUT, async {
            loop {
                if let Some(commands) = self.commands.get(session)
                    && expected.iter().all(|(identity, present)| {
                        // Slash names are flat; plugin ownership is advertised
                        // separately. Compare the catalog identity used by the
                        // fixture's model calls and disabled configuration.
                        let (plugin, name) = match identity.split_once(':') {
                            Some((plugin, name)) => (Some(plugin), name),
                            None => (None, *identity),
                        };
                        commands.iter().any(|command| {
                            command["_meta"]["grow/commandSource"] == "skill"
                                && command["name"] == name
                                && command["_meta"]["plugin_name"].as_str() == plugin
                        }) == *present
                    })
                {
                    return;
                }
                self.receive().await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("slash advertisement did not converge: {:?}", self.commands));
    }
}

fn skill(root: &Path, name: &str, description: &str, marker: &str) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n{marker}\n"),
    )
    .unwrap();
}

fn write_config(path: &Path, base: &str, paths: &[String], disabled: &[&str], ignore: &[String]) {
    std::fs::write(
        path,
        format!(
            "{base}\n[skills]\npaths = {}\ndisabled = {}\nignore = {}\n",
            json!(paths),
            json!(disabled),
            json!(ignore)
        ),
    )
    .unwrap();
}

fn log_count(sandbox: &TestSandbox, text: &str) -> usize {
    std::fs::read_to_string(sandbox.grow_home().join("leader.log"))
        .unwrap_or_default()
        .matches(text)
        .count()
}
async fn wait_log(sandbox: &TestSandbox, text: &str, count: usize) {
    tokio::time::timeout(TIMEOUT, async {
        while log_count(sandbox, text) < count {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("missing leader diagnostic {text}"));
}

async fn model_load(
    client: &mut Client,
    server: &MockInferenceServer,
    session: &str,
    name: &str,
    expected: &str,
    forbidden: Option<&str>,
) {
    let call_id = format!("call_ac06_{}", client.next_id);
    let scripted = server.expect_response(
        call_id.clone(),
        InferenceRequestMatcher::foreground(InferenceEndpoint::ChatCompletions),
        ScriptedResponse::sse(chat_completions_reasoning_then_tool_call_events(
            "Load the requested skill.",
            &call_id,
            "load_skill",
            &json!({"name":name}).to_string(),
            "ac06-wire",
        )),
    );
    let before = server.request_bodies().len();
    client.rpc("session/prompt", json!({"sessionId":session,"prompt":[{"type":"text","text":format!("Check {name} via load_skill.")}]})).await;
    scripted.assert_satisfied();
    let bodies = server.request_bodies();
    let output = bodies[before..]
        .iter()
        .filter_map(|body| body["messages"].as_array())
        .flatten()
        .find(|message| message["tool_call_id"] == call_id)
        .expect("model must receive the actual load_skill result")["content"]
        .to_string();
    assert!(output.contains(expected), "{name}: {output}");
    if let Some(forbidden) = forbidden {
        assert!(
            !output.contains(forbidden),
            "unavailable skill body leaked: {output}"
        );
    }
}

#[tokio::test]
#[ignore = "requires a freshly built grow binary"]
async fn leader_config_file_refreshes_catalog_slash_and_custom_root_events() {
    tokio::task::LocalSet::new().run_until(async {
        let server = MockInferenceServer::start().await.unwrap();
        server.set_response("Check complete.");
        let sandbox = TestSandbox::builder().git().mock_url(server.url()).build();
        let native = sandbox.workspace().join(".grow/skills");
        let plugin = sandbox.root().join("ac06-plugin");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join("plugin.json"), json!({"name":"ac06"}).to_string()).unwrap();
        skill(&native, "ac06-native", "Native config fixture", "AC06_NATIVE_BODY");
        skill(&plugin.join("skills"), "ac06-shared", "Plugin config fixture", "AC06_PLUGIN_BODY");
        let base = format!(r#"[models]
default = "ac06/model"
[provider.ac06]
api_backend = "chat_completions"
[provider.ac06.options]
base_url = "{}"
env_key = "GROW_API_KEY"
[provider.ac06.models.model]
model = "ac06-wire"
context_window = 200000
agent_type = "grow-build"
"#, server.url());
        let config_path = sandbox.grow_home().join("config.toml");
        write_config(&config_path, &base, &[], &[], &[]);
        let fixture = LeaderFixture::start(&server, sandbox.workspace(), &sandbox).await.unwrap();
        let mut client = Client {
            ipc: LeaderClient::connect(sandbox.grow_home().join("leader.sock"), "ac06-leader-test", ClientMode::Stdio,
                ClientCapabilities { permission_mode: diagnostics::enums::PermissionMode::AlwaysApprove, ..Default::default() }).await.unwrap(),
            next_id: 0,
            commands: BTreeMap::new(),
        };
        let result = std::panic::AssertUnwindSafe(async {
            let init = client.rpc("initialize", json!({"protocolVersion":1,"clientCapabilities":{},"_meta":{"startupHints":{"nonInteractive":true,"skipGitStatus":true,"skipProjectLayout":true}}})).await;
            let auth: acp_transport::protocol::InitializeResponse = serde_json::from_value(init).unwrap();
            let method = auth.auth_methods.iter().find(|method| &*method.id().0 == "provider.api_key").unwrap();
            client.rpc("authenticate", serde_json::to_value(acp_transport::protocol::AuthenticateRequest::new(method.id().clone()).meta(json!({"headless":true}).as_object().cloned())).unwrap()).await;
            let created = client.rpc("session/new", json!({"cwd":sandbox.workspace(),"mcpServers":[],"_meta":{"modelId":"ac06/model","pluginDirs":[plugin]}})).await;
            let session = created["sessionId"].as_str().unwrap();
            client.commands(session, &[("ac06-native", true), ("ac06:ac06-shared", true)]).await;
            wait_log(&sandbox, "config file watcher started", 1).await;
            model_load(&mut client, &server, session, "ac06:ac06-shared", "AC06_PLUGIN_BODY", None).await;

            // Only config.toml changes here: neither Skills API nor UI toggle
            // can supply the reload that this regression is intended to prove.
            write_config(&config_path, &base, &[], &["ac06:ac06-shared"], &[]);
            client.commands(session, &[("ac06-native", true), ("ac06:ac06-shared", false)]).await;
            model_load(&mut client, &server, session, "ac06:ac06-shared", "disabled", Some("AC06_PLUGIN_BODY")).await;
            let ignored = native.join("ac06-native").to_string_lossy().into_owned();
            write_config(&config_path, &base, &[], &[], &[ignored]);
            client.commands(session, &[("ac06-native", false), ("ac06:ac06-shared", true)]).await;
            model_load(&mut client, &server, session, "ac06-native", "not found", Some("AC06_NATIVE_BODY")).await;
            model_load(&mut client, &server, session, "ac06:ac06-shared", "AC06_PLUGIN_BODY", None).await;

            let external = sandbox.root().join("external-skills");
            std::fs::create_dir_all(&external).unwrap();
            let registrations = log_count(&sandbox, "skills file watcher started");
            write_config(&config_path, &base, &[external.to_string_lossy().into_owned()], &[], &[]);
            wait_log(&sandbox, "skills file watcher started", registrations + 1).await;
            skill(&external, "ac06-external", "External version one", "AC06_EXTERNAL_ONE");
            client.commands(session, &[("ac06-external", true), ("ac06-native", true)]).await;
            model_load(&mut client, &server, session, "ac06-external", "AC06_EXTERNAL_ONE", None).await;
            skill(&external, "ac06-external", "External version two", "AC06_EXTERNAL_TWO");
            tokio::time::timeout(TIMEOUT, async {
                loop {
                    if client.commands.get(session).is_some_and(|commands| commands.iter().any(|command| command["name"] == "ac06-external" && command["description"] == "External version two")) {
                        break;
                    }
                    client.receive().await;
                }
            }).await.expect("external skill edits must refresh slash metadata");
            // A fresh name proves model-callable catalog adoption without
            // confusing loaded-body snapshot semantics with disk refresh.
            skill(&external, "ac06-external-later", "External later", "AC06_EXTERNAL_LATER");
            client.commands(session, &[("ac06-external-later", true)]).await;
            model_load(&mut client, &server, session, "ac06-external-later", "AC06_EXTERNAL_LATER", None).await;
            let registrations = log_count(&sandbox, "skills file watcher started");
            write_config(&config_path, &base, &[], &[], &[]);
            client.commands(session, &[("ac06-external", false), ("ac06-external-later", false)]).await;
            wait_log(&sandbox, "skills file watcher started", registrations + 1).await;
            // Queued events may still cause an idempotent reload. Verify the
            // removed root cannot restore catalog visibility or model access;
            // native owner retirement is covered by leader_skills_tests.
            skill(&external, "ac06-after-removal", "Removed root", "MUST_NOT_DISCOVER");
            skill(&external, "ac06-external", "Edited after removal", "AC06_EXTERNAL_ONE");
            // A still-active native source supplies a real event/adoption
            // barrier instead of assuming the global reload count stays fixed.
            skill(&native, "ac06-removal-barrier", "Active root after removal", "AC06_ACTIVE_ROOT");
            client.commands(session, &[("ac06-removal-barrier", true), ("ac06-external", false), ("ac06-external-later", false), ("ac06-after-removal", false)]).await;
            model_load(&mut client, &server, session, "ac06-after-removal", "not found", Some("MUST_NOT_DISCOVER")).await;
            model_load(&mut client, &server, session, "ac06-external", "not found", Some("AC06_EXTERNAL_ONE")).await;
            model_load(&mut client, &server, session, "ac06-external-later", "not found", Some("AC06_EXTERNAL_LATER")).await;
            client.commands(session, &[("ac06-removal-barrier", true), ("ac06-external", false), ("ac06-external-later", false), ("ac06-after-removal", false)]).await;
        }).catch_unwind().await;
        client.ipc.cancel();
        fixture.close().await.expect("close concrete leader fixture");
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }).await;
}
