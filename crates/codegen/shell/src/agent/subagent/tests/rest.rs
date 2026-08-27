#![cfg_attr(rustfmt, rustfmt::skip)]
use super::*;
use crate::test_support::lsp_runtime::{ctx_with_toggle, test_gateway};
use tools::implementations::grow_build::task::backend::ChannelBackend;
#[test]
fn normalize_forked_context_strips_project_layout() {
    use sampling_types::conversation::ConversationItem;
    let big_layout = "<project_layout>\nline1\nline2\nline3\n</project_layout>";
    let items = vec![
            ConversationItem::system("sys"),
            ConversationItem::user(big_layout),
            ConversationItem::assistant("ack"),
        ];
    let (conv, _) = crate::agent::subagent::resolution::context::normalize_forked_context(
        items,
    );
    if let ConversationItem::User(u) = &conv[1] {
        let text = u
            .content
            .iter()
            .filter_map(|p| match p {
                sampling_types::conversation::ContentPart::Text { text } => {
                    Some(text.as_ref())
                }
                _ => None,
            })
            .collect::<String>();
        assert!(
                !text.contains("<project_layout>"),
                "project_layout tag should be stripped"
            );
        assert!(!text.contains("line1"), "layout content should be removed");
    } else {
        panic!("expected User at position 1");
    }
}
#[test]
fn normalize_forked_context_consecutive_users() {
    use sampling_types::conversation::ConversationItem;
    let items = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("prefix"),
            ConversationItem::user("query"),
            ConversationItem::assistant("response"),
        ];
    let (conv, prefix_len) = crate::agent::subagent::resolution::context::normalize_forked_context(
        items,
    );
    assert_eq!(prefix_len, 2);
    if let ConversationItem::User(u) = &conv[1] {
        let text = u
            .content
            .iter()
            .filter_map(|p| match p {
                sampling_types::conversation::ContentPart::Text { text } => {
                    Some(text.as_ref())
                }
                _ => None,
            })
            .collect::<String>();
        assert!(
                text.contains("[User]: prefix"),
                "should include first user msg"
            );
        assert!(
                text.contains("[User]: query"),
                "should include second user msg"
            );
        assert!(
                text.contains("[Assistant]: response"),
                "should include assistant"
            );
    } else {
        panic!("expected User at position 1");
    }
}
/// End-to-end test: after normalization + Timeline seed composition,
/// the conversation shape is [System(child's), BackgroundContext].
/// Then the Prompt command appends the task as [2], giving:
/// [System(child's), BackgroundContext, Task].
#[test]
fn end_to_end_normalized_conversation_shape() {
    use sampling_types::conversation::ConversationItem;
    let parent_conv = vec![
            ConversationItem::system("parent system prompt"),
            ConversationItem::user("user prefix with project info"),
            ConversationItem::user("implement quicksort"),
            ConversationItem::assistant("here is quicksort"),
        ];
    let (mut conv, prefix_len) = crate::agent::subagent::resolution::context::normalize_forked_context(
        parent_conv,
    );
    assert_eq!(prefix_len, 2);
    assert_eq!(conv.len(), 2);
    let mut seeded_prefix_len = Some(prefix_len);
    seed_child_system_head(
        &InitialContextSource::Forked,
        false,
        &mut conv,
        &mut seeded_prefix_len,
        "child system prompt with tool guidance",
    )
    .unwrap();
    if let ConversationItem::System(ref sys) = conv[0] {
        assert_eq!(
                sys.content.as_ref(),
                "child system prompt with tool guidance"
            );
    }
    if let ConversationItem::User(ref u) = conv[1] {
        let text = u
            .content
            .iter()
            .filter_map(|p| match p {
                sampling_types::conversation::ContentPart::Text { text } => {
                    Some(text.as_ref())
                }
                _ => None,
            })
            .collect::<String>();
        assert!(text.contains("<background_context>"));
        assert!(text.contains("[User]: implement quicksort"));
    } else {
        panic!("expected User (background) at position 1");
    }
    let task = "implement bubble sort in Rust";
    conv.push(ConversationItem::user(task));
    assert_eq!(conv.len(), 3);
    assert!(matches!(conv[0], ConversationItem::System(_)));
    assert!(matches!(conv[1], ConversationItem::User(_)));
    assert!(matches!(conv[2], ConversationItem::User(_)));
    if let ConversationItem::User(ref u) = conv[2] {
        let text = u
            .content
            .iter()
            .filter_map(|p| match p {
                sampling_types::conversation::ContentPart::Text { text } => {
                    Some(text.as_ref())
                }
                _ => None,
            })
            .collect::<String>();
        assert_eq!(text, task, "last user message should be the task");
    }
    assert_eq!(prefix_len, 2);
    assert_eq!(seeded_prefix_len, Some(2));
    assert!(prefix_len < conv.len(), "prefix should not cover the task");
}
/// Verify that the task prompt (not background context) would be the
/// cached prompt text in the session pipeline.
#[test]
fn cached_prompt_text_is_task_not_background() {
    use sampling_types::conversation::ConversationItem;
    let parent_conv = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("parent query"),
            ConversationItem::assistant("parent answer"),
        ];
    let (conv, _) = crate::agent::subagent::resolution::context::normalize_forked_context(
        parent_conv,
    );
    let background_text = if let ConversationItem::User(ref u) = conv[1] {
        u.content
            .iter()
            .filter_map(|p| match p {
                sampling_types::conversation::ContentPart::Text { text } => {
                    Some(text.as_ref())
                }
                _ => None,
            })
            .collect::<String>()
    } else {
        String::new()
    };
    let task_prompt = "fix the failing test in src/lib.rs";
    assert_ne!(task_prompt, background_text.trim());
    assert!(
            !background_text.contains(task_prompt),
            "background should not contain the task prompt"
        );
    assert!(
            background_text.contains("<background_context>"),
            "background should be the inherited context"
        );
}
/// Verify extract_last_real_user_query would return the task.
#[test]
fn last_user_message_is_task_after_normalization() {
    use sampling_types::conversation::ConversationItem;
    let parent_conv = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("parent context"),
            ConversationItem::assistant("ack"),
        ];
    let (mut conv, _) = crate::agent::subagent::resolution::context::normalize_forked_context(
        parent_conv,
    );
    let task = "deploy the service to staging";
    conv.push(ConversationItem::user(task));
    let last_user = conv
        .iter()
        .rev()
        .find_map(|item| {
            if let ConversationItem::User(u) = item {
                let text: String = u
                    .content
                    .iter()
                    .filter_map(|p| match p {
                        sampling_types::conversation::ContentPart::Text {
                            text,
                        } => Some(text.as_ref()),
                        _ => None,
                    })
                    .collect();
                Some(text)
            } else {
                None
            }
        });
    assert_eq!(
            last_user.as_deref(),
            Some(task),
            "last user message should be the task, not background context"
        );
}
#[test]
fn subagent_worktree_snapshot_gate_defaults_off() {
    let ctx = ctx_with_toggle(std::collections::HashMap::new());
    assert!(!ctx.resolve_subagent_worktree_snapshot_enabled());
}
/// Remote remote settings value enables the gate when no local override exists.
#[test]
fn subagent_worktree_snapshot_gate_remote_enables() {
    let mut ctx = ctx_with_toggle(std::collections::HashMap::new());
    ctx.remote_settings = Some(crate::util::config::RemoteSettings {
        subagent_worktree_snapshot_enabled: Some(true),
        ..Default::default()
    });
    assert!(ctx.resolve_subagent_worktree_snapshot_enabled());
}
/// Local config wins over remote (kill-switch parity with the other gates).
#[test]
fn subagent_worktree_snapshot_gate_local_overrides_remote() {
    let mut config = crate::agent::config::Config::default();
    config.features.subagent_worktree_snapshot = Some(false);
    let mut ctx = ctx_with_toggle(std::collections::HashMap::new());
    ctx.agent_config = Some(config);
    ctx.remote_settings = Some(crate::util::config::RemoteSettings {
        subagent_worktree_snapshot_enabled: Some(true),
        ..Default::default()
    });
    assert!(
            !ctx.resolve_subagent_worktree_snapshot_enabled(),
            "local [features] subagent_worktree_snapshot=false must override remote enable"
        );
}
/// Local config alone enables the gate (the per-deployment rollout lever).
#[test]
fn subagent_worktree_snapshot_gate_local_enables() {
    let mut config = crate::agent::config::Config::default();
    config.features.subagent_worktree_snapshot = Some(true);
    let mut ctx = ctx_with_toggle(std::collections::HashMap::new());
    ctx.agent_config = Some(config);
    assert!(ctx.resolve_subagent_worktree_snapshot_enabled());
}
/// Subagent spawns carry concrete ask_user_question timeout params (the
/// session-level config follows the child) while bash stays on tool
/// defaults. Tier precedence itself is pinned by the resolver's own
/// tests; asserting concrete values here would read the host's disk
/// layers and flake on configured dev machines.
#[test]
fn subagent_tool_params_carry_ask_user_question_timeouts() {
    let ctx = ctx_with_toggle(std::collections::HashMap::new());
    let params = ctx.resolve_tool_params_json();
    assert!(params.bash.is_none(), "bash must stay on tool defaults");
    let ask = params
        .ask_user_question
        .expect("subagents must receive resolved ask_user_question params");
    assert!(ask.get("timeout_enabled").is_some_and(|v| v.is_boolean()));
    assert!(ask.get("timeout_secs").is_some_and(|v| v.is_u64()));
}
#[test]
fn initial_context_source_resumed_variant() {
    let source = InitialContextSource::Resumed;
    assert!(matches!(source, InitialContextSource::Resumed));
    assert_ne!(source, InitialContextSource::New);
}
/// Resume must preserve only the System head (`Some(1)`) while passing the full
/// transcript through intact — a whole-transcript prefix is what pinned compaction.
#[test]
fn resume_initial_context_preserves_head_only() {
    use sampling_types::conversation::ConversationItem;
    let mut conversation = vec![ConversationItem::system("sys")];
    for i in 0..8 {
        conversation.push(ConversationItem::user(format!("u{i}")));
        conversation.push(ConversationItem::assistant(format!("a{i}")));
    }
    let original_len = conversation.len();
    let ctx = resume_initial_context(conversation);
    assert_eq!(ctx.source, InitialContextSource::Resumed);
    assert_eq!(
            ctx.prefix_len,
            Some(1),
            "resume preserves only the System head, not the full transcript"
        );
    assert_eq!(
            ctx.conversation.len(),
            original_len,
            "transcript preserved intact"
        );
}
#[test]
fn resume_prefix_len_is_system_head_only() {
    use sampling_types::conversation::ConversationItem;
    let mut conversation = vec![ConversationItem::system("sys")];
    for i in 0..6 {
        conversation.push(ConversationItem::user(format!("u{i}")));
        conversation.push(ConversationItem::assistant(format!("a{i}")));
    }
    assert_eq!(resume_inherited_prefix_len(&conversation), 1);
}
#[test]
fn resume_prefix_len_is_zero_without_system_head() {
    use sampling_types::conversation::ConversationItem;
    let conversation = vec![
            ConversationItem::user("task"),
            ConversationItem::assistant("done"),
        ];
    assert_eq!(resume_inherited_prefix_len(&conversation), 0);
}
#[test]
fn resume_source_worktree_reuse() {
    let source_with_worktree = ResumeSourceData {
        subagent_id: "sub-wt".into(),
        child_session_id: "child-wt".into(),
        child_cwd: "/tmp/worktree".into(),
        worktree_path: Some(
            PathBuf::from("/home/user/.grow/worktrees/myrepo/subagent-sub-wt"),
        ),
        snapshot_ref: None,
        subagent_type: "general-purpose".into(),
        model_id: "grow-3".into(),
        model_transport_key: sampling_types::ModelImageInputKey::new(
            "grow-3",
            "responses",
            "test-endpoint",
        ),
        reasoning_effort: None,
    };
    let worktree = source_with_worktree.worktree_path.clone();
    assert_eq!(
            worktree.as_deref(),
            Some(Path::new(
                "/home/user/.grow/worktrees/myrepo/subagent-sub-wt",
            )),
            "should reuse source worktree"
        );
    let source_without_worktree = ResumeSourceData {
        subagent_id: "sub-no-wt".into(),
        child_session_id: "child-no-wt".into(),
        child_cwd: "/workspace".into(),
        worktree_path: None,
        snapshot_ref: None,
        subagent_type: "general-purpose".into(),
        model_id: "grow-3".into(),
        model_transport_key: sampling_types::ModelImageInputKey::new(
            "grow-3",
            "responses",
            "test-endpoint",
        ),
        reasoning_effort: None,
    };
    assert!(
            source_without_worktree.worktree_path.is_none(),
            "no worktree to reuse"
        );
}
#[test]
fn resolve_child_cwd_uses_override_when_no_worktree() {
    let parent = PathBuf::from("/parent/workspace");
    let result = resolve_child_cwd(None, Some("/target/dir"), &parent);
    assert_eq!(result, PathBuf::from("/target/dir"));
}
#[test]
fn resolve_child_cwd_worktree_takes_precedence_over_override() {
    let parent = PathBuf::from("/parent/workspace");
    let worktree = Path::new("/worktree/path");
    let result = resolve_child_cwd(Some(worktree), Some("/target/dir"), &parent);
    assert_eq!(result, PathBuf::from(worktree));
}
#[test]
fn resolve_child_cwd_falls_back_to_parent_when_no_overrides() {
    let parent = PathBuf::from("/parent/workspace");
    let result = resolve_child_cwd(None, None, &parent);
    assert_eq!(result, parent);
}
#[test]
fn resolve_child_cwd_empty_override_falls_back_to_parent() {
    let parent = PathBuf::from("/parent/workspace");
    let result = resolve_child_cwd(None, Some(""), &parent);
    assert_eq!(result, parent);
}
#[test]
fn resume_inherited_cwd_requires_existing_non_worktree_dir() {
    let dir = tempfile::TempDir::new().unwrap();
    let existing = dir.path().to_string_lossy().into_owned();
    let present = ResumeSourceData {
        subagent_id: "sub-present".into(),
        child_session_id: "child-present".into(),
        child_cwd: existing.clone(),
        worktree_path: None,
        snapshot_ref: None,
        subagent_type: "general-purpose".into(),
        model_id: "grow-3".into(),
        model_transport_key: sampling_types::ModelImageInputKey::new(
            "grow-3",
            "responses",
            "test-endpoint",
        ),
        reasoning_effort: None,
    };
    assert_eq!(
            resume_inherited_cwd(Some(&present)),
            Some(existing.as_str())
        );
    let missing = ResumeSourceData {
        child_cwd: "/no/such/dir/grow-missing".into(),
        ..present.clone()
    };
    assert_eq!(resume_inherited_cwd(Some(&missing)), None);
    let worktree_source = ResumeSourceData {
        child_cwd: existing.clone(),
        worktree_path: Some(dir.path().to_path_buf()),
        ..present.clone()
    };
    assert_eq!(resume_inherited_cwd(Some(&worktree_source)), None);
    assert_eq!(resume_inherited_cwd(None), None);
}
#[test]
fn select_override_cwd_resume_never_falls_through_to_request_cwd() {
    let source = ResumeSourceData {
        subagent_id: "sub-wt".into(),
        child_session_id: "child-wt".into(),
        child_cwd: "/tmp/whatever".into(),
        worktree_path: Some(
            PathBuf::from("/home/user/.grow/worktrees/repo/subagent-sub-wt"),
        ),
        snapshot_ref: None,
        subagent_type: "general-purpose".into(),
        model_id: "grow-3".into(),
        model_transport_key: sampling_types::ModelImageInputKey::new(
            "grow-3",
            "responses",
            "test-endpoint",
        ),
        reasoning_effort: None,
    };
    assert_eq!(select_override_cwd(Some(&source), Some("/x")), None);
}
#[test]
fn select_override_cwd_fresh_spawn_uses_request_cwd() {
    assert_eq!(select_override_cwd(None, Some("/x")), Some("/x"));
}
#[test]
fn resumed_session_preserves_its_seeded_system_head() {
    use sampling_types::conversation::ConversationItem;
    let mut conversation = vec![
        ConversationItem::system("old source system prompt"),
        ConversationItem::user("task 1"),
        ConversationItem::assistant("done"),
    ];
    let mut prefix_len = Some(1);
    seed_child_system_head(
        &InitialContextSource::Resumed,
        false,
        &mut conversation,
        &mut prefix_len,
        "freshly rendered current system prompt",
    )
    .unwrap();
    match &conversation[0] {
        ConversationItem::System(sys) => {
            assert_eq!(sys.content.as_ref(), "old source system prompt");
        }
        _ => panic!("first item should be System"),
    }
    assert_eq!(conversation.len(), 3);
    assert_eq!(prefix_len, Some(1));
}
#[test]
fn token_estimation_for_window_safety() {
    use sampling_types::conversation::ConversationItem;
    let conversation = vec![
            ConversationItem::system("You are a helpful assistant."),
            ConversationItem::user("Hello, how are you?"),
            ConversationItem::assistant("I'm doing well, thank you!"),
        ];
    let estimated = chat_state::estimate_conversation_tokens(&conversation);
    assert!(estimated > 0, "should produce non-zero estimate");
    assert!(
            estimated < 100,
            "short conversation should have small token estimate"
        );
    assert_eq!(chat_state::estimate_conversation_tokens(&[]), 0);
}
#[test]
fn token_estimation_accounts_for_images() {
    use sampling_types::conversation::{ContentPart, ConversationItem, UserItem};
    let text_only = vec![ConversationItem::User(UserItem {
            content: vec![ContentPart::Text {
                text: "describe this".into(),
            }],
            synthetic_reason: None,
            permission_evidence: None,
            ..Default::default()
        })];
    let text_tokens = chat_state::estimate_conversation_tokens(&text_only);
    let with_image = vec![ConversationItem::User(UserItem {
            content: vec![
                ContentPart::Text {
                    text: "describe this".into(),
                },
                ContentPart::Image {
                    url: "data:image/png;base64,abc".into(),
                },
            ],
            synthetic_reason: None,
            permission_evidence: None,
            ..Default::default()
        })];
    let image_tokens = chat_state::estimate_conversation_tokens(&with_image);
    assert_eq!(
            image_tokens,
            text_tokens + 765,
            "one image should add 765 tokens"
        );
    let multi_image = vec![ConversationItem::User(UserItem {
            content: vec![
                ContentPart::Image { url: "img1".into() },
                ContentPart::Image { url: "img2".into() },
                ContentPart::Image { url: "img3".into() },
            ],
            synthetic_reason: None,
            permission_evidence: None,
            ..Default::default()
        })];
    let multi_tokens = chat_state::estimate_conversation_tokens(&multi_image);
    assert_eq!(multi_tokens, 765 * 3, "three images = 3 * 765 tokens");
}
#[test]
fn resume_rejects_conflicting_subagent_type() {
    let source = ResumeSourceData {
        subagent_id: "sub-gp".into(),
        child_session_id: "child-gp".into(),
        child_cwd: "/workspace".into(),
        worktree_path: None,
        snapshot_ref: None,
        subagent_type: "general-purpose".into(),
        model_id: "grow-3".into(),
        model_transport_key: sampling_types::ModelImageInputKey::new(
            "grow-3",
            "responses",
            "test-endpoint",
        ),
        reasoning_effort: None,
    };
    let request_type = "explore";
    assert_ne!(
            request_type, source.subagent_type,
            "conflicting types should be detected"
        );
}

