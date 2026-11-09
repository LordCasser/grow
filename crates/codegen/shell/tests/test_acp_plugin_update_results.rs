//! AC-07/AC-08: exercise Update through the real ACP extension, session handle and actor.
//! One test owns HOME/GROW_HOME for every phase because grow_home() is cached.

use std::path::{Path, PathBuf};
use std::time::Duration;

use acp_transport::{
    AcpAgentGatewayReceiver as GatewayReceiver, AcpAgentGatewaySender as GatewaySender,
    AcpAgentHandler as _, AcpClientHandler, LineBufferedRead, protocol as acp,
};
use agent::plugins::git_install::{self, InstallSource};
use agent::plugins::install_registry::{InstallKind, InstallRegistry, InstalledRepo};
use serde_json::{Value, json};
use shell::agent::config::Config as AgentConfig;
use shell::agent::mvp_agent::MvpAgent;
use tempfile::TempDir;
use test_support::MockInferenceServer;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

const RPC_TIMEOUT: Duration = Duration::from_secs(60);
const OLD_TIMESTAMP: &str = "2000-01-01T00:00:00+00:00";

struct EnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(key);
        unsafe { std::env::set_var(key, value) };
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

struct CwdGuard(PathBuf);
impl Drop for CwdGuard {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).expect("restore cwd");
    }
}

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run fixture git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git stdout")
        .trim()
        .to_owned()
}

fn write_plugin(root: &Path, subdir: &str, name: &str, version: &str) {
    let dir = root.join(subdir);
    std::fs::create_dir_all(&dir).expect("plugin dir");
    std::fs::write(
        dir.join("plugin.json"),
        json!({"name": name, "version": version}).to_string(),
    )
    .expect("plugin manifest");
}

struct BranchFixture {
    work: PathBuf,
    remote: PathBuf,
    name: String,
    old_commit: String,
}

impl BranchFixture {
    fn new(home: &Path, name: &str) -> Self {
        let work = home.join(format!("{name}-work"));
        let remote = home.join(format!("{name}.git"));
        std::fs::create_dir_all(&work).expect("worktree dir");
        git(
            home,
            &[
                "init",
                "--bare",
                "--initial-branch=main",
                remote.to_str().unwrap(),
            ],
        );
        git(&work, &["init", "--initial-branch=main"]);
        git(&work, &["config", "user.name", "ACP fixture"]);
        git(
            &work,
            &["config", "user.email", "acp-fixture@example.invalid"],
        );
        write_plugin(&work, "original-dir", name, "1.0.0");
        git(&work, &["add", "."]);
        git(&work, &["commit", "-m", "old plugin"]);
        git(
            &work,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&work, &["push", "-u", "origin", "main"]);
        let old_commit = git(&work, &["rev-parse", "HEAD"]);
        Self {
            work,
            remote,
            name: name.to_owned(),
            old_commit,
        }
    }

    fn source(&self, git_ref: &str) -> InstallSource {
        InstallSource::Git {
            url: self.remote.to_string_lossy().into_owned(),
            git_ref: Some(git_ref.to_owned()),
            git_sha: None,
            subdir: None,
        }
    }

    fn advance(&self) -> String {
        write_plugin(&self.work, "original-dir", &self.name, "2.0.0");
        write_plugin(
            &self.work,
            "added-dir",
            &format!("{}-added", self.name),
            "1.0.0",
        );
        git(&self.work, &["add", "."]);
        git(
            &self.work,
            &["commit", "-m", "advance plugin and discover another"],
        );
        git(&self.work, &["push", "origin", "main"]);
        git(&self.work, &["rev-parse", "HEAD"])
    }
}

fn install(registry: &mut InstallRegistry, source: &InstallSource) -> String {
    let result =
        git_install::install_from_source(source, registry, false).expect("fixture install");
    let mut repo = git_install::build_installed_repo(&result, source);
    repo.installed_at = OLD_TIMESTAMP.to_owned();
    repo.updated_at = OLD_TIMESTAMP.to_owned();
    registry.insert(result.repo_key.clone(), repo);
    result.repo_key
}

