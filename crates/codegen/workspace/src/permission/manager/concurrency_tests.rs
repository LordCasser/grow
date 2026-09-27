use super::*;

use crate::permission::auto_mode::{ClassifierOutcome, ClassifierVerdict, PermissionClassifier};
use std::future::Future;
use std::pin::Pin;
use tokio::sync::{mpsc, oneshot};

struct PendingPrompt {
    request: acp::RequestPermissionRequest,
    respond: oneshot::Sender<acp::PermissionOptionId>,
}

struct ControlledPromptClient {
    pending: mpsc::UnboundedSender<PendingPrompt>,
}

#[async_trait::async_trait(?Send)]
impl acp_transport::AcpClientHandler for ControlledPromptClient {
    async fn request_permission(
        &self,
        request: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        let (respond, response) = oneshot::channel();
        self.pending
            .send(PendingPrompt { request, respond })
            .expect("test must keep the prompt receiver alive");
        let Ok(option_id) = response.await else {
            return Ok(acp::RequestPermissionResponse::new(
                acp::RequestPermissionOutcome::Cancelled,
            ));
        };
        Ok(acp::RequestPermissionResponse::new(
            acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(option_id)),
        ))
    }

    async fn session_notification(&self, _: acp::SessionNotification) -> acp::Result<()> {
        Ok(())
    }
}

fn controlled_prompt_manager(
    cwd: &AbsPathBuf,
) -> (
    PermissionHandle,
    mpsc::UnboundedReceiver<PendingPrompt>,
    mpsc::UnboundedReceiver<PermissionEvent>,
) {
    let (pending_tx, pending_rx) = mpsc::unbounded_channel();
    let (manager, events) = manager_with_recording_client_remember(
        cwd,
        None,
        ControlledPromptClient {
            pending: pending_tx,
        },
        ClientType::Generic,
        true,
    );
    (manager, pending_rx, events)
}

async fn next_prompt(pending: &mut mpsc::UnboundedReceiver<PendingPrompt>) -> PendingPrompt {
    tokio::time::timeout(std::time::Duration::from_secs(5), pending.recv())
        .await
        .expect("permission prompt must arrive")
        .expect("prompt channel must remain open")
}

fn response_sender(
    prompt: PendingPrompt,
    option_id: &str,
) -> oneshot::Sender<acp::PermissionOptionId> {
    assert!(
        prompt
            .request
            .options
            .iter()
            .any(|option| option.option_id.0.as_ref() == option_id),
        "permission prompt must expose the selected option"
    );
    prompt.respond
}

fn primary_bash_request(
    manager: PermissionHandle,
    command: &'static str,
) -> tokio::task::JoinHandle<Decision> {
    tokio::task::spawn_local(async move {
        manager
            .request(
                AccessKind::Bash(command.to_owned()),
                tool_call(),
                None,
                None,
                None,
            )
            .await
    })
}

async fn child_bash_request(manager: &PermissionHandle, child_id: &str) -> Decision {
    manager
        .request_with_mode(
            AccessKind::Bash("curl https://child.example.test".into()),
            tool_call(),
            Some(child_id.to_owned()),
            Some("explore".to_owned()),
            None,
            Some(RequestPermissionMode::Ask),
        )
        .await
}

#[tokio::test]
#[should_panic(expected = "test permission request requires an explicit child identity")]
async fn test_shorthand_rejects_partial_child_identity() {
    let manager = PermissionHandle::allow_all();
    let _ = manager
        .request(
            AccessKind::Read(None),
            tool_call(),
            Some("child-without-type".into()),
            None,
            None,
        )
        .await;
}

#[tokio::test]
async fn typed_child_without_display_type_keeps_its_release_boundary() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let tmp = tempfile::tempdir().unwrap();
            let cwd = AbsPathBuf::new(tmp.path().to_path_buf()).unwrap();
            let (manager, mut pending, _events) = controlled_prompt_manager(&cwd);
            let child_manager = manager.clone();
            let child = tokio::task::spawn_local(async move {
                child_manager
                    .request_with_context(
                        AccessKind::Bash("curl https://child.example.test".into()),
                        tool_call(),
                        None,
                        PermissionRequestContext {
                            source: PermissionRequestSource::Child {
                                session_id: "child-without-type".into(),
                                subagent_type: None,
                                subagent_description: None,
                            },
                            request_mode: Some(RequestPermissionMode::Ask),
                            within_capability_fence: false,
                            execution_cwd: None,
                            classifier_turns: None,
                            call_evidence: None,
                        },
                    )
                    .await
            });
            let _prompt = next_prompt(&mut pending).await;
            manager.release_child("child-without-type".into()).await;
            assert_eq!(child.await.unwrap(), Decision::Cancelled);
        })
        .await;
}