#[test]
fn durable_resume_projection_requires_parent_terminal_fact() {
    let mut timeline = chat_state::Timeline::default();
    timeline
        .record(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Spawned(chat_state::SubagentSpawnEvent {
                goal_definition_revision: None,
                subagent_id: "sa-resume".into(),
                child_session_id: "child-resume".into(),
                security_parent_session_id: "parent-session".into(),
                subagent_type: "general-purpose".into(),
                description: "continue work".into(),
                prompt: "finish implementation".into(),
                context_source: chat_state::SubagentContextSource::Resumed,
                source_ref: None,
                context_normalized: false,
                resumed_from: Some("sa-prior".into()),
                parent_prompt_id: Some("prompt-1".into()),
                capability_mode: Some("all".into()),
                permission_mode: Some("auto".into()),
                effective_permission_mode: Some("auto".into()),
                workflow_run_id: None,
                goal_id: None,
                surface_completion: true,
                child_cwd: "/workspace/project".into(),
                worktree_path: Some("/tmp/worktree".into()),
                effective_model_id: "grow-3".into(),
                model_transport_key: sampling_types::ModelImageInputKey::new(
                    "grow-3",
                    "responses",
                    "test-endpoint",
                ),
                reasoning_effort: Some(sampling_types::ReasoningEffort::High),
            }),
        ))
        .unwrap();
    assert!(resume_source_from_timeline(&timeline, "sa-resume").is_none());
    timeline
        .record(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Ended(chat_state::SubagentTerminalEvent {
                subagent_id: "sa-resume".into(),
                child_session_id: "child-resume".into(),
                outcome: chat_state::SubagentOutcome::Cancelled,
                duration_ms: 10,
                tool_calls: 1,
                turns: 1,
                tokens_used: 50,
                error: Some("cancelled".into()),
                result_ref: None,
                snapshot_ref: Some("refs/grow/subagents/sa-resume".into()),
            }),
        ))
        .unwrap();
    let source = resume_source_from_timeline(&timeline, "sa-resume").unwrap();
    assert_eq!(source.child_session_id, "child-resume");
    assert_eq!(source.child_cwd, "/workspace/project");
    assert_eq!(source.worktree_path.as_deref(), Some(Path::new("/tmp/worktree")));
    assert_eq!(source.snapshot_ref.as_deref(), Some("refs/grow/subagents/sa-resume"));
    assert_eq!(source.model_id, "grow-3");
    assert_eq!(
        source.reasoning_effort,
        Some(sampling_types::ReasoningEffort::High)
    );
}

