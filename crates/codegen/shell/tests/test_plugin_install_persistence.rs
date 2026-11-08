//! AC-08: ordinary Install/Uninstall through the shared application and actual ACP actor.
//! Re-exec before HOME/GROW_HOME OnceLocks or runtime initialization.

use std::path::{Path, PathBuf};
use std::time::Duration;

use acp_transport::{
    AcpAgentGatewayReceiver as GatewayReceiver, AcpAgentGatewaySender as GatewaySender,
    AcpAgentHandler as _, AcpClientHandler, LineBufferedRead, protocol as acp,
};
use agent::plugins::git_install::{self, InstallSource};
use agent::plugins::install_registry::{InstallError, InstallRegistry};
use serde_json::{Value, json};
use shell::agent::config::Config as AgentConfig;
use shell::agent::mvp_agent::MvpAgent;
use test_support::MockInferenceServer;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

fn isolated(name: &str) -> Option<PathBuf> {
    const CHILD: &str = "GROW_TEST_INSTALL_PERSISTENCE_CHILD";
    if let Some(home) = std::env::var_os(CHILD) {
        return Some(PathBuf::from(home));
    }
    let temp = tempfile::tempdir().unwrap();
    let home = dunce::canonicalize(temp.path()).unwrap();
    std::fs::create_dir_all(home.join(".grow")).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, &home)
        .env("HOME", &home)
        .env("GROW_HOME", home.join(".grow"))
        .env("GROW_MARKETPLACE_REQUIRE_SHA", "1")
        .env("GROW_LSP_TOOLS", "0")
        .env_remove("GROW_TEST_FAIL_REGISTRY_SAVE_AFTER_SERIALIZE")
        .current_dir(&home)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "isolated {name} failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    None
}

fn source(home: &Path, name: &str) -> PathBuf {
    let path = home.join(name);
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("plugin.json"),
        json!({"name": name, "version": "1.0.0"}).to_string(),
    )
    .unwrap();
    std::fs::write(path.join("payload.txt"), "original snapshot").unwrap();
    path
}

struct Fixture {
    home: PathBuf,
    install_dir: PathBuf,
    registry_path: PathBuf,
    registry_bytes: Vec<u8>,
    config_path: PathBuf,
    config_bytes: Vec<u8>,
    old_artifact: PathBuf,
}

impl Fixture {
    fn new(home: PathBuf) -> Self {
        let install_dir = home.join(".grow/installed-plugins");
        let old_source = InstallSource::Local {
            path: source(&home, "existing"),
            subdir: None,
        };
        let mut registry = InstallRegistry::empty(install_dir.clone());
        let result = git_install::install_from_source(&old_source, &registry, true).unwrap();
        let repo = git_install::build_installed_repo(&result, &old_source);
        registry.insert(result.repo_key, repo);
        registry.save().unwrap();
        let config_path = home.join(".grow/config.toml");
        let config_bytes = b"[plugins]\nenabled = [\"existing\"]\n".to_vec();
        std::fs::write(&config_path, &config_bytes).unwrap();
        let registry_path = install_dir.join("registry.json");
        let registry_bytes = std::fs::read(&registry_path).unwrap();
        Self {
            home,
            install_dir,
            registry_path,
            registry_bytes,
            config_path,
            config_bytes,
            old_artifact: result.repo_path,
        }
    }

    fn artifact(&self, source: &Path) -> PathBuf {
        self.install_dir
            .join(InstallRegistry::repo_key(source.to_str().unwrap()))
    }

    fn unchanged(&self) {
        assert_eq!(
            std::fs::read(&self.registry_path).unwrap(),
            self.registry_bytes
        );
        assert_eq!(std::fs::read(&self.config_path).unwrap(), self.config_bytes);
        assert_eq!(
            std::fs::read_to_string(self.old_artifact.join("payload.txt")).unwrap(),
            "original snapshot"
        );
    }
}

