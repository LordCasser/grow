use super::*;

#[allow(clippy::too_many_arguments)]
fn upsert_workflow_block(
    agent: &mut AgentView,
    run_id: &str,
    name: &str,
    objective: &str,
    status: &str,
    phases: &[shell::extensions::notification::WorkflowPhaseInfo],
    current_phase: Option<&str>,
    active_agents: u32,
    elapsed_ms: u64,
) {
    use crate::scrollback::blocks::{WorkflowBlock, WorkflowBlockPhase, WorkflowBlockStatus};

    let elapsed = std::time::Duration::from_millis(elapsed_ms);
    let block_status = match status {
        "active" => WorkflowBlockStatus::Running,
        "complete" => WorkflowBlockStatus::Done { elapsed },
        "failed" | "interrupted" => WorkflowBlockStatus::Failed { elapsed },
        "cancelled" => WorkflowBlockStatus::Cancelled { elapsed },
        "cleared" => {
            if let Some(id) = agent.workflow_blocks.remove(run_id) {
                agent.scrollback.finish_running(id);
            }
            return;
        }
        _ => WorkflowBlockStatus::Paused { elapsed },
    };
    let is_running = matches!(block_status, WorkflowBlockStatus::Running);
    let terminal = matches!(
        block_status,
        WorkflowBlockStatus::Done { .. }
            | WorkflowBlockStatus::Failed { .. }
            | WorkflowBlockStatus::Cancelled { .. }
    );

    let mapped_entry = agent
        .workflow_blocks
        .get(run_id)
        .copied()
        .filter(|id| agent.scrollback.get_by_id(*id).is_some());
    if mapped_entry.is_none() {
        agent.workflow_blocks.remove(run_id);
    }
    let entry_id = match mapped_entry {
        Some(id) => id,
        None => {
            let block = WorkflowBlock::started(run_id, name, objective);
            let id = agent.scrollback.push_block(RenderBlock::Workflow(block));
            agent.scrollback.set_last_running(true);
            agent.workflow_blocks.insert(run_id.to_string(), id);
            id
        }
    };

    if let Some(entry) = agent.scrollback.get_by_id_mut(entry_id)
        && let RenderBlock::Workflow(ref mut wb) = entry.block
    {
        wb.status = block_status;
        wb.phases = phases
            .iter()
            .map(|p| WorkflowBlockPhase {
                title: p.title.clone(),
                state: p.state.clone(),
            })
            .collect();
        wb.current_phase = current_phase.map(str::to_owned);
        wb.active_agents = active_agents;
        wb.elapsed = elapsed;
        entry.invalidate_cache();
    }
    if is_running {
        agent.scrollback.set_entry_running(entry_id, true);
    } else {
        agent.scrollback.finish_running(entry_id);
        if terminal {
            agent.workflow_blocks.remove(run_id);
        }
    }
}

/// Fields of a private `WorkflowUpdated` that keep a run visible while it
/// stays out of every public Workflow management surface.
struct PrivateWorkflowUpdate {
    run_id: String,
    definition_id: Option<String>,
    definition_scope: Option<String>,
    definition_hash: Option<String>,
    revision: u64,
    name: String,
    objective: String,
    status: String,
    phases: Vec<shell::extensions::notification::WorkflowPhaseInfo>,
    current_phase: Option<String>,
    agent_budget: Option<u64>,
    agents_used: u64,
    agents_remaining: Option<u64>,
    agent_usage_incomplete: bool,
    elapsed_ms: u64,
    agents: Vec<shell::extensions::notification::WorkflowAgentInfo>,
    pause_message: Option<String>,
    result_summary: Option<String>,
}

