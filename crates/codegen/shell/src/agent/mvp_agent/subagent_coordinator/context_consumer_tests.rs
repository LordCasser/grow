//! Real coordinator routing and Shell owner reads with controlled execution ACKs.
//! Provider admission/first-spawn persistence remains a separate milestone.
use super::*;
use crate::session::acp_types::ParentPlanBinding;
use crate::session::actor::tests::support::test_agent_with_tools;
use tools::implementations::grow_build::task::backend::{ChannelBackend, SubagentBackend};
use tools::implementations::grow_build::task::coordinator::{
    ChildCompletion, ChildControl, ChildRunOutcome, ChildRunOutput, ChildRunRequest,
    CoordinatorConfig, LocalBoxFuture, StartedChild, SubagentCoordinator, SubagentProgress,
};
use tools::implementations::grow_build::task::types::{
    SubagentOwner, SubagentRequest, SubagentResult, SubagentValidateTypeOutcome,
};
use tools::implementations::grow_build::todo::TodoReadTool;

#[derive(Clone)]
struct Control {
    session_id: String,
    cancellation: CancellationToken,
}

impl ChildControl for Control {
    type SecurityContext = String;
    fn progress_snapshot(&self) -> Option<SubagentProgress> {
        None
    }
    fn security_context(&self) -> String {
        self.session_id.clone()
    }
    fn cancel(&self) {
        self.cancellation.cancel();
    }
}

struct Runner {
    shell: ShellChildRunner,
    started: mpsc::UnboundedSender<String>,
}

impl ChildRunner for Runner {
    type Control = Control;
    type CompletionData = ();
    type RunFuture = LocalBoxFuture<ChildRunOutcome<()>>;
    type ValidateFuture = std::future::Ready<SubagentValidateTypeOutcome>;

    fn run(&self, run: ChildRunRequest<Control>) -> Self::RunFuture {
        let started = self.started.clone();
        Box::pin(async move {
            let session_id = match run.request.id.as_str() {
                "task-A" => "direct-parent-A",
                "task-16" => "actual-child-B",
                _ => panic!("unexpected controlled execution"),
            };
            assert!(
                run.reporter
                    .started(StartedChild {
                        child_session_id: session_id.into(),
                        resumed_from: None,
                        definition_background: false,
                        control: Control {
                            session_id: session_id.into(),
                            cancellation: run.cancellation.clone()
                        },
                    })
                    .await
            );
            started.send(run.request.id.clone()).unwrap();
            run.cancellation.cancelled().await;
            ChildRunOutcome::Finished(ChildRunOutput {
                result: SubagentResult {
                    subagent_id: run.request.id,
                    child_session_id: session_id.into(),
                    ..Default::default()
                },
                completion_data: (),
            })
        })
    }

    fn read_parent_plan(
        &self,
        request: ParentPlanningRequest,
        parent: String,
        task: String,
        step: Option<String>,
    ) {
        // The production coordinator resolves every argument. The real Shell
        // trait handler performs handle lookup and canonical ChatState reads.
        self.shell.read_parent_plan(request, parent, task, step);
    }

    fn validate_type(&self, _: String, _: String) -> Self::ValidateFuture {
        std::future::ready(SubagentValidateTypeOutcome::Ok)
    }
    fn on_completed(&self, _: ChildCompletion<()>) {}
}

fn request(id: &str, step: Option<&str>) -> SubagentRequest {
    SubagentRequest {
        id: id.into(),
        origin_tool_call_id: None,
        prompt: "Authored objective; contains no injected parent plan".into(),
        description: "controlled execution route".into(),
        subagent_type: "general-purpose".into(),
        parent_session_id: "overwritten-by-bound-backend".into(),
        parent_step_id: step.map(str::to_owned),
        parent_prompt_id: None,
        resume_from: None,
        cwd: None,
        runtime_overrides: Default::default(),
        run_in_background: true,
        surface_completion: false,
        await_to_completion: false,
        fork_context: false,
        owner: SubagentOwner::Task,
        goal_context: None,
        cancel_token: CancellationToken::new(),
    }
}

async fn start(
    backend: ChannelBackend,
    request: SubagentRequest,
    starts: &mut mpsc::UnboundedReceiver<String>,
) -> tokio::task::JoinHandle<Result<SubagentResult, tool_runtime::ToolError>> {
    let expected = request.id.clone();
    let spawn = tokio::task::spawn_local(async move { backend.spawn(request).await });
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), starts.recv())
            .await
            .unwrap(),
        Some(expected)
    );
    spawn
}

fn context_json(text: &str) -> serde_json::Value {
    assert!(text.len() <= 8192);
    serde_json::from_str(text.split_once('\n').unwrap().1).unwrap()
}