#[tokio::test]
async fn manager_admission_saturates_without_blocking_control_and_releases_on_cancel() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let tmp = tempfile::tempdir().unwrap();
            let cwd = AbsPathBuf::new(tmp.path().to_path_buf()).unwrap();
            let (manager, mut pending, _events) = controlled_prompt_manager(&cwd);
            let mut requests = Vec::new();
            for index in 0..MAX_IN_FLIGHT_PERMISSION_REQUESTS {
                let handle = manager.clone();
                requests.push(tokio::task::spawn_local(async move {
                    handle
                        .request_with_mode(
                            AccessKind::Bash(format!("curl https://pending-{index}.example.test")),
                            tool_call(),
                            Some(format!("child-{index}")),
                            Some("explore".into()),
                            None,
                            Some(RequestPermissionMode::Ask),
                        )
                        .await
                }));
            }
            let mut held_prompts = Vec::new();
            for _ in 0..MAX_IN_FLIGHT_PERMISSION_REQUESTS {
                held_prompts.push(next_prompt(&mut pending).await);
            }
            let primary = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                manager.request(AccessKind::Read(None), tool_call(), None, None, None),
            )
            .await
            .expect("child saturation must not block the primary");
            assert_eq!(primary, Decision::Allow);
            let overloaded = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                manager.request(
                    AccessKind::Bash("curl https://overloaded.example.test".into()),
                    tool_call(),
                    Some("child-overloaded".into()),
                    Some("explore".into()),
                    None,
                ),
            )
            .await
            .expect("overload must not wait behind open prompts");
            assert_eq!(
                overloaded,
                Decision::PolicyDeny(PERMISSION_REQUEST_OVERLOADED.into())
            );

            for index in MAX_IN_FLIGHT_PERMISSION_REQUESTS
                ..MAX_TOTAL_IN_FLIGHT_PERMISSION_REQUESTS
            {
                let handle = manager.clone();
                requests.push(tokio::task::spawn_local(async move {
                    handle
                        .request(
                            AccessKind::Bash(format!("curl https://primary-{index}.example.test")),
                            tool_call(),
                            None,
                            None,
                            None,
                        )
                        .await
                }));
            }
            for _ in MAX_IN_FLIGHT_PERMISSION_REQUESTS
                ..MAX_TOTAL_IN_FLIGHT_PERMISSION_REQUESTS
            {
                held_prompts.push(next_prompt(&mut pending).await);
            }
            let primary_overloaded = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                manager.request(
                    AccessKind::Bash("curl https://primary-overloaded.example.test".into()),
                    tool_call(),
                    None,
                    None,
                    None,
                ),
            )
            .await
            .expect("primary requests must also have a finite capacity");
            assert_eq!(
                primary_overloaded,
                Decision::PolicyDeny(PERMISSION_REQUEST_OVERLOADED.into())
            );

            tokio::time::timeout(std::time::Duration::from_secs(2), manager.reset_state())
                .await
                .expect("control must progress at saturated request capacity")
                .unwrap();
            for request in requests {
                assert_eq!(request.await.unwrap(), Decision::Cancelled);
            }
            drop(held_prompts);

            let next = primary_bash_request(manager.clone(), "curl https://fresh.example.test");
            let prompt = next_prompt(&mut pending).await;
            response_sender(prompt, "allow-once")
                .send(acp::PermissionOptionId::new("allow-once"))
                .unwrap();
            assert_eq!(next.await.unwrap(), Decision::Allow);
        })
        .await;
}