struct SaveFault;
impl SaveFault {
    fn new() -> Self {
        unsafe { std::env::set_var("GROW_TEST_FAIL_REGISTRY_SAVE_AFTER_SERIALIZE", "1") };
        Self
    }
}
impl Drop for SaveFault {
    fn drop(&mut self) {
        unsafe { std::env::remove_var("GROW_TEST_FAIL_REGISTRY_SAVE_AFTER_SERIALIZE") };
    }
}

#[test]
fn shared_install_reports_persistence_failures() {
    let Some(home) = isolated("shared_install_reports_persistence_failures") else {
        return;
    };
    let fixture = Fixture::new(home);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let failed_source = source(&fixture.home, "shared-save-failure");
        let fault = SaveFault::new();
        let result = shell::plugin::install_plugin(failed_source.to_str().unwrap(), &fixture.home).await;
        drop(fault);
        let artifact = fixture.artifact(&failed_source);
        fixture.unchanged();
        assert!(artifact.join("plugin.json").is_file(), "save failure preserves published files");
        let error = match result {
            Err(error) => error.to_string(),
            Ok(outcome) => panic!("registry save failure returned Ok: repo={}, names={:?}, warnings={:?}, artifact={}, old registry bytes unchanged", outcome.repo_key, outcome.plugin_names, outcome.warnings, artifact.display()),
        };
        assert!(error.contains("test-injected registry save failure"), "{error}");
        assert!(error.contains(artifact.to_str().unwrap()), "{error}");
        assert!(error.contains(artifact.file_name().unwrap().to_str().unwrap()), "{error}");
        assert!(error.contains("published") && error.contains("incomplete"), "{error}");

        let bad_source = source(&fixture.home, "shared-bad-registry");
        let corrupt = b"{ broken install registry";
        std::fs::write(&fixture.registry_path, corrupt).unwrap();
        assert!(matches!(shell::plugin::install_plugin(bad_source.to_str().unwrap(), &fixture.home).await, Err(InstallError::Json { .. })));
        assert_eq!(std::fs::read(&fixture.registry_path).unwrap(), corrupt);
        assert!(!fixture.artifact(&bad_source).exists());
        std::fs::remove_file(&fixture.registry_path).unwrap();
        std::fs::create_dir(&fixture.registry_path).unwrap();
        assert!(matches!(shell::plugin::install_plugin(bad_source.to_str().unwrap(), &fixture.home).await, Err(InstallError::Io { .. })));
        assert!(fixture.registry_path.is_dir());
        assert!(!fixture.artifact(&bad_source).exists());
        std::fs::remove_dir(&fixture.registry_path).unwrap();
        std::fs::write(&fixture.registry_path, &fixture.registry_bytes).unwrap();
        fixture.unchanged();

        let refused = shell::plugin::install_plugin("https://example.invalid/unpinned.git", &fixture.home).await;
        assert!(matches!(refused, Err(InstallError::UnpinnedRemoteRefused { .. })));
        fixture.unchanged();

        let normal_source = source(&fixture.home, "shared-success");
        let normal = shell::plugin::install_plugin(normal_source.to_str().unwrap(), &fixture.home).await.unwrap();
        assert_eq!(normal.plugin_names, ["shared-success"]);
        assert!(normal.is_local);
        assert!(normal.warnings.is_empty(), "{:?}", normal.warnings);
        let registry = InstallRegistry::try_load_from(fixture.install_dir.clone()).unwrap();
        let repo = registry.get_repo(&normal.repo_key).unwrap();
        assert_eq!(repo.path, fixture.artifact(&normal_source));
        assert!(repo.plugins.contains_key("shared-success"));
        std::fs::write(normal_source.join("payload.txt"), "changed source").unwrap();
        assert_eq!(std::fs::read_to_string(repo.path.join("payload.txt")).unwrap(), "original snapshot");
        assert!(std::fs::read_to_string(&fixture.config_path).unwrap().contains("shared-success"));

        // Core persistence succeeds even when the existing auto-enable policy warns.
        let warning_source = source(&fixture.home, "shared-warning");
        std::fs::write(&fixture.config_path, "invalid = [").unwrap();
        let warning = shell::plugin::install_plugin(warning_source.to_str().unwrap(), &fixture.home).await.unwrap();
        assert_eq!(warning.plugin_names, ["shared-warning"]);
        assert!(warning.warnings.iter().any(|w| w.contains("auto-enable shared-warning")), "{:?}", warning.warnings);
        assert!(InstallRegistry::try_load_from(fixture.install_dir.clone()).unwrap().get_repo(&warning.repo_key).is_some());
        assert_eq!(std::fs::read_to_string(&fixture.config_path).unwrap(), "invalid = [");
    });
}

