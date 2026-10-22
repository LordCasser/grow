//! Physical session-tree handoff. No business cancellation is issued here.
use super::*;
use chat_state::session_runtime::{RuntimeNodeRef, RuntimePhase, SessionRuntimeEvent};
use tools::implementations::grow_build::task::types::{FrozenChildRef, SubagentEvent};

#[derive(Default, PartialEq, Eq)]
struct CanonicalChildFacts {
    spawned: std::collections::BTreeMap<String, chat_state::SubagentSpawnEvent>,
    ended: std::collections::BTreeMap<String, chat_state::SubagentTerminalEvent>,
}

impl CanonicalChildFacts {
    fn read(events: Vec<chat_state::TimelineEvent>) -> Result<Self, String> {
        let timeline = chat_state::Timeline::from_events(events).map_err(|error| {
            format!("invalid root Timeline while reconciling children: {error}")
        })?;
        Self::from_validated_timeline(&timeline)
    }

    fn from_validated_timeline(timeline: &chat_state::Timeline) -> Result<Self, String> {
        Self::scan(timeline.events())
    }

    fn scan(events: &[chat_state::TimelineEvent]) -> Result<Self, String> {
        let mut facts = Self::default();
        for event in events {
            match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) => {
                    if facts
                        .spawned
                        .insert(spawn.subagent_id.clone(), spawn.as_ref().clone())
                        .is_some()
                    {
                        return Err(format!(
                            "root Timeline repeats child spawn {:?}",
                            spawn.subagent_id
                        ));
                    }
                }
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(
                    ended,
                )) => {
                    let spawn = facts.spawned.get(&ended.subagent_id).ok_or_else(|| {
                        format!(
                            "child terminal {:?} has no original spawn",
                            ended.subagent_id
                        )
                    })?;
                    if spawn.child_session_id != ended.child_session_id
                        || facts
                            .ended
                            .insert(ended.subagent_id.clone(), ended.clone())
                            .is_some()
                    {
                        return Err(format!(
                            "child terminal {:?} conflicts with its original spawn",
                            ended.subagent_id
                        ));
                    }
                }
                _ => {}
            }
        }
        Ok(facts)
    }

    fn open(&self) -> impl Iterator<Item = (&String, &chat_state::SubagentSpawnEvent)> {
        self.spawned
            .iter()
            .filter(|(task_id, _)| !self.ended.contains_key(*task_id))
    }
}

fn restore_tree_nodes(
    root_session_id: &str,
    runtime: &chat_state::session_runtime::RuntimeLifecycle,
    open_children: &std::collections::BTreeSet<String>,
    ended_children: &std::collections::BTreeSet<String>,
) -> Result<Vec<RuntimeNodeRef>, String> {
    let root_node = runtime
        .tree
        .iter()
        .find(|node| {
            node.session_id == root_session_id
                && Some(node.checkpoint_seq) == runtime.checkpoint_seq
        })
        .cloned()
        .ok_or("suspended root is missing its own checkpoint")?;
    let mut selected = vec![root_node];
    let mut found_open = std::collections::BTreeSet::new();
    for node in &runtime.tree {
        if node.session_id == root_session_id {
            continue;
        }
        if open_children.contains(&node.session_id) {
            found_open.insert(node.session_id.clone());
            selected.push(node.clone());
        } else if !ended_children.contains(&node.session_id) {
            return Err(format!(
                "suspended tree contains child checkpoint {} without an open spawn or terminal",
                node.session_id
            ));
        }
    }
    if found_open != *open_children {
        return Err("suspended tree is missing a checkpoint for an open child".into());
    }
    Ok(selected)
}

fn partial_tree_nodes(
    root_session_id: &str,
    operation_id: &str,
    root_runtime: &chat_state::session_runtime::RuntimeLifecycle,
    open: &std::collections::BTreeMap<String, chat_state::SubagentSpawnEvent>,
    child_runtimes: &std::collections::BTreeMap<
        String,
        chat_state::session_runtime::RuntimeLifecycle,
    >,
) -> Result<Vec<RuntimeNodeRef>, String> {
    if root_runtime.phase != RuntimePhase::Suspended
        || root_runtime.operation_id.as_deref() != Some(operation_id)
        || root_runtime.root_session_id.as_deref() != Some(root_session_id)
    {
        return Err("root runtime does not own this partial suspension".into());
    }
    let root_checkpoint = root_runtime
        .checkpoint_seq
        .ok_or("suspended root is missing its own checkpoint")?;
    if root_runtime.checkpoint.is_none() || root_runtime.commands.is_none() {
        return Err("suspended root checkpoint or command settlement is missing".into());
    }

    let mut session_to_spawn = std::collections::BTreeMap::new();
    for (task_id, spawn) in open {
        if task_id != &spawn.subagent_id || spawn.child_session_id == root_session_id {
            return Err("open child identity is inconsistent with its canonical spawn".into());
        }
        if session_to_spawn
            .insert(spawn.child_session_id.as_str(), (task_id.as_str(), spawn))
            .is_some()
        {
            return Err("multiple open child spawns claim the same session".into());
        }
    }

    for spawn in open.values() {
        if spawn.security_parent_session_id != root_session_id
            && !session_to_spawn.contains_key(spawn.security_parent_session_id.as_str())
        {
            return Err(format!(
                "open child {} has no open direct parent",
                spawn.subagent_id
            ));
        }
        let mut current = spawn.security_parent_session_id.as_str();
        let mut seen = std::collections::BTreeSet::new();
        while current != root_session_id {
            if !seen.insert(current) {
                return Err("open child ownership contains a cycle".into());
            }
            let (_, parent) = session_to_spawn.get(current).copied().ok_or_else(|| {
                format!(
                    "open child {} has an incomplete parent chain",
                    spawn.subagent_id
                )
            })?;
            current = parent.security_parent_session_id.as_str();
        }
    }

    if child_runtimes.len() != open.len() {
        return Err("not every open child has a verified checkpoint".into());
    }
    let mut nodes = vec![RuntimeNodeRef {
        session_id: root_session_id.to_owned(),
        checkpoint_seq: root_checkpoint,
    }];
    for spawn in open.values() {
        let child = child_runtimes
            .get(&spawn.child_session_id)
            .ok_or_else(|| format!("open child {} has no runtime checkpoint", spawn.subagent_id))?;
        if child.phase != RuntimePhase::Suspended
            || child.operation_id.as_deref() != Some(operation_id)
            || child.root_session_id.as_deref() != Some(root_session_id)
            || child.checkpoint.is_none()
            || child.commands.is_none()
        {
            return Err(format!(
                "open child {} has no matching checkpoint and command settlement",
                spawn.subagent_id
            ));
        }
        nodes.push(RuntimeNodeRef {
            session_id: spawn.child_session_id.clone(),
            checkpoint_seq: child.checkpoint_seq.ok_or_else(|| {
                format!(
                    "open child {} checkpoint sequence is missing",
                    spawn.subagent_id
                )
            })?,
        });
    }
    Ok(nodes)
}

fn claim_partial_child_writer(
    storage: &crate::session::storage::jsonl::JsonlStorageAdapter,
    spawn: &chat_state::SubagentSpawnEvent,
) -> Result<SessionInfo, String> {
    let info = SessionInfo {
        id: acp::SessionId::new(spawn.child_session_id.clone()),
        cwd: spawn.child_cwd.clone(),
    };
    storage.claim_recovery_writer(&info).map_err(|error| {
        format!(
            "original child {} writer is still active or unavailable: {error}",
            spawn.subagent_id
        )
    })?;
    Ok(info)
}