#[tokio::test]
async fn cancelled_request_keeps_admission_until_actor_discards_its_command() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let tmp = tempfile::tempdir().unwrap();
            let cwd = AbsPathBuf::new(tmp.path().to_path_buf()).unwrap();
            let (manager, _pending, _events) = controlled_prompt_manager(&cwd);
            let PermissionHandle::Actor { ref in_flight, .. } = manager else {
                panic!("expected actor-backed permission manager");
            };
            let mut request = Box::pin(manager.request(
                AccessKind::Bash("curl https://abandoned.example.test".into()),
                tool_call(),
                Some("abandoned-child".into()),
                Some("explore".into()),
                None,
            ));
            assert!(matches!(futures::poll!(request.as_mut()), std::task::Poll::Pending));
            drop(request);
            assert_eq!(in_flight.load(Ordering::Relaxed), 1);
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                while in_flight.load(Ordering::Relaxed) != 0 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("actor must discard the cancelled command and release admission");
        })
        .await;
}

#[tokio::test]
async fn reset_ack_waits_for_saved_state_and_reports_write_failure() {
    for fail_write in [false, true] {
        tokio::task::LocalSet::new()
            .run_until(async move {
                let tmp = tempfile::tempdir().unwrap();
                let cwd = AbsPathBuf::new(tmp.path().to_path_buf()).unwrap();
                let command = "curl https://remembered.example.test";
                let mut seed = PermissionState::default();
                seed.allowed_bash_commands.insert(command.into());
                persist_state(&cwd, &seed, None).await.unwrap();
                let (manager, mut pending, _events) = controlled_prompt_manager(&cwd);
                manager.set_mode(PermissionMode::Ask).await;
                assert_eq!(
                    manager
                        .request(
                            AccessKind::Bash(command.into()),
                            tool_call(),
                            None,
                            None,
                            None,
                        )
                        .await,
                    Decision::Allow,
                    "seeded grant must be loaded before reset"
                );

                let state_path = config::sessions_cwd_dir(cwd.as_str()).join("permission.toml");
                let backup = state_path.with_extension("backup");
                if fail_write {
                    tokio::fs::rename(&state_path, &backup).await.unwrap();
                    tokio::fs::create_dir(&state_path).await.unwrap();
                }
                let reset = manager.reset_state().await;
                if fail_write {
                    assert!(reset.is_err(), "Reset must expose the storage failure");
                } else {
                    reset.unwrap();
                    let saved = load_state_from_disk(&cwd, None).await;
                    assert!(!saved.allowed_bash_commands.contains(command));
                }

                let next = primary_bash_request(manager.clone(), command);
                let prompt = next_prompt(&mut pending).await;
                response_sender(prompt, "allow-once")
                    .send(acp::PermissionOptionId::new("allow-once"))
                    .unwrap();
                assert_eq!(next.await.unwrap(), Decision::Allow);
                manager.shutdown_and_drain().await;
                if fail_write {
                    tokio::fs::remove_dir(&state_path).await.unwrap();
                    tokio::fs::rename(backup, state_path).await.unwrap();
                }
            })
            .await;
    }
}

#[tokio::test]
async fn concurrent_same_scope_prompts_merge_remembered_grants() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let tmp = tempfile::tempdir().unwrap();
            let cwd = AbsPathBuf::new(tmp.path().to_path_buf()).unwrap();
            let (manager, mut pending, _events) = controlled_prompt_manager(&cwd);

            let a = primary_bash_request(manager.clone(), "curl https://a.example.test");
            let b = primary_bash_request(manager.clone(), "curl https://b.example.test");
            let first = next_prompt(&mut pending).await;
            let second = next_prompt(&mut pending).await;

            // Complete in reverse order to prove each prompt commits into the
            // latest scope state instead of replacing it with its old snapshot.
            response_sender(second, "always-allow")
                .send(acp::PermissionOptionId::new("always-allow"))
                .unwrap();
            response_sender(first, "always-allow")
                .send(acp::PermissionOptionId::new("always-allow"))
                .unwrap();
            assert_eq!(a.await.unwrap(), Decision::Allow);
            assert_eq!(b.await.unwrap(), Decision::Allow);

            for command in ["curl https://a.example.test", "curl https://b.example.test"] {
                assert_eq!(
                    manager
                        .request(
                            AccessKind::Bash(command.to_owned()),
                            tool_call(),
                            None,
                            None,
                            None,
                        )
                        .await,
                    Decision::Allow,
                    "both concurrent grants must remain in the scope"
                );
            }
            assert!(
                pending.try_recv().is_err(),
                "remembered grants skip prompts"
            );
            manager.shutdown_and_drain().await;
            let saved = load_state_from_disk(&cwd, None).await;
            for command in ["curl https://a.example.test", "curl https://b.example.test"] {
                assert!(saved.allowed_bash_commands.contains(command));
            }
        })
        .await;
}