struct QuietClient;
#[async_trait::async_trait(?Send)]
impl AcpClientHandler for QuietClient {
    async fn request_permission(
        &self,
        _: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        Ok(acp::RequestPermissionResponse::new(
            acp::RequestPermissionOutcome::Cancelled,
        ))
    }
    async fn session_notification(&self, _: acp::SessionNotification) -> acp::Result<()> {
        Ok(())
    }
}

async fn connect(base_url: &str) -> acp_transport::ClientSideConnection {
    let raw: toml::Value = toml::from_str(&format!(
        "[features]\nlsp_tools = false\n\n[models]\ndefault = \"test/test-model\"\n\n[provider.test]\napi_backend = \"chat_completions\"\n\n[provider.test.options]\nbase_url = \"{base_url}\"\n\n[provider.test.models.test-model]\nname = \"Test Model\"\ncontext_window = 128000\n"
    )).unwrap();
    let mut config = AgentConfig::new_from_toml_cfg(&raw).unwrap();
    config.models.default = Some("test/test-model".into());
    config.default_model_override = None;
    config.remote_settings = Some(shell::util::config::RemoteSettings::default());
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = MvpAgent::new(GatewaySender::new(tx), &config).unwrap();
    let (client_write, agent_read) = tokio::io::duplex(8 * 1024 * 1024);
    let (agent_write, client_read) = tokio::io::duplex(8 * 1024 * 1024);
    let (agent_conn, agent_io) = acp_transport::connect_agent_v1(
        agent,
        agent_write.compat_write(),
        LineBufferedRead::spawn_local(agent_read.compat()),
        |f| {
            tokio::task::spawn_local(f);
        },
    );
    tokio::task::spawn_local(GatewayReceiver::new(rx, agent_conn).run());
    tokio::task::spawn_local(agent_io);
    let (conn, client_io) = acp_transport::connect_client_v1(
        QuietClient,
        client_write.compat_write(),
        LineBufferedRead::spawn_local(client_read.compat()),
        |f| {
            tokio::task::spawn_local(f);
        },
    );
    tokio::task::spawn_local(client_io);
    let init = conn.initialize(acp::InitializeRequest::new(acp::ProtocolVersion::V1).meta(json!({"startupHints": {"nonInteractive": true, "skipGitStatus": true, "skipProjectLayout": true}, "clientType": "acp-plugin-install-test", "clientVersion": "0.0-test"}).as_object().cloned())).await.unwrap();
    let method = init
        .auth_methods
        .iter()
        .find(|m| &*m.id().0 == "provider.api_key")
        .unwrap();
    conn.authenticate(
        acp::AuthenticateRequest::new(method.id().clone())
            .meta(json!({"headless": true}).as_object().cloned()),
    )
    .await
    .unwrap();
    conn
}

async fn ext(conn: &acp_transport::ClientSideConnection, method: &str, params: Value) -> Value {
    let raw = serde_json::value::RawValue::from_string(params.to_string()).unwrap();
    let response = conn
        .ext_method(acp::ExtRequest::new(method, std::sync::Arc::from(raw)))
        .await
        .unwrap();
    serde_json::from_str(response.0.get()).unwrap()
}

async fn install(
    conn: &acp_transport::ClientSideConnection,
    sid: &acp::SessionId,
    source: &str,
) -> Value {
    let response = ext(
        conn,
        "grow/plugins/action",
        json!({"sessionId": sid.0.as_ref(), "action": {"type": "install", "source": source}}),
    )
    .await;
    assert!(response.get("error").is_none(), "{response}");
    response["result"].clone()
}