impl MvpAgent {
    pub(super) async fn reconcile_loaded_children(
        &self,
        session_id: &acp::SessionId,
        projections: &crate::session::storage::SubagentProjectionState,
        emit_replay: bool,
        cold_recovery: bool,
    ) {
        let Some(handle) = self.control_session_handle(session_id) else {
            return;
        };
        let mut direct_routes = std::collections::HashMap::new();
        direct_routes.extend(
            self.sessions
                .borrow()
                .iter()
                .map(|(id, handle)| (id.0.to_string(), handle.cmd_tx.clone())),
        );
        direct_routes.extend(
            self.active_child_sessions
                .borrow()
                .iter()
                .map(|(id, handle)| (id.0.to_string(), handle.cmd_tx.clone())),
        );
        crate::agent::subagent::reconcile_orphaned_subagents_with_backend(
            projections,
            emit_replay,
            cold_recovery,
            &tools::implementations::grow_build::task::backend::ChannelBackend::for_authorized_observer(
                self.subagent_event_tx.clone(),
            ),
            session_id.0.as_ref(),
            &handle.chat_state_handle,
            Some(&handle.workflow_tracker),
            &self.gateway,
            Some(&handle.cmd_tx),
            &direct_routes,
        )
        .await;
    }

    async fn verify_ended_children(
        &self,
        root_handle: &SessionHandle,
        root_session_id: &str,
        facts: &CanonicalChildFacts,
        task_ids: &std::collections::BTreeSet<String>,
        deadline: Option<tokio::time::Instant>,
    ) -> Result<std::collections::BTreeSet<String>, String> {
        let mut verified = std::collections::BTreeSet::new();
        for task_id in task_ids {
            let Some(terminal) = facts.ended.get(task_id) else {
                continue;
            };
            let spawn = facts
                .spawned
                .get(task_id)
                .ok_or_else(|| format!("ended child {task_id:?} has no original spawn"))?;
            if terminal.result_ref.is_none() {
                return Err(format!(
                    "ended child {task_id:?} has no canonical result reference"
                ));
            }
            let operation = crate::agent::subagent::durable_child_operation(
                root_session_id,
                task_id,
                &root_handle.chat_state_handle,
            );
            let operation = match deadline {
                Some(deadline) => tokio::time::timeout_at(deadline, operation)
                    .await
                    .map_err(|_| "child terminal verification exceeded suspension deadline")??,
                None => operation.await?,
            };
            match operation {
                crate::agent::subagent::DurableChildOperation::Completed(result)
                    if result.child_session_id == spawn.child_session_id =>
                {
                    verified.insert(task_id.clone());
                }
                crate::agent::subagent::DurableChildOperation::Completed(_) => {
                    return Err(format!(
                        "ended child {task_id:?} result belongs to another session"
                    ));
                }
                crate::agent::subagent::DurableChildOperation::Missing
                | crate::agent::subagent::DurableChildOperation::Open => {
                    return Err(format!(
                        "ended child {task_id:?} has no canonical completed result"
                    ));
                }
            }
        }
        Ok(verified)
    }

    async fn child_has_verified_terminal(
        &self,
        root_handle: &SessionHandle,
        root_session_id: &str,
        task_id: &str,
        deadline: tokio::time::Instant,
    ) -> Result<bool, String> {
        let events =
            tokio::time::timeout_at(deadline, root_handle.chat_state_handle.timeline_events())
                .await
                .map_err(|_| "root Timeline query exceeded suspension deadline")?
                .ok_or("root Timeline unavailable while reconciling child completion")?;
        let facts = CanonicalChildFacts::read(events)?;
        let candidates = std::collections::BTreeSet::from([task_id.to_owned()]);
        Ok(self
            .verify_ended_children(
                root_handle,
                root_session_id,
                &facts,
                &candidates,
                Some(deadline),
            )
            .await?
            .contains(task_id))
    }

    /// An executable child load belongs to the original tree. Resolve before
    /// taking lifecycle/catalog locks or claiming any child writer.
    pub(super) async fn resolve_runtime_load_root(
        &self,
        mut arguments: acp::LoadSessionRequest,
    ) -> Result<acp::LoadSessionRequest, acp::Error> {
        let requested_id = arguments.session_id.0.to_string();
        let summary = tokio::task::spawn_blocking(move || {
            crate::session::persistence::find_summary_by_session_id(&requested_id)
        })
        .await
        .map_err(|error| acp::Error::internal_error().data(error.to_string()))?;
        let Some(summary) = summary.filter(|summary| summary.parent_session_id.is_some()) else {
            return Ok(arguments);
        };
        let (child, observer) = crate::session::persistence::load_light(&summary.info, None, false)
            .await
            .map_err(|error| crate::session::persistence::io_error_to_acp(&error))?;
        drop(observer);
        let runtime = child.timeline.runtime_lifecycle();
        if runtime.phase == RuntimePhase::Running {
            return Ok(arguments);
        }
        let root_id = runtime.root_session_id.clone().ok_or_else(|| {
            acp::Error::internal_error().data("suspended child has no lifecycle root")
        })?;
        if root_id == arguments.session_id.0.as_ref() {
            return Ok(arguments);
        }
        let root_lookup = root_id.clone();
        let root_summary = tokio::task::spawn_blocking(move || {
            crate::session::persistence::find_summary_by_session_id(&root_lookup)
        })
        .await
        .map_err(|error| acp::Error::internal_error().data(error.to_string()))?
        .ok_or_else(|| {
            acp::Error::internal_error().data("suspended child's original root is missing")
        })?;
        let (root, observer) =
            crate::session::persistence::load_light(&root_summary.info, None, false)
                .await
                .map_err(|error| crate::session::persistence::io_error_to_acp(&error))?;
        drop(observer);
        let root_runtime = root.timeline.runtime_lifecycle();
        let root_identity_matches = root_runtime.root_session_id.as_deref() == Some(&root_id)
            && root_runtime.operation_id == runtime.operation_id;
        let existing_barrier_matches = root_runtime.tree.iter().any(|node| {
            node.session_id == arguments.session_id.0.as_ref()
                && Some(node.checkpoint_seq) == runtime.checkpoint_seq
        });
        let root_facts = CanonicalChildFacts::from_validated_timeline(&root.timeline)
            .map_err(|error| acp::Error::internal_error().data(error))?;
        let canonical_open_child = root_facts.open().any(|(_, spawn)| {
            spawn.child_session_id == arguments.session_id.0.as_ref()
                && spawn.child_cwd == summary.info.cwd
        });
        let partial_barrier_can_be_reconciled = root_runtime.tree.is_empty()
            && root_runtime.phase == RuntimePhase::Suspended
            && root_runtime.checkpoint.is_some()
            && root_runtime.checkpoint_seq.is_some()
            && runtime.checkpoint.is_some()
            && runtime.commands.is_some()
            && canonical_open_child;
        if !root_identity_matches
            || (!existing_barrier_matches && !partial_barrier_can_be_reconciled)
        {
            return Err(acp::Error::internal_error()
                .data("suspended child does not belong to the root checkpoint tree"));
        }
        arguments.session_id = root_summary.info.id;
        arguments.cwd = root_summary.info.cwd.into();
        Ok(arguments)
    }