#[tokio::test]
async fn concurrent_classifier_denials_commit_against_latest_scope_counters() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let tmp = tempfile::tempdir().unwrap();
            let cwd = AbsPathBuf::new(tmp.path().to_path_buf()).unwrap();
            let (manager, mut events) = test_manager(&cwd, PermissionMode::Ask);
            let (entered_tx, mut entered_rx) = mpsc::unbounded_channel();
            manager.set_mode(PermissionMode::Auto).await;
            manager.set_classifier(Some(Arc::new(DeferredClassifier {
                entered: entered_tx,
            })));

            let a = primary_bash_request(manager.clone(), "curl https://a.example.test");
            let b = primary_bash_request(manager.clone(), "curl https://b.example.test");
            let (_, deny_a) =
                tokio::time::timeout(std::time::Duration::from_secs(5), entered_rx.recv())
                    .await
                    .expect("first classifier must start")
                    .expect("classifier channel must remain open");
            let (_, deny_b) =
                tokio::time::timeout(std::time::Duration::from_secs(5), entered_rx.recv())
                    .await
                    .expect("second classifier must start while the first is pending")
                    .expect("classifier channel must remain open");

            // Keep A pending while B commits, then let A commit against the
            // updated counters. The requests therefore overlap but have a
            // deterministic completion order.
            deny_b.send(ClassifierVerdict::Block).unwrap();
            assert!(matches!(b.await.unwrap(), Decision::PolicyDeny(_)));
            let first_event =
                tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
                    .await
                    .expect("first classifier decision event")
                    .expect("permission event stream must remain open");
            assert_eq!(first_event.auto_denials_consecutive, Some(1));
            assert_eq!(first_event.auto_denials_total, Some(1));

            deny_a.send(ClassifierVerdict::Block).unwrap();
            assert!(matches!(a.await.unwrap(), Decision::PolicyDeny(_)));
            let second_event =
                tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
                    .await
                    .expect("second classifier decision event")
                    .expect("permission event stream must remain open");
            assert_eq!(second_event.auto_denials_consecutive, Some(2));
            assert_eq!(second_event.auto_denials_total, Some(2));
        })
        .await;
}