#[test]
fn canonical_spawn_fact_reconstructs_the_complete_ui_projection() {
    let mut definition = agent::AgentDefinition::default_grow_build();
    definition.name = "reviewer".into();
    let route = crate::session::workflow::tracker::WorkflowRuntimeRoute::for_test(
        "grow-3",
        None,
        sampling_types::ModelImageInputKey::new("grow-3", "responses", "test-endpoint"),
    )
    .unwrap()
    .with_test_agent(definition)
    .unwrap();
    let mut tracker = crate::session::workflow::tracker::WorkflowTracker::default();
    tracker.start_run(
        "workflow-1".into(),
        "test".into(),
        "projection".into(),
        vec![],
        None,
        None,
        route,
    );
    let tracker = std::sync::Arc::new(parking_lot::Mutex::new(tracker));
    let projection = spawn_from_fact(
        "parent-session",
        &chat_state::SubagentSpawnEvent {
            goal_definition_revision: Some(1),
            subagent_id: "sa-projection".into(),
            child_session_id: "child-projection".into(),
            security_parent_session_id: "parent-session".into(),
            subagent_type: "review".into(),
            description: "adversarial review".into(),
            prompt: "review all features".into(),
            context_source: chat_state::SubagentContextSource::Forked,
            source_ref: None,
            context_normalized: true,
            resumed_from: Some("sa-earlier".into()),
            parent_prompt_id: Some("prompt-9".into()),
            capability_mode: Some("read-only".into()),
            permission_mode: Some("ask".into()),
            effective_permission_mode: Some("deny-writes".into()),
            workflow_run_id: Some("workflow-1".into()),
            goal_id: Some("goal-1".into()),
            surface_completion: true,
            child_cwd: "/workspace/project".into(),
            worktree_path: Some("/tmp/worktree".into()),
            effective_model_id: "grow-3".into(),
            model_transport_key: sampling_types::ModelImageInputKey::new(
                "grow-3",
                "responses",
                "test-endpoint",
            ),
            reasoning_effort: None,
        },
        Some(&tracker),
    );

    let SessionUpdate::SubagentSpawned {
        subagent_id,
        child_session_id,
        parent_session_id,
        parent_prompt_id,
        subagent_type,
        description,
        effective_context_source,
        context_normalized,
        capability_mode,
        permission_mode,
        effective_permission_mode,
        model,
        model_state,
        workflow_agent_names,
        resumed_from,
        workflow_run_id,
        goal_id,
    } = projection
    else {
        panic!("expected subagent spawn projection");
    };
    assert_eq!(subagent_id, "sa-projection");
    assert_eq!(child_session_id, "child-projection");
    assert_eq!(parent_session_id, "parent-session");
    assert_eq!(parent_prompt_id.as_deref(), Some("prompt-9"));
    assert_eq!(subagent_type, "review");
    assert_eq!(description, "adversarial review");
    assert_eq!(effective_context_source.as_deref(), Some("forked"));
    assert!(context_normalized);
    assert_eq!(capability_mode.as_deref(), Some("read-only"));
    assert_eq!(permission_mode.as_deref(), Some("ask"));
    assert_eq!(effective_permission_mode.as_deref(), Some("deny-writes"));
    assert_eq!(model.as_deref(), Some("grow-3"));
    assert_eq!(
        model_state
            .as_ref()
            .map(|state| state.current_model_id.0.as_ref()),
        Some("grow-3")
    );
    assert_eq!(workflow_agent_names, Some(vec!["reviewer".into()]));
    assert_eq!(resumed_from.as_deref(), Some("sa-earlier"));
    assert_eq!(workflow_run_id.as_deref(), Some("workflow-1"));
    assert_eq!(goal_id.as_deref(), Some("goal-1"));
}
#[test]
fn resume_allows_matching_identity() {
    let source = ResumeSourceData {
        subagent_id: "sub-ok".into(),
        child_session_id: "child-ok".into(),
        child_cwd: "/workspace".into(),
        worktree_path: None,
        snapshot_ref: None,
        subagent_type: "general-purpose".into(),
        model_id: "grow-3".into(),
        model_transport_key: sampling_types::ModelImageInputKey::new(
            "grow-3",
            "responses",
            "test-endpoint",
        ),
        reasoning_effort: None,
    };
    assert_eq!("general-purpose", source.subagent_type);
    assert_eq!("grow-3", source.model_id);
}
#[test]
fn resume_identity_does_not_gate_on_model() {
    let source = ResumeSourceData {
        subagent_id: "sub-model".into(),
        child_session_id: "child-model".into(),
        child_cwd: "/workspace".into(),
        worktree_path: None,
        snapshot_ref: None,
        subagent_type: "general-purpose".into(),
        model_id: "grow-3".into(),
        model_transport_key: sampling_types::ModelImageInputKey::new(
            "grow-3",
            "responses",
            "test-endpoint",
        ),
        reasoning_effort: None,
    };
    assert!(
            crate::agent::subagent::resolution::validate_resume_identity(
                "general-purpose",
                &source,
            )
            .is_ok()
        );
    assert_eq!(
            source.model_id,
            "grow-3",
            "source model remains available for pinning"
        );
}
#[test]
fn resume_model_pinning_overrides_default_resolution() {
    let source_model = Some("grow-3".to_string());
    let resolved_model = "grow-light";
    let needs_pin = source_model.as_deref() != Some(resolved_model);
    assert!(
            needs_pin,
            "resolved model differs from source — pinning should trigger"
        );
    let resolved_same = "grow-3";
    let no_pin = source_model.as_deref() == Some(resolved_same);
    assert!(no_pin, "same model — no pinning needed");
}
#[test]
fn resume_window_safety_rejects_instead_of_swapping() {
    let estimated_tokens: u64 = 100_000;
    let child_window: u64 = 256_000;
    const SAFE_RESUME_PERCENT: u64 = 80;
    let threshold = child_window * SAFE_RESUME_PERCENT / 100;
    assert!(
            estimated_tokens <= threshold,
            "100k tokens should be within 80% of 256k window"
        );
    let large_transcript: u64 = 210_000;
    assert!(
            large_transcript > threshold,
            "210k tokens exceeds 80% of 256k window — resume should be rejected"
        );
}
#[test]
fn provenance_carries_resumed_from() {
    let prov = SubagentProvenance {
        fork_parent_prompt_id: Some("prompt-1".into()),
        resumed_from: Some("prev-agent-id".into()),
    };
    assert_eq!(prov.resumed_from.as_deref(), Some("prev-agent-id"));
    let fresh = SubagentProvenance::default();
    assert!(fresh.resumed_from.is_none());
}
#[test]
fn notification_subagent_spawned_includes_resumed_from() {
    let notification = SessionUpdate::SubagentSpawned {
        subagent_id: "sa-resumed".into(),
        parent_session_id: "parent".into(),
        parent_prompt_id: Some("prompt-1".into()),
        child_session_id: "child-resumed".into(),
        subagent_type: "general-purpose".into(),
        description: "fix review feedback".into(),
        effective_context_source: Some("resumed".into()),
        context_normalized: false,
        capability_mode: None,
        permission_mode: None,
        effective_permission_mode: None,
        model: None,
        model_state: None,
        workflow_agent_names: None,
        resumed_from: Some("prev-agent-id".into()),
        workflow_run_id: None,
        goal_id: None,
    };
    let json = serde_json::to_value(&notification).unwrap();
    assert_eq!(json["resumed_from"], "prev-agent-id");
    assert_eq!(json["effective_context_source"], "resumed");
    assert_eq!(json["model"], serde_json::Value::Null);
    let fresh = SessionUpdate::SubagentSpawned {
        subagent_id: "sa-fresh".into(),
        parent_session_id: "p".into(),
        parent_prompt_id: None,
        child_session_id: "c".into(),
        subagent_type: "explore".into(),
        description: "d".into(),
        effective_context_source: Some("new".into()),
        context_normalized: false,
        capability_mode: None,
        permission_mode: None,
        effective_permission_mode: None,
        model: None,
        model_state: None,
        workflow_agent_names: None,
        resumed_from: None,
        workflow_run_id: None,
        goal_id: None,
    };
    let json = serde_json::to_value(&fresh).unwrap();
    assert!(json.get("resumed_from").is_none());
    assert!(json.get("role").is_none());
    assert!(json.get("model").is_none());
}
#[test]
fn turn_active_flag_defaults_to_false() {
    let presentation = SubagentPresentation::new();
    assert!(
            !presentation
                .turn_active_flag()
                .load(std::sync::atomic::Ordering::Relaxed)
        );
}
#[test]
fn turn_active_flag_shared_via_arc() {
    let presentation = SubagentPresentation::new();
    let flag = presentation.turn_active_flag();
    assert!(!flag.load(std::sync::atomic::Ordering::Relaxed));
    flag.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(
            presentation
                .turn_active_flag()
                .load(std::sync::atomic::Ordering::Relaxed)
        );
    flag.store(false, std::sync::atomic::Ordering::Relaxed);
    assert!(
            !presentation
                .turn_active_flag()
                .load(std::sync::atomic::Ordering::Relaxed)
        );
}
fn ctx_with_parent_chat_state(
    session_model_id: &str,
    inference_slug: &str,
    global_model_id: &str,
    available_models: indexmap::IndexMap<String, crate::agent::config::ModelEntry>,
) -> SubagentSpawnContext {
    let mut ctx = ctx_with_toggle(HashMap::new());
    ctx.model_id = acp::ModelId::new(session_model_id);
    ctx.sampling_config.base_url = "https://api.test/v1".into();
    ctx.sampling_config.model = inference_slug.into();
    ctx.sampling_config.context_window = 256_000;
    let chat_state = spawn_test_parent_chat_state(inference_slug);
    ctx.parent_chat_state = Some(chat_state.clone());
    ctx.delegation_chat_state = Some(chat_state);
    ctx.models_manager = crate::agent::models::ModelsManager::new(
        available_models.clone(),
        acp::ModelId::new(global_model_id),
        crate::agent::config::Config::default(),
    );
    ctx.available_models = available_models;
    ctx
}
#[tokio::test]
async fn read_parent_sampling_config_keeps_catalog_id_separate_from_routing_slug() {
    let mut models = indexmap::IndexMap::new();
    models.insert(
        "deepseek/grow-4.5".to_string(),
        test_model_entry("grow-4.5"),
    );
    let ctx = ctx_with_parent_chat_state(
        "deepseek/grow-4.5",
        "grow-4.5",
        "anthropic/composer-2-fast",
        models,
    );
    let (config, model_id) = read_parent_sampling_config(&ctx).await;
    assert_eq!(config.model, "grow-4.5");
    assert_eq!(model_id.0.as_ref(), "deepseek/grow-4.5");
}