    async fn runtime_snapshot(
        &self,
        handle: &SessionHandle,
    ) -> Result<
        (
            chat_state::session_runtime::RuntimeLifecycle,
            Vec<chat_state::TimelineEvent>,
        ),
        String,
    > {
        if !handle.cmd_tx.is_closed() {
            let runtime = handle.chat_state_handle.runtime_state().await?;
            let events = handle
                .chat_state_handle
                .timeline_events()
                .await
                .ok_or("runtime Timeline unavailable")?;
            return Ok((runtime, events));
        }
        let (loaded, observer) = crate::session::persistence::load_light(
            &handle.info,
            Some(self.gateway.clone()),
            false,
        )
        .await
        .map_err(|error| error.to_string())?;
        let snapshot = (
            loaded.timeline.runtime_lifecycle().clone(),
            loaded.timeline.events().to_vec(),
        );
        drop(observer);
        Ok(snapshot)
    }

    pub(super) async fn runtime_state_before(
        &self,
        handle: &SessionHandle,
        deadline: tokio::time::Instant,
    ) -> Result<chat_state::session_runtime::RuntimeLifecycle, String> {
        // Root selection and retired-node checks need no event payload copies.
        tokio::time::timeout_at(deadline, async {
            if !handle.cmd_tx.is_closed() {
                return handle.chat_state_handle.runtime_state().await;
            }
            let (loaded, observer) = crate::session::persistence::load_light(
                &handle.info,
                Some(self.gateway.clone()),
                false,
            )
            .await
            .map_err(|error| error.to_string())?;
            let runtime = loaded.timeline.runtime_lifecycle().clone();
            drop(observer);
            Ok(runtime)
        })
        .await
        .map_err(|_| "runtime state query exceeded suspension deadline".to_string())?
    }

    async fn runtime_snapshot_before(
        &self,
        handle: &SessionHandle,
        deadline: tokio::time::Instant,
    ) -> Result<
        (
            chat_state::session_runtime::RuntimeLifecycle,
            Vec<chat_state::TimelineEvent>,
        ),
        String,
    > {
        tokio::time::timeout_at(deadline, self.runtime_snapshot(handle))
            .await
            .map_err(|_| "runtime snapshot exceeded suspension deadline".to_string())?
    }

    pub(super) async fn suspend_for_exit(&self, exit_id: String) -> Result<(), String> {
        if uuid::Uuid::parse_str(&exit_id).is_err() {
            return Err("invalid exit request identity".into());
        }
        let existing = self.exit_suspend.borrow().clone();
        let response = if let Some((previous_id, previous)) = existing {
            if previous_id == exit_id || previous.peek().is_none() {
                previous
            } else {
                self.start_exit_suspension(exit_id)
            }
        } else {
            self.start_exit_suspension(exit_id)
        };
        response
            .await
            .map_err(|_| "exit suspension owner stopped before confirmation".to_string())?
    }

    fn start_exit_suspension(
        &self,
        exit_id: String,
    ) -> futures::future::Shared<oneshot::Receiver<Result<(), String>>> {
        use futures::FutureExt;
        let (respond, response) = oneshot::channel();
        let response = response.shared();
        *self.exit_suspend.borrow_mut() = Some((exit_id, response.clone()));
        let agent = LocalRef::new(self);
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        // Register this owner with MvpAgent, not with the transport waiter.
        // An RPC timeout cannot detach writer or sampler settlement ownership.
        self.spawn_owned_local(async move {
            let agent = agent.get();
            let roots = agent.sessions.borrow().keys().cloned().collect::<Vec<_>>();
            for root in &roots {
                if let Some(handle) = agent.control_session_handle(root) {
                    handle
                        .runtime_suspend
                        .store(true, std::sync::atomic::Ordering::Release);
                }
            }
            let mut failure = None;
            for root in roots {
                if tokio::time::Instant::now() >= deadline {
                    failure = Some(
                        "exit deadline elapsed; one or more roots remain unconfirmed".to_string(),
                    );
                    break;
                }
                let started = std::time::Instant::now();
                if let Err(error) = agent.suspend_session_tree(&root, deadline).await {
                    tracing::warn!(root_session_id = %root, elapsed_ms = started.elapsed().as_millis() as u64, %error, "session tree suspension remains unconfirmed");
                    failure = Some(error);
                    break;
                }
                tracing::info!(root_session_id = %root, elapsed_ms = started.elapsed().as_millis() as u64, "session tree suspension confirmed");
            }
            let _ = respond.send(failure.map_or(Ok(()), Err));
        });
        response
    }