#[tokio::test]
async fn reset_release_and_mode_change_cancel_old_prompts_and_ignore_late_allows() {
    #[derive(Clone, Copy)]
    enum Control {
        Reset,
        ReleaseChild,
        SetMode,
    }

    for control in [Control::Reset, Control::ReleaseChild, Control::SetMode] {
        tokio::task::LocalSet::new()
            .run_until(async move {
                let tmp = tempfile::tempdir().unwrap();
                let cwd = AbsPathBuf::new(tmp.path().to_path_buf()).unwrap();
                let (manager, mut pending, _events) = controlled_prompt_manager(&cwd);
                let is_child = matches!(control, Control::ReleaseChild);
                let first_manager = manager.clone();
                let first = tokio::task::spawn_local(async move {
                    if is_child {
                        child_bash_request(&first_manager, "child-reset-target").await
                    } else {
                        first_manager
                            .request(
                                AccessKind::Bash("curl https://cancel.example.test".into()),
                                tool_call(),
                                None,
                                None,
                                None,
                            )
                            .await
                    }
                });
                let old_prompt = next_prompt(&mut pending).await;
                let stale_option = if is_child {
                    "allow-once"
                } else {
                    "always-allow"
                };
                let stale_allow = response_sender(old_prompt, stale_option);

                match control {
                    Control::Reset => manager.reset_state().await.unwrap(),
                    Control::ReleaseChild => {
                        manager.release_child("child-reset-target".into()).await
                    }
                    Control::SetMode => {
                        manager.set_mode(PermissionMode::AlwaysApprove).await;
                    }
                }
                assert_eq!(
                    tokio::time::timeout(std::time::Duration::from_secs(5), first)
                        .await
                        .expect("revocation must not wait for the prompt deadline")
                        .unwrap(),
                    Decision::Cancelled
                );
                // The ACP gateway may still finish its handler after the
                // manager abandons the prompt. Whether the transport accepts
                // this late response is irrelevant: it must not revive this
                // request or commit a remembered grant.
                let _ = stale_allow.send(acp::PermissionOptionId::new(stale_option));

                if matches!(control, Control::SetMode) {
                    manager.set_mode(PermissionMode::Ask).await;
                }
                let next = if is_child {
                    let request_manager = manager.clone();
                    tokio::task::spawn_local(async move {
                        child_bash_request(&request_manager, "child-reset-target").await
                    })
                } else {
                    primary_bash_request(manager.clone(), "curl https://cancel.example.test")
                };
                let new_prompt = next_prompt(&mut pending).await;
                let next_option = if is_child {
                    "allow-once"
                } else {
                    "always-allow"
                };
                response_sender(new_prompt, next_option)
                    .send(acp::PermissionOptionId::new(next_option))
                    .unwrap();
                assert_eq!(next.await.unwrap(), Decision::Allow);
            })
            .await;
    }
}

struct DeferredClassifier {
    entered: mpsc::UnboundedSender<(String, oneshot::Sender<ClassifierVerdict>)>,
}

// Enqueue a control from inside the pending judgment, forcing the control to
// race with the old verdict in one LocalSet poll.
struct ReentrantControlClassifier {
    manager: PermissionHandle,
    control: &'static str,
}

impl PermissionClassifier for ReentrantControlClassifier {
    fn classify<'a>(
        &'a self,
        _tool_name: &'a str,
        _access: &'a AccessKind,
        _access_detail: Option<&'a str>,
        _context: crate::permission::auto_mode::ClassifierContext,
    ) -> Pin<Box<dyn Future<Output = ClassifierOutcome> + Send + 'a>> {
        let manager = self.manager.clone();
        let control = self.control;
        Box::pin(async move {
            match control {
                "reset" => manager.reset_state().await.unwrap(),
                "release" => manager.release_child("probe-child".into()).await,
                "mode" => manager.set_mode(PermissionMode::Ask).await,
                _ => unreachable!(),
            }
            ClassifierVerdict::Allow.into()
        })
    }
}

#[tokio::test]
async fn controls_revoke_before_reentrant_classifier_can_allow() {
    let mut outcomes = Vec::new();
    for control in ["reset", "release", "mode"] {
        let result = tokio::task::LocalSet::new()
            .run_until(async move {
                let tmp = tempfile::tempdir().unwrap();
                let cwd = AbsPathBuf::new(tmp.path().to_path_buf()).unwrap();
                let (manager, _events) = test_manager(&cwd, PermissionMode::Auto);
                manager.set_classifier(Some(Arc::new(ReentrantControlClassifier {
                    manager: manager.clone(),
                    control,
                })));
                let result = if control == "release" {
                    manager
                        .request_with_mode(
                            AccessKind::Bash("curl https://probe.example.test".into()),
                            tool_call(),
                            Some("probe-child".into()),
                            Some("explore".into()),
                            None,
                            Some(RequestPermissionMode::Auto),
                        )
                        .await
                } else {
                    manager
                        .request(
                            AccessKind::Bash("curl https://probe.example.test".into()),
                            tool_call(),
                            None,
                            None,
                            None,
                        )
                        .await
                };
                result
            })
            .await;
        outcomes.push((control, result));
    }
    assert_eq!(
        outcomes,
        vec![
            ("reset", Decision::Cancelled),
            ("release", Decision::Cancelled),
            ("mode", Decision::Cancelled),
        ]
    );
}