/// Shared snapshot mapping for public and private runs. The two paths differ
/// only in `management_available`/`builtin` (private runs are never
/// manageable and never rendered by definition views).
#[allow(clippy::too_many_arguments)]
fn build_workflow_run_snapshot(
    run_id: String,
    definition_id: Option<String>,
    definition_scope: Option<String>,
    definition_hash: Option<String>,
    name: String,
    objective: String,
    status: String,
    management_available: bool,
    builtin: bool,
    phases: &[shell::extensions::notification::WorkflowPhaseInfo],
    current_phase: Option<String>,
    agents: &[shell::extensions::notification::WorkflowAgentInfo],
    agent_budget: Option<u64>,
    agents_used: u64,
    agents_remaining: Option<u64>,
    agent_usage_incomplete: bool,
    elapsed_ms: u64,
    pause_message: Option<String>,
    result_summary: Option<String>,
) -> crate::app::agent::WorkflowRunSnapshot {
    crate::app::agent::WorkflowRunSnapshot {
        run_id,
        definition_id,
        definition_scope,
        definition_hash,
        name,
        objective,
        status,
        management_available,
        builtin,
        phases: phases
            .iter()
            .map(|p| (p.title.clone(), p.state.clone()))
            .collect(),
        current_phase,
        agents: agents
            .iter()
            .map(|a| crate::app::agent::WorkflowAgentRowView {
                agent_id: a.agent_id.clone(),
                label: a.label.clone(),
                phase: a.phase.clone(),
                model: a.model.clone(),
                state: a.state.clone(),
                tokens_used: a.tokens_used,
                duration_ms: a.duration_ms,
            })
            .collect(),
        agent_budget,
        agents_used,
        agents_remaining,
        agent_usage_incomplete,
        active_agents: agents.iter().filter(|a| a.state == "running").count() as u32,
        elapsed_ms,
        received_at: std::time::Instant::now(),
        pause_message,
        result_summary,
    }
}

pub(super) fn ingest_workflow_update(agent: &mut AgentView, update: GrowSessionUpdate) -> bool {
    let GrowSessionUpdate::WorkflowUpdated {
        run_id,
        private,
        definition_id,
        definition_scope,
        definition_hash,
        revision,
        name,
        objective,
        status,
        foreground: _,
        phases,
        current_phase,
        agent_budget,
        agents_used,
        agents_remaining,
        agent_usage_incomplete,
        elapsed_ms,
        active_agents: _,
        current_agent_label: _,
        agents,
        pause_message,
        result_summary,
        ..
    } = update
    else {
        return false;
    };
    if private {
        return ingest_private_workflow_update(
            agent,
            PrivateWorkflowUpdate {
                run_id,
                definition_id,
                definition_scope,
                definition_hash,
                revision,
                name,
                objective,
                status,
                phases,
                current_phase,
                agent_budget,
                agents_used,
                agents_remaining,
                agent_usage_incomplete,
                elapsed_ms,
                agents,
                pause_message,
                result_summary,
            },
        );
    }
    if status != "cleared" {
        match agent.session.workflow_run_revisions.get(&run_id).copied() {
            Some(last) if revision == 0 && last > 0 => return false,
            Some(last) if revision > 0 && revision <= last => return false,
            _ => {}
        }
        if revision == 0 && agent.session.cleared_workflow_runs.contains(&run_id) {
            return false;
        }
    }
    if revision > 0 {
        agent
            .session
            .workflow_run_revisions
            .insert(run_id.clone(), revision);
    }
    if status == "cleared" {
        agent.session.cleared_workflow_runs.insert(run_id.clone());
    }
    let management_available = agent
        .session
        .available_commands
        .iter()
        .any(|c| c.name == "workflow-run");
    let builtin = super::is_builtin_workflow_handle(&agent.session.available_commands, &name);
    if status == "cleared" {
        agent
            .session
            .workflow_runs
            .retain(|run| run.run_id != run_id);
    } else {
        let snapshot = build_workflow_run_snapshot(
            run_id.clone(),
            definition_id,
            definition_scope,
            definition_hash,
            name.clone(),
            objective.clone(),
            status.clone(),
            management_available,
            builtin,
            &phases,
            current_phase.clone(),
            &agents,
            agent_budget,
            agents_used,
            agents_remaining,
            agent_usage_incomplete,
            elapsed_ms,
            pause_message,
            result_summary,
        );
        match agent
            .session
            .workflow_runs
            .iter_mut()
            .find(|run| run.run_id == run_id)
        {
            Some(existing) => *existing = snapshot,
            None => agent.session.workflow_runs.push(snapshot),
        }
    }
    let active = agents.iter().filter(|a| a.state == "running").count() as u32;
    upsert_workflow_block(
        agent,
        &run_id,
        &name,
        &objective,
        &status,
        &phases,
        current_phase.as_deref(),
        active,
        elapsed_ms,
    );
    true
}