    /// Called while load_session still owns the root lifecycle lock. Every
    /// node remains closed until all sources, routes and control links exist.
    pub(super) async fn restore_session_tree(&self, root: &acp::SessionId) -> Result<(), String> {
        use tools::implementations::grow_build::task::coordinator::ChildCheckpoint;
        use tools::implementations::grow_build::task::types::{
            SubagentExecutionRecipe, SubagentRequest,
        };
        let root_handle = self
            .control_session_handle(root)
            .ok_or("root runtime is unavailable")?;
        let mut runtime = root_handle.chat_state_handle.runtime_state().await?;
        if runtime.phase == RuntimePhase::Running {
            return Ok(());
        }
        if runtime.root_session_id.as_deref() != Some(root.0.as_ref()) {
            return Err(format!(
                "suspended execution belongs to root {}; resume that root to restore the complete tree",
                runtime.root_session_id.as_deref().unwrap_or("unknown")
            ));
        }
        if runtime.tree.is_empty() && runtime.phase == RuntimePhase::Suspended {
            self.complete_partial_tree_barrier(root, &root_handle, &runtime)
                .await?;
            runtime = root_handle.chat_state_handle.runtime_state().await?;
        }
        if runtime.tree.is_empty()
            || !matches!(
                runtime.phase,
                RuntimePhase::Suspended | RuntimePhase::Restoring
            )
        {
            return Err(
                "the original tree suspension barrier is incomplete; no node was released".into(),
            );
        }
        let operation_id = runtime
            .operation_id
            .clone()
            .ok_or("suspension operation missing")?;
        let instance_id = runtime
            .instance_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
        let events = root_handle
            .chat_state_handle
            .timeline_events()
            .await
            .ok_or("root Timeline unavailable")?;
        let facts = CanonicalChildFacts::read(events)?;
        let tree_sessions = runtime
            .tree
            .iter()
            .filter(|node| node.session_id != root.0.as_ref())
            .map(|node| node.session_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let terminal_task_ids = facts
            .ended
            .iter()
            .filter(|(_, ended)| tree_sessions.contains(&ended.child_session_id))
            .map(|(task_id, _)| task_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let verified_terminal_task_ids = self
            .verify_ended_children(
                &root_handle,
                root.0.as_ref(),
                &facts,
                &terminal_task_ids,
                None,
            )
            .await?;
        let open = facts
            .open()
            .map(|(task_id, spawn)| (task_id.clone(), spawn.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        let open_child_sessions = open
            .values()
            .map(|spawn| spawn.child_session_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let ended_child_sessions = verified_terminal_task_ids
            .iter()
            .filter_map(|task_id| facts.spawned.get(task_id))
            .map(|spawn| spawn.child_session_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let restore_nodes = restore_tree_nodes(
            root.0.as_ref(),
            &runtime,
            &open_child_sessions,
            &ended_child_sessions,
        )?;
        let mut pending = Vec::new();
        for spawn in open.into_values() {
            let node = runtime
                .tree
                .iter()
                .find(|node| node.session_id == spawn.child_session_id)
                .ok_or("suspended child is missing from root barrier")?;
            let recipe = SubagentExecutionRecipe::from_value(&spawn.execution_recipe)
                .map_err(|error| error.to_string())?;
            if recipe.owner.workflow_run_id() != spawn.workflow_run_id.as_deref()
                || recipe.owner.goal_id() != spawn.goal_id.as_deref()
                || recipe.owner.goal_definition_revision() != spawn.goal_definition_revision
            {
                return Err(format!(
                    "original child {} has inconsistent execution ownership",
                    spawn.subagent_id
                ));
            }
            let info = SessionInfo {
                id: acp::SessionId::new(spawn.child_session_id.clone()),
                cwd: spawn.child_cwd.clone(),
            };
            let (loaded, observer) =
                crate::session::persistence::load_light(&info, Some(self.gateway.clone()), false)
                    .await
                    .map_err(|error| error.to_string())?;
            let child_runtime = loaded.timeline.runtime_lifecycle().clone();
            let already_released = child_runtime.phase == RuntimePhase::Running
                && child_runtime.resume_committed
                && child_runtime.operation_id.as_ref() == Some(&operation_id)
                && child_runtime.instance_id.as_ref() == Some(&instance_id)
                && child_runtime.root_session_id.as_deref() == Some(root.0.as_ref())
                && child_runtime.checkpoint_seq == Some(node.checkpoint_seq)
                && self
                    .control_session_handle(&info.id)
                    .is_some_and(|handle| !handle.cmd_tx.is_closed());
            if already_released {
                // A lost release ACK in this process does not transfer or
                // replay the live child's already-consumed execution right.
                drop(observer);
                continue;
            }
            if !matches!(
                child_runtime.phase,
                RuntimePhase::Suspended | RuntimePhase::Restoring
            ) || child_runtime.operation_id.as_ref() != Some(&operation_id)
                || child_runtime.root_session_id.as_deref() != Some(root.0.as_ref())
                || child_runtime.checkpoint_seq != Some(node.checkpoint_seq)
                || (child_runtime.phase == RuntimePhase::Restoring
                    && child_runtime.instance_id.as_ref() != Some(&instance_id))
            {
                return Err(format!(
                    "original child {} has no matching unused checkpoint",
                    spawn.subagent_id
                ));
            }
            drop(observer);
            let request = SubagentRequest {
                id: spawn.subagent_id.clone(),
                origin_tool_call_id: spawn.origin_tool_call_id.clone(),
                prompt: spawn.prompt.clone(),
                description: spawn.description.clone(),
                subagent_type: spawn.subagent_type.clone(),
                parent_session_id: root.0.to_string(),
                parent_step_id: spawn.parent_step_id.clone(),
                parent_prompt_id: spawn.parent_prompt_id.clone(),
                resume_from: spawn.resumed_from.clone(),
                cwd: Some(spawn.child_cwd.clone()),
                runtime_overrides: recipe.runtime_overrides,
                run_in_background: recipe.run_in_background,
                surface_completion: spawn.surface_completion,
                await_to_completion: recipe.await_to_completion,
                fork_context: recipe.fork_context,
                owner: recipe.owner,
                goal_context: recipe.goal_context,
                cancel_token: tokio_util::sync::CancellationToken::new(),
            };
            pending.push((
                spawn.security_parent_session_id,
                request,
                ChildCheckpoint {
                    session_id: node.session_id.clone(),
                    operation_id: operation_id.clone(),
                    checkpoint_seq: node.checkpoint_seq.get(),
                },
            ));
        }
        self.freeze_tree_admission(
            root,
            &operation_id,
            tokio::time::Instant::now() + std::time::Duration::from_secs(5),
        )
        .await?;
        let (respond_to, response) = oneshot::channel();
        root_handle
            .cmd_tx
            .send(SessionCommand::PrepareRuntimeRestore {
                operation_id: operation_id.clone(),
                instance_id: instance_id.clone(),
                respond_to,
            })
            .map_err(|_| "root closed before restoration ownership was prepared")?;
        response
            .await
            .map_err(|_| "root preparation acknowledgement was lost")??;
        let mut installed = std::collections::BTreeSet::from([root.0.to_string()]);
        let mut installation_order = vec![root.0.to_string()];
        while !pending.is_empty() {
            let position = pending
                .iter()
                .position(|(parent, _, _)| installed.contains(parent))
                .ok_or("suspended child ownership is cyclic or incomplete")?;
            let (security_parent_session_id, request, checkpoint) = pending.remove(position);
            let child_id = checkpoint.session_id.clone();
            let existing = self.control_session_handle(&acp::SessionId::new(child_id.clone()));
            let already_installed = match existing {
                Some(handle) if !handle.cmd_tx.is_closed() => {
                    let child_runtime = handle.chat_state_handle.runtime_state().await?;
                    matches!(
                        child_runtime.phase,
                        RuntimePhase::Suspended | RuntimePhase::Restoring
                    ) && child_runtime.operation_id.as_ref() == Some(&operation_id)
                        && child_runtime.checkpoint_seq.map(|seq| seq.get())
                            == Some(checkpoint.checkpoint_seq)
                }
                _ => false,
            };
            if !already_installed {
                let (respond_to, response) = oneshot::channel();
                self.subagent_event_tx
                    .send(SubagentEvent::AdoptSuspended {
                        request: Box::new(request),
                        security_parent_session_id,
                        checkpoint,
                        respond_to,
                    })
                    .map_err(|_| "child coordinator closed during adoption")?;
                response
                    .await
                    .map_err(|_| "original child adoption acknowledgement was lost")??;
            }
            installed.insert(child_id.clone());
            installation_order.push(child_id);
        }
        let mut handles = Vec::new();
        for session_id in &installation_order {
            let node = restore_nodes
                .iter()
                .find(|node| &node.session_id == session_id)
                .ok_or("installed node has no original checkpoint")?;
            let handle = self
                .control_session_handle(&acp::SessionId::new(node.session_id.clone()))
                .ok_or_else(|| {
                    format!("restored runtime {} has no control route", node.session_id)
                })?;
            let (respond_to, response) = oneshot::channel();
            handle
                .cmd_tx
                .send(SessionCommand::PrepareRuntimeRestore {
                    operation_id: operation_id.clone(),
                    instance_id: instance_id.clone(),
                    respond_to,
                })
                .map_err(|_| "restored node closed before preparation")?;
            response
                .await
                .map_err(|_| "node preparation acknowledgement was lost")??;
            handles.push(handle);
        }
        for handle in &handles {
            let (respond_to, response) = oneshot::channel();
            handle
                .cmd_tx
                .send(SessionCommand::CommitRuntimeResume {
                    operation_id: operation_id.clone(),
                    instance_id: instance_id.clone(),
                    respond_to,
                })
                .map_err(|_| "restored node closed before resume commit")?;
            response
                .await
                .map_err(|_| "node resume commit acknowledgement was lost")??;
        }
        let (respond_to, response) = oneshot::channel();
        self.subagent_event_tx
            .send(SubagentEvent::ReleaseRestoredTree {
                root_session_id: root.0.to_string(),
                operation_id,
                respond_to,
            })
            .map_err(|_| "restored coordinator release unavailable")?;
        response
            .await
            .map_err(|_| "coordinator release acknowledgement lost")??;
        // Adopt parents before children, then release in the reverse order.
        // A released parent therefore never hides an unreleased descendant;
        // the root's durable Running state is the final acknowledgement.
        for handle in handles.iter().rev() {
            let (respond_to, response) = oneshot::channel();
            handle
                .cmd_tx
                .send(SessionCommand::ReleaseRuntimeRestore {
                    instance_id: instance_id.clone(),
                    respond_to,
                })
                .map_err(|_| "restored node closed before execution release")?;
            response
                .await
                .map_err(|_| "node release acknowledgement lost")??;
        }
        Ok(())
    }

    async fn complete_partial_tree_barrier(
        &self,
        root: &acp::SessionId,
        root_handle: &SessionHandle,
        root_runtime: &chat_state::session_runtime::RuntimeLifecycle,
    ) -> Result<(), String> {
        use tools::implementations::grow_build::task::types::SubagentExecutionRecipe;

        if root_runtime.phase != RuntimePhase::Suspended
            || !root_runtime.tree.is_empty()
            || root_runtime.root_session_id.as_deref() != Some(root.0.as_ref())
        {
            return Err("partial suspension does not have a closed root checkpoint".into());
        }
        let operation_id = root_runtime
            .operation_id
            .as_deref()
            .ok_or("partial suspension operation identity is missing")?;
        let events = root_handle
            .chat_state_handle
            .timeline_events()
            .await
            .ok_or("root Timeline unavailable while completing partial barrier")?;
        let facts = CanonicalChildFacts::read(events)?;
        let open = facts
            .open()
            .map(|(task_id, spawn)| (task_id.clone(), spawn.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();

        // A checkpoint is only a verifiable fact after the old child writer
        // has actually retired. Keep every acquired lease alive through the
        // durable root barrier acknowledgement below.
        let storage = crate::session::storage::jsonl::JsonlStorageAdapter::new();
        let mut child_runtimes = std::collections::BTreeMap::new();
        for spawn in open.values() {
            let recipe = SubagentExecutionRecipe::from_value(&spawn.execution_recipe)
                .map_err(|error| error.to_string())?;
            if recipe.owner.workflow_run_id() != spawn.workflow_run_id.as_deref()
                || recipe.owner.goal_id() != spawn.goal_id.as_deref()
                || recipe.owner.goal_definition_revision() != spawn.goal_definition_revision
            {
                return Err(format!(
                    "original child {} has inconsistent execution ownership",
                    spawn.subagent_id
                ));
            }
            let info = claim_partial_child_writer(&storage, spawn)?;
            let (loaded, observer) = crate::session::persistence::load_light(&info, None, false)
                .await
                .map_err(|error| {
                    format!(
                        "cannot verify original child {} checkpoint: {error}",
                        spawn.subagent_id
                    )
                })?;
            let child_runtime = loaded.timeline.runtime_lifecycle().clone();
            drop(observer);
            if child_runtime.phase != RuntimePhase::Suspended
                || child_runtime.operation_id.as_deref() != Some(operation_id)
                || child_runtime.root_session_id.as_deref() != Some(root.0.as_ref())
                || child_runtime.checkpoint.is_none()
                || child_runtime.checkpoint_seq.is_none()
                || child_runtime.commands.is_none()
            {
                return Err(format!(
                    "original child {} has no matching settled checkpoint",
                    spawn.subagent_id
                ));
            }
            child_runtimes.insert(spawn.child_session_id.clone(), child_runtime);
        }

        let nodes = partial_tree_nodes(
            root.0.as_ref(),
            operation_id,
            root_runtime,
            &open,
            &child_runtimes,
        )?;
        // The ChatState writer owns this exact immutable event through any
        // lost persistence ACK. Never issue a second TreeSuspended append here.
        root_handle
            .chat_state_handle
            .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
                SessionRuntimeEvent::TreeSuspended {
                    operation_id: operation_id.to_owned(),
                    nodes,
                },
            ))
            .await
            .map_err(|error| {
                format!("partial tree barrier was not durably acknowledged: {error}")
            })?;
        drop(storage);
        Ok(())
    }

    async fn freeze_tree_admission(
        &self,
        root: &acp::SessionId,
        operation_id: &str,
        deadline: tokio::time::Instant,
    ) -> Result<Vec<FrozenChildRef>, String> {
        let (respond_to, response) = oneshot::channel();
        self.subagent_event_tx
            .send(SubagentEvent::FreezeAdmission {
                root_session_id: root.0.to_string(),
                operation_id: operation_id.to_owned(),
                respond_to,
            })
            .map_err(|_| "child coordinator is unavailable".to_string())?;
        tokio::time::timeout_at(deadline, response)
            .await
            .map_err(|_| "child admission freeze is not confirmed".to_string())?
            .map_err(|_| "child admission freeze acknowledgement was lost".to_string())?
    }

    pub(super) async fn suspend_session_tree(
        &self,
        requested: &acp::SessionId,
        deadline: tokio::time::Instant,
    ) -> Result<(String, Vec<RuntimeNodeRef>), String> {
        let requested_handle = self
            .control_session_handle(requested)
            .ok_or_else(|| "session is not resident".to_string())?;
        let previous = self
            .runtime_state_before(&requested_handle, deadline)
            .await?;
        let root = acp::SessionId::new(
            previous
                .root_session_id
                .as_deref()
                .unwrap_or(&requested.0)
                .to_owned(),
        );
        let _owner = tokio::time::timeout_at(deadline, self.lock_session_lifecycle(&root))
            .await
            .map_err(|_| "session lifecycle ownership is not confirmed".to_string())?;
        let root_handle = self
            .control_session_handle(&root)
            .ok_or_else(|| "the owning root session is not resident".to_string())?;
        root_handle
            .runtime_suspend
            .store(true, std::sync::atomic::Ordering::Release);
        let (runtime, initial_facts) = self.runtime_snapshot_before(&root_handle, deadline).await?;
        let initial_open_ids = CanonicalChildFacts::read(initial_facts)?
            .open()
            .map(|(task_id, _)| task_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        if runtime.phase == RuntimePhase::Restoring {
            return Err("tree restoration still owns this runtime".into());
        }
        let operation_id = if runtime.phase == RuntimePhase::Running {
            uuid::Uuid::now_v7().to_string()
        } else {
            runtime
                .operation_id
                .clone()
                .ok_or("runtime handoff identity is missing")?
        };
        // Persist ownership before coordinator freeze can take effect. A failed
        // acknowledgement or initializer drain must retry the same operation.
        if runtime.phase == RuntimePhase::Running {
            root_handle
                .chat_state_handle
                .record_timeline_event_durably_before(
                    chat_state::TimelineEventKind::Runtime(SessionRuntimeEvent::SuspendRequested {
                        operation_id: operation_id.clone(),
                        root_session_id: root.0.to_string(),
                    }),
                    deadline,
                )
                .await
                .map_err(|error| error.to_string())?;
        }
        let frozen_children = loop {
            let children = self
                .freeze_tree_admission(&root, &operation_id, deadline)
                .await?;
            if children.iter().all(|child| !child.initializing) {
                break children;
            }
            // Already-admitted initializers still own their write-ahead Spawn.
            // Their Started ACK closes the child gate before its first prompt.
            tokio::time::timeout_at(
                deadline,
                tokio::time::sleep(std::time::Duration::from_millis(20)),
            )
            .await
            .map_err(|_| "child initialization is still settling".to_string())?;
        };
        let (_, facts) = self.runtime_snapshot_before(&root_handle, deadline).await?;
        let canonical = CanonicalChildFacts::read(facts)?;
        let frozen_ids = frozen_children
            .iter()
            .map(|child| child.task_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let mut terminal_candidates = initial_open_ids.clone();
        terminal_candidates.extend(frozen_ids.iter().cloned());
        terminal_candidates.extend(canonical.open().map(|(task_id, _)| task_id.clone()));
        let verified_terminal_ids = self
            .verify_ended_children(
                &root_handle,
                root.0.as_ref(),
                &canonical,
                &terminal_candidates,
                Some(deadline),
            )
            .await?;
        let open = canonical
            .open()
            .map(|(id, spawn)| (id.clone(), spawn.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        let children = frozen_children
            .into_iter()
            .filter(|child| open.contains_key(&child.task_id))
            .collect::<Vec<_>>();
        let mut coordinator_task_ids = frozen_ids;
        coordinator_task_ids.extend(initial_open_ids);
        coordinator_task_ids.extend(open.keys().cloned());
        for id in &coordinator_task_ids {
            if !open.contains_key(id) && !verified_terminal_ids.contains(id) {
                return Err(format!(
                    "original child {id} is absent from the current open tree and has no verified terminal"
                ));
            }
        }
        for (id, spawn) in &open {
            if !children.iter().any(|child| {
                &child.task_id == id
                    && child.session_id == spawn.child_session_id
                    && child.security_parent_session_id == spawn.security_parent_session_id
            }) {
                return Err(format!(
                    "original child {id} has not confirmed its execution ownership"
                ));
            }
        }
        // Drain descendants before parents, keeping the root accounting mailbox
        // alive to acknowledge each child's final provider settlement.
        let parents: HashMap<_, _> = children
            .iter()
            .map(|child| {
                (
                    child.session_id.clone(),
                    child.security_parent_session_id.clone(),
                )
            })
            .collect();
        let depth = |child: &FrozenChildRef| -> Result<usize, String> {
            let mut current = child.session_id.as_str();
            let mut seen = std::collections::BTreeSet::new();
            while current != root.0.as_ref() {
                if !seen.insert(current) {
                    return Err("child ownership contains a cycle".into());
                }
                current = parents.get(current).map(String::as_str).ok_or_else(|| {
                    format!("child {} has a missing direct parent", child.task_id)
                })?;
            }
            Ok(seen.len())
        };
        let mut ordered = children
            .iter()
            .map(|child| Ok((depth(child)?, child)))
            .collect::<Result<Vec<_>, String>>()?;
        ordered.sort_by_key(|(depth, _)| std::cmp::Reverse(*depth));
        let mut nodes = Vec::new();
        let mut handles = Vec::new();
        let mut root_retire_handle = None;
        for sid in ordered
            .iter()
            .map(|(_, child)| acp::SessionId::new(child.session_id.clone()))
            .chain(std::iter::once(root.clone()))
        {
            let handle = self
                .control_session_handle(&sid)
                .ok_or_else(|| format!("original runtime {sid} is unavailable"))?;
            let task_id = ordered
                .iter()
                .find(|(_, child)| child.session_id == sid.0.as_ref())
                .map(|(_, child)| child.task_id.clone());
            if handle.cmd_tx.is_closed() {
                if let Some(task_id) = task_id.as_deref()
                    && self
                        .child_has_verified_terminal(
                            &root_handle,
                            root.0.as_ref(),
                            task_id,
                            deadline,
                        )
                        .await?
                {
                    continue;
                }
                let saved = self.runtime_state_before(&handle, deadline).await?;
                if saved.phase != RuntimePhase::Suspended
                    || saved.operation_id.as_deref() != Some(&operation_id)
                {
                    return Err(format!(
                        "runtime {sid} retired without its suspension checkpoint"
                    ));
                }
                nodes.push(RuntimeNodeRef {
                    session_id: sid.0.to_string(),
                    checkpoint_seq: saved.checkpoint_seq.ok_or("saved node frontier missing")?,
                });
                continue;
            }
            let (respond_to, response) = oneshot::channel();
            if let Err(error) = handle.cmd_tx.send(SessionCommand::SuspendRuntimeNode {
                operation_id: operation_id.clone(),
                root_session_id: root.0.to_string(),
                deadline,
                respond_to,
            }) {
                if let Some(task_id) = task_id.as_deref()
                    && self
                        .child_has_verified_terminal(
                            &root_handle,
                            root.0.as_ref(),
                            task_id,
                            deadline,
                        )
                        .await?
                {
                    continue;
                }
                return Err(format!("runtime {sid} closed before checkpoint: {error}"));
            }
            let node = match tokio::time::timeout_at(deadline, response).await {
                Ok(Ok(Ok(node))) => node,
                response_result => {
                    let failure = match response_result {
                        Ok(Ok(Err(error))) => format!("runtime {sid} rejected checkpoint: {error}"),
                        Ok(Err(_)) => format!("runtime {sid} lost its checkpoint acknowledgement"),
                        Err(_) => format!("runtime {sid} suspension remains unconfirmed"),
                        Ok(Ok(Ok(_))) => unreachable!(),
                    };
                    if let Some(task_id) = task_id.as_deref()
                        && self
                            .child_has_verified_terminal(
                                &root_handle,
                                root.0.as_ref(),
                                task_id,
                                deadline,
                            )
                            .await?
                    {
                        continue;
                    }
                    return Err(failure);
                }
            };
            nodes.push(node);
            if sid == root {
                root_retire_handle = Some(handle);
            } else {
                handles.push(handle);
            }
        }
        for handle in &handles {
            let (respond_to, response) = oneshot::channel();
            handle
                .cmd_tx
                .send(SessionCommand::RetireSuspendedRuntime {
                    operation_id: operation_id.clone(),
                    deadline,
                    respond_to,
                })
                .map_err(|_| {
                    "saved runtime closed before retirement acknowledgement".to_string()
                })?;
            tokio::time::timeout_at(deadline, response)
                .await
                .map_err(|_| "tree saved; runtime retirement remains unconfirmed".to_string())?
                .map_err(|_| {
                    "tree saved; runtime retirement acknowledgement was lost".to_string()
                })??;
        }
        loop {
            let (respond_to, response) = oneshot::channel();
            self.subagent_event_tx
                .send(SubagentEvent::CheckSuspendedRetirement {
                    root_session_id: root.0.to_string(),
                    operation_id: operation_id.clone(),
                    task_ids: coordinator_task_ids.iter().cloned().collect(),
                    respond_to,
                })
                .map_err(|_| "child retirement owner is unavailable".to_string())?;
            let child_retired = tokio::time::timeout_at(deadline, response)
                .await
                .map_err(|_| "child writer retirement remains unconfirmed".to_string())?
                .map_err(|_| "child writer retirement acknowledgement was lost".to_string())??;
            if child_retired {
                break;
            }
            tokio::time::timeout_at(
                deadline,
                tokio::time::sleep(std::time::Duration::from_millis(20)),
            )
            .await
            .map_err(|_| "child writers are still retiring".to_string())?;
        }
        // Child retirement drains finalization and receipt writes. Re-read the
        // root facts only after that owner confirms completion, so a terminal
        // committed during the checkpoint race is reflected in the barrier.
        let (latest_runtime, latest_events) =
            self.runtime_snapshot_before(&root_handle, deadline).await?;
        let latest_facts = CanonicalChildFacts::read(latest_events)?;
        let latest_terminal_ids = self
            .verify_ended_children(
                &root_handle,
                root.0.as_ref(),
                &latest_facts,
                &coordinator_task_ids,
                Some(deadline),
            )
            .await?;
        let latest_open = latest_facts
            .open()
            .filter(|(task_id, _)| coordinator_task_ids.contains(*task_id))
            .map(|(task_id, spawn)| (task_id.clone(), spawn.clone()))
            .collect::<std::collections::BTreeMap<_, _>>();
        let latest_open_sessions = latest_open
            .values()
            .map(|spawn| spawn.child_session_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for task_id in &coordinator_task_ids {
            if !latest_open.contains_key(task_id) && !latest_terminal_ids.contains(task_id) {
                return Err(format!(
                    "original child {task_id} has neither a current open spawn nor a verified terminal"
                ));
            }
        }
        nodes.retain(|node| {
            node.session_id == root.0.as_ref() || latest_open_sessions.contains(&node.session_id)
        });
        if latest_open_sessions
            .iter()
            .any(|session_id| !nodes.iter().any(|node| &node.session_id == session_id))
        {
            return Err("a live child has no acknowledged suspension checkpoint".into());
        }
        if latest_runtime.tree.is_empty() {
            root_handle
                .chat_state_handle
                .record_timeline_event_durably_before(
                    chat_state::TimelineEventKind::Runtime(SessionRuntimeEvent::TreeSuspended {
                        operation_id: operation_id.clone(),
                        nodes: nodes.clone(),
                    }),
                    deadline,
                )
                .await
                .map_err(|error| error.to_string())?;
        }
        if let Some(handle) = root_retire_handle {
            let (respond_to, response) = oneshot::channel();
            handle
                .cmd_tx
                .send(SessionCommand::RetireSuspendedRuntime {
                    operation_id: operation_id.clone(),
                    deadline,
                    respond_to,
                })
                .map_err(|_| "saved root runtime closed before retirement acknowledgement")?;
            tokio::time::timeout_at(deadline, response)
                .await
                .map_err(|_| "tree saved; root runtime retirement remains unconfirmed")?
                .map_err(|_| "tree saved; root runtime retirement acknowledgement was lost")??;
        }
        loop {
            let root_thread = self.session_threads.borrow().get(&root).cloned();
            if root_thread
                .as_ref()
                .is_some_and(crate::session::SessionThread::is_finished)
            {
                root_thread
                    .expect("checked root writer")
                    .join()
                    .map_err(|_| "tree saved; original root writer panicked".to_string())?;
                self.session_threads.borrow_mut().remove(&root);
                self.take_session(&root);
                return Ok((operation_id, nodes));
            }
            tokio::time::timeout_at(
                deadline,
                tokio::time::sleep(std::time::Duration::from_millis(20)),
            )
            .await
            .map_err(|_| "tree saved; original root writer is still retiring".to_string())?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::storage::StorageAdapter;
    use chat_state::EventSeq;

    fn runtime_tree(sessions: &[(&str, u64)]) -> chat_state::session_runtime::RuntimeLifecycle {
        chat_state::session_runtime::RuntimeLifecycle {
            phase: RuntimePhase::Suspended,
            checkpoint_seq: Some(EventSeq::new(1)),
            tree: sessions
                .iter()
                .map(|(session_id, checkpoint_seq)| RuntimeNodeRef {
                    session_id: (*session_id).into(),
                    checkpoint_seq: EventSeq::new(*checkpoint_seq),
                })
                .collect(),
            ..Default::default()
        }
    }

    fn spawn(task: &str, session: &str, parent: &str) -> chat_state::SubagentSpawnEvent {
        chat_state::SubagentSpawnEvent {
            subagent_id: task.into(),
            child_session_id: session.into(),
            origin_tool_call_id: None,
            security_parent_session_id: parent.into(),
            parent_step_id: None,
            subagent_type: "general-purpose".into(),
            description: "test child".into(),
            prompt: "test".into(),
            execution_recipe: serde_json::json!({}),
            context_source: chat_state::SubagentContextSource::New,
            source_ref: None,
            context_normalized: false,
            resumed_from: None,
            parent_prompt_id: None,
            capability_mode: None,
            permission_mode: None,
            effective_permission_mode: None,
            workflow_run_id: None,
            goal_id: None,
            goal_definition_revision: None,
            surface_completion: false,
            child_cwd: "/workspace".into(),
            worktree_path: None,
            effective_model_id: "model".into(),
            model_transport_key: sampling_types::ModelImageInputKey::new(
                "model",
                "responses",
                "test-endpoint",
            ),
            reasoning_effort: None,
        }
    }

    fn read_borrowed_clone_path(
        events: &[chat_state::TimelineEvent],
    ) -> Result<CanonicalChildFacts, String> {
        chat_state::Timeline::from_events(events.to_vec()).map_err(|error| {
            format!("invalid root Timeline while reconciling children: {error}")
        })?;
        CanonicalChildFacts::scan(events)
    }

    #[test]
    #[ignore = "local clone-versus-owned Timeline measurement; no timing threshold"]
    fn canonical_child_facts_owned_read_matches_borrowed_clone_path() {
        let mut timeline = chat_state::Timeline::default();
        for _ in 0..8_000 {
            timeline
                .record(chat_state::TimelineEventKind::Observation(
                    chat_state::ObservationEvent {
                        scope: "handoff_measurement".into(),
                        name: "independent_history_event".into(),
                        turn: None,
                        step: None,
                        data: Some(serde_json::json!({ "payload": "x".repeat(4096) })),
                    },
                ))
                .unwrap();
        }
        timeline
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Spawned(Box::new(spawn(
                    "measurement-task",
                    "measurement-child",
                    "root",
                ))),
            ))
            .unwrap();
        timeline
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Ended(chat_state::SubagentTerminalEvent {
                    subagent_id: "measurement-task".into(),
                    child_session_id: "measurement-child".into(),
                    outcome: chat_state::SubagentOutcome::Cancelled,
                    duration_ms: 0,
                    tool_calls: 0,
                    turns: 0,
                    tokens_used: 0,
                    error: Some("measurement fixture".into()),
                    result_ref: None,
                    snapshot_ref: None,
                }),
            ))
            .unwrap();
        let events = timeline.events().to_vec();
        let owned_candidate_events = events.clone();
        let serialized_bytes = serde_json::to_vec(&events).unwrap().len();

        let started = std::time::Instant::now();
        let borrowed = read_borrowed_clone_path(&events).unwrap();
        let borrowed_elapsed = started.elapsed();

        let started = std::time::Instant::now();
        let owned = CanonicalChildFacts::read(owned_candidate_events).unwrap();
        let owned_elapsed = started.elapsed();

        assert_eq!(borrowed.spawned, owned.spawned);
        assert_eq!(borrowed.ended, owned.ended);
        assert!(serialized_bytes > 32 * 1024 * 1024);
        println!(
            "events={} serialized_bytes={} borrowed_clone_replay_us={} owned_replay_us={}",
            events.len(),
            serialized_bytes,
            borrowed_elapsed.as_micros(),
            owned_elapsed.as_micros()
        );
    }

    #[test]
    fn canonical_child_facts_reject_duplicate_spawn() {
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Spawned(Box::new(spawn("task", "child", "root"))),
            ))
            .unwrap();
        let mut events = timeline.events().to_vec();
        let mut duplicate = events[0].clone();
        duplicate.seq = EventSeq::new(2);
        events.push(duplicate);

        assert!(CanonicalChildFacts::read(events).is_err());
    }

    #[test]
    fn canonical_child_facts_reject_terminal_for_another_child_session() {
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Spawned(Box::new(spawn("task", "child", "root"))),
            ))
            .unwrap();
        let mut events = timeline.events().to_vec();
        let mut mismatched_terminal = events[0].clone();
        mismatched_terminal.seq = EventSeq::new(2);
        mismatched_terminal.kind = chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Ended(chat_state::SubagentTerminalEvent {
                subagent_id: "task".into(),
                child_session_id: "different-child".into(),
                outcome: chat_state::SubagentOutcome::Cancelled,
                duration_ms: 0,
                tool_calls: 0,
                turns: 0,
                tokens_used: 0,
                error: Some("test".into()),
                result_ref: None,
                snapshot_ref: None,
            }),
        );
        events.push(mismatched_terminal);

        assert!(CanonicalChildFacts::read(events).is_err());
    }

    #[test]
    fn canonical_child_facts_reject_damaged_timeline_sequence() {
        let mut timeline = chat_state::Timeline::default();
        for name in ["first", "second"] {
            timeline
                .record(chat_state::TimelineEventKind::Observation(
                    chat_state::ObservationEvent {
                        scope: "handoff_test".into(),
                        name: name.into(),
                        turn: None,
                        step: None,
                        data: None,
                    },
                ))
                .unwrap();
        }
        let mut events = timeline.events().to_vec();
        events[1].seq = events[0].seq;

        assert!(CanonicalChildFacts::read(events).is_err());
    }

    fn suspended_runtime(
        root: &str,
        operation: &str,
        seq: u64,
    ) -> chat_state::session_runtime::RuntimeLifecycle {
        chat_state::session_runtime::RuntimeLifecycle {
            phase: RuntimePhase::Suspended,
            operation_id: Some(operation.into()),
            root_session_id: Some(root.into()),
            checkpoint: Some(chat_state::session_runtime::RuntimeCheckpoint {
                frontier: EventSeq::new(seq.saturating_sub(1)),
                runtime_source_hash: "a".repeat(64),
                turn: None,
                step: None,
            }),
            checkpoint_seq: Some(EventSeq::new(seq)),
            commands: Some(Vec::new()),
            ..Default::default()
        }
    }

    #[test]
    fn restoration_ignores_checkpoint_for_durably_ended_child() {
        let runtime = runtime_tree(&[("root", 1), ("open-child", 2), ("ended-child", 3)]);
        let selected = restore_tree_nodes(
            "root",
            &runtime,
            &std::collections::BTreeSet::from(["open-child".into()]),
            &std::collections::BTreeSet::from(["ended-child".into()]),
        )
        .expect("an ended child's saved checkpoint is audit-only");

        assert_eq!(
            selected
                .iter()
                .map(|node| node.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["root", "open-child"]
        );
    }

    #[test]
    fn restoration_rejects_missing_open_checkpoint_and_unlinked_tree_node() {
        let missing_open = runtime_tree(&[("root", 1)]);
        assert!(
            restore_tree_nodes(
                "root",
                &missing_open,
                &std::collections::BTreeSet::from(["open-child".into()]),
                &std::collections::BTreeSet::new(),
            )
            .is_err()
        );

        let unknown_extra = runtime_tree(&[("root", 1), ("unknown-child", 3)]);
        assert!(
            restore_tree_nodes(
                "root",
                &unknown_extra,
                &std::collections::BTreeSet::new(),
                &std::collections::BTreeSet::new(),
            )
            .is_err()
        );
    }

    #[test]
    fn partial_barrier_can_be_reconstructed_from_all_settled_direct_children() {
        let root = suspended_runtime("root", "op", 2);
        let open = std::collections::BTreeMap::from([
            ("child-task".into(), spawn("child-task", "child", "root")),
            (
                "grandchild-task".into(),
                spawn("grandchild-task", "grandchild", "child"),
            ),
        ]);
        let children = std::collections::BTreeMap::from([
            ("child".into(), suspended_runtime("root", "op", 4)),
            ("grandchild".into(), suspended_runtime("root", "op", 6)),
        ]);

        let nodes = partial_tree_nodes("root", "op", &root, &open, &children)
            .expect("all canonical open children have settled checkpoints");
        assert_eq!(
            nodes
                .iter()
                .map(|node| (node.session_id.as_str(), node.checkpoint_seq.get()))
                .collect::<Vec<_>>(),
            vec![("root", 2), ("child", 4), ("grandchild", 6)]
        );
    }

    #[test]
    fn partial_barrier_rejects_missing_checkpoint_and_invalid_parent_topology() {
        let root = suspended_runtime("root", "op", 2);
        let open = std::collections::BTreeMap::from([
            ("child-task".into(), spawn("child-task", "child", "root")),
            (
                "grandchild-task".into(),
                spawn("grandchild-task", "grandchild", "child"),
            ),
        ]);
        let only_child = std::collections::BTreeMap::from([(
            "child".into(),
            suspended_runtime("root", "op", 4),
        )]);
        assert!(partial_tree_nodes("root", "op", &root, &open, &only_child).is_err());

        let mut orphan = open;
        orphan
            .get_mut("grandchild-task")
            .expect("fixture child")
            .security_parent_session_id = "ended-parent".into();
        let children = std::collections::BTreeMap::from([
            ("child".into(), suspended_runtime("root", "op", 4)),
            ("grandchild".into(), suspended_runtime("root", "op", 6)),
        ]);
        assert!(partial_tree_nodes("root", "op", &root, &orphan, &children).is_err());
    }

    #[tokio::test]
    async fn partial_recovery_cannot_claim_a_child_with_a_live_writer() {
        let temp = tempfile::tempdir().expect("temporary session root");
        let info = SessionInfo {
            id: acp::SessionId::new("partial-writer-child"),
            cwd: "/workspace".into(),
        };
        let old_owner = crate::session::storage::jsonl::JsonlStorageAdapter::with_root(
            temp.path().to_path_buf(),
        );
        old_owner
            .init_session(&info, crate::session::persistence::default_model_id())
            .await
            .expect("create child session");
        old_owner
            .claim_recovery_writer(&info)
            .expect("simulate the prior child writer still owning its lease");

        let recovery = crate::session::storage::jsonl::JsonlStorageAdapter::with_root(
            temp.path().to_path_buf(),
        );
        let spawn = spawn("child-task", info.id.0.as_ref(), "root");
        assert!(claim_partial_child_writer(&recovery, &spawn).is_err());
    }
}