impl PermissionClassifier for DeferredClassifier {
    fn classify<'a>(
        &'a self,
        tool_name: &'a str,
        _access: &'a AccessKind,
        _access_detail: Option<&'a str>,
        _context: crate::permission::auto_mode::ClassifierContext,
    ) -> Pin<Box<dyn Future<Output = ClassifierOutcome> + Send + 'a>> {
        let tool_name = tool_name.to_owned();
        Box::pin(async move {
            let (respond, response) = oneshot::channel();
            self.entered
                .send((tool_name, respond))
                .expect("test must keep the classifier receiver alive");
            response
                .await
                .unwrap_or(ClassifierVerdict::Unavailable)
                .into()
        })
    }
}

#[tokio::test]
async fn pending_child_classifier_does_not_block_primary_local_read() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let tmp = tempfile::tempdir().unwrap();
            let cwd = AbsPathBuf::new(tmp.path().to_path_buf()).unwrap();
            let (manager, _events) = test_manager(&cwd, PermissionMode::Ask);
            let (entered_tx, mut entered_rx) = mpsc::unbounded_channel();
            manager.set_mode(PermissionMode::Auto).await;
            manager.set_classifier(Some(Arc::new(DeferredClassifier {
                entered: entered_tx,
            })));

            let child_manager = manager.clone();
            let child = tokio::task::spawn_local(async move {
                child_manager
                    .request_with_mode(
                        AccessKind::Bash("curl https://child.example.test".into()),
                        tool_call(),
                        Some("waiting-child".into()),
                        Some("explore".into()),
                        None,
                        Some(RequestPermissionMode::Auto),
                    )
                    .await
            });
            let (tool_name, allow_classifier) =
                tokio::time::timeout(std::time::Duration::from_secs(5), entered_rx.recv())
                    .await
                    .expect("child must enter its classifier independently")
                    .expect("classifier channel must remain open");
            assert!(!tool_name.is_empty());
            assert!(!child.is_finished(), "child judgment remains gated");

            let primary_read = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                manager.request(
                    AccessKind::Read(Some("README.md".into())),
                    tool_call(),
                    None,
                    None,
                    None,
                ),
            )
            .await
            .expect("a child classifier must not block another scope's local policy check");
            assert_eq!(primary_read, Decision::Allow);
            assert!(
                !child.is_finished(),
                "the child classifier should still be pending"
            );

            manager.set_mode(PermissionMode::AlwaysApprove).await;
            assert_eq!(
                primary_bash_request(manager.clone(), "curl https://primary.example.test")
                    .await
                    .unwrap(),
                Decision::Allow
            );
            assert!(
                !child.is_finished(),
                "primary mode change must not cancel or allow the child"
            );
            assert!(
                entered_rx.try_recv().is_err(),
                "new primary mode applies without a model call"
            );
            allow_classifier.send(ClassifierVerdict::Allow).unwrap();
            assert_eq!(child.await.unwrap(), Decision::Allow);
        })
        .await;
}

#[tokio::test]
async fn remembered_deny_revokes_an_older_pending_allow() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let tmp = tempfile::tempdir().unwrap();
            let cwd = AbsPathBuf::new(tmp.path().to_path_buf()).unwrap();
            let (pending_tx, mut pending) = mpsc::unbounded_channel();
            let (manager, _events) = manager_with_recording_client_remember(
                &cwd,
                None,
                ControlledPromptClient {
                    pending: pending_tx,
                },
                ClientType::GrowPager,
                true,
            );
            let a = primary_bash_request(manager.clone(), "curl https://denied.example.test");
            let first = next_prompt(&mut pending).await;
            let b = primary_bash_request(manager.clone(), "curl https://denied.example.test");
            let second = next_prompt(&mut pending).await;
            response_sender(second, "reject-always-command")
                .send(acp::PermissionOptionId::new("reject-always-command"))
                .unwrap();
            assert!(matches!(b.await.unwrap(), Decision::Reject(_)));
            assert_eq!(
                tokio::time::timeout(std::time::Duration::from_secs(1), a)
                    .await
                    .expect("remembered deny must revoke pending decisions")
                    .unwrap(),
                Decision::Cancelled
            );
            let _ = response_sender(first, "allow-always-command")
                .send(acp::PermissionOptionId::new("allow-always-command"));
            assert!(matches!(
                primary_bash_request(manager.clone(), "curl https://denied.example.test")
                    .await
                    .unwrap(),
                Decision::PolicyDeny(_) | Decision::Reject(_)
            ));
            manager.shutdown_and_drain().await;
            let saved = load_state_from_disk(&cwd, None).await;
            assert!(
                saved
                    .disallowed_bash_commands
                    .contains("curl https://denied.example.test")
            );
            assert!(
                saved.allowed_bash_commands.is_empty(),
                "stale allow must not be persisted"
            );
        })
        .await;
}