fn failed(outcome: &Value) {
    assert_eq!(outcome["status"], "internal_error", "{outcome}");
    assert_eq!(outcome["requiresReload"], false, "{outcome}");
    assert_eq!(outcome["requiresRestart"], false, "{outcome}");
}

#[test]
fn acp_install_reports_persistence_failures() {
    let Some(home) = isolated("acp_install_reports_persistence_failures") else {
        return;
    };
    diagnostics::tls::install_ring_provider_once();
    let fixture = Fixture::new(home);
    let mock_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let server = mock_rt.block_on(MockInferenceServer::start()).unwrap();
    unsafe {
        std::env::set_var("GROW_CLI_CHAT_PROXY_BASE_URL", server.url());
        std::env::set_var("GROW_INFERENCE_BASE_URL", server.url());
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    rt.block_on(local.run_until(async {
        tokio::time::timeout(Duration::from_secs(120), async {
            let conn = connect(&server.url()).await;
            let sid = conn
                .new_session(
                    acp::NewSessionRequest::new(fixture.home.clone())
                        .meta(json!({"modelId": "test-model"}).as_object().cloned()),
                )
                .await
                .unwrap()
                .session_id;
            let empty = install(&conn, &sid, "").await;
            assert_eq!(empty["status"], "validation_error", "{empty}");
            assert_eq!(empty["requiresReload"], false, "{empty}");
            fixture.unchanged();
            let failed_source = source(&fixture.home, "acp-save-failure");
            let fault = SaveFault::new();
            let outcome = install(&conn, &sid, failed_source.to_str().unwrap()).await;
            drop(fault);
            fixture.unchanged();
            let artifact = fixture.artifact(&failed_source);
            assert!(artifact.join("plugin.json").is_file());
            failed(&outcome);
            let message = outcome["message"].as_str().unwrap();
            assert!(
                message.contains("test-injected registry save failure")
                    && message.contains(artifact.to_str().unwrap())
                    && message.contains("incomplete"),
                "{outcome}"
            );

            let bad_source = source(&fixture.home, "acp-bad-registry");
            std::fs::write(&fixture.registry_path, "{ broken").unwrap();
            failed(&install(&conn, &sid, bad_source.to_str().unwrap()).await);
            assert_eq!(
                std::fs::read_to_string(&fixture.registry_path).unwrap(),
                "{ broken"
            );
            assert!(!fixture.artifact(&bad_source).exists());
            std::fs::remove_file(&fixture.registry_path).unwrap();
            std::fs::create_dir(&fixture.registry_path).unwrap();
            failed(&install(&conn, &sid, bad_source.to_str().unwrap()).await);
            assert!(fixture.registry_path.is_dir());
            assert!(!fixture.artifact(&bad_source).exists());
            std::fs::remove_dir(&fixture.registry_path).unwrap();
            std::fs::write(&fixture.registry_path, &fixture.registry_bytes).unwrap();
            fixture.unchanged();

            let normal_source = source(&fixture.home, "acp-success");
            let normal = install(&conn, &sid, normal_source.to_str().unwrap()).await;
            assert_eq!(normal["status"], "success", "{normal}");
            assert_eq!(normal["requiresReload"], true, "{normal}");
            assert!(
                normal["message"]
                    .as_str()
                    .unwrap()
                    .contains("Installed 1 plugin(s)"),
                "{normal}"
            );
            let registry = InstallRegistry::try_load_from(fixture.install_dir.clone()).unwrap();
            let (_, repo, _) = registry.find_plugin("acp-success").unwrap();
            assert_eq!(repo.path, fixture.artifact(&normal_source));
            assert!(
                std::fs::read_to_string(&fixture.config_path)
                    .unwrap()
                    .contains("acp-success")
            );

            let warning_source = source(&fixture.home, "acp-warning");
            std::fs::write(&fixture.config_path, "invalid = [").unwrap();
            let warning = install(&conn, &sid, warning_source.to_str().unwrap()).await;
            assert_eq!(warning["status"], "success", "{warning}");
            assert_eq!(warning["requiresReload"], true, "{warning}");
            assert!(
                warning["message"]
                    .as_str()
                    .unwrap()
                    .contains("(warning: auto-enable acp-warning:"),
                "{warning}"
            );
            std::fs::write(&fixture.config_path, &fixture.config_bytes).unwrap();
            let close = ext(
                &conn,
                "grow/session/close",
                json!({"sessionId": sid.0.as_ref()}),
            )
            .await;
            assert_eq!(close["result"]["success"], true, "{close}");
        })
        .await
        .expect("ACP install test timeout");
    }));
    drop(local);
    drop(rt);
    drop(server);
    drop(mock_rt);
}

fn uninstall_error(
    result: Result<shell::plugin::UninstallOutcome, shell::plugin::UninstallError>,
) -> String {
    match result {
        Err(error) => error.to_string(),
        Ok(outcome) => panic!("unexpected uninstall success: {}", outcome.repo_key),
    }
}

fn plugin_data(fixture: &Fixture) -> PathBuf {
    let id = agent::plugins::discovery::PluginId::new(
        agent::plugins::discovery::PluginScope::User,
        &fixture.old_artifact,
        "existing",
    );
    let path = fixture.home.join(".grow/plugin-data").join(&id.0);
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("data.txt"), "preserved").unwrap();
    path
}