/// Private workflow runs (deep research) must stay visible while running but
/// never enter any public Workflow management surface. The shell keeps
/// publishing `WorkflowUpdated` for them; this path keeps the transcript
/// progress block, the tasks pane row and the activity projection alive while
/// `agent.session.workflow_runs` is never touched.
fn ingest_private_workflow_update(agent: &mut AgentView, update: PrivateWorkflowUpdate) -> bool {
    let PrivateWorkflowUpdate {
        run_id,
        definition_id,
        definition_scope,
        definition_hash,
        revision,
        name,
        objective,
        status,
        phases,
        current_phase,
        agent_budget,
        agents_used,
        agents_remaining,
        agent_usage_incomplete,
        elapsed_ms,
        agents,
        pause_message,
        result_summary,
    } = update;

    // Same revision/cleared dedup guards as the public path. Run ids share the
    // `wf_<uuid>` namespace, so the shared maps are collision-free.
    if status != "cleared" {
        match agent.session.workflow_run_revisions.get(&run_id).copied() {
            Some(last) if revision == 0 && last > 0 => return false,
            Some(last) if revision > 0 && revision <= last => return false,
            _ => {}
        }
        if revision == 0 && agent.session.cleared_workflow_runs.contains(&run_id) {
            return false;
        }
    }
    if revision > 0 {
        agent
            .session
            .workflow_run_revisions
            .insert(run_id.clone(), revision);
    }
    if status == "cleared" {
        agent.session.cleared_workflow_runs.insert(run_id.clone());
    }

    // Settled = cleared, terminal, or budget-limited: private runs have no
    // user clear path and can never be resumed, so nothing may accumulate in
    // the tasks pane. `budget_limited` is grouped with terminal states here
    // for the same reason.
    let settled = status == "cleared"
        || matches!(
            status.as_str(),
            "complete" | "failed" | "cancelled" | "interrupted" | "budget_limited"
        );
    if settled {
        agent
            .session
            .private_workflow_runs
            .retain(|run| run.run_id != run_id);
    } else {
        let snapshot = build_workflow_run_snapshot(
            run_id.clone(),
            definition_id,
            definition_scope,
            definition_hash,
            name.clone(),
            objective.clone(),
            status.clone(),
            false, // private runs are not manageable
            false, // never rendered by definition views
            &phases,
            current_phase.clone(),
            &agents,
            agent_budget,
            agents_used,
            agents_remaining,
            agent_usage_incomplete,
            elapsed_ms,
            pause_message,
            result_summary,
        );
        match agent
            .session
            .private_workflow_runs
            .iter_mut()
            .find(|run| run.run_id == run_id)
        {
            Some(existing) => *existing = snapshot,
            None => agent.session.private_workflow_runs.push(snapshot),
        }
    }

    // `budget_limited` has no terminal `WorkflowBlockStatus`, and a private
    // run can never be resumed, so the block converges to Failed instead of a
    // misleading "paused".
    let block_status = if status == "budget_limited" {
        "failed"
    } else {
        status.as_str()
    };
    let active = agents.iter().filter(|a| a.state == "running").count() as u32;
    upsert_workflow_block(
        agent,
        &run_id,
        &name,
        &objective,
        block_status,
        &phases,
        current_phase.as_deref(),
        active,
        elapsed_ms,
    );
    true
}