#[tokio::test]
async fn child_remembered_grants_are_isolated_from_parent_and_siblings() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let tmp = tempfile::tempdir().unwrap();
            let cwd = AbsPathBuf::new(tmp.path().to_path_buf()).unwrap();
            let (manager, mut pending, _events) = controlled_prompt_manager(&cwd);
            let parent = primary_bash_request(manager.clone(), "curl https://child.example.test");
            response_sender(next_prompt(&mut pending).await, "always-allow")
                .send(acp::PermissionOptionId::new("always-allow"))
                .unwrap();
            assert_eq!(parent.await.unwrap(), Decision::Allow);
            for child_id in ["child-a", "child-b", "child-a"] {
                let handle = manager.clone();
                let child =
                    tokio::task::spawn_local(
                        async move { child_bash_request(&handle, child_id).await },
                    );
                let prompt = next_prompt(&mut pending).await;
                assert!(prompt.request.options.iter().all(|option| matches!(
                    option.kind,
                    acp::PermissionOptionKind::AllowOnce | acp::PermissionOptionKind::RejectOnce
                )));
                response_sender(prompt, "allow-once")
                    .send(acp::PermissionOptionId::new("allow-once"))
                    .unwrap();
                assert_eq!(child.await.unwrap(), Decision::Allow);
            }
            assert_eq!(
                primary_bash_request(manager.clone(), "curl https://child.example.test")
                    .await
                    .unwrap(),
                Decision::Allow
            );
            manager.shutdown_and_drain().await;
            let saved = load_state_from_disk(&cwd, None).await;
            assert_eq!(saved.allowed_bash_commands.len(), 1);
            assert!(
                saved
                    .allowed_bash_commands
                    .contains("curl https://child.example.test")
            );
        })
        .await;
}

#[tokio::test]
async fn reset_and_release_cancel_in_flight_classification() {
    for release_child in [false, true] {
        tokio::task::LocalSet::new()
            .run_until(async {
                let tmp = tempfile::tempdir().unwrap();
                let cwd = AbsPathBuf::new(tmp.path().to_path_buf()).unwrap();
                let (manager, mut events) = test_manager(&cwd, PermissionMode::Auto);
                let (entered, mut pending) = mpsc::unbounded_channel();
                manager.set_classifier(Some(Arc::new(DeferredClassifier { entered })));
                let handle = manager.clone();
                let request = tokio::task::spawn_local(async move {
                    handle
                        .request_with_mode(
                            AccessKind::Bash("curl https://child.example.test".into()),
                            tool_call(),
                            Some("cancelled-child".into()),
                            Some("explore".into()),
                            None,
                            Some(RequestPermissionMode::Auto),
                        )
                        .await
                });
                let (_, late_allow) =
                    tokio::time::timeout(std::time::Duration::from_secs(1), pending.recv())
                        .await
                        .unwrap()
                        .unwrap();
                if release_child {
                    manager.release_child("cancelled-child".into()).await;
                } else {
                    manager.reset_state().await.unwrap();
                }
                assert_eq!(
                    tokio::time::timeout(std::time::Duration::from_secs(1), request)
                        .await
                        .unwrap()
                        .unwrap(),
                    Decision::Cancelled
                );
                assert!(late_allow.send(ClassifierVerdict::Allow).is_err());
                let event = events.recv().await.unwrap();
                assert_eq!(
                    event.decision_reason.as_deref(),
                    Some(reasons::AUTHORIZATION_CHANGED)
                );
                assert!(event.classifier_verdict.is_none());
                manager.shutdown_and_drain().await;
                assert!(
                    load_state_from_disk(&cwd, None)
                        .await
                        .allowed_bash_commands
                        .is_empty()
                );
            })
            .await;
    }
}
