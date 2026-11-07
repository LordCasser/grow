//! AC-03b: management discovery uses the named primary's cwd and plugin snapshot.
//! One process-isolated test drives real MvpAgent -> ACP requests. The cwd-only
//! assertion also distinguishes the old latest-registry implementation without
//! depending on the new sessionId parameter parser rejecting old input.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use acp_transport::{
    AcpAgentGatewayReceiver as GatewayReceiver, AcpAgentGatewaySender as GatewaySender,
    AcpAgentHandler as _, AcpClientHandler, LineBufferedRead, protocol as acp,
};
use serde_json::{Value, json};
use shell::agent::config::Config as AgentConfig;
use shell::agent::mvp_agent::MvpAgent;
use test_support::MockInferenceServer;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

const RPC_TIMEOUT: Duration = Duration::from_secs(60);

struct EnvGuard(&'static str, Option<std::ffi::OsString>);
impl EnvGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(key);
        // This integration binary has one test and owns its environment.
        unsafe { std::env::set_var(key, value) };
        Self(key, previous)
    }
}
impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            match self.1.take() {
                Some(value) => std::env::set_var(self.0, value),
                None => std::env::remove_var(self.0),
            }
        }
    }
}
struct CwdGuard(PathBuf);
impl CwdGuard {
    fn set(path: &Path) -> Self {
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(path).unwrap();
        Self(previous)
    }
}
impl Drop for CwdGuard {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).unwrap();
    }
}

fn skill(root: &Path, name: &str, conditional: bool) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let paths = if conditional {
        "paths: [\"**/*.rs\"]\n"
    } else {
        ""
    };
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Snapshot fixture\n{paths}---\nFixture body\n"),
    )
    .unwrap();
}
fn plugin(root: &Path, name: &str) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("plugin.json"), json!({"name": name}).to_string()).unwrap();
    skill(&dir.join("skills"), "common", false);
    dunce::canonicalize(dir).unwrap()
}
fn directory(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::create_dir_all(&path).unwrap();
    dunce::canonicalize(path).unwrap()
}

struct Client;
#[async_trait::async_trait(?Send)]
impl AcpClientHandler for Client {
    async fn request_permission(
        &self,
        _args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        Ok(acp::RequestPermissionResponse::new(
            acp::RequestPermissionOutcome::Cancelled,
        ))
    }
    async fn session_notification(&self, _args: acp::SessionNotification) -> acp::Result<()> {
        Ok(())
    }
}
async fn request(
    conn: &acp_transport::ClientSideConnection,
    method: &str,
    params: Value,
) -> acp::Result<Value> {
    let raw = serde_json::value::RawValue::from_string(params.to_string()).unwrap();
    let response = tokio::time::timeout(
        RPC_TIMEOUT,
        conn.ext_method(acp::ExtRequest::new(method, std::sync::Arc::from(raw))),
    )
    .await
    .unwrap_or_else(|_| panic!("{method} timed out"))?;
    Ok(serde_json::from_str(response.0.get()).unwrap())
}
async fn success(conn: &acp_transport::ClientSideConnection, method: &str, params: Value) -> Value {
    let response = request(conn, method, params).await.unwrap();
    assert!(response.get("error").is_none(), "{method}: {response}");
    assert!(!response["result"].is_null(), "{method}: {response}");
    response["result"].clone()
}
fn keys(response: &Value) -> BTreeSet<String> {
    response["skills"]
        .as_array()
        .expect("skills list")
        .iter()
        .map(|entry| match entry["plugin_name"].as_str() {
            Some(plugin) => format!("{plugin}:{}", entry["name"].as_str().unwrap()),
            None => entry["name"].as_str().unwrap().to_owned(),
        })
        .collect()
}
fn expected(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}
async fn connect(config: &str, cli_plugins: &[PathBuf]) -> acp_transport::ClientSideConnection {
    let raw: toml::Value = toml::from_str(config).unwrap();
    let mut config = AgentConfig::new_from_toml_cfg(&raw).unwrap();
    config.models.default = Some("test/test-model".into());
    config.default_model_override = None;
    config.remote_settings = Some(shell::util::config::RemoteSettings::default());
    config.plugins.cli_plugin_dirs = cli_plugins.to_vec();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = MvpAgent::new(GatewaySender::new(tx), &config).unwrap();
    let (c2a_a, c2a_b) = tokio::io::duplex(8 * 1024 * 1024);
    let (a2c_a, a2c_b) = tokio::io::duplex(8 * 1024 * 1024);
    let (agent_conn, agent_io) = acp_transport::connect_agent_v1(
        agent,
        a2c_a.compat_write(),
        LineBufferedRead::spawn_local(c2a_b.compat()),
        |future| {
            tokio::task::spawn_local(future);
        },
    );
    tokio::task::spawn_local(GatewayReceiver::new(rx, agent_conn).run());
    tokio::task::spawn_local(agent_io);
    let (conn, client_io) = acp_transport::connect_client_v1(
        Client,
        c2a_a.compat_write(),
        LineBufferedRead::spawn_local(a2c_b.compat()),
        |future| {
            tokio::task::spawn_local(future);
        },
    );
    tokio::task::spawn_local(client_io);
    let initialized = conn
        .initialize(
            acp::InitializeRequest::new(acp::ProtocolVersion::V1).meta(
                json!({"startupHints": {"nonInteractive": true, "skipGitStatus": true,
                "skipProjectLayout": true}, "clientType": "skills-snapshot-test"})
                .as_object()
                .cloned(),
            ),
        )
        .await
        .unwrap();
    let auth = initialized
        .auth_methods
        .iter()
        .find(|method| &*method.id().0 == "provider.api_key")
        .unwrap();
    conn.authenticate(
        acp::AuthenticateRequest::new(auth.id().clone())
            .meta(json!({"headless": true}).as_object().cloned()),
    )
    .await
    .unwrap();
    conn
}
async fn session(
    conn: &acp_transport::ClientSideConnection,
    cwd: &Path,
    plugins: &[PathBuf],
) -> acp::SessionId {
    tokio::time::timeout(
        RPC_TIMEOUT,
        conn.new_session(
            acp::NewSessionRequest::new(cwd.to_path_buf()).meta(
                json!({"modelId": "test-model", "pluginDirs": plugins})
                    .as_object()
                    .cloned(),
            ),
        ),
    )
    .await
    .unwrap()
    .unwrap()
    .session_id
}