#[test]
fn shared_uninstall_reports_persistence_failures() {
    let Some(home) = isolated("shared_uninstall_reports_persistence_failures") else {
        return;
    };
    let fixture = Fixture::new(home);
    let data = plugin_data(&fixture);
    let uninstall =
        |confirm, keep_data| shell::plugin::uninstall_plugin("existing", confirm, keep_data);

    std::fs::write(&fixture.registry_path, "{ broken registry").unwrap();
    assert!(uninstall_error(uninstall(false, false)).contains("failed to load registry"));
    assert!(fixture.old_artifact.join("payload.txt").is_file());
    assert!(data.is_dir());
    assert_eq!(
        std::fs::read_to_string(&fixture.registry_path).unwrap(),
        "{ broken registry"
    );
    std::fs::remove_file(&fixture.registry_path).unwrap();
    std::fs::create_dir(&fixture.registry_path).unwrap();
    assert!(uninstall_error(uninstall(false, false)).contains("failed to load registry"));
    assert!(fixture.registry_path.is_dir());
    assert!(fixture.old_artifact.join("payload.txt").is_file());
    std::fs::remove_dir(&fixture.registry_path).unwrap();
    std::fs::write(&fixture.registry_path, &fixture.registry_bytes).unwrap();

    std::fs::remove_dir_all(&fixture.old_artifact).unwrap();
    std::fs::write(&fixture.old_artifact, "unexpected ordinary file").unwrap();
    let error = uninstall_error(uninstall(false, false));
    assert!(
        error.contains("still exists") && error.contains("retained"),
        "{error}"
    );
    assert_eq!(
        std::fs::read_to_string(&fixture.old_artifact).unwrap(),
        "unexpected ordinary file"
    );
    assert_eq!(
        std::fs::read(&fixture.registry_path).unwrap(),
        fixture.registry_bytes
    );
    assert!(data.is_dir());
    std::fs::remove_file(&fixture.old_artifact).unwrap();
    std::fs::create_dir(&fixture.old_artifact).unwrap();
    std::fs::write(
        fixture.old_artifact.join("payload.txt"),
        "original snapshot",
    )
    .unwrap();

    let mut registry = InstallRegistry::try_load_from(fixture.install_dir.clone()).unwrap();
    let key = registry.find_plugin("existing").unwrap().0.to_owned();
    let repo = registry.get_repo_mut(&key).unwrap();
    repo.plugins
        .insert("sibling".into(), repo.plugins["existing"].clone());
    registry.save().unwrap();
    let before = std::fs::read(&fixture.registry_path).unwrap();
    assert!(matches!(
        uninstall(false, true),
        Err(shell::plugin::UninstallError::NeedsConfirm { total: 2, .. })
    ));
    assert!(fixture.old_artifact.is_dir());
    assert_eq!(std::fs::read(&fixture.registry_path).unwrap(), before);

    let fault = SaveFault::new();
    let error = uninstall_error(uninstall(true, true));
    drop(fault);
    assert!(!fixture.old_artifact.exists());
    assert!(data.is_dir());
    assert_eq!(std::fs::read(&fixture.registry_path).unwrap(), before);
    assert!(
        error.contains("was removed")
            && error.contains("incomplete")
            && error.contains("test-injected registry save failure"),
        "{error}"
    );
    assert!(
        error.contains(&key) && error.contains(fixture.old_artifact.to_str().unwrap()),
        "{error}"
    );
    // Retrying an already removed path can finish deregistration without touching data.
    let outcome = uninstall(true, true).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(outcome.removed_plugins.len(), 2);
    assert!(
        InstallRegistry::try_load_from(fixture.install_dir.clone())
            .unwrap()
            .get_repo(&key)
            .is_none()
    );
    assert!(data.is_dir());
    // Existing keep_data=false policy still removes the exact plugin data directory.
    std::fs::write(&fixture.registry_path, &fixture.registry_bytes).unwrap();
    std::fs::create_dir(&fixture.old_artifact).unwrap();
    uninstall(false, false).unwrap_or_else(|error| panic!("{error}"));
    assert!(!data.exists());
    assert_eq!(
        std::fs::read(&fixture.config_path).unwrap(),
        fixture.config_bytes
    );
}

