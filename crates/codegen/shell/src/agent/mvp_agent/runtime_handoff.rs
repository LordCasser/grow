//! Physical session-tree handoff. No business cancellation is issued here.
use super::*;
use crate::session::wait_before;
use chat_state::session_runtime::{RuntimeNodeRef, RuntimePhase, SessionRuntimeEvent};
use tools::implementations::grow_build::task::types::{FrozenChildRef, SubagentEvent};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ExitScope {
    client_id: u64,
    session_ids: Vec<String>,
}

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
        historical_workflows: &[(String, u64)],
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
            historical_workflows,
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
            let operation = wait_before(deadline, operation)
                .await
                .map_err(|_| "child terminal verification exceeded suspension deadline")??;
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
        deadline: Option<tokio::time::Instant>,
    ) -> Result<bool, String> {
        let events = wait_before(deadline, root_handle.chat_state_handle.timeline_events())
            .await
            .map_err(|_| "root Timeline query exceeded suspension deadline")?
            .ok_or("root Timeline unavailable while reconciling child completion")?;
        let facts = CanonicalChildFacts::read(events)?;
        let candidates = std::collections::BTreeSet::from([task_id.to_owned()]);
        Ok(self
            .verify_ended_children(root_handle, root_session_id, &facts, &candidates, deadline)
            .await?
            .contains(task_id))
    }

    /// An executable child load belongs to the original tree. Resolve before
    /// taking lifecycle/catalog locks or claiming any child writer.
    async fn resolve_original_workspace_alias(
        &self,
        mut arguments: acp::LoadSessionRequest,
        info: &SessionInfo,
    ) -> Result<acp::LoadSessionRequest, acp::Error> {
        if !crate::session::storage::same_workspace_directory(
            std::path::Path::new(&info.cwd),
            &arguments.cwd,
        )
        .await
        .map_err(|error| crate::session::persistence::io_error_to_acp(&error))?
        {
            return Err(acp::Error::invalid_params()
                .data("Session workspace differs from its recorded directory."));
        }
        arguments.cwd = std::path::PathBuf::from(&info.cwd);
        Ok(arguments)
    }

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
        let Some(summary) = summary else {
            return Ok(arguments);
        };
        if summary.parent_session_id.is_none() {
            return self
                .resolve_original_workspace_alias(arguments, &summary.info)
                .await;
        }
        let (child, observer) = crate::session::persistence::load_light(&summary.info, None, false)
            .await
            .map_err(|error| crate::session::persistence::io_error_to_acp(&error))?;
        drop(observer);
        let runtime = child.timeline.runtime_lifecycle();
        if runtime.phase == RuntimePhase::Running {
            return self
                .resolve_original_workspace_alias(arguments, &summary.info)
                .await;
        }
        let root_id = runtime.root_session_id.clone().ok_or_else(|| {
            acp::Error::internal_error().data("suspended child has no lifecycle root")
        })?;
        if root_id == arguments.session_id.0.as_ref() {
            return self
                .resolve_original_workspace_alias(arguments, &summary.info)
                .await;
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
            CanonicalChildFacts,
        ),
        String,
    > {
        if !handle.cmd_tx.is_closed() {
            let (runtime, events) = handle.chat_state_handle.runtime_tree_snapshot().await?;
            // The canonical owner already validated this source. This is the
            // complete Spawned/Ended projection, not a standalone ledger.
            return Ok((runtime, CanonicalChildFacts::scan(&events)?));
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
            CanonicalChildFacts::from_validated_timeline(&loaded.timeline)?,
        );
        drop(observer);
        Ok(snapshot)
    }

    pub(super) async fn runtime_state_before(
        &self,
        handle: &SessionHandle,
        deadline: impl Into<Option<tokio::time::Instant>>,
    ) -> Result<chat_state::session_runtime::RuntimeLifecycle, String> {
        // Root selection and retired-node checks need no event payload copies.
        wait_before(deadline.into(), async {
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
        deadline: Option<tokio::time::Instant>,
    ) -> Result<
        (
            chat_state::session_runtime::RuntimeLifecycle,
            CanonicalChildFacts,
        ),
        String,
    > {
        wait_before(deadline, self.runtime_snapshot(handle))
            .await
            .map_err(|_| "runtime snapshot exceeded suspension deadline".to_string())?
    }

    pub(super) async fn suspend_for_exit(
        &self,
        exit_id: String,
        scope: Option<ExitScope>,
    ) -> Result<(), String> {
        if uuid::Uuid::parse_str(&exit_id).is_err() {
            return Err("invalid exit request identity".into());
        }
        let owner = scope.as_ref().map(|scope| scope.client_id);
        let existing = self.exit_suspend.borrow().get(&owner).cloned();
        let response = if let Some((previous_id, previous)) = existing {
            if previous_id == exit_id
                || futures::FutureExt::now_or_never(previous.clone()).is_none()
            {
                previous
            } else {
                self.start_exit_suspension(exit_id, scope)
            }
        } else {
            self.start_exit_suspension(exit_id, scope)
        };
        response
            .await
            .map_err(|_| "exit suspension owner stopped before confirmation".to_string())?
    }

    fn start_exit_suspension(
        &self,
        exit_id: String,
        scope: Option<ExitScope>,
    ) -> futures::future::Shared<oneshot::Receiver<Result<(), String>>> {
        use futures::FutureExt;
        let (respond, response) = oneshot::channel();
        let response = response.shared();
        let owner = scope.as_ref().map(|scope| scope.client_id);
        self.exit_suspend
            .borrow_mut()
            .insert(owner, (exit_id, response.clone()));
        let agent = LocalRef::new(self);
        // Keep writer and sampler settlement owned by MvpAgent even if
        // the transport disconnects while the operation is still running.
        self.spawn_owned_local(async move {
            let agent = agent.get();
            let roots = match scope {
                Some(scope) => scope.session_ids.into_iter().map(acp::SessionId::new).collect(),
                None => agent.sessions.borrow().keys().chain(agent.session_threads.borrow().keys())
                    .cloned().collect::<std::collections::HashSet<_>>().into_iter().collect::<Vec<_>>(),
            };
            for root in &roots {
                if let Some(handle) = agent.control_session_handle(root) {
                    handle
                        .runtime_suspend
                        .store(true, std::sync::atomic::Ordering::Release);
                }
            }
            let mut failure = None;
            for root in roots {
                let started = std::time::Instant::now();
                if let Err(error) = agent.suspend_exit_root(&root).await {
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
            crate::agent::subagent::resolution::restored_birth_capability(
                spawn.capability_mode.as_deref(),
            )
            .map_err(|error| format!("original child {}: {error}", spawn.subagent_id))?;
            if spawn.birth_mcp_servers.is_none() {
                return Err(format!(
                    "original child {} is missing birth MCP membership",
                    spawn.subagent_id
                ));
            }
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
            Some(tokio::time::Instant::now() + std::time::Duration::from_secs(5)),
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
        deadline: Option<tokio::time::Instant>,
    ) -> Result<Vec<FrozenChildRef>, String> {
        let (respond_to, response) = oneshot::channel();
        self.subagent_event_tx
            .send(SubagentEvent::FreezeAdmission {
                root_session_id: root.0.to_string(),
                operation_id: operation_id.to_owned(),
                respond_to,
            })
            .map_err(|_| "child coordinator is unavailable".to_string())?;
        wait_before(deadline, response)
            .await
            .map_err(|_| "child admission freeze is not confirmed".to_string())?
            .map_err(|_| "child admission freeze acknowledgement was lost".to_string())?
    }

    #[cfg(test)]
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
        let _owner = wait_before(Some(deadline), self.lock_session_lifecycle(&root))
            .await
            .map_err(|_| "session lifecycle ownership is not confirmed".to_string())?;
        self.suspend_root_tree_locked(&root, Some(deadline)).await
    }

    async fn suspend_exit_root(&self, root: &acp::SessionId) -> Result<(), String> {
        let _owner = self.lock_session_lifecycle(root).await;
        if self.control_session_handle(root).is_some() {
            self.suspend_root_tree_locked(root, None).await?;
        } else {
            // Explicit close may already have removed the actor while retaining
            // its writer. The same lifecycle guard must prove drain completion.
            let draining = self.session_threads.borrow().contains_key(root);
            let exit = self.drain_old_session_thread_before(root, None).await?;
            if draining {
                let state = if exit == SessionThreadExit::Panicked {
                    SessionLiveState::DeadFailed
                } else {
                    SessionLiveState::Completed
                };
                self.remove_session_terminal(root, state);
            }
        }
        Ok(())
    }

    async fn suspend_root_tree_locked(
        &self,
        root: &acp::SessionId,
        deadline: Option<tokio::time::Instant>,
    ) -> Result<(String, Vec<RuntimeNodeRef>), String> {
        let started = std::time::Instant::now();
        let root = root.clone();
        let root_handle = self
            .control_session_handle(&root)
            .ok_or_else(|| "the owning root session is not resident".to_string())?;
        root_handle
            .runtime_suspend
            .store(true, std::sync::atomic::Ordering::Release);
        let (runtime, initial_facts) = self.runtime_snapshot_before(&root_handle, deadline).await?;
        let initial_open_ids = initial_facts
            .open()
            .map(|(task_id, _)| task_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        tracing::info!(root_session_id = %root, phase = "initial_facts", elapsed_ms = started.elapsed().as_millis() as u64, "tree suspension progress");
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
            wait_before(
                deadline,
                tokio::time::sleep(std::time::Duration::from_millis(20)),
            )
            .await
            .map_err(|_| "child initialization is still settling".to_string())?;
        };
        let (_, canonical) = self.runtime_snapshot_before(&root_handle, deadline).await?;
        tracing::info!(root_session_id = %root, phase = "frozen_facts", elapsed_ms = started.elapsed().as_millis() as u64, "tree suspension progress");
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
                deadline,
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
            let node = match wait_before(deadline, response).await {
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
            wait_before(deadline, response)
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
            let child_retired = wait_before(deadline, response)
                .await
                .map_err(|_| "child writer retirement remains unconfirmed".to_string())?
                .map_err(|_| "child writer retirement acknowledgement was lost".to_string())??;
            if child_retired {
                break;
            }
            wait_before(
                deadline,
                tokio::time::sleep(std::time::Duration::from_millis(20)),
            )
            .await
            .map_err(|_| "child writers are still retiring".to_string())?;
        }
        tracing::info!(root_session_id = %root, phase = "children_retired", elapsed_ms = started.elapsed().as_millis() as u64, "tree suspension progress");
        // Child retirement drains finalization and receipt writes. Re-read the
        // root facts only after that owner confirms completion, so a terminal
        // committed during the checkpoint race is reflected in the barrier.
        let (latest_runtime, latest_facts) =
            self.runtime_snapshot_before(&root_handle, deadline).await?;
        tracing::info!(root_session_id = %root, phase = "latest_facts", elapsed_ms = started.elapsed().as_millis() as u64, "tree suspension progress");
        let latest_terminal_ids = self
            .verify_ended_children(
                &root_handle,
                root.0.as_ref(),
                &latest_facts,
                &coordinator_task_ids,
                deadline,
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
            wait_before(deadline, response)
                .await
                .map_err(|_| "tree saved; root runtime retirement remains unconfirmed")?
                .map_err(|_| "tree saved; root runtime retirement acknowledgement was lost")??;
        }
        let root_thread = self
            .session_threads
            .borrow()
            .get(&root)
            .cloned()
            .ok_or("tree saved; original root writer is unavailable")?;
        loop {
            if root_thread.is_finished() {
                root_thread
                    .join()
                    .map_err(|_| "tree saved; original root writer panicked".to_string())?;
                self.session_threads.borrow_mut().remove(&root);
                self.take_session(&root);
                return Ok((operation_id, nodes));
            }
            wait_before(
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
            birth_mcp_servers: Some(Default::default()),
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

    #[test]
    #[ignore = "real long-history ACP exit measurement; runs in an isolated home"]
    fn long_history_exit_through_acp() {
        const TEST: &str =
            "agent::mvp_agent::runtime_handoff::tests::long_history_exit_through_acp";
        const HOME_ENV: &str = "GROW_TEST_EXIT_HISTORY_HOME";
        let Ok(home) = std::env::var(HOME_ENV) else {
            let temp = tempfile::tempdir().unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    TEST,
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("HOME", temp.path())
                .env("GROW_HOME", temp.path())
                .env(HOME_ENV, temp.path())
                .output()
                .unwrap();
            println!("{}", String::from_utf8_lossy(&output.stdout));
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            assert!(output.status.success(), "long-history subprocess failed");
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
            return;
        };
        super::super::tests::run_local_for_bridge_test(|| async move {
            use acp_transport::AcpAgentHandler as _;
            let _ = tracing_subscriber::fmt()
                .with_test_writer()
                .with_env_filter("shell::agent::mvp_agent=info,shell::session=info")
                .try_init();
            let cwd = std::path::Path::new(&home).join("workspace");
            std::fs::create_dir_all(&cwd).unwrap();
            let raw: toml::Value = toml::from_str(
                r#"
[models]
default = "test/default"
[provider.test]
api_backend = "chat_completions"
[provider.test.options]
base_url = "http://127.0.0.1:9/v1"
api_key = "test-only"
[provider.test.models.default]
context_window = 128000
"#,
            )
            .unwrap();
            let mut cfg = crate::agent::config::Config::new_from_toml_cfg(&raw).unwrap();
            cfg.default_model_override = None;
            cfg.remote_settings = Some(crate::util::config::RemoteSettings::default());
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::task::spawn_local(async move {
                while let Some(message) = rx.recv().await {
                    match message {
                        acp_transport::AcpClientMessage::SessionNotification(args) => {
                            let _ = args.response_tx.send(Ok(()));
                        }
                        acp_transport::AcpClientMessage::ExtNotification(args) => {
                            let _ = args.response_tx.send(Ok(()));
                        }
                        _ => {}
                    }
                }
            });
            let agent = MvpAgent::new(GatewaySender::new(tx), &cfg).unwrap();
            agent.initialize(acp::InitializeRequest::new(acp::ProtocolVersion::V1).meta(
                serde_json::json!({"startupHints":{"nonInteractive":true,"skipGitStatus":true,"skipProjectLayout":true}}).as_object().cloned()
            )).await.unwrap();
            agent.set_auth_method(acp::AuthMethodId::new("provider.api_key"));
            let sid = agent
                .new_session(acp::NewSessionRequest::new(cwd))
                .await
                .unwrap()
                .session_id;
            let handle = agent.control_session_handle(&sid).unwrap();
            let count = std::env::var("GROW_TEST_EXIT_HISTORY_EVENTS")
                .map(|value| value.parse::<usize>().unwrap())
                .unwrap_or(8_000);
            let payload = "x".repeat(4096);
            let setup = std::time::Instant::now();
            for _ in 0..count {
                handle
                    .chat_state_handle
                    .push_user_message(sampling_types::ConversationItem::user(&payload));
            }
            // Same mailbox: this durable barrier follows all ordinary history
            // appends, so fixture setup cannot be mistaken for exit failure.
            handle
                .chat_state_handle
                .record_timeline_event_durably(chat_state::TimelineEventKind::Observation(
                    chat_state::ObservationEvent {
                        scope: "exit_history_fixture".into(),
                        name: "ready".into(),
                        turn: None,
                        step: None,
                        data: None,
                    },
                ))
                .await
                .unwrap();
            let events = handle.chat_state_handle.timeline_events().await.unwrap();
            let bytes = serde_json::to_vec(&events).unwrap().len();
            let event_count = events.len();
            assert!(
                event_count >= count,
                "all history appends must reach the canonical owner"
            );
            drop(events);
            println!(
                "exit_history ready input_events={count} canonical_events={event_count} serialized_bytes={bytes} setup_ms={}",
                setup.elapsed().as_millis()
            );
            let started = std::time::Instant::now();
            let response = agent
                .ext_method(acp::ExtRequest::new(
                    "grow/session/suspend_for_exit",
                    serde_json::value::to_raw_value(
                        &serde_json::json!({"exit_id":uuid::Uuid::now_v7().to_string()}),
                    )
                    .unwrap()
                    .into(),
                ))
                .await
                .unwrap();
            let elapsed = started.elapsed();
            let result: serde_json::Value = serde_json::from_str(response.0.get()).unwrap();
            println!(
                "exit_history result={result} elapsed_ms={} former_deadline_ms=5000",
                elapsed.as_millis()
            );
            assert_eq!(
                result["status"], "suspended",
                "actual long-history exit remained unconfirmed: {result}"
            );
            assert!(
                agent.control_session_handle(&sid).is_none(),
                "actor must retire"
            );
            assert!(
                !agent.session_threads.borrow().contains_key(&sid),
                "writer must retire"
            );
            let (loaded, observer) =
                crate::session::persistence::load_light(&handle.info, None, false)
                    .await
                    .unwrap();
            assert_eq!(
                loaded.timeline.runtime_lifecycle().phase,
                RuntimePhase::Suspended
            );
            assert_eq!(loaded.timeline.runtime_lifecycle().tree.len(), 1);
            assert!(loaded.timeline.events().len() > event_count);
            drop(observer);
            assert!(handle.cmd_tx.is_closed());
            let (cold_runtime, cold_facts) = agent.runtime_snapshot(&handle).await.unwrap();
            assert_eq!(cold_runtime, *loaded.timeline.runtime_lifecycle());
            assert!(cold_facts.spawned.is_empty());
            // The disposable session has no remaining writer. Even this
            // known-handle branch must validate fresh storage on every read.
            use std::io::Write as _;
            std::fs::OpenOptions::new()
                .append(true)
                .open(crate::session::persistence::session_dir(&handle.info).join("timeline.jsonl"))
                .unwrap()
                .write_all(b"not-a-json-record\n")
                .unwrap();
            assert!(agent.runtime_snapshot(&handle).await.is_err());
        });
    }

    fn complex_exit_history(
        session_id: &str,
        exchanges: usize,
        result_bytes: usize,
    ) -> (chat_state::Timeline, Vec<chat_state::SidebandTimeline>) {
        use chat_state::{
            MessageCause, MessageEvent, RequestEvent, StepEvent, TimelineEventKind as K, TurnEvent,
        };
        use sampling_types::ConversationItem;
        let mut timeline = chat_state::Timeline::default();
        timeline
            .append(
                ConversationItem::system("complex exit canonical fixture"),
                MessageCause::Seed,
            )
            .unwrap();
        for index in 0..exchanges {
            let turn = chat_state::TurnId(index as u64 + 1);
            let step = chat_state::StepId { turn, index: 0 };
            let request = format!("complex-request-{index}");
            let call = format!("complex-call-{index}");
            timeline
                .record(K::Turn(TurnEvent::Started {
                    id: turn,
                    prompt_id: format!("complex-prompt-{index}"),
                    input_ids: vec![],
                    identity: chat_state::TurnIdentity {
                        origin: "user".into(),
                        turn_kind: "internal".into(),
                        goal_id: None,
                        goal_definition_revision: None,
                        stage_id: None,
                    },
                    model_id: "test/default".into(),
                    input_message_count: timeline.surface_len(),
                    prompt_index: index,
                    prompt_text: format!("fixture task {index}"),
                    input_kind: chat_state::TurnInputKind::Prompt,
                    redirect_kind: None,
                }))
                .unwrap();
            timeline
                .append(
                    ConversationItem::user(format!("fixture task {index}")),
                    MessageCause::User,
                )
                .unwrap();
            timeline
                .record(K::Step(StepEvent::Started { id: step }))
                .unwrap();
            timeline
                .record(K::Request(RequestEvent::Started {
                    id: request.clone(),
                    turn,
                    step,
                    model_id: "test/default".into(),
                    input_message_count: timeline.surface_len(),
                    tool_count: 1,
                }))
                .unwrap();
            timeline
                .record(K::Request(RequestEvent::Completed {
                    id: request.clone(),
                    duration_ms: 1,
                    time_to_first_token_ms: None,
                    usage: Default::default(),
                    response_message_count: 1,
                    attempt: 1,
                    provider_terminal: None,
                }))
                .unwrap();
            timeline
                .record(K::Messages(MessageEvent {
                    cause: MessageCause::Assistant,
                    items: vec![ConversationItem::assistant_tool_calls(vec![
                        sampling_types::ToolCall {
                            id: call.clone().into(),
                            name: "read_file".into(),
                            arguments: "{\"path\":\"fixture.txt\"}".into(),
                        },
                    ])],
                    surface: chat_state::SurfaceOp::Append,
                    response_admission: Some(chat_state::ResponseAdmission {
                        identity: chat_state::ResponseAdmissionIdentity {
                            request_id: request,
                            attempt: 1,
                        },
                        quarantined_tool_exchanges: 0,
                    }),
                }))
                .unwrap();
            timeline
                .append(
                    ConversationItem::tool_result(
                        call,
                        format!("fixture result {index}\n{}", "x".repeat(result_bytes)),
                    ),
                    MessageCause::ToolResult,
                )
                .unwrap();
            timeline
                .record(K::Step(StepEvent::Ended {
                    id: step,
                    outcome: "completed".into(),
                    duration_ms: 1,
                }))
                .unwrap();
            timeline
                .record(K::Turn(TurnEvent::Ended {
                    id: turn,
                    outcome: "completed".into(),
                    duration_ms: 1,
                    tool_count: 1,
                    terminal: chat_state::TurnTerminal {
                        source: chat_state::TurnTerminalSource::Host,
                        stop_reason: "end_turn".into(),
                        completion_kind: "completed".into(),
                    },
                    cancellation_category: None,
                    details: None,
                }))
                .unwrap();
        }
        // Genuine duplicate-result repair: retain every original append and
        // record the repaired Surface as a new canonical replacement fact.
        for _ in 0..17 {
            let duplicate = timeline.surface().last().unwrap().clone();
            assert!(matches!(duplicate, ConversationItem::ToolResult(_)));
            timeline
                .append(duplicate, MessageCause::ToolResult)
                .unwrap();
            let mut repaired = timeline.surface().to_vec();
            sampling_types::dedup_duplicate_tool_results(&mut repaired);
            assert_eq!(repaired.len() + 1, timeline.surface_len());
            timeline
                .replace_all(repaired, MessageCause::IntegrityRepair)
                .unwrap();
        }
        // All summary evidence is generated in this disposable fixture. No
        // external root, child tree, or provider is consulted.
        let mut sidebands = Vec::new();
        for index in 0..31 {
            let id = format!("fixture-compaction-{index}");
            timeline
                .record(K::Compaction(chat_state::CompactionEvent::Started {
                    id: id.clone(),
                    mode: chat_state::CompactionMode::Foreground,
                    source_items: 3,
                    prompt_index: exchanges - 1,
                }))
                .unwrap();
            let input_ref = chat_state::TimelineRangeRef {
                timeline_id: session_id.into(),
                first_seq: 0,
                last_seq: timeline.next_seq().get() - 1,
            };
            let sideband_id = uuid::Uuid::now_v7().to_string();
            timeline
                .record(K::Sideband(chat_state::SidebandSpawnEvent {
                    sideband_id: sideband_id.clone(),
                    purpose: chat_state::SidebandPurpose::CompactionSummary,
                    source_refs: vec![input_ref.clone()],
                }))
                .unwrap();
            let raw_output = format!("fixture compacted exchange {index}");
            let mut sideband = chat_state::SidebandTimeline::new(sideband_id.clone()).unwrap();
            for kind in [
                chat_state::SidebandEventKind::Request(chat_state::SidebandRequest {
                    purpose: chat_state::SidebandPurpose::CompactionSummary,
                    prompt: "summarize the fixture exchange".into(),
                    source_refs: vec![input_ref.clone()],
                    budget_policy: chat_state::SidebandBudgetPolicy {
                        max_attempts: 1,
                        max_input_tokens_per_attempt: 1,
                        max_output_tokens_per_attempt: Some(1),
                    },
                    route: chat_state::SidebandRoute {
                        model: "test".into(),
                        backend: sampling_types::ApiBackend::ChatCompletions,
                    },
                    initiator_ref: format!("t:{session_id}/sideband:{sideband_id}"),
                    executor: "main".into(),
                    output_schema: None,
                }),
                chat_state::SidebandEventKind::Attempt(chat_state::SidebandAttempt {
                    attempt_no: 1,
                    input_refs: vec![input_ref.clone()],
                    assembly_manifest: chat_state::SidebandAssemblyManifest {
                        strategy: "all-sources".into(),
                        strategy_version: 1,
                        source_revision: Some(1),
                        context_surface_ids: vec![],
                        selected_surface_ids: vec![],
                        materialized_input_tokens: 1,
                        max_output_tokens: Some(1),
                    },
                    feedback: None,
                }),
                chat_state::SidebandEventKind::Result(chat_state::SidebandResult {
                    raw_output: raw_output.clone(),
                    structured_output: None,
                    usage: Default::default(),
                    finish: "stop".into(),
                    source_event_seqs: [0, 1],
                    evidence_refs: vec![],
                }),
                chat_state::SidebandEventKind::End(chat_state::SidebandEnd {
                    outcome: chat_state::SidebandOutcome::Completed,
                    error: None,
                }),
            ] {
                let event = sideband.prepare(kind).unwrap();
                sideband.accept(event).unwrap();
            }
            let start = if index == 0 { 1 } else { 2 };
            let ids = timeline.surface_ids()[start..start + 3].to_vec();
            let target = chat_state::SurfaceRange {
                start: ids[0],
                end: ids[2],
                shadowed: ids,
            };
            timeline
                .record(K::Compaction(chat_state::CompactionEvent::Summary {
                    id: id.clone(),
                    input_ref,
                    result_ref: chat_state::TimelineRangeRef {
                        timeline_id: sideband_id,
                        first_seq: 2,
                        last_seq: 2,
                    },
                    target: target.clone(),
                    source_tokens: 1,
                    summary_chars: raw_output.chars().count(),
                }))
                .unwrap();
            timeline
                .replace_compaction_range(
                    target,
                    vec![ConversationItem::user_meta(
                        compaction::format_compact_summary_content(&raw_output),
                    )],
                )
                .unwrap();
            timeline
                .record(K::Compaction(chat_state::CompactionEvent::Completed {
                    id,
                    source_items: 3,
                    result_items: 1,
                    duration_ms: 1,
                }))
                .unwrap();
            sidebands.push(sideband);
        }
        (timeline, sidebands)
    }

    #[test]
    #[ignore = "real complex canonical history load and ACP exit"]
    fn complex_history_exit_through_acp() {
        const TEST: &str =
            "agent::mvp_agent::runtime_handoff::tests::complex_history_exit_through_acp";
        const HOME_ENV: &str = "GROW_TEST_COMPLEX_EXIT_HOME";
        let Ok(home) = std::env::var(HOME_ENV) else {
            let temp = tempfile::tempdir().unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    TEST,
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("HOME", temp.path())
                .env("GROW_HOME", temp.path())
                .env(HOME_ENV, temp.path())
                .output()
                .unwrap();
            println!("{}", String::from_utf8_lossy(&output.stdout));
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            assert!(output.status.success(), "complex-history subprocess failed");
            return;
        };
        super::super::tests::run_local_for_bridge_test(|| async move {
            use crate::session::history::{ResponseAnchor, encode_domain};
            use acp_transport::AcpAgentHandler as _;
            use sha2::{Digest, Sha256};
            use std::io::Write as _;
            let _ = tracing_subscriber::fmt()
                .with_test_writer()
                .with_env_filter("shell::agent::mvp_agent=info,shell::session=info")
                .try_init();
            let home = std::path::PathBuf::from(home);
            assert!(
                home.canonicalize()
                    .unwrap()
                    .starts_with(std::path::Path::new("/tmp").canonicalize().unwrap())
                    || std::env::var(HOME_ENV).unwrap().contains("/var/folders/")
            );
            let cwd = home.join("workspace");
            std::fs::create_dir_all(&cwd).unwrap();
            let info = SessionInfo {
                id: acp::SessionId::new(uuid::Uuid::now_v7().to_string()),
                cwd: cwd.canonicalize().unwrap().to_string_lossy().into_owned(),
            };
            let config_text = r#"
[models]
default = "test/default"
[provider.test]
api_backend = "chat_completions"
[provider.test.options]
base_url = "http://127.0.0.1:9/v1"
api_key = "test-only"
[provider.test.models.default]
context_window = 128000
"#;
            std::fs::write(home.join("config.toml"), config_text).unwrap();
            let mut cfg = crate::agent::config::Config::new_from_toml_cfg(
                &toml::from_str(config_text).unwrap(),
            )
            .unwrap();
            cfg.default_model_override = None;
            cfg.remote_settings = Some(crate::util::config::RemoteSettings::default());
            let exchanges = std::env::var("GROW_TEST_COMPLEX_EXCHANGES")
                .map(|v| v.parse::<usize>().unwrap())
                .unwrap_or(256);
            let result_bytes = std::env::var("GROW_TEST_COMPLEX_RESULT_BYTES")
                .map(|v| v.parse::<usize>().unwrap())
                .unwrap_or(65536);
            assert!(exchanges >= 32);
            let started = std::time::Instant::now();
            let (timeline, sidebands) =
                complex_exit_history(info.id.0.as_ref(), exchanges, result_bytes);
            let source_events = timeline.events().len();
            let build_ms = started.elapsed().as_millis();
            let started = std::time::Instant::now();
            let old_facts = CanonicalChildFacts::read(timeline.events().to_vec()).unwrap();
            let old_elapsed = started.elapsed();
            assert!(old_facts.spawned.is_empty());
            println!(
                "complex_exit build_ms={build_ms} exchanges={exchanges} result_bytes={result_bytes} source_events={source_events} old_copy_replay_ms={} deadline_ms=5000 old_budget_exhausted={}",
                old_elapsed.as_millis(),
                old_elapsed >= std::time::Duration::from_secs(5)
            );
            let storage =
                crate::session::storage::jsonl::JsonlStorageAdapter::with_root(home.clone());
            storage
                .init_session(&info, crate::agent::models::ModelId::new("test/default"))
                .await
                .unwrap();
            for sideband in &sidebands {
                for event in sideband.events() {
                    storage
                        .append_sideband_event_durable(&info, event)
                        .await
                        .unwrap();
                }
            }
            drop(sidebands);
            drop(storage);
            let path = crate::session::persistence::session_dir(&info).join("timeline.jsonl");
            assert_eq!(
                std::fs::metadata(&path).unwrap().len(),
                0,
                "only initialize this disposable new source"
            );
            let mut file = std::io::BufWriter::new(
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .unwrap(),
            );
            let mut hash = Sha256::new();
            let mut source_bytes = 0usize;
            for (position, event) in timeline.events().iter().enumerate() {
                let anchor = event
                    .messages()
                    .and_then(|message| message.response_admission.as_ref())
                    .map(|_| ResponseAnchor::Admission);
                let record = encode_domain(
                    position as u64,
                    &serde_json::value::to_raw_value(event).unwrap(),
                    anchor,
                )
                .unwrap();
                hash.update(&record);
                file.write_all(&record).unwrap();
                source_bytes += record.len();
            }
            file.flush().unwrap();
            file.get_ref().sync_all().unwrap();
            drop(file);
            let source_hash = format!("{:x}", hash.finalize());
            std::fs::write(home.join("complex-exit-fixture.json"), serde_json::to_vec_pretty(&serde_json::json!({"sessionId":info.id.0.as_ref(),"cwd":info.cwd,"timeline":path,"sourceBytes":source_bytes,"sourceEvents":source_events,"sha256":source_hash,"oldCopyReplayMs":old_elapsed.as_millis()})).unwrap()).unwrap();
            println!(
                "complex_exit fixture={} source_bytes={source_bytes} source_hash={source_hash}",
                home.join("complex-exit-fixture.json").display()
            );
            drop(timeline);
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::task::spawn_local(async move {
                while let Some(message) = rx.recv().await {
                    match message {
                        acp_transport::AcpClientMessage::SessionNotification(args) => {
                            let _ = args.response_tx.send(Ok(()));
                        }
                        acp_transport::AcpClientMessage::ExtNotification(args) => {
                            let _ = args.response_tx.send(Ok(()));
                        }
                        _ => {}
                    }
                }
            });
            let agent = MvpAgent::new(GatewaySender::new(tx), &cfg).unwrap();
            agent.initialize(acp::InitializeRequest::new(acp::ProtocolVersion::V1).meta(serde_json::json!({"startupHints":{"nonInteractive":true,"skipGitStatus":true,"skipProjectLayout":true}}).as_object().cloned())).await.unwrap();
            agent.set_auth_method(acp::AuthMethodId::new("provider.api_key"));
            let started = std::time::Instant::now();
            agent
                .load_session(acp::LoadSessionRequest::new(
                    info.id.clone(),
                    std::path::PathBuf::from(&info.cwd),
                ))
                .await
                .unwrap();
            println!(
                "complex_exit real_load_ms={}",
                started.elapsed().as_millis()
            );
            let started = std::time::Instant::now();
            let response = agent
                .ext_method(acp::ExtRequest::new(
                    "grow/session/suspend_for_exit",
                    serde_json::value::to_raw_value(
                        &serde_json::json!({"exit_id":uuid::Uuid::now_v7().to_string()}),
                    )
                    .unwrap()
                    .into(),
                ))
                .await
                .unwrap();
            let result: serde_json::Value = serde_json::from_str(response.0.get()).unwrap();
            println!(
                "complex_exit actual_acp_result={result} elapsed_ms={} former_deadline_ms=5000",
                started.elapsed().as_millis()
            );
            assert_eq!(
                result["status"], "suspended",
                "actual complex-history exit unconfirmed: {result}"
            );
            assert!(agent.control_session_handle(&info.id).is_none());
            assert!(!agent.session_threads.borrow().contains_key(&info.id));
            // Verify immutable original prefix and the actual durable tree.
            let mut file = std::fs::File::open(&path).unwrap();
            use std::io::Read as _;
            let mut prefix = Sha256::new();
            let mut remaining = source_bytes;
            let mut buffer = [0u8; 65536];
            while remaining > 0 {
                let limit = remaining.min(buffer.len());
                let count = file.read(&mut buffer[..limit]).unwrap();
                assert!(
                    count > 0,
                    "the immutable source prefix must remain complete"
                );
                prefix.update(&buffer[..count]);
                remaining -= count;
            }
            assert_eq!(format!("{:x}", prefix.finalize()), source_hash);
            let started = std::time::Instant::now();
            let (loaded, observer) = crate::session::persistence::load_light(&info, None, false)
                .await
                .unwrap();
            assert_eq!(
                loaded.timeline.runtime_lifecycle().phase,
                RuntimePhase::Suspended
            );
            assert_eq!(loaded.timeline.runtime_lifecycle().tree.len(), 1);
            assert!(loaded.timeline.events().len() > source_events);
            drop(observer);
            println!(
                "complex_exit cold_validation_ms={} source_prefix_unchanged=true writers_retired=true",
                started.elapsed().as_millis()
            );
            // B verifies the real exit chain; A separately proves the field
            // ledger exhausts the old five-second budget. Keep B's old timing
            // as a measured control without making it a product assertion.
        });
    }

    #[test]
    #[ignore = "real validated owner deadline measurement; requires pinned /tmp field ledger"]
    fn validated_owner_snapshot_deadline_probe() {
        super::super::tests::run_local_for_bridge_test(|| async {
            use crate::session::history::{JournalFact, decode_record};
            let path =
                std::path::PathBuf::from(std::env::var("GROW_TEST_EXIT_TIMELINE_COPY").unwrap());
            assert!(
                path.canonicalize()
                    .unwrap()
                    .starts_with(std::path::Path::new("/tmp").canonicalize().unwrap())
            );
            let setup = std::time::Instant::now();
            let bytes = std::fs::read(&path).unwrap();
            assert!(bytes.ends_with(b"\n"));
            let events = bytes
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
                .filter_map(|line| match decode_record(line).unwrap().fact {
                    JournalFact::Domain { event, .. } => Some(event),
                    _ => None,
                })
                .collect::<Vec<_>>();
            drop(bytes);
            let timeline = chat_state::Timeline::from_events(events).unwrap();
            let source_events = timeline.events().len();
            println!(
                "owner_probe setup_decode_validate_ms={} source_events={source_events}",
                setup.elapsed().as_millis()
            );
            let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
            let config = serde_json::from_value(serde_json::json!({"base_url":"http://127.0.0.1:9", "model":"test", "context_window":256000})).unwrap();
            let cancel = tokio_util::sync::CancellationToken::new();
            let owner_started = std::time::Instant::now();
            let chat = chat_state::ChatStateActor::spawn_from_validated_timeline(
                timeline,
                config,
                None,
                Box::new(chat_state::NullTimelinePersistence),
                event_tx,
                cancel.clone(),
            )
            .await
            .unwrap();
            chat.runtime_state().await.unwrap();
            println!(
                "owner_probe owner_ready_ms={}",
                owner_started.elapsed().as_millis()
            );
            let agent = super::super::tests::build_minimal_agent_for_tests();
            let (mut handle, _commands) =
                super::super::tests::make_test_handle_with_receiver("test", None);
            handle.chat_state_handle = chat.clone();
            assert!(!handle.cmd_tx.is_closed());
            let budget = std::time::Duration::from_secs(5);
            let old_started = std::time::Instant::now();
            let old_deadline = tokio::time::Instant::now() + budget;
            // Exact old split: timeout enclosed owner queries; the caller
            // synchronously reconstructed facts after that timeout returned.
            let (old_runtime, old_events) = tokio::time::timeout_at(old_deadline, async {
                let runtime = chat.runtime_state().await?;
                let events = chat
                    .timeline_events()
                    .await
                    .ok_or("runtime Timeline unavailable")?;
                Ok::<_, String>((runtime, events))
            })
            .await
            .expect("old canonical query must actually complete")
            .unwrap();
            let query_ms = old_started.elapsed().as_millis();
            let replay_started = std::time::Instant::now();
            let old = CanonicalChildFacts::read(old_events).unwrap();
            let old_elapsed = old_started.elapsed();
            let old_expired = tokio::time::Instant::now() >= old_deadline;
            println!(
                "owner_probe old_return=ok query_ms={query_ms} replay_ms={} total_ms={} deadline_ms=5000 deadline_expired={old_expired}",
                replay_started.elapsed().as_millis(),
                old_elapsed.as_millis()
            );
            assert!(
                old_expired && old_elapsed >= budget,
                "field ledger must reproduce old synchronous budget exhaustion"
            );
            // Tokio cannot preempt synchronous replay: old_return was Ok, but
            // the following suspension stage inherits an expired deadline.
            let new_started = std::time::Instant::now();
            let new_deadline = tokio::time::Instant::now() + budget;
            let (new_runtime, new) = agent
                .runtime_snapshot_before(&handle, Some(new_deadline))
                .await
                .unwrap();
            let new_elapsed = new_started.elapsed();
            println!(
                "owner_probe new_return=ok total_us={} deadline_ms=5000 deadline_expired={} spawned={} ended={}",
                new_elapsed.as_micros(),
                tokio::time::Instant::now() >= new_deadline,
                new.spawned.len(),
                new.ended.len()
            );
            assert_eq!(old_runtime, new_runtime);
            assert!(old == new);
            assert!(new_elapsed < budget && tokio::time::Instant::now() < new_deadline);
            cancel.cancel();
        });
    }

    #[test]
    #[ignore = "read-only fixed-prefix field ledger timing; requires /tmp copy"]
    fn fixed_prefix_child_facts_probe() {
        use crate::session::history::{JournalFact, decode_record};
        use sha2::{Digest, Sha256};
        let path = std::path::PathBuf::from(
            std::env::var("GROW_TEST_EXIT_TIMELINE_COPY")
                .expect("set GROW_TEST_EXIT_TIMELINE_COPY to the pinned /tmp copy"),
        );
        assert!(
            path.canonicalize()
                .unwrap()
                .starts_with(std::path::Path::new("/tmp").canonicalize().unwrap()),
            "probe only accepts a disposable /tmp copy"
        );
        let started = std::time::Instant::now();
        let bytes = std::fs::read(&path).unwrap();
        let read_ms = started.elapsed().as_millis();
        assert!(
            bytes.ends_with(b"\n"),
            "fixed prefix must end at a complete physical record"
        );
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let started = std::time::Instant::now();
        let mut events = Vec::new();
        let mut physical_records = 0usize;
        let mut previous = None;
        let mut replacements = 0usize;
        let mut causes = std::collections::BTreeMap::<String, usize>::new();
        for line in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let record = decode_record(line).unwrap();
            if let Some(previous) = previous {
                assert_eq!(record.seq, previous + 1);
            }
            previous = Some(record.seq);
            physical_records += 1;
            if let JournalFact::Domain { event, .. } = record.fact {
                if let chat_state::TimelineEventKind::Messages(message) = &event.kind {
                    *causes.entry(format!("{:?}", message.cause)).or_default() += 1;
                    replacements += usize::from(matches!(
                        message.surface,
                        chat_state::SurfaceOp::Replace { .. }
                    ));
                }
                events.push(event);
            }
        }
        let decode_ms = started.elapsed().as_millis();
        println!(
            "field_probe path={} bytes={} sha256={hash} complete_newline=true physical_records={physical_records} last_physical_seq={previous:?} domain_events={} last_domain_seq={:?} replacements={replacements} causes={causes:?} read_ms={read_ms} decode_ms={decode_ms}",
            path.display(),
            bytes.len(),
            events.len(),
            events.last().map(|event| event.seq.get())
        );
        drop(bytes);
        let started = std::time::Instant::now();
        let timeline = chat_state::Timeline::from_events(events).unwrap();
        println!(
            "field_probe first_full_validation_ms={}",
            started.elapsed().as_millis()
        );
        let started = std::time::Instant::now();
        let full = timeline.events().to_vec();
        let full_copy_us = started.elapsed().as_micros();
        let replay_started = std::time::Instant::now();
        let old = CanonicalChildFacts::read(full).unwrap();
        println!(
            "field_probe old_full_copy_us={full_copy_us} old_replay_us={} old_total_us={}",
            replay_started.elapsed().as_micros(),
            started.elapsed().as_micros()
        );
        let started = std::time::Instant::now();
        let projection = timeline
            .events()
            .iter()
            .filter(|event| {
                matches!(
                    &event.kind,
                    chat_state::TimelineEventKind::Subagent(
                        chat_state::SubagentEvent::Spawned(_) | chat_state::SubagentEvent::Ended(_)
                    )
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        let new = CanonicalChildFacts::scan(&projection).unwrap();
        let narrow_us = started.elapsed().as_micros();
        assert!(
            old == new,
            "full replay and validated projection must have identical child facts"
        );
        println!(
            "field_probe narrow_filter_scan_us={narrow_us} projection_events={} projection_bytes={} spawned={} ended={} facts_equal=true",
            projection.len(),
            serde_json::to_vec(&projection).unwrap().len(),
            new.spawned.len(),
            new.ended.len()
        );
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

        assert!(CanonicalChildFacts::scan(&events).is_err());
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

        assert!(CanonicalChildFacts::scan(&events).is_err());
        assert!(CanonicalChildFacts::read(events).is_err());
    }

    #[test]
    fn canonical_child_facts_reject_duplicate_and_unspawned_terminals() {
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Spawned(Box::new(spawn("task", "child", "root"))),
            ))
            .unwrap();
        timeline
            .record(chat_state::TimelineEventKind::Subagent(
                chat_state::SubagentEvent::Ended(chat_state::SubagentTerminalEvent {
                    subagent_id: "task".into(),
                    child_session_id: "child".into(),
                    outcome: chat_state::SubagentOutcome::Cancelled,
                    duration_ms: 0,
                    tool_calls: 0,
                    turns: 0,
                    tokens_used: 0,
                    error: Some("fixture cancellation".into()),
                    result_ref: None,
                    snapshot_ref: None,
                }),
            ))
            .unwrap();
        let mut events = timeline.events().to_vec();
        assert!(CanonicalChildFacts::scan(&events[1..]).is_err());
        let mut duplicate = events[1].clone();
        duplicate.seq = EventSeq::new(3);
        events.push(duplicate);
        assert!(CanonicalChildFacts::scan(&events).is_err());
    }

    async fn suspension_observes_child_retirement_frontier(late_terminal: bool) {
        use tools::implementations::grow_build::task::types::{FrozenChildRef, SubagentEvent};
        let agent = super::super::tests::build_minimal_agent_for_tests();
        let (root_tx, root_exit) = std::sync::mpsc::channel();
        let root_thread = std::thread::spawn(move || {
            let _ = root_exit.recv();
        });
        let mut root_chat = None;
        let mut drivers = Vec::new();
        for sid in ["test", "child"] {
            let (events, _events_rx) = tokio::sync::mpsc::unbounded_channel();
            let config = serde_json::from_value(serde_json::json!({
                "base_url":"http://localhost", "model":"test", "context_window":256000
            }))
            .unwrap();
            let chat = chat_state::ChatStateActor::spawn(
                vec![],
                config,
                Box::new(chat_state::NullTimelinePersistence),
                events,
                tokio_util::sync::CancellationToken::new(),
            );
            if sid == "test" {
                root_chat = Some(chat.clone());
            }
            let (mut handle, mut commands) =
                super::super::tests::make_test_handle_with_receiver("test", None);
            handle.info.id = acp::SessionId::new(sid);
            handle.chat_state_handle = chat.clone();
            agent
                .sessions
                .borrow_mut()
                .insert(handle.info.id.clone(), handle);
            let release = root_tx.clone();
            drivers.push(tokio::spawn(async move {
                let SessionCommand::SuspendRuntimeNode {
                    operation_id,
                    deadline,
                    respond_to,
                    ..
                } = commands.recv().await.unwrap()
                else {
                    panic!("expected checkpoint");
                };
                if chat.runtime_state().await.unwrap().phase == RuntimePhase::Running {
                    chat.record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
                        SessionRuntimeEvent::SuspendRequested {
                            operation_id: operation_id.clone(),
                            root_session_id: "test".into(),
                        },
                    ))
                    .await
                    .unwrap();
                }
                chat.record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
                    SessionRuntimeEvent::CommandsSettled {
                        operation_id: operation_id.clone(),
                        commands: vec![],
                    },
                ))
                .await
                .unwrap();
                let checkpoint = chat
                    .checkpoint_runtime(operation_id, "a".repeat(64), None, None, deadline)
                    .await
                    .unwrap();
                respond_to
                    .send(Ok(RuntimeNodeRef {
                        session_id: sid.into(),
                        checkpoint_seq: checkpoint.seq,
                    }))
                    .unwrap();
                let SessionCommand::RetireSuspendedRuntime { respond_to, .. } =
                    commands.recv().await.unwrap()
                else {
                    panic!("expected retirement");
                };
                respond_to.send(Ok(())).unwrap();
                if sid == "test" {
                    let _ = release.send(());
                }
            }));
        }
        let root = acp::SessionId::new("test");
        agent.session_threads.borrow_mut().insert(
            root.clone(),
            crate::session::SessionThread::from_handle(root_thread),
        );
        let chat = root_chat.unwrap();
        chat.record_timeline_event_durably(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Spawned(Box::new(spawn("task", "child", "test"))),
        ))
        .await
        .unwrap();
        let mut coordinator = agent.subagent_event_rx.borrow_mut().take().unwrap();
        let final_chat = chat.clone();
        let coordination = tokio::spawn(async move {
            let Some(SubagentEvent::FreezeAdmission { respond_to, .. }) = coordinator.recv().await
            else {
                panic!("expected freeze");
            };
            respond_to
                .send(Ok(vec![FrozenChildRef {
                    task_id: "task".into(),
                    session_id: "child".into(),
                    security_parent_session_id: "test".into(),
                    origin_tool_call_id: None,
                    initializing: false,
                }]))
                .unwrap();
            let Some(SubagentEvent::CheckSuspendedRetirement { respond_to, .. }) =
                coordinator.recv().await
            else {
                panic!("expected retirement frontier");
            };
            if late_terminal {
                // Fault injection: a terminal appears only while the child
                // finalization owner drains. Its missing result must still
                // prevent a successful tree barrier after retirement.
                final_chat
                    .record_timeline_event_durably(chat_state::TimelineEventKind::Subagent(
                        chat_state::SubagentEvent::Ended(chat_state::SubagentTerminalEvent {
                            subagent_id: "task".into(),
                            child_session_id: "child".into(),
                            outcome: chat_state::SubagentOutcome::Cancelled,
                            duration_ms: 0,
                            tool_calls: 0,
                            turns: 0,
                            tokens_used: 0,
                            error: Some("fixture cancellation".into()),
                            result_ref: None,
                            snapshot_ref: None,
                        }),
                    ))
                    .await
                    .unwrap();
            }
            respond_to.send(Ok(true)).unwrap();
        });
        let result = agent
            .suspend_session_tree(
                &root,
                tokio::time::Instant::now() + std::time::Duration::from_secs(5),
            )
            .await;
        if late_terminal {
            let error = result.unwrap_err();
            assert!(
                error.contains("has no canonical result reference"),
                "{error}"
            );
            assert!(chat.runtime_state().await.unwrap().tree.is_empty());
        } else {
            let (_, nodes) = result.unwrap();
            assert_eq!(nodes.len(), 2);
            assert!(nodes.iter().any(|node| node.session_id == "child"));
            assert_eq!(chat.runtime_state().await.unwrap().tree, nodes);
        }
        coordination.await.unwrap();
        let _ = root_tx.send(());
        for driver in drivers {
            if late_terminal {
                driver.abort();
                let _ = driver.await;
            } else {
                driver.await.unwrap();
            }
        }
        if let Some(thread) = agent.session_threads.borrow_mut().remove(&root) {
            thread.join().unwrap();
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn suspension_preserves_an_open_child_checkpoint() {
        suspension_observes_child_retirement_frontier(false).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn suspension_rechecks_a_terminal_committed_at_child_retirement() {
        suspension_observes_child_retirement_frontier(true).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_snapshot_scans_complete_child_projection_and_observes_late_terminal() {
        let agent = super::super::tests::build_minimal_agent_for_tests();
        let (mut handle, _commands) =
            super::super::tests::make_test_handle_with_receiver("test", None);
        let (events, _events_rx) = tokio::sync::mpsc::unbounded_channel();
        let config = serde_json::from_value(serde_json::json!({
            "base_url":"http://localhost", "model":"test", "context_window":256000
        }))
        .unwrap();
        let chat = chat_state::ChatStateActor::spawn(
            vec![],
            config,
            Box::new(chat_state::NullTimelinePersistence),
            events,
            tokio_util::sync::CancellationToken::new(),
        );
        handle.chat_state_handle = chat.clone();
        chat.push_user_message(sampling_types::ConversationItem::user("unrelated history"));
        chat.record_timeline_event_durably(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Spawned(Box::new(spawn("task", "child", "test"))),
        ))
        .await
        .unwrap();
        let (runtime, first) = agent.runtime_snapshot(&handle).await.unwrap();
        assert_eq!(runtime, chat.runtime_state().await.unwrap());
        assert_eq!(
            first.open().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
            vec!["task"]
        );
        chat.record_timeline_event_durably(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Ended(chat_state::SubagentTerminalEvent {
                subagent_id: "task".into(),
                child_session_id: "child".into(),
                outcome: chat_state::SubagentOutcome::Cancelled,
                duration_ms: 0,
                tool_calls: 0,
                turns: 0,
                tokens_used: 0,
                error: Some("fixture cancellation".into()),
                result_ref: None,
                snapshot_ref: None,
            }),
        ))
        .await
        .unwrap();
        let (_, latest) = agent.runtime_snapshot(&handle).await.unwrap();
        assert_eq!(latest.spawned, first.spawned);
        assert_eq!(latest.ended["task"].child_session_id, "child");
        assert_eq!(latest.open().count(), 0);
        assert_eq!(first.open().count(), 1, "an earlier snapshot cannot mutate");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_snapshot_reports_unavailable_live_owner() {
        let agent = super::super::tests::build_minimal_agent_for_tests();
        let (mut handle, _commands) =
            super::super::tests::make_test_handle_with_receiver("test", None);
        handle.chat_state_handle = chat_state::ChatStateHandle::noop();
        assert!(!handle.cmd_tx.is_closed());
        assert!(agent.runtime_snapshot(&handle).await.is_err());
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
