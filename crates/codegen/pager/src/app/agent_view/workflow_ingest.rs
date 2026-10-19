use shell::extensions::notification::SessionUpdate as GrowSessionUpdate;

use super::AgentView;
use crate::scrollback::block::RenderBlock;

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

/// Map a workflow update into the pager's common workflow snapshot.
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
) -> crate::app::session::WorkflowRunSnapshot {
    crate::app::session::WorkflowRunSnapshot {
        run_id,
        definition_id,
        definition_scope,
        definition_hash,
        name,
        objective,
        status,
        management_available,
        phases: phases
            .iter()
            .map(|p| (p.title.clone(), p.state.clone()))
            .collect(),
        current_phase,
        agents: agents
            .iter()
            .map(|a| crate::app::session::WorkflowAgentRowView {
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

impl AgentView {
    /// Apply one workflow projection update together with its transcript
    /// block. Keeping both writes behind one view-domain method prevents ACP
    /// routing from exposing a partially updated workflow to rendering.
    pub(crate) fn ingest_workflow_update(&mut self, update: GrowSessionUpdate) -> bool {
        let GrowSessionUpdate::WorkflowUpdated {
            run_id,
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
        match self.session.workflow_run_revisions.get(&run_id).copied() {
            Some(last) if revision > 0 && revision <= last => return false,
            Some(last) if status != "cleared" && revision == 0 && last > 0 => return false,
            _ => {}
        }
        if status != "cleared"
            && revision == 0
            && self.session.cleared_workflow_runs.contains(&run_id)
        {
            return false;
        }
        if revision > 0 {
            self.session
                .workflow_run_revisions
                .insert(run_id.clone(), revision);
        }
        if status == "cleared" {
            self.session.cleared_workflow_runs.insert(run_id.clone());
        }
        let management_available = self
            .session
            .available_commands
            .iter()
            .any(|c| c.name == "workflow-run");
        if status == "cleared" {
            self.session
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
            match self
                .session
                .workflow_runs
                .iter_mut()
                .find(|run| run.run_id == run_id)
            {
                Some(existing) => *existing = snapshot,
                None => self.session.workflow_runs.push(snapshot),
            }
        }
        let active = agents.iter().filter(|a| a.state == "running").count() as u32;
        upsert_workflow_block(
            self,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::agent_view::test_agent_view;
    use std::path::PathBuf;

    fn update(run_id: &str, revision: u64, status: &str) -> GrowSessionUpdate {
        GrowSessionUpdate::WorkflowUpdated {
            run_id: run_id.into(),
            definition_id: None,
            definition_scope: None,
            definition_hash: None,
            revision,
            name: "workflow".into(),
            objective: "inspect".into(),
            status: status.into(),
            foreground: false,
            phases: vec![],
            current_phase: None,
            agent_budget: None,
            agents_used: 0,
            agents_remaining: None,
            agent_usage_incomplete: false,
            elapsed_ms: 10,
            active_agents: 0,
            current_agent_label: None,
            agents: vec![],
            pause_message: None,
            result_summary: None,
        }
    }

    #[test]
    fn stale_positive_clear_does_not_remove_newer_live_run() {
        let mut agent = test_agent_view(Some("s1"), PathBuf::from("/tmp"));
        assert!(agent.ingest_workflow_update(update("run-1", 7, "active")));
        let block_id = agent.workflow_blocks["run-1"];

        assert!(!agent.ingest_workflow_update(update("run-1", 6, "cleared")));
        assert_eq!(agent.session.workflow_runs.len(), 1);
        assert_eq!(agent.session.workflow_runs[0].status, "active");
        assert_eq!(agent.session.workflow_run_revisions.get("run-1"), Some(&7));
        assert!(!agent.session.cleared_workflow_runs.contains("run-1"));
        assert!(
            agent
                .scrollback
                .get_by_id(block_id)
                .is_some_and(|entry| entry.is_running)
        );
    }

    #[test]
    fn newer_clear_and_revisionless_compatibility_preserve_run_history() {
        let mut agent = test_agent_view(Some("s1"), PathBuf::from("/tmp"));
        assert!(agent.ingest_workflow_update(update("run-1", 7, "active")));
        let block_id = agent.workflow_blocks["run-1"];
        assert!(agent.ingest_workflow_update(update("run-1", 8, "cleared")));
        assert!(agent.session.workflow_runs.is_empty());
        assert_eq!(agent.session.workflow_run_revisions.get("run-1"), Some(&8));
        assert!(agent.session.cleared_workflow_runs.contains("run-1"));
        assert!(!agent.workflow_blocks.contains_key("run-1"));
        assert!(
            agent
                .scrollback
                .get_by_id(block_id)
                .is_some_and(|entry| !entry.is_running)
        );
        assert!(!agent.ingest_workflow_update(update("run-1", 8, "active")));

        assert!(agent.ingest_workflow_update(update("legacy", 2, "active")));
        assert!(agent.ingest_workflow_update(update("legacy", 0, "cleared")));
        assert_eq!(agent.session.workflow_run_revisions.get("legacy"), Some(&2));
        assert!(agent.session.cleared_workflow_runs.contains("legacy"));
        assert!(!agent.ingest_workflow_update(update("legacy", 0, "active")));
        assert!(agent.ingest_workflow_update(update("new-run", 1, "active")));
        assert_eq!(agent.session.workflow_runs.len(), 1);
        assert_eq!(agent.session.workflow_runs[0].run_id, "new-run");
    }
}