#[cfg(unix)]
#[test]
fn shared_uninstall_retains_record_on_actual_remove_error() {
    let Some(home) = isolated("shared_uninstall_retains_record_on_actual_remove_error") else {
        return;
    };
    let fixture = Fixture::new(home);
    let mut registry = InstallRegistry::try_load_from(fixture.install_dir.clone()).unwrap();
    let key = registry.find_plugin("existing").unwrap().0.to_owned();
    // unlinkat cannot remove a final dot component, independently of chmod/root privileges.
    registry.get_repo_mut(&key).unwrap().path = fixture.old_artifact.join(".");
    registry.save().unwrap();
    let before = std::fs::read(&fixture.registry_path).unwrap();
    let error = uninstall_error(shell::plugin::uninstall_plugin("existing", true, false));
    assert!(
        error.contains("failed to remove") && error.contains("partially removed"),
        "{error}"
    );
    assert_eq!(std::fs::read(&fixture.registry_path).unwrap(), before);
    assert!(fixture.old_artifact.is_dir());
}

async fn uninstall(
    conn: &acp_transport::ClientSideConnection,
    sid: &acp::SessionId,
    plugin: &str,
    confirmed: bool,
) -> Value {
    let response = ext(conn, "grow/plugins/action", json!({"sessionId": sid.0.as_ref(), "action": {"type": "uninstall", "plugin_id": plugin, "confirmed": confirmed}})).await;
    assert!(response.get("error").is_none(), "{response}");
    response["result"].clone()
}