#[test]
fn context_follows_nested_coordinator_route_and_refreshes_current_owner_facts() {
    run_local_for_bridge_test(|| async {
        let agent = build_minimal_agent_for_tests();
        let mut root = Fixture::new("lifecycle-ROOT").await;
        let mut parent = Fixture::new("direct-parent-A").await;
        let mut child = Fixture::new("actual-child-B").await;
        let mut config = tools::registry::types::ToolConfig::for_tool::<TodoReadTool>();
        config.name_override = Some("resolved_91_read_planning".into());
        *child.actor.agent.borrow_mut() = test_agent_with_tools(vec![config]).await;
        child.actor.startup_hints.parent_session_id = Some("lifecycle-ROOT".into());
        child.actor.startup_hints.delegated_parent_plan = Some(ParentPlanBinding {
            parent_session_id: "direct-parent-A".into(),
            task_id: "task-16".into(),
            parent_step_id: "step-16".into(),
        });
        let decoy = LocalPlan::new("lifecycle-ROOT", "root-decoy-scope");
        commit_plan(&root, &decoy).await;
        let mut plan = complete_plan("direct-parent-A", "actual-child-B");
        plan.steps[16].removed = false;
        plan.steps[16].status = PlanStepStatus::InProgress;
        plan.steps[16].content = "initial current-parent objective".into();
        plan.links[16].terminal = false;
        plan.links[16].result_id = None;
        plan.reviews.retain(|review| review.link_id != "task-16");
        plan.validate().unwrap();
        let mut seq = commit_plan(&parent, &plan).await;
        agent
            .sessions
            .borrow_mut()
            .insert(root.handle.info.id.clone(), root.handle.clone());
        agent
            .active_child_sessions
            .borrow_mut()
            .insert(parent.handle.info.id.clone(), parent.handle.clone());
        agent
            .active_child_sessions
            .borrow_mut()
            .insert(child.handle.info.id.clone(), child.handle.clone());
        let (events, receiver) = mpsc::unbounded_channel();
        let (started, mut starts) = mpsc::unbounded_channel();
        let coordinator = tokio::task::spawn_local(
            SubagentCoordinator::new(
                receiver,
                Runner {
                    shell: ShellChildRunner {
                        agent_ref: LocalRef::new(&agent),
                    },
                    started,
                },
                CoordinatorConfig::default(),
            )
            .run(),
        );
        let root_backend = ChannelBackend::for_session(events.clone(), "lifecycle-ROOT");
        let parent_spawn = start(root_backend, request("task-A", None), &mut starts).await;
        let parent_backend = ChannelBackend::for_session(events.clone(), "direct-parent-A");
        let child_spawn = start(
            parent_backend,
            request("task-16", Some("step-16")),
            &mut starts,
        )
        .await;
        let bridge = child.actor.agent.borrow().tool_bridge().clone();
        bridge
            .update_resource(ChannelBackend::for_session(events, "actual-child-B").into_resource())
            .await;
        let initial = child.actor.local_planning_context().await.unwrap().unwrap();
        for pass in 0..2 {
            let before = (
                root.observables().await,
                parent.observables().await,
                child.observables().await,
            );
            let text =
                tokio::time::timeout(Duration::from_secs(2), child.actor.local_planning_context())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
            let value = context_json(&text);
            assert_eq!(
                value["parent"]["binding"]["parent_session_id"],
                "direct-parent-A"
            );
            assert_eq!(value["parent"]["source"]["session_id"], "direct-parent-A");
            assert_eq!(value["parent"]["source"]["scope_id"], "scope:direct-parent");
            assert_eq!(value["parent"]["source"]["timeline_seq"], seq.get());
            assert_eq!(value["parent"]["source"]["revision"], plan.revision);
            assert_eq!(value["parent"]["associated_step"]["position"], 17);
            assert_eq!(value["read_entry"]["tool"], "resolved_91_read_planning");
            assert!(!text.contains("root-decoy-scope"));
            assert!(value["self"].is_null());
            if pass == 1 {
                assert!(text.contains("latest parent objective"));
                assert!(!text.contains("initial current-parent objective"));
                assert_ne!(text, initial);
            }
            assert_eq!(
                (
                    root.observables().await,
                    parent.observables().await,
                    child.observables().await
                ),
                before
            );
            if pass == 0 {
                let mut updated = plan.steps[16].clone();
                updated.content = "latest parent objective".into();
                plan.update_steps(PlanUpdateMode::Merge, vec![updated])
                    .unwrap();
                seq = parent
                    .actor
                    .chat_state_handle
                    .record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                        chat_state::timeline::PlanningEvent {
                            previous: Some(seq),
                            plan: Box::new(plan.clone()),
                        },
                    ))
                    .await
                    .unwrap()
                    .seq;
            }
        }
        agent
            .active_child_sessions
            .borrow_mut()
            .remove(&parent.handle.info.id);
        let unavailable = child.actor.local_planning_context().await.unwrap().unwrap();
        assert_eq!(
            context_json(&unavailable)["parent"]["error_code"],
            "unavailable"
        );
        assert_eq!(
            context_json(&unavailable)["parent"]["binding"]["task_id"],
            "task-16"
        );
        assert!(
            !unavailable.contains("latest parent objective"),
            "a failed read must not reuse cached facts"
        );
        root.assert_no_publication();
        parent.assert_no_publication();
        child.assert_no_publication();
        coordinator.abort();
        assert!(coordinator.await.unwrap_err().is_cancelled());
        for spawn in [parent_spawn, child_spawn] {
            if !spawn.is_finished() {
                spawn.abort();
            }
            let _ = spawn.await;
        }
    });
}
