use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use super::support::create_test_actor;
use super::*;
use crate::session::persistence::PersistenceMsg;
use crate::session::workflow::manager::WorkflowManager;
use crate::session::workflow::notify::WorkflowNotifySender;
use crate::session::workflow::store::WorkflowRunStore;

#[tokio::test(flavor = "current_thread")]
async fn saved_workflow_dynamic_command_with_agent_preflight_creates_run() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let project = tempfile::tempdir().expect("create project");
            git2::Repository::init(project.path()).expect("initialize project repository");
            crate::agent::folder_trust::record_for_test(project.path(), true);

            let unique = uuid::Uuid::now_v7().simple().to_string();
            let name = format!("saved-agent-{}", &unique[..12]);
            let workflows = project.path().join(".grow/workflows");
            std::fs::create_dir_all(&workflows).expect("create Workflow Definition directory");
            std::fs::write(
                workflows.join(format!("{name}.rhai")),
                format!(
                    "let meta = #{{ name: \"{name}\", description: \"saved agent workflow\" }};\n\
             let result = agent(\"perform the saved task\");\n\
             complete(result.output);"
                ),
            )
            .expect("write saved Workflow Definition");

            let (gateway_tx, _gateway_rx) = tokio::sync::mpsc::unbounded_channel();
            let (actor_persistence_tx, _actor_persistence_rx) =
                tokio::sync::mpsc::unbounded_channel();
            let mut actor =
                create_test_actor(0, 256_000, 85, gateway_tx, actor_persistence_tx).await;

            let session_root = tempfile::tempdir().expect("create session root");
            let session_dir = session_root.path().join("session");
            std::fs::create_dir(&session_dir).expect("create session directory");
            let session_directory = Arc::new(
                crate::session::storage::ContainedDirectory::open(
                    session_root.path(),
                    Path::new("session"),
                    "Workflow launch test session",
                    false,
                )
                .expect("pin session directory"),
            );
            actor.session_info.cwd = project.path().display().to_string();
            actor.session_dir = session_dir;
            actor.session_directory = session_directory.clone();

            let tracker = Arc::new(parking_lot::Mutex::new(
                crate::session::workflow::tracker::WorkflowTracker::default(),
            ));
            let (workflow_persistence_tx, mut workflow_persistence_rx) =
                tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(async move {
                while let Some(message) = workflow_persistence_rx.recv().await {
                    if let PersistenceMsg::WorkflowRunStateAndAck { respond_to, .. } = message {
                        let _ = respond_to.send(Ok(()));
                    }
                }
            });
            let store = WorkflowRunStore::new(
                Some(session_directory.clone()),
                workflow_persistence_tx.clone(),
            );
            let (workflow_gateway_tx, _workflow_gateway_rx) =
                tokio::sync::mpsc::unbounded_channel();
            let notify = WorkflowNotifySender::new(
                agent_client_protocol::SessionId::new("workflow-launch-test"),
                acp_transport::AcpAgentGatewaySender::new(workflow_gateway_tx),
                workflow_persistence_tx,
                store.clone(),
            );
            let manager = WorkflowManager::new(
                "workflow-launch-test".into(),
                Some(session_directory),
                project.path().to_path_buf(),
                tracker.clone(),
                store,
                notify,
                tokio::sync::mpsc::unbounded_channel().0,
                Arc::new(|_, _, _| {}),
                tokio::sync::mpsc::unbounded_channel().0,
                actor.chat_state_handle.clone(),
                Default::default(),
                crate::session::workflow::tracker::WorkflowRuntimeRoute::for_test(
                    "test-model",
                    Some(sampling_types::ReasoningEffort::Medium),
                    sampling_types::ModelImageInputKey::new(
                        "test-model",
                        "responses",
                        "test-endpoint",
                    ),
                )
                .unwrap(),
            );
            actor.workflow_manager = Arc::new(tokio::sync::Mutex::new(manager));
            actor
                .behavior
                .lock()
                .select_behavior(tool_types::BehaviorId::Workflow);
            actor.background_workflows_enabled = true;
            let actor = Arc::new(actor);

            let response = tokio::time::timeout(
                Duration::from_secs(10),
                actor.launch_named_workflow(&name, r#"{"objective":"verify async launch"}"#),
            )
            .await
            .expect("dynamic Workflow command must not block or panic");

            assert!(response.contains("started in the background"), "{response}");
            let runs = tracker.lock().list();
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].name, name);
            assert_eq!(runs[0].objective, "verify async launch");
            assert_eq!(
                actor
                    .workflow_manager
                    .lock()
                    .await
                    .args_copy_for(&runs[0].run_id),
                serde_json::json!({ "objective": "verify async launch" })
            );
            assert_eq!(
                runs[0].definition_hash,
                actor
                    .workflow_manager
                    .lock()
                    .await
                    .script_copy_for(&runs[0].run_id)
                    .map(|script| crate::session::workflow::registry::content_hash(&script))
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn handwritten_workflow_launch_cannot_bypass_disabled_feature_gate() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (actor, _gateway_rx) = super::support::build_actor().await;
            actor
                .behavior
                .lock()
                .select_behavior(tool_types::BehaviorId::Workflow);

            let response = actor.launch_named_workflow("deep-research", "").await;

            assert!(response.contains("disabled for this session"), "{response}");
            assert!(
                actor
                    .workflow_manager
                    .lock()
                    .await
                    .tracker()
                    .lock()
                    .list()
                    .is_empty()
            );
        })
        .await;
}
