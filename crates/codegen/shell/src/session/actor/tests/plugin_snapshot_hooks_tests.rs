//! AC-02 (clear-hooks-on-empty-plugin-snapshots): adopting an empty plugin
//! snapshot must revoke the prior `plugin/` Hook contribution while
//! non-plugin (user-config) hooks keep executing.
//!
//! Tier-3 real-entry test: it drives the production
//! `apply_plugin_registry_snapshot` adoption on a real actor and observes
//! actual command-hook side effects (marker files), never a registry helper.

use super::support::*;
use super::*;

use ::hooks::config::{HandlerType, HookProvenance, HookSpec, PLUGIN_HOOK_PREFIX};
use ::hooks::event::{GateKind, HookEventName, HookPayload};

/// Marker-counting helper: every hook run appends one `x`. A missing file
/// means zero executions; any other read error is a failure, never silence.
fn marker_count(path: &std::path::Path) -> usize {
    match std::fs::read_to_string(path) {
        Ok(content) => content.chars().filter(|c| *c == 'x').count(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        Err(e) => panic!("marker {} unreadable: {e}", path.display()),
    }
}

/// Poll until the marker reaches `expected` writes (bounded), so spawned
/// hook processes are observed without a fixed sleep. On timeout the panic
/// carries the dispatch outcomes, separating "handler skipped/failed" from
/// "command ran but wrote nothing".
async fn await_marker_count(path: &std::path::Path, expected: usize, outcomes: &[String]) {
    for _ in 0..200 {
        if marker_count(path) >= expected {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!(
        "marker {} never reached {expected} writes (now {}); dispatch outcomes: {outcomes:?}",
        path.display(),
        marker_count(path)
    );
}

/// A user-config command hook writing to `marker` on every SessionStart.
fn control_spec(marker: &std::path::Path) -> HookSpec {
    let command = format!("sh -c 'printf x >> {}'", marker.display());
    HookSpec {
        name: "global/ac02-control".to_string(),
        event: HookEventName::SessionStart,
        handler_type: HandlerType::Command,
        configured_matcher: None,
        matcher: None,
        enabled: true,
        command: Some(std::path::PathBuf::from(&command)),
        command_raw: Some(command),
        url: None,
        url_raw: None,
        timeout_ms: 10_000,
        on_failure: Default::default(),
        source_dir: std::env::temp_dir(),
        extra_env: std::collections::HashMap::new(),
        layer: HookProvenance::User,
    }
}

/// Build a plugin registry whose single active plugin contributes an inline
/// SessionStart command hook writing to `plugin_marker`.
fn marker_plugin_registry(
    root: &std::path::Path,
    plugin_marker: &std::path::Path,
) -> std::sync::Arc<agent::plugins::PluginRegistry> {
    let manifest_value = serde_json::json!({
        "name": "ac02-marker",
        "hooks": {
            "hooks": {
                "session_start": [
                    {
                        "hooks": [
                            {
                                "type": "command",
                                "command": format!("sh -c 'printf x >> {}'", plugin_marker.display()),
                            }
                        ]
                    }
                ]
            }
        }
    });
    std::fs::write(
        root.join("plugin.json"),
        serde_json::to_string(&manifest_value).unwrap(),
    )
    .expect("write plugin manifest");
    let manifest: agent::plugins::PluginManifest =
        serde_json::from_value(manifest_value).expect("manifest must parse");
    let canonical_root = std::path::PathBuf::from(root);
    let discovered = agent::plugins::DiscoveredPlugin {
        id: agent::plugins::discovery::PluginId::new(
            agent::plugins::PluginScope::CliOverride,
            &canonical_root,
            &manifest.name,
        ),
        manifest,
        root: root.to_path_buf(),
        canonical_root: canonical_root.clone(),
        scope: agent::plugins::PluginScope::CliOverride,
        origin: agent::plugins::PluginOrigin::CliOverride,
        trusted: true,
        skill_dirs: Vec::new(),
        command_dirs: Vec::new(),
        agent_dirs: Vec::new(),
        hooks_path: None,
        mcp_config_path: None,
        lsp_config_path: None,
        conflict: None,
    };
    std::sync::Arc::new(agent::plugins::PluginRegistry::from_discovered(
        vec![discovered],
        &[],
        &["ac02-marker".to_string()],
    ))
}

/// Fire one SessionStart occurrence through the production dispatcher.
/// Fire one SessionStart occurrence through the production dispatcher and
/// return the aggregate so handler outcomes are observable evidence.
async fn dispatch_session_start(
    actor: &SessionActor,
) -> super::super::hook_dispatch::HookAggregate {
    let envelope = actor.make_hook_envelope(
        HookEventName::SessionStart,
        None,
        HookPayload::SessionStart {
            source: "test".to_string(),
            model_id: None,
            agent_type: None,
        },
    );
    actor
        .dispatch_hook_occurrence(
            HookEventName::SessionStart,
            chat_state::HookCause::Session {
                session_id: actor.session_id_string(),
            },
            envelope,
            GateKind::Observe,
            super::super::hook_dispatch::HookDispatchPolicy::Execute,
        )
        .await
        .expect("the hook lifecycle must remain durable")
}
/// One line per handler outcome, so a failed positive control pinpoints the
/// exact skip reason / runner error instead of only "marker never reached 1".
fn describe_outcomes(aggregate: &super::super::hook_dispatch::HookAggregate) -> Vec<String> {
    use ::hooks::result::HookRunResult;
    let results = match aggregate {
        super::super::hook_dispatch::HookAggregate::Observe { results } => results,
        _ => panic!("SessionStart dispatch must aggregate as Observe"),
    };
    results
        .iter()
        .map(|r| match r {
            HookRunResult::Success { hook_name, .. } => format!("{hook_name}: success"),
            HookRunResult::Skipped { hook_name, reason } => {
                format!("{hook_name}: skipped({reason:?})")
            }
            HookRunResult::Failed {
                hook_name, error, ..
            } => {
                format!("{hook_name}: failed({error})")
            }
            HookRunResult::TimedOut {
                hook_name,
                timeout_ms,
                ..
            } => {
                format!("{hook_name}: timed_out({timeout_ms}ms)")
            }
            HookRunResult::Cancelled { hook_name, .. } => format!("{hook_name}: cancelled"),
            HookRunResult::Blocked {
                hook_name, detail, ..
            } => {
                format!("{hook_name}: blocked({detail})")
            }
        })
        .collect()
}

/// Assert every expected handler executed successfully and return the
/// descriptions for the failure message of marker-based assertions.
fn require_successful_dispatch(
    aggregate: &super::super::hook_dispatch::HookAggregate,
    expected_handlers: &[&str],
) -> Vec<String> {
    let described = describe_outcomes(aggregate);
    assert!(
        !described.is_empty(),
        "SessionStart dispatch planned no handlers at all"
    );
    for expected in expected_handlers {
        assert!(
            described.iter().any(|line| line.starts_with(expected)),
            "handler {expected} missing from dispatch outcomes: {described:?}"
        );
    }
    for line in &described {
        assert!(
            line.ends_with(": success"),
            "handler did not execute successfully: {line} (all outcomes: {described:?})"
        );
    }
    described
}

/// Answer gateway notifications so adoption's client updates never block.
fn spawn_notification_ack(
    gateway_rx: tokio::sync::mpsc::UnboundedReceiver<acp_transport::AcpClientMessage>,
) {
    let mut gateway_rx = gateway_rx;
    tokio::task::spawn_local(async move {
        while let Some(msg) = gateway_rx.recv().await {
            match msg {
                acp_transport::AcpClientMessage::SessionNotification(args) => {
                    let _ = args.response_tx.send(Ok(()));
                }
                acp_transport::AcpClientMessage::ExtMethod(args) => {
                    let response: Arc<serde_json::value::RawValue> =
                        serde_json::value::to_raw_value(&serde_json::json!({}))
                            .unwrap()
                            .into();
                    let _ = args.response_tx.send(Ok(acp::ExtResponse::new(response)));
                }
                _ => {}
            }
        }
    });
}

fn registry_hook_names(actor: &SessionActor) -> Vec<String> {
    let reg = actor.hooks.registry.borrow();
    match &*reg {
        Some(registry) => registry
            .all_hooks()
            .iter()
            .map(|spec| spec.name.clone())
            .collect(),
        None => Vec::new(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn empty_plugin_snapshot_revokes_plugin_hooks_but_keeps_config_hooks() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let work = tempfile::tempdir().expect("create isolated test workspace");
            std::fs::create_dir_all(work.path().join("plugin")).unwrap();
            let control_marker = work.path().join("control.marker");
            let plugin_marker = work.path().join("plugin.marker");

            let (gateway_tx, gateway_rx) =
                tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
            let (persistence_tx, _persistence_rx) =
                tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
            let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
            spawn_notification_ack(gateway_rx);
            let mut actor = actor;
            // Point the hook runner's spawn cwd at a real directory: the
            // support fixture leaves `resolved_workspace_root` empty and
            // `Command::current_dir("")` fails every spawn with ENOENT.
            actor.hooks.resolved_workspace_root = work.path().to_string_lossy().to_string();
            let actor = std::sync::Arc::new(actor);

            // Seed a non-plugin (user-config) registry so the control hook's
            // retention is observable and adoption never bootstraps from the
            // developer's real config layers.
            {
                let mut registry = ::hooks::discovery::HookRegistry::default();
                registry.append_specs(vec![control_spec(&control_marker)]);
                *actor.hooks.registry.borrow_mut() = Some(std::sync::Arc::new(registry));
            }

            // Positive control: adopting Some(plugin) installs the plugin hook
            // and both handlers execute for one SessionStart occurrence.
            let plugin_registry =
                marker_plugin_registry(&work.path().join("plugin"), &plugin_marker);
            let (hooks_reloaded, _, _) = actor
                .clone()
                .apply_plugin_registry_snapshot(Some(plugin_registry.clone()))
                .await;
            assert_eq!(hooks_reloaded, 1, "the marker plugin hook must load");
            let names = registry_hook_names(&actor);
            assert!(
                names
                    .iter()
                    .any(|name| name.starts_with(PLUGIN_HOOK_PREFIX)),
                "plugin hook must be listed after Some adoption: {names:?}"
            );
            assert!(
                names.contains(&"global/ac02-control".to_string()),
                "config hook must remain: {names:?}"
            );

            let aggregate = dispatch_session_start(&actor).await;
            let outcomes = require_successful_dispatch(
                &aggregate,
                &[PLUGIN_HOOK_PREFIX, "global/ac02-control"],
            );
            await_marker_count(&plugin_marker, 1, &outcomes).await;
            await_marker_count(&control_marker, 1, &outcomes).await;

            // None snapshot: the plugin contribution is revoked (generation
            // bumps, listing drops it, the handler no longer executes) while
            // the config hook still runs.
            let generation_before = actor.hook_config_generation();
            let (_, _, _) = actor.clone().apply_plugin_registry_snapshot(None).await;
            assert!(
                actor.hook_config_generation() > generation_before,
                "removing the last plugin contribution must publish a generation change"
            );
            assert!(actor.plugin_registry.read().is_none());
            let names = registry_hook_names(&actor);
            assert!(
                !names
                    .iter()
                    .any(|name| name.starts_with(PLUGIN_HOOK_PREFIX)),
                "plugin hook must be gone after None adoption: {names:?}"
            );
            assert!(
                names.contains(&"global/ac02-control".to_string()),
                "config hook must survive None adoption: {names:?}"
            );

            let aggregate = dispatch_session_start(&actor).await;
            let outcomes = require_successful_dispatch(&aggregate, &["global/ac02-control"]);
            assert!(
                !outcomes
                    .iter()
                    .any(|line| line.starts_with(PLUGIN_HOOK_PREFIX)),
                "revoked plugin hook must not even be planned after None adoption: {outcomes:?}"
            );
            await_marker_count(&control_marker, 2, &outcomes).await;
            assert_eq!(
                marker_count(&plugin_marker),
                1,
                "revoked plugin hook must not execute after None adoption"
            );

            // Some(empty registry) is the paired existing path: same removal.
            let (_, _, _) = actor
                .clone()
                .apply_plugin_registry_snapshot(Some(std::sync::Arc::new(
                    agent::plugins::PluginRegistry::empty(),
                )))
                .await;
            assert!(actor.plugin_registry.read().is_some());
            assert!(
                actor
                    .plugin_registry
                    .read()
                    .as_ref()
                    .unwrap()
                    .list()
                    .is_empty()
            );
            let aggregate = dispatch_session_start(&actor).await;
            let outcomes = require_successful_dispatch(&aggregate, &["global/ac02-control"]);
            await_marker_count(&control_marker, 3, &outcomes).await;
            assert_eq!(
                marker_count(&plugin_marker),
                1,
                "empty Some registry must revoke plugin hooks identically"
            );

            // Re-adoption restores the plugin handler, proving removal was the
            // snapshot's contribution replacement and not permanent damage.
            let (_, _, _) = actor
                .clone()
                .apply_plugin_registry_snapshot(Some(plugin_registry))
                .await;
            let aggregate = dispatch_session_start(&actor).await;
            let outcomes = require_successful_dispatch(
                &aggregate,
                &[PLUGIN_HOOK_PREFIX, "global/ac02-control"],
            );
            await_marker_count(&plugin_marker, 2, &outcomes).await;
            await_marker_count(&control_marker, 4, &outcomes).await;

            // `work` (TempDir) is intentionally still in scope: dropping it
            // cleans the fixture even when an assertion panics above.
        })
        .await;
}