#[test]
fn acp_uninstall_reports_persistence_failures() {
    let Some(home) = isolated("acp_uninstall_reports_persistence_failures") else {
        return;
    };
    diagnostics::tls::install_ring_provider_once();
    let fixture = Fixture::new(home);
    let data = plugin_data(&fixture);
    let mock_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let server = mock_rt.block_on(MockInferenceServer::start()).unwrap();
    unsafe {
        std::env::set_var("GROW_CLI_CHAT_PROXY_BASE_URL", server.url());
        std::env::set_var("GROW_INFERENCE_BASE_URL", server.url());
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    rt.block_on(local.run_until(async {
        tokio::time::timeout(Duration::from_secs(120), async {
            let conn = connect(&server.url()).await;
            let sid = conn
                .new_session(
                    acp::NewSessionRequest::new(fixture.home.clone())
                        .meta(json!({"modelId": "test-model"}).as_object().cloned()),
                )
                .await
                .unwrap()
                .session_id;
            assert_eq!(
                uninstall(&conn, &sid, "", false).await["status"],
                "validation_error"
            );
            assert_eq!(
                uninstall(&conn, &sid, "missing", false).await["status"],
                "not_found"
            );
            std::fs::write(&fixture.registry_path, "{ broken registry").unwrap();
            failed(&uninstall(&conn, &sid, "existing", false).await);
            assert!(fixture.old_artifact.is_dir());
            assert_eq!(
                std::fs::read_to_string(&fixture.registry_path).unwrap(),
                "{ broken registry"
            );
            std::fs::remove_file(&fixture.registry_path).unwrap();
            std::fs::create_dir(&fixture.registry_path).unwrap();
            failed(&uninstall(&conn, &sid, "existing", false).await);
            assert!(fixture.registry_path.is_dir());
            assert!(fixture.old_artifact.is_dir());
            std::fs::remove_dir(&fixture.registry_path).unwrap();
            std::fs::write(&fixture.registry_path, &fixture.registry_bytes).unwrap();
            std::fs::remove_dir_all(&fixture.old_artifact).unwrap();
            std::fs::write(&fixture.old_artifact, "unexpected file").unwrap();
            let remaining = uninstall(&conn, &sid, "existing", false).await;
            failed(&remaining);
            assert!(
                remaining["message"]
                    .as_str()
                    .unwrap()
                    .contains("still exists")
            );
            assert_eq!(
                std::fs::read(&fixture.registry_path).unwrap(),
                fixture.registry_bytes
            );
            std::fs::remove_file(&fixture.old_artifact).unwrap();
            std::fs::create_dir(&fixture.old_artifact).unwrap();
            let mut registry = InstallRegistry::try_load_from(fixture.install_dir.clone()).unwrap();
            let key = registry.find_plugin("existing").unwrap().0.to_owned();
            let repo = registry.get_repo_mut(&key).unwrap();
            repo.plugins
                .insert("sibling".into(), repo.plugins["existing"].clone());
            registry.save().unwrap();
            let before = std::fs::read(&fixture.registry_path).unwrap();
            let confirm = uninstall(&conn, &sid, "user/unused/existing", false).await;
            assert_eq!(confirm["status"], "confirmation_required", "{confirm}");
            assert_eq!(confirm["requiresReload"], false);
            assert!(fixture.old_artifact.is_dir());
            let fault = SaveFault::new();
            let result = uninstall(&conn, &sid, "user/unused/existing", true).await;
            drop(fault);
            failed(&result);
            let message = result["message"].as_str().unwrap();
            assert!(
                message.contains("was removed")
                    && message.contains("incomplete")
                    && message.contains("test-injected registry save failure"),
                "{result}"
            );
            assert!(!fixture.old_artifact.exists());
            assert_eq!(std::fs::read(&fixture.registry_path).unwrap(), before);
            assert!(data.is_dir(), "ACP always preserves plugin data");
            let retry = uninstall(&conn, &sid, "existing", true).await;
            assert_eq!(retry["status"], "success", "{retry}");
            assert_eq!(retry["requiresReload"], true, "{retry}");
            assert!(
                InstallRegistry::try_load_from(fixture.install_dir.clone())
                    .unwrap()
                    .get_repo(&key)
                    .is_none()
            );
            assert!(data.is_dir());
            assert_eq!(
                std::fs::read(&fixture.config_path).unwrap(),
                fixture.config_bytes
            );
            let close = ext(
                &conn,
                "grow/session/close",
                json!({"sessionId": sid.0.as_ref()}),
            )
            .await;
            assert_eq!(close["result"]["success"], true, "{close}");
        })
        .await
        .expect("ACP uninstall test timeout");
    }));
    drop(local);
    drop(rt);
    drop(server);
    drop(mock_rt);
}