#[test]
fn session_skills_use_exact_owner_snapshot() {
    diagnostics::tls::install_ring_provider_once();
    let mock_runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let server = mock_runtime.block_on(MockInferenceServer::start()).unwrap();
    let home_temp = tempfile::tempdir().unwrap();
    let home = dunce::canonicalize(home_temp.path()).unwrap();
    let grow = directory(&home, ".grow");
    let _home = EnvGuard::set("HOME", &home);
    let _grow = EnvGuard::set("GROW_HOME", &grow);
    let _proxy = EnvGuard::set("GROW_CLI_CHAT_PROXY_BASE_URL", server.url());
    let _inference = EnvGuard::set("GROW_INFERENCE_BASE_URL", server.url());
    let launcher = directory(&home, "launcher");
    let neutral = directory(&home, "neutral");
    let cwd_a = directory(&home, "project-a");
    let cwd_b = directory(&home, "project-b");
    let shared = directory(&home, "shared");
    skill(&cwd_a.join(".grow/skills"), "native-a", false);
    skill(&cwd_b.join(".grow/skills"), "native-b", false);
    skill(&shared.join(".grow/skills"), "waiting", true);
    skill(&neutral.join(".grow/skills"), "native-only", false);
    plugin(&launcher.join(".grow/plugins"), "latest");
    let cli = plugin(&home.join("plugins"), "process");
    let alpha = plugin(&home.join("plugins"), "alpha");
    let beta = plugin(&home.join("plugins"), "beta");
    let config = format!(
        "[models]\ndefault = 'test/test-model'\n\
         [provider.test]\napi_backend = 'chat_completions'\n\
         [provider.test.options]\nbase_url = '{}'\n\
         [provider.test.models.test-model]\nname = 'Test Model'\ncontext_window = 128000\n\
         [plugins]\nenabled = ['latest']\n",
        server.url(),
    );
    std::fs::write(grow.join("config.toml"), &config).unwrap();
    let _cwd = CwdGuard::set(&launcher);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(tokio::task::LocalSet::new().run_until(async {
        let conn = connect(&config, std::slice::from_ref(&cli)).await;
        let a = session(&conn, &cwd_a, std::slice::from_ref(&alpha)).await;
        let b = session(&conn, &cwd_b, std::slice::from_ref(&beta)).await;
        let c = session(&conn, &shared, std::slice::from_ref(&alpha)).await;
        let d = session(&conn, &shared, std::slice::from_ref(&beta)).await;
        // Prove the production launch-dir registry really contains the poison.
        let latest = success(&conn, "grow/plugins/list", json!({"sessionId": "unknown"})).await;
        assert!(
            latest["plugins"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["name"] == "latest")
        );
        // Old-red target: this valid legacy request must not borrow latest.
        let disk = success(&conn, "grow/skills/list", json!({"cwd": neutral})).await;
        assert_eq!(
            keys(&disk),
            expected(&["native-only", "process:common"]),
            "cwd-only discovery borrowed the shared latest registry"
        );
        for (id, names) in [
            (&a, vec!["native-a", "alpha:common", "process:common"]),
            (&b, vec!["native-b", "beta:common", "process:common"]),
            (&c, vec!["waiting", "alpha:common", "process:common"]),
            (&d, vec!["waiting", "beta:common", "process:common"]),
        ] {
            let catalog = success(&conn, "grow/skills/list", json!({"sessionId": id.0})).await;
            assert_eq!(keys(&catalog), expected(&names));
        }
        // Global disabled identity is preserved; the management catalog keeps it.
        let toggled = success(
            &conn,
            "grow/skills/toggle",
            json!({"sessionId": a.0, "name": "alpha:common", "enabled": false}),
        )
        .await;
        assert!(
            toggled["skills"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["plugin_name"] == "alpha" && s["enabled"] == false)
        );
        let shared_catalog = success(
            &conn,
            "grow/skills/list",
            json!({"sessionId": c.0, "cwd": null}),
        )
        .await;
        assert_eq!(
            keys(&shared_catalog),
            expected(&["waiting", "alpha:common", "process:common"])
        );
        assert!(
            shared_catalog["skills"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["plugin_name"] == "alpha" && s["enabled"] == false)
        );
        let shared_disk = success(&conn, "grow/skills/list", json!({"cwd": shared})).await;
        assert_eq!(
            keys(&shared_disk),
            expected(&["waiting", "process:common"]),
            "disk discovery must exclude roots from both sessions sharing cwd"
        );
        let still_latest =
            success(&conn, "grow/plugins/list", json!({"sessionId": "unknown"})).await;
        assert_eq!(
            still_latest, latest,
            "read-only discovery must not publish a new latest"
        );
        let configured = success(&conn, "grow/skills/config", json!({"sessionId": b.0})).await;
        assert_eq!(
            keys(&configured),
            expected(&["native-b", "beta:common", "process:common"])
        );
        let added = success(
            &conn,
            "grow/skills/add",
            json!({"sessionId": a.0, "path": "custom"}),
        )
        .await;
        assert_eq!(
            added["path"],
            cwd_a.join("custom").to_string_lossy().as_ref()
        );
        assert_eq!(
            keys(&added),
            expected(&["native-a", "alpha:common", "process:common"])
        );
        let removed = success(
            &conn,
            "grow/skills/remove",
            json!({"sessionId": a.0, "path": "custom"}),
        )
        .await;
        assert_eq!(removed["path"], added["path"]);
        let reset = success(&conn, "grow/skills/reset", json!({"sessionId": c.0})).await;
        assert_eq!(
            keys(&reset),
            expected(&["waiting", "alpha:common", "process:common"])
        );
        for params in [json!({}), json!({"cwd": null})] {
            let default = success(&conn, "grow/skills/config", params).await;
            assert_eq!(
                keys(&default),
                expected(&["latest:common", "process:common"])
            );
        }
        let before = std::fs::read(grow.join("config.toml")).unwrap();
        let missing_skill = request(
            &conn,
            "grow/skills/toggle",
            json!({"sessionId": a.0,
            "name": "beta:common", "enabled": false}),
        )
        .await
        .unwrap();
        assert!(
            missing_skill["error"]
                .as_str()
                .unwrap()
                .contains("not found"),
            "toggle must validate against the named owner: {missing_skill}"
        );
        assert_eq!(std::fs::read(grow.join("config.toml")).unwrap(), before);
        for params in [json!({}), json!({"cwd": null})] {
            let error = request(&conn, "grow/skills/list", params)
                .await
                .unwrap_err();
            assert_eq!(error.code, acp::Error::invalid_params().code);
        }
        for method in ["grow/skills/reset", "grow/skills/config"] {
            for params in [Value::Null, json!([]), json!({"cwd": false})] {
                let error = request(&conn, method, params).await.unwrap_err();
                assert_eq!(error.code, acp::Error::invalid_params().code);
                assert_eq!(std::fs::read(grow.join("config.toml")).unwrap(), before);
            }
        }
        for method in ["add", "remove", "reset", "config", "toggle", "list"] {
            for context in [
                json!({"sessionId": "missing"}),
                json!({"sessionId": ""}),
                json!({"sessionId": 1}),
                json!({"sessionId": a.0, "cwd": neutral}),
            ] {
                let mut params = context.as_object().unwrap().clone();
                params.insert("path".into(), json!("would-mutate"));
                params.insert("name".into(), json!("alpha:common"));
                params.insert("enabled".into(), json!(true));
                assert!(
                    request(
                        &conn,
                        &format!("grow/skills/{method}"),
                        Value::Object(params)
                    )
                    .await
                    .is_err(),
                    "{method} accepted invalid context: {context}"
                );
                assert_eq!(std::fs::read(grow.join("config.toml")).unwrap(), before);
            }
        }
        for id in [&a, &b, &c, &d] {
            success(&conn, "grow/session/close", json!({"sessionId": id.0})).await;
        }
        assert!(
            request(&conn, "grow/skills/reset", json!({"sessionId": a.0}))
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(grow.join("config.toml")).unwrap(), before);
        // A separate real owner acknowledges None: native-only remains success.
        let _neutral_cwd = CwdGuard::set(&neutral);
        let empty = connect(&config, &[]).await;
        let id = session(&empty, &neutral, &[]).await;
        let plugins = success(&empty, "grow/plugins/list", json!({"sessionId": id.0})).await;
        assert_eq!(plugins["plugins"], json!([]));
        let native = success(&empty, "grow/skills/list", json!({"sessionId": id.0})).await;
        assert_eq!(keys(&native), expected(&["native-only"]));
        success(&empty, "grow/session/close", json!({"sessionId": id.0})).await;
    }));
}