fn commit(repo: &InstalledRepo) -> &str {
    match &repo.kind {
        InstallKind::Git { commit, .. } => commit,
        _ => panic!("expected git registration"),
    }
}

struct QuietClient;
#[async_trait::async_trait(?Send)]
impl AcpClientHandler for QuietClient {
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

async fn connect(base_url: &str) -> acp_transport::ClientSideConnection {
    let raw: toml::Value = toml::from_str(&format!(
        "[features]\nlsp_tools = false\n\n\
         [models]\ndefault = \"test/test-model\"\n\n\
         [provider.test]\napi_backend = \"chat_completions\"\n\n\
         [provider.test.options]\nbase_url = \"{base_url}\"\n\n\
         [provider.test.models.test-model]\nname = \"Test Model\"\ncontext_window = 128000\n"
    ))
    .expect("model TOML");
    let mut config = AgentConfig::new_from_toml_cfg(&raw).expect("agent config");
    config.models.default = Some("test/test-model".to_owned());
    config.default_model_override = None;
    config.remote_settings = Some(shell::util::config::RemoteSettings::default());
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = MvpAgent::new(GatewaySender::new(tx), &config).expect("agent");
    let (client_write, agent_read) = tokio::io::duplex(8 * 1024 * 1024);
    let (agent_write, client_read) = tokio::io::duplex(8 * 1024 * 1024);
    let (agent_conn, agent_io) = acp_transport::connect_agent_v1(
        agent,
        agent_write.compat_write(),
        LineBufferedRead::spawn_local(agent_read.compat()),
        |future| {
            tokio::task::spawn_local(future);
        },
    );
    tokio::task::spawn_local(GatewayReceiver::new(rx, agent_conn).run());
    tokio::task::spawn_local(agent_io);
    let (conn, client_io) = acp_transport::connect_client_v1(
        QuietClient,
        client_write.compat_write(),
        LineBufferedRead::spawn_local(client_read.compat()),
        |future| {
            tokio::task::spawn_local(future);
        },
    );
    tokio::task::spawn_local(client_io);
    let init = tokio::time::timeout(RPC_TIMEOUT, conn.initialize(
        acp::InitializeRequest::new(acp::ProtocolVersion::V1).meta(json!({
            "startupHints": {"nonInteractive": true, "skipGitStatus": true, "skipProjectLayout": true},
            "clientType": "acp-plugin-update-test", "clientVersion": "0.0-test"
        }).as_object().cloned())
    )).await.expect("initialize timeout").expect("initialize");
    let method = init
        .auth_methods
        .iter()
        .find(|m| &*m.id().0 == "provider.api_key")
        .expect("API key auth method");
    tokio::time::timeout(
        RPC_TIMEOUT,
        conn.authenticate(
            acp::AuthenticateRequest::new(method.id().clone())
                .meta(json!({"headless": true}).as_object().cloned()),
        ),
    )
    .await
    .expect("authenticate timeout")
    .expect("authenticate");
    conn
}

async fn ext(conn: &acp_transport::ClientSideConnection, method: &str, params: Value) -> Value {
    let raw =
        serde_json::value::RawValue::from_string(params.to_string()).expect("extension params");
    let response = tokio::time::timeout(
        RPC_TIMEOUT,
        conn.ext_method(acp::ExtRequest::new(method, std::sync::Arc::from(raw))),
    )
    .await
    .expect("extension timeout")
    .expect("extension response");
    serde_json::from_str(response.0.get()).expect("extension JSON")
}

async fn update(
    conn: &acp_transport::ClientSideConnection,
    sid: &acp::SessionId,
    plugin: Option<&str>,
) -> Value {
    let response = ext(
        conn,
        "grow/plugins/action",
        json!({
            "sessionId": sid.0.as_ref(), "action": {"type": "update", "plugin_id": plugin}
        }),
    )
    .await;
    assert!(
        response.get("error").is_none(),
        "ACP update error: {response}"
    );
    response["result"].clone()
}

#[test]
fn acp_update_adopts_branch_result_and_preserves_noops() {
    diagnostics::tls::install_ring_provider_once();
    let mock_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let server = mock_rt
        .block_on(MockInferenceServer::start())
        .expect("mock server");
    let temp = TempDir::new().expect("isolated home");
    let home = dunce::canonicalize(temp.path()).expect("canonical home");
    let grow_home = home.join(".grow");
    std::fs::create_dir_all(&grow_home).unwrap();
    let _env = [
        EnvGuard::set("HOME", &home),
        EnvGuard::set("GROW_HOME", &grow_home),
        EnvGuard::set("GROW_CLI_CHAT_PROXY_BASE_URL", server.url()),
        EnvGuard::set("GROW_INFERENCE_BASE_URL", server.url()),
        EnvGuard::set("GROW_LSP_TOOLS", "0"),
        EnvGuard::set("GROW_MARKETPLACE_REQUIRE_SHA", "0"),
        EnvGuard::set("GIT_CONFIG_NOSYSTEM", "1"),
        EnvGuard::set("GIT_CONFIG_GLOBAL", "/dev/null"),
    ];
    let _cwd = CwdGuard(std::env::current_dir().unwrap());
    std::env::set_current_dir(&home).unwrap();
    let install_dir = grow_home.join("installed-plugins");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    rt.block_on(local.run_until(async {
        let conn = connect(&server.url()).await;
        let sid = tokio::time::timeout(
            RPC_TIMEOUT,
            conn.new_session(
                acp::NewSessionRequest::new(home.clone())
                    .meta(json!({"modelId": "test-model"}).as_object().cloned()),
            ),
        )
        .await
        .expect("session timeout")
        .expect("session")
        .session_id;
        let empty = update(&conn, &sid, None).await;
        assert_eq!(empty["status"], "not_found", "{empty}");
        assert_eq!(empty["requiresReload"], false, "{empty}");

        let selected = BranchFixture::new(&home, "selected");
        let other = BranchFixture::new(&home, "other");
        let mut registry = InstallRegistry::empty(install_dir.clone());
        let selected_key = install(&mut registry, &selected.source("main"));
        let other_key = install(&mut registry, &other.source("main"));
        registry.save().expect("save old registrations");
        let new_commit = selected.advance();
        let other_tip = other.advance();
        let missing = update(&conn, &sid, Some("user/unused/missing")).await;
        assert_eq!(missing["status"], "not_found", "{missing}");
        assert_eq!(missing["requiresReload"], false, "{missing}");

        let registry_path = install_dir.join("registry.json");
        let before = std::fs::read(&registry_path).unwrap();
        let checkout = registry.get_repo(&selected_key).unwrap().path.clone();
        std::fs::write(&registry_path, "{ broken registry").unwrap();
        let corrupt = update(&conn, &sid, Some("user/unused/selected")).await;
        assert_eq!(corrupt["status"], "internal_error", "{corrupt}");
        assert_eq!(corrupt["requiresReload"], false, "{corrupt}");
        assert_eq!(git(&checkout, &["rev-parse", "HEAD"]), selected.old_commit);
        assert_eq!(
            std::fs::read_to_string(&registry_path).unwrap(),
            "{ broken registry"
        );
        std::fs::remove_file(&registry_path).unwrap();
        std::fs::create_dir(&registry_path).unwrap();
        let unreadable = update(&conn, &sid, Some("selected")).await;
        assert_eq!(unreadable["status"], "internal_error", "{unreadable}");
        assert_eq!(unreadable["requiresReload"], false, "{unreadable}");
        assert!(registry_path.is_dir());
        assert_eq!(git(&checkout, &["rev-parse", "HEAD"]), selected.old_commit);
        std::fs::remove_dir(&registry_path).unwrap();
        std::fs::write(&registry_path, &before).unwrap();
        let fault = EnvGuard::set("GROW_TEST_FAIL_REGISTRY_SAVE_AFTER_SERIALIZE", "1");
        let incomplete = update(&conn, &sid, Some("user/unused/selected")).await;
        drop(fault);
        assert_eq!(incomplete["status"], "internal_error", "{incomplete}");
        assert_eq!(incomplete["requiresReload"], false, "{incomplete}");
        assert_eq!(incomplete["requiresRestart"], false, "{incomplete}");
        let message = incomplete["message"].as_str().unwrap();
        assert!(
            message.contains("incomplete")
                && message.contains("test-injected registry save failure"),
            "{incomplete}"
        );
        assert!(
            message.contains(&selected_key) && message.contains(checkout.to_str().unwrap()),
            "{incomplete}"
        );
        assert_eq!(git(&checkout, &["rev-parse", "HEAD"]), new_commit);
        assert_eq!(std::fs::read(&registry_path).unwrap(), before);
        // The checkout already advanced. Explicit retry must adopt its actual tip and map.
        let changed = update(&conn, &sid, Some("user/unused/selected")).await;
        assert_eq!(changed["status"], "success", "{changed}");
        assert_eq!(changed["requiresReload"], true, "{changed}");
        assert_eq!(changed["message"], format!("{selected_key}: updated"));
        let adopted =
            InstallRegistry::try_load_from(install_dir.clone()).expect("durable registry");
        let entry = adopted
            .get_repo(&selected_key)
            .expect("selected registration");
        let head = git(&entry.path, &["rev-parse", "HEAD"]);
        assert_eq!(head, new_commit, "ACP must actually advance the checkout");
        eprintln!(
            "AC-07 adoption evidence: checkout={head}, recorded={}, updated_at={}, plugins={:?}",
            commit(entry),
            entry.updated_at,
            entry.plugins
        );
        assert_eq!(
            commit(entry),
            head,
            "ACP updated checkout but did not adopt its commit into durable registry"
        );
        assert_ne!(entry.updated_at, OLD_TIMESTAMP);
        assert_eq!(entry.installed_at, OLD_TIMESTAMP);
        assert_eq!(entry.plugins.len(), 2);
        assert_eq!(
            entry.plugins["selected"].subdir.as_deref(),
            Some("original-dir")
        );
        assert_eq!(entry.plugins["selected"].version.as_deref(), Some("2.0.0"));
        assert_eq!(
            entry.plugins["selected-added"].subdir.as_deref(),
            Some("added-dir")
        );
        assert_eq!(
            entry.plugins["selected-added"].version.as_deref(),
            Some("1.0.0")
        );
        let untouched = adopted.get_repo(&other_key).unwrap();
        assert_eq!(commit(untouched), other.old_commit);
        assert_eq!(
            git(&untouched.path, &["rev-parse", "HEAD"]),
            other.old_commit
        );
        assert_eq!(untouched.updated_at, OLD_TIMESTAMP);

        let unchanged_bytes = std::fs::read(&registry_path).unwrap();
        let fault = EnvGuard::set("GROW_TEST_FAIL_REGISTRY_SAVE_AFTER_SERIALIZE", "1");
        let second = update(&conn, &sid, Some("user/unused/selected")).await;
        drop(fault);
        assert_eq!(second["status"], "success", "{second}");
        assert_eq!(std::fs::read(&registry_path).unwrap(), unchanged_bytes);
        assert_eq!(second["requiresReload"], false, "{second}");
        assert_eq!(
            second["message"],
            format!("{selected_key}: already up to date")
        );
        let unchanged = InstallRegistry::try_load_from(install_dir.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(unchanged.get_repo(&selected_key)).unwrap(),
            serde_json::to_value(Some(entry)).unwrap()
        );

        // None updates the other repo too; only its changed outcome asks for reload.
        let all = update(&conn, &sid, None).await;
        assert_eq!(all["requiresReload"], true, "{all}");
        let all_message = all["message"].as_str().unwrap();
        assert!(
            all_message.contains(&format!("{selected_key}: already up to date")),
            "{all}"
        );
        assert!(
            all_message.contains(&format!("{other_key}: updated")),
            "{all}"
        );
        let mut registry = InstallRegistry::try_load_from(install_dir.clone()).unwrap();
        assert_eq!(commit(registry.get_repo(&other_key).unwrap()), other_tip);

        // Actual SHA install and local snapshot exercise the actor's nonmutable outcomes.
        let pinned_source = BranchFixture::new(&home, "pinned-plugin");
        let pinned_key = install(
            &mut registry,
            &pinned_source.source(&pinned_source.old_commit),
        );
        let local_path = home.join("local-source");
        write_plugin(&local_path, "", "local-plugin", "1.0.0");
        let local_key = install(
            &mut registry,
            &InstallSource::Local {
                path: local_path,
                subdir: None,
            },
        );
        registry.save().unwrap();
        let before_noops = serde_json::to_value(&registry).unwrap();
        let fault = EnvGuard::set("GROW_TEST_FAIL_REGISTRY_SAVE_AFTER_SERIALIZE", "1");
        let pinned = update(&conn, &sid, Some("user/unused/pinned-plugin")).await;
        assert_eq!(pinned["status"], "success", "{pinned}");
        assert_eq!(pinned["requiresReload"], false, "{pinned}");
        assert_eq!(
            pinned["message"],
            format!("{pinned_key}: pinned to {}", pinned_source.old_commit)
        );
        let local_outcome = update(&conn, &sid, Some("user/unused/local-plugin")).await;
        drop(fault);
        assert_eq!(local_outcome["status"], "success", "{local_outcome}");
        assert_eq!(local_outcome["requiresReload"], false, "{local_outcome}");
        assert!(
            local_outcome["message"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{local_key}: local")),
            "{local_outcome}"
        );
        assert_eq!(
            serde_json::to_value(InstallRegistry::try_load_from(install_dir.clone()).unwrap())
                .unwrap(),
            before_noops
        );

        // Update does not implicitly run Reload or replace the session snapshot.
        let list = ext(
            &conn,
            "grow/plugins/list",
            json!({"sessionId": sid.0.as_ref()}),
        )
        .await;
        assert_eq!(list["result"]["plugins"], json!([]), "{list}");
        let close = ext(
            &conn,
            "grow/session/close",
            json!({"sessionId": sid.0.as_ref()}),
        )
        .await;
        assert_eq!(close["result"]["success"], true, "{close}");
    }));
    // LocalSet and runtime own all ACP tasks; no external LSP/MCP command is configured.
    drop(local);
    drop(rt);
    drop(server);
    drop(mock_rt);
}

#[cfg(unix)]
#[test]
fn mixed_update_keeps_marketplace_publication_separate_from_pending_git_results() {
    use std::os::unix::fs::PermissionsExt;

    const TEST: &str =
        "mixed_update_keeps_marketplace_publication_separate_from_pending_git_results";
    const CHILD: &str = "GROW_TEST_MIXED_UPDATE_CHILD";
    let Some(home) = std::env::var_os(CHILD).map(PathBuf::from) else {
        let temp = TempDir::new().unwrap();
        let home = dunce::canonicalize(temp.path()).unwrap();
        std::fs::create_dir_all(home.join(".grow")).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([TEST, "--exact", "--nocapture", "--test-threads=1"])
            .env(CHILD, &home)
            .env("HOME", &home)
            .env("GROW_HOME", home.join(".grow"))
            .env("GROW_MARKETPLACE_REQUIRE_SHA", "0")
            .env_remove("GROW_TEST_FAIL_REGISTRY_SAVE_AFTER_SERIALIZE")
            .current_dir(&home)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        return;
    };
    let install_dir = home.join(".grow/installed-plugins");
    let mut registry = InstallRegistry::empty(install_dir.clone());
    let first = BranchFixture::new(&home, "aaa-ordinary");
    let last = BranchFixture::new(&home, "zzz-ordinary");
    let first_key = install(&mut registry, &first.source("main"));
    let last_key = install(&mut registry, &last.source("main"));
    let marketplace = home.join("marketplace");
    write_plugin(&marketplace, "plugins/mmm-market", "mmm-market", "1.0.0");
    std::fs::create_dir_all(marketplace.join(".grow-plugin")).unwrap();
    std::fs::write(marketplace.join(".grow-plugin/marketplace.json"), json!({"version": 1, "name": "test", "plugins": [{"name": "mmm-market", "source": {"type": "local", "path": "plugins/mmm-market"}}]}).to_string()).unwrap();
    let market_key = match plugin_marketplace::installer::install_from_marketplace(
        &marketplace,
        "plugins/mmm-market",
        agent::plugins::install_registry::MarketplaceProvenance {
            source_url_or_path: marketplace.to_string_lossy().into_owned(),
            source_display_name: "test".into(),
            plugin_subdir: "plugins/mmm-market".into(),
        },
        &mut registry,
    )
    .unwrap()
    {
        plugin_marketplace::installer::MarketplaceInstallResult::Installed { repo_key } => repo_key,
        _ => panic!("expected new marketplace installation"),
    };
    assert!(
        first_key < market_key && market_key < last_key,
        "fixture must exercise ordinary before marketplace publication"
    );
    let new_commit = first.advance();
    write_plugin(&marketplace, "plugins/mmm-market", "mmm-market", "2.0.0");

    // The last ordinary repo's real pull is after the marketplace transaction.
    // Preserve its published registry, then obstruct only the final rename. This
    // uses the existing hermetic Git seam and does not add a production fault hook.
    let wrapper = home.join("git-fault-wrapper");
    std::fs::write(&wrapper, r#"#!/bin/sh
if [ "$PWD" = "$GROW_TEST_LATE_REPO" ] && [ "$2" = "pull" ]; then
    mv "$GROW_TEST_INSTALL_DIR/registry.json" "$GROW_TEST_INSTALL_DIR/published-registry.json" || exit 90
    mkdir "$GROW_TEST_INSTALL_DIR/registry.json" || exit 91
fi
exec git "$@"
"#).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let fault = [
        EnvGuard::set("GIT_BIN_PATH", &wrapper),
        EnvGuard::set(
            "GROW_TEST_LATE_REPO",
            &registry.get_repo(&last_key).unwrap().path,
        ),
        EnvGuard::set("GROW_TEST_INSTALL_DIR", &install_dir),
    ];
    let error = match shell::plugin::update_plugins(None) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("final ordinary publication must fail"),
    };
    drop(fault);
    assert!(
        error.contains("incomplete") && error.contains(&first_key),
        "{error}"
    );
    assert!(install_dir.join("registry.json").is_dir());
    std::fs::remove_dir(install_dir.join("registry.json")).unwrap();
    std::fs::rename(
        install_dir.join("published-registry.json"),
        install_dir.join("registry.json"),
    )
    .unwrap();
    let published = InstallRegistry::try_load_from(install_dir.clone()).unwrap();
    let ordinary = published.get_repo(&first_key).unwrap();
    assert_eq!(git(&ordinary.path, &["rev-parse", "HEAD"]), new_commit);
    assert_eq!(
        commit(ordinary),
        first.old_commit,
        "marketplace must not publish pending ordinary adoption"
    );
    assert_eq!(ordinary.plugins.len(), 1);
    let market = published.get_repo(&market_key).unwrap();
    assert_eq!(
        market.plugins["mmm-market"].version.as_deref(),
        Some("2.0.0")
    );
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(market.path.join("plugin.json")).unwrap()).unwrap();
    assert_eq!(
        manifest["version"], "2.0.0",
        "completed marketplace update must stay committed"
    );

    // Explicit retry adopts the already advanced ordinary checkout while retaining
    // the marketplace record and its independently committed contents.
    let outcomes = shell::plugin::update_plugins(None).unwrap_or_else(|error| panic!("{error}"));
    assert!(outcomes.iter().any(|outcome| matches!(outcome, shell::plugin::RepoUpdateOutcome::Updated { repo_key, .. } if repo_key == &first_key)));
    let complete = InstallRegistry::try_load_from(install_dir).unwrap();
    let ordinary = complete.get_repo(&first_key).unwrap();
    assert_eq!(commit(ordinary), new_commit);
    assert_eq!(ordinary.plugins.len(), 2);
    assert_eq!(
        complete.get_repo(&market_key).unwrap().plugins["mmm-market"]
            .version
            .as_deref(),
        Some("2.0.0")
    );
}