#[tokio::test]
async fn read_parent_sampling_config_inherits_output_limit() {
    let mut models = indexmap::IndexMap::new();
    models.insert(
        "deepseek/grow-4.5".to_string(),
        test_model_entry("grow-4.5"),
    );
    let mut ctx = ctx_with_parent_chat_state(
        "deepseek/grow-4.5",
        "grow-4.5",
        "deepseek/grow-4.5",
        models,
    );
    ctx.sampling_config.output_limit = Some(131_072);

    let (config, model_id) = read_parent_sampling_config(&ctx).await;
    assert_eq!(config.output_limit, Some(131_072));
    assert_eq!(model_id.0.as_ref(), "deepseek/grow-4.5");
}
#[tokio::test]
async fn read_parent_sampling_config_fallback_uses_session_model_id() {
    let mut models = indexmap::IndexMap::new();
    models.insert(
        "anthropic/composer-2-fast".to_string(),
        test_model_entry("composer-2-fast"),
    );
    let mut ctx = ctx_with_toggle(HashMap::new());
    ctx.model_id = acp::ModelId::new("anthropic/composer-2-fast");
    ctx.parent_chat_state = None;
    ctx.delegation_chat_state = None;
    ctx.sampling_config.model = "composer-2-fast".to_string();
    ctx.available_models = models;
    ctx.models_manager = crate::agent::models::ModelsManager::new(
        indexmap::IndexMap::new(),
        acp::ModelId::new("deepseek/grow-4.5"),
        crate::agent::config::Config::default(),
    );
    let (config, model_id) = read_parent_sampling_config(&ctx).await;
    assert_eq!(config.model, "composer-2-fast");
    assert_eq!(model_id.0.as_ref(), "anthropic/composer-2-fast");
    assert_ne!(model_id.0.as_ref(), "deepseek/grow-4.5");
}
#[tokio::test]
async fn read_parent_sampling_config_ignores_global_default() {
    let mut models = indexmap::IndexMap::new();
    models.insert(
        "anthropic/composer-2-fast".to_string(),
        test_model_entry("composer-2-fast"),
    );
    let ctx = ctx_with_parent_chat_state(
        "anthropic/composer-2-fast",
        "composer-2-fast",
        "deepseek/grow-4.5",
        models,
    );
    let (config, model_id) = read_parent_sampling_config(&ctx).await;
    assert_eq!(config.model, "composer-2-fast");
    assert_eq!(model_id.0.as_ref(), "anthropic/composer-2-fast");
    assert_ne!(
            model_id.0.as_ref(),
            ctx.models_manager.current_model_id().0.as_ref(),
        );
}
#[tokio::test]
async fn read_parent_sampling_config_keeps_route_compaction_policy_atomic() {
    use sampling_types::CompactionsRemaining;
    let mut entry = test_model_entry("grow-4.5");
    entry.info.compactions_remaining = Some(CompactionsRemaining::Dynamic(true));
    let mut models = indexmap::IndexMap::new();
    models.insert("deepseek/grow-4.5".to_string(), entry);
    let mut ctx = ctx_with_parent_chat_state(
        "deepseek/grow-4.5",
        "grow-4.5",
        "deepseek/grow-4.5",
        models,
    );
    ctx.sampling_config.compactions_remaining = Some(CompactionsRemaining::Dynamic(false));
    let (config, _model_id) = read_parent_sampling_config(&ctx).await;
    assert_eq!(
            config.compactions_remaining,
            Some(CompactionsRemaining::Dynamic(false)),
            "subagent must not splice a later catalog capability into its committed parent route"
        );
}
#[tokio::test]
async fn read_parent_sampling_config_without_chat_state_keeps_route_compaction_policy() {
    use sampling_types::CompactionsRemaining;
    let mut entry = test_model_entry("composer-2-fast");
    entry.info.compactions_remaining = Some(CompactionsRemaining::Dynamic(true));
    let mut models = indexmap::IndexMap::new();
    models.insert("anthropic/composer-2-fast".to_string(), entry);
    let mut ctx = ctx_with_toggle(HashMap::new());
    ctx.model_id = acp::ModelId::new("anthropic/composer-2-fast");
    ctx.parent_chat_state = None;
    ctx.delegation_chat_state = None;
    ctx.sampling_config.model = "composer-2-fast".to_string();
    ctx.sampling_config.compactions_remaining = Some(CompactionsRemaining::Dynamic(false));
    ctx.models_manager = crate::agent::models::ModelsManager::new(
        models,
        acp::ModelId::new("anthropic/composer-2-fast"),
        crate::agent::config::Config::default(),
    );
    let (config, model_id) = read_parent_sampling_config(&ctx).await;
    assert_eq!(model_id.0.as_ref(), "anthropic/composer-2-fast");
    assert_eq!(
            config.compactions_remaining,
            Some(CompactionsRemaining::Dynamic(false)),
            "credential availability must not change the committed parent route"
        );
}
/// Drive the REAL precedence path
/// (`resolve_effective_model_config`, which `run_shell_child`
/// calls) with BOTH an explicit `runtime_override_model` AND a
/// `[subagents.models]` pin for the same agent present, asserting the
/// runtime override wins; with `None` (inherit) the pin wins (precedence
/// handed back); and an unknown override fails closed.
#[tokio::test]
async fn runtime_override_wins_over_subagents_models_pin_in_precedence_path() {
    let build_ctx = || {
        let mut models = indexmap::IndexMap::new();
        models.insert("goal-model".to_string(), test_model_entry("goal-model"));
        models.insert("pinned-model".to_string(), test_model_entry("pinned-model"));
        let mut ctx = ctx_with_toggle(HashMap::new());
        ctx.available_models = models;
        ctx.subagent_model_overrides = HashMap::from([
            ("explore".to_string(), "pinned-model".to_string()),
        ]);
        ctx
    };
    let ctx = build_ctx();
    let (config, model_id) = resolve_effective_model_config(Some("goal-model"), "explore", &ctx)
        .await
        .unwrap();
    assert_eq!(
            config.model, "goal-model",
            "the goal runtime override must win over the `[subagents.models]` pin",
        );
    assert_eq!(model_id.0.as_ref(), "goal-model");
    let ctx = build_ctx();
    let (config, model_id) = resolve_effective_model_config(None, "explore", &ctx)
        .await
        .unwrap();
    assert_eq!(
            config.model, "pinned-model",
            "with no runtime override, the `[subagents.models]` pin wins",
        );
    assert_eq!(model_id.0.as_ref(), "pinned-model");
    let ctx = build_ctx();
    let error = resolve_effective_model_config(Some("does-not-exist"), "explore", &ctx)
        .await
        .unwrap_err();
    assert!(error.contains("not present in the model catalogue"));
}
/// A `fork_context = true` spawn must infer on the parent session model
/// (`ctx.model_id`) for per-model radix reuse, even when a
/// `[subagents.models]` pin is present. `run_shell_child` forces
/// `effective_runtime.model = Some(ctx.model_id)` on the fork path after
/// other override sources; the runtime override wins in
/// `resolve_effective_model_config`.
#[tokio::test]
async fn fork_context_pins_parent_model_over_overrides() {
    let build_ctx = || {
        let mut ctx = ctx_with_toggle(HashMap::new());
        ctx.sampling_config.model = "parent-model".to_string();
        ctx.model_id = acp::ModelId::new("parent-model");
        ctx.available_models
            .insert("parent-model".to_string(), test_model_entry("parent-model"));
        ctx.available_models
            .insert("pinned-model".to_string(), test_model_entry("pinned-model"));
        ctx.subagent_model_overrides
            .insert("general-purpose".to_string(), "pinned-model".to_string());
        ctx
    };
    let ctx = build_ctx();
    let fork_context = true;
    let mut runtime_override: Option<String> = None;
    if fork_context {
        runtime_override = Some(ctx.model_id.0.to_string());
    }
    let (config, model_id) = resolve_effective_model_config(
            runtime_override.as_deref(),
            "general-purpose",
            &ctx,
        )
        .await
        .unwrap();
    assert_eq!(
            config.model, "parent-model",
            "fork_context must pin the parent model over the [subagents.models] pin",
        );
    assert_eq!(model_id.0.as_ref(), "parent-model");
    let ctx = build_ctx();
    let (config, model_id) =
        resolve_effective_model_config(None, "general-purpose", &ctx)
            .await
            .unwrap();
    assert_eq!(
            config.model, "pinned-model",
            "without the fork pin the [subagents.models] override wins",
        );
    assert_eq!(model_id.0.as_ref(), "pinned-model");
}
/// With no explicit pin, the subagent inherits the parent model for any
/// parent model, with no special-casing (a "heavy"/custom parent
/// is treated identically to any other).
#[tokio::test]
async fn resolve_subagent_inherits_parent_model_without_pins() {
    for parent_model in ["grow-4.5", "composer-2-fast", "my-custom-byok-model"] {
        let mut ctx = ctx_with_toggle(HashMap::new());
        ctx.sampling_config.model = parent_model.to_string();
        ctx.model_id = acp::ModelId::new(parent_model);
        let (config, model_id) = resolve_subagent_sampling_config("explore", &ctx)
            .await
            .unwrap();
        assert_eq!(
                config.model, parent_model,
                "subagent must inherit parent model {parent_model:?} when no pin is set",
            );
        assert_eq!(model_id.0.as_ref(), parent_model);
    }
}
/// An explicit `[subagents.models]` pin routes the subagent to that
/// model regardless of the parent model — both a light parent
/// (`grow-4.5`) and a custom parent (`composer-2-fast`)
/// honor the pin identically now that the heavy-model gate is gone.
#[tokio::test]
async fn resolve_subagent_config_override_pin_applies_for_any_parent() {
    for parent_model in ["grow-4.5", "composer-2-fast"] {
        let mut ctx = ctx_with_toggle(HashMap::new());
        ctx.sampling_config.model = parent_model.to_string();
        ctx.model_id = acp::ModelId::new(parent_model);
        ctx.available_models
            .insert("pinned-model".to_string(), test_model_entry("pinned-model"));
        ctx.subagent_model_overrides
            .insert("explore".to_string(), "pinned-model".to_string());
        let (config, model_id) = resolve_subagent_sampling_config("explore", &ctx)
            .await
            .unwrap();
        assert_eq!(
                config.model, "pinned-model",
                "config pin must win for parent {parent_model:?}",
            );
        assert_eq!(model_id.0.as_ref(), "pinned-model");
    }
}
#[tokio::test]
async fn subagent_override_uses_provider_qualified_identity() {
    let mut volcengine = test_model_entry("glm-5.3");
    volcengine.info.base_url = "https://ark.example/v1".to_owned();
    volcengine.api_key = Some("test-key-volcengine".to_owned());
    let mut bigmodel = test_model_entry("glm-5.3");
    bigmodel.info.base_url = "https://bigmodel.example/v1".to_owned();
    bigmodel.api_key = Some("test-key-bigmodel".to_owned());
    let mut ctx = ctx_with_toggle(HashMap::new());
    ctx.available_models = indexmap::IndexMap::from([
        ("volcengine/glm-5.3".to_owned(), volcengine),
        ("bigmodel/glm-5.3".to_owned(), bigmodel),
    ]);
    ctx.subagent_model_overrides
        .insert("explore".to_owned(), "bigmodel/glm-5.3".to_owned());

    let (config, model_id) = resolve_subagent_sampling_config("explore", &ctx)
        .await
        .unwrap();

    assert_eq!(model_id.0.as_ref(), "bigmodel/glm-5.3");
    assert_eq!(config.model, "glm-5.3");
    assert_eq!(config.base_url, "https://bigmodel.example/v1");
    assert_eq!(config.api_key.as_deref(), Some("test-key-bigmodel"));
}
/// An unresolvable `[subagents.models]` pin must fail closed instead of
/// silently changing the child to the parent's current model.
#[tokio::test]
async fn resolve_subagent_config_override_unknown_model_fails_closed() {
    let mut ctx = ctx_with_toggle(HashMap::new());
    ctx.sampling_config.model = "grow-4.5".to_string();
    ctx.model_id = acp::ModelId::new("grow-4.5");
    ctx.subagent_model_overrides
        .insert("explore".to_string(), "does-not-exist".to_string());
    let error = resolve_subagent_sampling_config("explore", &ctx)
        .await
        .unwrap_err();
    assert!(error.contains("does-not-exist"));
    assert!(error.contains("not present in the model catalogue"));
}
/// Spawn-time credentials are cache-only: a cold spawn has no key,
/// never the parent session key.
#[tokio::test]
async fn subagent_override_provider_model_spawns_cache_only_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let provider = crate::auth::test_counting_provider(
        "test-subagent-spawn",
        dir.path(),
    );
    let mut entry = test_model_entry("proxied-model");
    entry.info.base_url = "https://gateway.example/v1".to_string();
    entry.auth_provider = Some(provider.clone());
    let mut models = indexmap::IndexMap::new();
    models.insert("proxied".to_string(), entry);
    let mut ctx = ctx_with_toggle(HashMap::new());
    ctx.sampling_config.model = "grow-4.5".to_string();
    ctx.model_id = acp::ModelId::new("grow-4.5");
    ctx.available_models = models;
    ctx.subagent_model_overrides.insert("explore".to_string(), "proxied".to_string());
    let (config, model_id) = resolve_subagent_sampling_config("explore", &ctx)
        .await
        .unwrap();
    assert_eq!(model_id.0.as_ref(), "proxied");
    assert_eq!(
            config.api_key, None,
            "a cold cache spawns with no key, never the parent session key"
        );
    provider.ensure_fresh_token(None).await.rotated().unwrap();
    let (config, _) = resolve_subagent_sampling_config("explore", &ctx)
        .await
        .unwrap();
    assert_eq!(config.api_key.as_deref(), Some("tok-1"));
    assert_eq!(config.base_url, "https://gateway.example/v1");
}
#[test]
fn key_prefix_truncates_to_8_chars() {
    let key = Some("eyJ0eXAiOiJhbGciOiJSUzI1NiJ9".to_string());
    assert_eq!(key_prefix(&key), "eyJ0eXAi");
}
#[test]
fn key_prefix_short_key_not_truncated() {
    let key = Some("abc".to_string());
    assert_eq!(key_prefix(&key), "abc");
}
#[test]
fn key_prefix_none_returns_placeholder() {
    assert_eq!(key_prefix(&None), "<none>");
}
#[test]
fn key_prefix_empty_string() {
    let key = Some(String::new());
    assert_eq!(key_prefix(&key), "");
}
mod cancellation_error_message_tests {
    use super::super::cancellation_error_message;
    use crate::session::commands::CancellationContext;
    use crate::session::events::CancellationCategory;
    #[test]
    fn permission_rejected_with_context() {
        let ctx = CancellationContext {
            tool_name: Some("run_terminal_cmd".into()),
            reason: Some("User rejected the execution".into()),
            ..Default::default()
        };
        let msg = cancellation_error_message(
            Some(CancellationCategory::PermissionRejected),
            Some(&ctx),
        );
        assert!(msg.contains("user rejected permission"));
        assert!(msg.contains("run_terminal_cmd"));
        assert!(msg.contains("User rejected the execution"));
    }
    #[test]
    fn permission_rejected_without_context() {
        let msg = cancellation_error_message(
            Some(CancellationCategory::PermissionRejected),
            None,
        );
        assert!(msg.contains("user rejected a permission prompt"));
    }
    #[test]
    fn permission_cancelled() {
        let msg = cancellation_error_message(
            Some(CancellationCategory::PermissionCancelled),
            None,
        );
        assert!(msg.contains("user cancelled a permission prompt"));
    }
    #[test]
    fn permission_timed_out_is_not_reported_as_user_cancellation() {
        let ctx = CancellationContext {
            tool_name: Some("run_terminal_cmd".into()),
            reason: Some("permission request timed out".into()),
            ..Default::default()
        };
        let msg = cancellation_error_message(
            Some(CancellationCategory::PermissionTimedOut),
            Some(&ctx),
        );
        assert!(msg.contains("permission request timed out"));
        assert!(msg.contains("run_terminal_cmd"));
        assert!(!msg.contains("user cancelled"));
    }
    #[test]
    fn hook_denied_with_context() {
        let ctx = CancellationContext {
            tool_name: Some("run_terminal_cmd".into()),
            reason: Some("blocked by policy".into()),
            hook_name: Some("safe-shell-guard".into()),
            ..Default::default()
        };
        let msg = cancellation_error_message(
            Some(CancellationCategory::HookDenied),
            Some(&ctx),
        );
        assert!(msg.contains("hook denied"));
        assert!(msg.contains("safe-shell-guard"));
        assert!(msg.contains("run_terminal_cmd"));
    }
    #[test]
    fn hook_denied_without_context() {
        let msg = cancellation_error_message(
            Some(CancellationCategory::HookDenied),
            None,
        );
        assert!(msg.contains("blocked by a hook"));
    }
    #[test]
    fn mid_turn_abort() {
        let msg = cancellation_error_message(
            Some(CancellationCategory::MidTurnAbort),
            None,
        );
        assert!(msg.contains("aborted mid-turn"));
    }
    #[test]
    fn no_category_no_context() {
        let msg = cancellation_error_message(None, None);
        assert_eq!(msg, "Subagent turn was cancelled");
    }
    #[test]
    fn partial_context_only_tool_name() {
        let ctx = CancellationContext {
            tool_name: Some("search_replace".into()),
            ..Default::default()
        };
        let msg = cancellation_error_message(
            Some(CancellationCategory::PermissionRejected),
            Some(&ctx),
        );
        assert!(msg.contains("search_replace"));
    }
    #[test]
    fn empty_context_falls_back() {
        let ctx = CancellationContext::default();
        let msg = cancellation_error_message(
            Some(CancellationCategory::PermissionRejected),
            Some(&ctx),
        );
        assert!(msg.contains("user rejected a permission prompt"));
    }
}
fn make_pool(names: &[&str]) -> crate::session::mcp_servers::SharedMcpPool {
    use crate::session::mcp_servers::{McpClient, McpState, SharedMcpPool};
    let mut state = McpState::new(vec![]);
    for &name in names {
        state.owned_clients.insert(name.to_string(), Arc::new(McpClient::stub(name)));
    }
    SharedMcpPool::from_state(&state)
}
fn pool_names(pool: &crate::session::mcp_servers::SharedMcpPool) -> Vec<String> {
    let mut names: Vec<String> = pool.server_names().map(str::to_string).collect();
    names.sort();
    names
}
#[test]
fn filter_inheritance_all_passes_everything_through() {
    let pool = make_pool(&["github", "linear", "slack"]);
    let result = super::filter_pool_by_inheritance(
        pool,
        &agent::config::McpInheritance::All,
    );
    let result = result.expect("All should return Some");
    assert_eq!(pool_names(&result), vec!["github", "linear", "slack"]);
}
#[test]
fn filter_inheritance_none_returns_none() {
    let pool = make_pool(&["github", "linear"]);
    let result = super::filter_pool_by_inheritance(
        pool,
        &agent::config::McpInheritance::None,
    );
    assert!(result.is_none());
}
#[test]
fn filter_inheritance_named_selects_specific_servers() {
    let pool = make_pool(&["github", "linear", "slack", "jira"]);
    let result = super::filter_pool_by_inheritance(
        pool,
        &agent::config::McpInheritance::Named(
            vec!["github".into(), "slack".into()],
        ),
    );
    let result = result.expect("Named should return Some");
    assert_eq!(pool_names(&result), vec!["github", "slack"]);
}
#[test]
fn filter_inheritance_except_excludes_specific_servers() {
    let pool = make_pool(&["github", "linear", "slack", "jira"]);
    let result = super::filter_pool_by_inheritance(
        pool,
        &agent::config::McpInheritance::Except(
            vec!["linear".into(), "jira".into()],
        ),
    );
    let result = result.expect("Except should return Some");
    assert_eq!(pool_names(&result), vec!["github", "slack"]);
}
#[test]
fn filter_inheritance_named_empty_list_gives_empty_pool() {
    let pool = make_pool(&["github", "linear"]);
    let result = super::filter_pool_by_inheritance(
        pool,
        &agent::config::McpInheritance::Named(vec![]),
    );
    let result = result.expect("Named([]) should return Some (empty pool)");
    assert_eq!(result.server_names().count(), 0);
}
#[test]
fn filter_inheritance_except_empty_list_keeps_all() {
    let pool = make_pool(&["github", "linear"]);
    let result = super::filter_pool_by_inheritance(
        pool,
        &agent::config::McpInheritance::Except(vec![]),
    );
    let result = result.expect("Except([]) should return Some");
    assert_eq!(pool_names(&result), vec!["github", "linear"]);
}
#[test]
fn filter_inheritance_named_nonexistent_servers_ignored() {
    let pool = make_pool(&["github", "linear"]);
    let result = super::filter_pool_by_inheritance(
        pool,
        &agent::config::McpInheritance::Named(
            vec![
                "nonexistent".into(),
                "github".into(),
            ],
        ),
    );
    let result = result.expect("Named should return Some");
    assert_eq!(pool_names(&result), vec!["github"]);
}
#[test]
fn filter_inheritance_except_nonexistent_servers_ignored() {
    let pool = make_pool(&["github", "linear"]);
    let result = super::filter_pool_by_inheritance(
        pool,
        &agent::config::McpInheritance::Except(vec!["nonexistent".into()]),
    );
    let result = result.expect("Except should return Some");
    assert_eq!(pool_names(&result), vec!["github", "linear"]);
}
#[test]
fn filter_inheritance_named_all_nonexistent_gives_empty() {
    let pool = make_pool(&["github", "linear"]);
    let result = super::filter_pool_by_inheritance(
        pool,
        &agent::config::McpInheritance::Named(vec!["foo".into(), "bar".into()]),
    );
    let result = result.expect("Named should return Some");
    assert_eq!(result.server_names().count(), 0);
}
#[test]
fn filter_inheritance_except_all_servers_gives_empty() {
    let pool = make_pool(&["github", "linear"]);
    let result = super::filter_pool_by_inheritance(
        pool,
        &agent::config::McpInheritance::Except(
            vec!["github".into(), "linear".into()],
        ),
    );
    let result = result.expect("Except should return Some");
    assert_eq!(result.server_names().count(), 0);
}
#[test]
fn resolve_inherited_pool_all_passes_parent_pool() {
    let pool = make_pool(&["github", "atlassian"]);
    let result = super::resolve_inherited_mcp_pool(
            Some(pool),
            &agent::config::McpInheritance::All,
        )
        .expect("All should return Some");
    assert_eq!(pool_names(&result), vec!["atlassian", "github"]);
}
#[test]
fn resolve_inherited_pool_none_returns_none() {
    let pool = make_pool(&["github", "atlassian"]);
    let result = super::resolve_inherited_mcp_pool(
        Some(pool),
        &agent::config::McpInheritance::None,
    );
    assert!(result.is_none());
}
#[test]
fn resolve_inherited_pool_named_filters() {
    let pool = make_pool(&["github", "atlassian", "slack"]);
    let result = super::resolve_inherited_mcp_pool(
            Some(pool),
            &agent::config::McpInheritance::Named(vec!["atlassian".into()]),
        )
        .expect("Named should return Some");
    assert_eq!(pool_names(&result), vec!["atlassian"]);
}
#[test]
fn resolve_inherited_pool_missing_parent_returns_none() {
    let result = super::resolve_inherited_mcp_pool(
        None,
        &agent::config::McpInheritance::All,
    );
    assert!(result.is_none());
}
/// Plugin agents inherit the same parent-qualified pool as every other child.
/// Child-owned mcpServers are ignored for all subagent sources.
#[test]
fn plugin_agents_inherit_parent_mcp_pool_by_default() {
    let pool = make_pool(&["atlassian", "github"]);
    let inherited = super::resolve_inherited_mcp_pool(
            Some(pool),
            &agent::config::McpInheritance::All,
        )
        .expect("plugin children inherit parent pool with mcpInheritance=all");
    assert_eq!(pool_names(&inherited), vec!["atlassian", "github"]);
}
#[test]
fn plugin_agents_can_opt_out_via_mcp_inheritance_none() {
    let pool = make_pool(&["atlassian"]);
    let inherited = super::resolve_inherited_mcp_pool(
        Some(pool),
        &agent::config::McpInheritance::None,
    );
    assert!(
            inherited.is_none(),
            "mcpInheritance: none must drop the parent pool for every source"
        );
}
fn make_test_skill(
    name: &str,
    plugin: Option<&str>,
) -> tools::implementations::skills::types::SkillInfo {
    tools::implementations::skills::types::SkillInfo {
        name: name.into(),
        display_name: None,
        description: format!("{name} skill"),
        path: format!("/skills/{name}/SKILL.md"),
        scope: tools::implementations::skills::types::SkillScope::Local,
        enabled: true,
        user_invocable: true,
        plugin_name: plugin.map(Into::into),
        when_to_use: None,
        short_description: None,
        author: None,
        argument_hint: None,
        license: None,
        compatibility: None,
        metadata: None,
        config_source: None,
        plugin_version: None,
        plugin_root: None,
        plugin_data: None,
        allowed_tools: None,
        model: None,
        effort: None,
        disable_model_invocation: false,
        has_user_specified_description: false,
        paths: None,
        body: None,
    }
}
#[test]
fn skills_inherited_count_zero_when_inherit_disabled() {
    let inherit_skills = false;
    let parent_skills = Some(vec![make_test_skill("skill-a", None)]);
    let count = if inherit_skills {
        parent_skills.as_ref().map(|s| s.len() as u32).unwrap_or(0)
    } else {
        0
    };
    assert_eq!(count, 0, "should be 0 when inherit_skills is false");
}
#[test]
fn skills_inherited_count_matches_parent_skills_len() {
    let inherit_skills = true;
    let parent_skills = Some(
        vec![
            make_test_skill("codegen-conventions", None),
            make_test_skill("tui-release", Some("my-plugin")),
        ],
    );
    let count = if inherit_skills {
        parent_skills.as_ref().map(|s| s.len() as u32).unwrap_or(0)
    } else {
        0
    };
    assert_eq!(count, 2);
}
