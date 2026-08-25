use super::AgentView;
#[cfg(test)]
use super::test_fixtures;
use crate::app::actions::Action;
use crate::app::root::InputOutcome;
use crate::key;
use crossterm::event::{Event, KeyCode, KeyEventKind};

fn management_command(
    op: &str,
    target: Option<&crate::app::session::WorkflowRunSnapshot>,
) -> Option<String> {
    let run = target?;
    let allowed = match op {
        "pause" => run.can_pause(),
        "resume" => run.can_resume(),
        "stop" => run.can_stop(),
        _ => false,
    };
    if !allowed {
        return None;
    }
    Some(format!("/workflow-run {op} {}", run.name))
}

fn resolve_management_command(
    op: &str,
    target: Option<&crate::app::session::WorkflowRunSnapshot>,
) -> Option<String> {
    management_command(op, target).or_else(|| {
        if op == "resume"
            && target.is_some_and(|r| r.status == "budget_limited" && r.management_available)
        {
            target.map(|r| format!("/workflow-run resume {}", r.name))
        } else {
            None
        }
    })
}

fn definition_command(
    operation: &str,
    definition: Option<&crate::views::workflows::WorkflowDefinitionSnapshot>,
) -> Option<String> {
    let definition = definition?;
    let instruction = match operation {
        "focus" => format!(
            "Focus Workflow Definition `{}` and report its scope and current state.",
            definition.definition_id
        ),
        "edit" => format!(
            "Inspect Workflow Definition `{}` and prepare it for editing. If it is saved, derive its session draft first. Remind me that changes affect only the next Run.",
            definition.definition_id
        ),
        "validate" => format!(
            "Validate Workflow Definition `{}` at its current content hash and show the result.",
            definition.definition_id
        ),
        "run" => format!(
            "Run Workflow Definition `{}`. Show its scope, content hash, and new Run handle before continuing.",
            definition.definition_id
        ),
        "publish" if definition.scope == "session" => format!(
            "Publish Workflow Definition `{}` after validation. Ask me to choose Project or User scope; do not infer the scope.",
            definition.definition_id
        ),
        "discard" if definition.scope == "session" => format!(
            "Discard session Workflow Definition `{}` and report the new focus.",
            definition.definition_id
        ),
        _ => return None,
    };
    Some(format!("/workflow {instruction}"))
}

fn transcript_target(
    run: &crate::app::session::WorkflowRunSnapshot,
    phase: Option<&str>,
) -> Option<String> {
    let all_agents = run.phases.is_empty() && run.current_phase.is_none();
    let agents = run.agents_in_phase(if all_agents { None } else { phase });
    agents
        .iter()
        .rev()
        .find(|agent| agent.state == "running")
        .or_else(|| agents.last())
        .map(|agent| agent.agent_id.clone())
}

impl AgentView {
    pub(crate) fn open_workflow_detail(&mut self, name: &str) {
        let Some(run_id) = self
            .session
            .workflow_runs
            .iter()
            .find(|r| r.name == name)
            .map(|r| r.run_id.clone())
        else {
            return;
        };
        self.open_workflow_detail_by_run_id(&run_id);
    }

    pub(crate) fn open_workflow_detail_by_run_id(&mut self, run_id: &str) {
        if !self
            .session
            .workflow_runs
            .iter()
            .any(|r| r.run_id == run_id)
        {
            return;
        }
        self.workflows_view.reset();
        self.workflows_view.selected_run_id = Some(run_id.to_string());
        self.workflows_view.detail_run_id = Some(run_id.to_string());
        self.show_workflows = true;
        self.set_goal_detail_visible(false);
    }

    pub(super) fn handle_workflows_overlay_input(&mut self, ev: &Event) -> Option<InputOutcome> {
        if self.show_workflows {
            if let Event::Key(key) = ev
                && key.kind != KeyEventKind::Release
            {
                if key!('q', CONTROL).matches(key) {
                    return Some(InputOutcome::Unchanged);
                }
                if !key.modifiers.is_empty() {
                    return Some(InputOutcome::Changed);
                }
                let runs = self.workflow_runs_newest_first();
                let mut view = self.workflows_view.clone();
                view.normalize(&runs);
                let in_detail = view.detail_run(&runs).is_some();
                let manage_target = view.detail_run(&runs).or_else(|| {
                    (view.list_section == crate::views::workflows::WorkflowListSection::Runs)
                        .then(|| runs.get(view.selected_run).copied())
                        .flatten()
                });
                let outcome = match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {
                        if in_detail && runs.len() > 1 {
                            view.detail_run_id = None;
                            view.phase_pinned = false;
                            self.workflows_view = view;
                        } else {
                            self.show_workflows = false;
                        }
                        InputOutcome::Changed
                    }
                    KeyCode::Char('g') => {
                        self.show_workflows = false;
                        InputOutcome::Changed
                    }
                    KeyCode::Tab if in_detail && runs.len() > 1 => {
                        view.detail_run_id = None;
                        view.phase_pinned = false;
                        self.workflows_view = view;
                        InputOutcome::Changed
                    }
                    KeyCode::Left if in_detail => {
                        view.detail_run_id = None;
                        view.phase_pinned = false;
                        self.workflows_view = view;
                        InputOutcome::Changed
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if let Some(run) = view.detail_run(&runs) {
                            view.select_phase(view.selected_phase.saturating_sub(1), run);
                        } else {
                            view.select_previous_list_item(&runs);
                        }
                        self.workflows_view = view;
                        InputOutcome::Changed
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if let Some(run) = view.detail_run(&runs) {
                            view.select_phase(view.selected_phase.saturating_add(1), run);
                        } else {
                            view.select_next_list_item(&runs);
                        }
                        self.workflows_view = view;
                        InputOutcome::Changed
                    }
                    KeyCode::Enter if in_detail => {
                        if let Some(run) = view.detail_run(&runs)
                            && let Some(agent_id) =
                                transcript_target(run, view.selected_phase_name.as_deref())
                        {
                            self.open_subagent_fullscreen(agent_id);
                        }
                        self.workflows_view = view;
                        InputOutcome::Changed
                    }
                    KeyCode::Enter | KeyCode::Right if !in_detail => {
                        if let Some(command) =
                            definition_command("edit", view.selected_definition())
                        {
                            self.show_workflows = false;
                            return Some(InputOutcome::Action(
                                Action::SendSlashCommandPreservingDraft(command),
                            ));
                        } else if let Some(run) = runs.get(view.selected_run) {
                            view.selected_run_id = Some(run.run_id.clone());
                            view.detail_run_id = Some(run.run_id.clone());
                            view.phase_pinned = false;
                        }
                        self.workflows_view = view;
                        InputOutcome::Changed
                    }
                    KeyCode::Char('f')
                    | KeyCode::Char('e')
                    | KeyCode::Char('v')
                    | KeyCode::Char('d')
                        if !in_detail && view.selected_definition().is_some() =>
                    {
                        let operation = match key.code {
                            KeyCode::Char('f') => "focus",
                            KeyCode::Char('e') => "edit",
                            KeyCode::Char('v') => "validate",
                            KeyCode::Char('d') => "discard",
                            _ => unreachable!(),
                        };
                        match definition_command(operation, view.selected_definition()) {
                            Some(command) => {
                                self.show_workflows = false;
                                InputOutcome::Action(Action::SendSlashCommandPreservingDraft(
                                    command,
                                ))
                            }
                            None => InputOutcome::Changed,
                        }
                    }
                    KeyCode::Char('r') | KeyCode::Char('p')
                        if !in_detail && view.selected_definition().is_some() =>
                    {
                        let operation = if key.code == KeyCode::Char('r') {
                            "run"
                        } else {
                            "publish"
                        };
                        match definition_command(operation, view.selected_definition()) {
                            Some(command) => {
                                self.show_workflows = false;
                                InputOutcome::Action(Action::SendSlashCommandPreservingDraft(
                                    command,
                                ))
                            }
                            None => InputOutcome::Changed,
                        }
                    }
                    KeyCode::Char('p') | KeyCode::Char('r') | KeyCode::Char('x') => {
                        let op = match key.code {
                            KeyCode::Char('p') => "pause",
                            KeyCode::Char('r') => "resume",
                            KeyCode::Char('x') => "stop",
                            _ => unreachable!(),
                        };
                        let command = resolve_management_command(op, manage_target);
                        match command {
                            Some(command) => {
                                self.show_workflows = false;
                                InputOutcome::Action(Action::SendSlashCommandPreservingDraft(
                                    command,
                                ))
                            }
                            None => InputOutcome::Changed,
                        }
                    }
                    _ => InputOutcome::Changed,
                };
                return Some(outcome);
            }
            if let Event::Mouse(mouse) = ev {
                use crate::views::modal_window::{ModalWindowOutcome, handle_modal_mouse};
                use crate::views::workflows::shortcut_ids;

                let runs = self.workflow_runs_newest_first();
                let mut view = self.workflows_view.clone();
                view.normalize(&runs);
                let in_detail = view.detail_run(&runs).is_some();
                let manage_target = view.detail_run(&runs).or_else(|| {
                    (view.list_section == crate::views::workflows::WorkflowListSection::Runs)
                        .then(|| runs.get(view.selected_run).copied())
                        .flatten()
                });
                let outcome =
                    handle_modal_mouse(&mut view.window, mouse.kind, mouse.column, mouse.row);

                let result = match outcome {
                    ModalWindowOutcome::CloseRequested => {
                        if in_detail && runs.len() > 1 {
                            view.detail_run_id = None;
                            view.phase_pinned = false;
                        } else {
                            self.show_workflows = false;
                        }
                        InputOutcome::Changed
                    }
                    ModalWindowOutcome::ShortcutActivated(shortcut_ids::OPEN) => {
                        if let Some(run) = runs.get(view.selected_run) {
                            view.selected_run_id = Some(run.run_id.clone());
                            view.detail_run_id = Some(run.run_id.clone());
                            view.phase_pinned = false;
                        }
                        InputOutcome::Changed
                    }
                    ModalWindowOutcome::ShortcutActivated(shortcut_ids::RUNS) => {
                        view.detail_run_id = None;
                        view.phase_pinned = false;
                        InputOutcome::Changed
                    }
                    ModalWindowOutcome::ShortcutActivated(id) => {
                        let definition_operation = match id {
                            shortcut_ids::FOCUS => Some("focus"),
                            shortcut_ids::EDIT => Some("edit"),
                            shortcut_ids::VALIDATE => Some("validate"),
                            shortcut_ids::RUN_DEFINITION => Some("run"),
                            shortcut_ids::PUBLISH => Some("publish"),
                            shortcut_ids::DISCARD => Some("discard"),
                            _ => None,
                        };
                        if let Some(command) = definition_operation.and_then(|operation| {
                            definition_command(operation, view.selected_definition())
                        }) {
                            self.show_workflows = false;
                            return Some(InputOutcome::Action(
                                Action::SendSlashCommandPreservingDraft(command),
                            ));
                        }
                        let op = match id {
                            shortcut_ids::PAUSE => Some("pause"),
                            shortcut_ids::RESUME => Some("resume"),
                            shortcut_ids::STOP => Some("stop"),
                            _ => None,
                        };
                        match op.and_then(|op| resolve_management_command(op, manage_target)) {
                            Some(cmd) => {
                                self.show_workflows = false;
                                InputOutcome::Action(Action::SendSlashCommandPreservingDraft(cmd))
                            }
                            None => InputOutcome::Changed,
                        }
                    }
                    ModalWindowOutcome::Unhandled
                        if matches!(
                            mouse.kind,
                            crossterm::event::MouseEventKind::Down(
                                crossterm::event::MouseButton::Left
                            )
                        ) =>
                    {
                        let hit = |r: &ratatui::layout::Rect| {
                            mouse.column >= r.x
                                && mouse.column < r.x + r.width
                                && mouse.row >= r.y
                                && mouse.row < r.y + r.height
                        };
                        if in_detail {
                            if let Some(agent_id) = view
                                .agent_hits
                                .iter()
                                .find(|(r, _)| hit(r))
                                .map(|(_, id)| id.clone())
                            {
                                self.open_subagent_fullscreen(agent_id);
                            } else if let Some(phase_name) = view
                                .phase_hits
                                .iter()
                                .find(|(rect, _)| hit(rect))
                                .map(|(_, phase_name)| phase_name.clone())
                                && let Some(run) = view.detail_run(&runs)
                                && let Some(idx) = crate::views::workflows::phase_rail(run)
                                    .iter()
                                    .position(|(title, _)| title == &phase_name)
                            {
                                view.select_phase(idx, run);
                            }
                        } else if let Some(definition_id) = view
                            .definition_hits
                            .iter()
                            .find(|(rect, _)| hit(rect))
                            .map(|(_, id)| id.clone())
                        {
                            if let Some(pos) = view
                                .definitions
                                .iter()
                                .position(|definition| definition.definition_id == definition_id)
                            {
                                view.select_definition(pos);
                            }
                        } else if let Some(run_id) = view
                            .run_hits
                            .iter()
                            .find(|(r, _)| hit(r))
                            .map(|(_, id)| id.clone())
                        {
                            if let Some(pos) = runs.iter().position(|run| run.run_id == run_id) {
                                view.select_run(pos, &runs);
                            }
                            view.detail_run_id = Some(run_id);
                            view.phase_pinned = false;
                        }
                        InputOutcome::Changed
                    }
                    _ => InputOutcome::Changed,
                };
                self.workflows_view = view;
                return Some(result);
            }
        }
        None
    }
}

#[cfg(test)]
mod workflows_overlay_key_tests {
    use super::AgentView;
    use super::test_fixtures::make_agent;
    use crate::actions::ActionRegistry;
    use crate::app::actions::Action;
    use crate::app::root::InputOutcome;
    use crate::app::session::WorkflowRunSnapshot;
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn modified_key(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
    }

    fn make_workflow_run(run_id: &str) -> WorkflowRunSnapshot {
        WorkflowRunSnapshot {
            run_id: run_id.to_string(),
            definition_id: None,
            definition_scope: None,
            definition_hash: None,
            name: "deep-research".to_string(),
            objective: "obj".to_string(),
            status: "active".to_string(),
            management_available: true,
            builtin: false,
            phases: Vec::new(),
            current_phase: None,
            agents: Vec::new(),
            agent_budget: None,
            agents_used: 0,
            agents_remaining: None,
            agent_usage_incomplete: false,
            active_agents: 0,
            elapsed_ms: 1_000,
            received_at: std::time::Instant::now(),
            pause_message: None,
            result_summary: None,
        }
    }

    fn workflows_agent(run_ids: &[&str]) -> AgentView {
        let mut agent = make_agent();
        for id in run_ids {
            agent.session.workflow_runs.push(make_workflow_run(id));
        }
        agent.show_workflows = true;
        agent
    }

    #[test]
    fn ctrl_q_bubbles_and_g_closes_from_detail() {
        let mut agent = workflows_agent(&["wf_old", "wf_new"]);
        let reg = ActionRegistry::defaults();
        let mut effects = Vec::new();

        let ctrl_q = modified_key(KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert!(matches!(
            agent.handle_input(&ctrl_q, &reg, &mut effects),
            InputOutcome::Unchanged
        ));
        assert!(agent.show_workflows);

        assert!(matches!(
            agent.handle_input(&key(KeyCode::Enter), &reg, &mut effects),
            InputOutcome::Changed
        ));
        assert!(agent.workflows_view.detail_run_id.is_some());
        assert!(matches!(
            agent.handle_input(&key(KeyCode::Char('g')), &reg, &mut effects),
            InputOutcome::Changed
        ));
        assert!(!agent.show_workflows);
    }

    #[test]
    fn left_in_detail_returns_to_runs_list() {
        let mut agent = workflows_agent(&["wf_old", "wf_new"]);
        let reg = ActionRegistry::defaults();
        let mut effects = Vec::new();

        assert!(matches!(
            agent.handle_input(&key(KeyCode::Enter), &reg, &mut effects),
            InputOutcome::Changed
        ));
        assert_eq!(
            agent.workflows_view.detail_run_id.as_deref(),
            Some("wf_new")
        );

        assert!(matches!(
            agent.handle_input(&key(KeyCode::Left), &reg, &mut effects),
            InputOutcome::Changed
        ));
        assert_eq!(agent.workflows_view.detail_run_id, None);
        assert!(agent.show_workflows, "Left must not close the overlay");
    }

    #[test]
    fn left_in_detail_has_no_run_count_guard() {
        let mut agent = workflows_agent(&["wf_only"]);
        let reg = ActionRegistry::defaults();
        let mut effects = Vec::new();
        agent.workflows_view.detail_run_id = Some("wf_only".to_string());
        agent.workflows_view.phase_pinned = true;

        assert!(matches!(
            agent.handle_input(&key(KeyCode::Left), &reg, &mut effects),
            InputOutcome::Changed
        ));
        assert_eq!(agent.workflows_view.detail_run_id, None);
        assert!(!agent.workflows_view.phase_pinned);
        assert!(agent.show_workflows);
    }

    #[test]
    fn left_on_list_is_consumed_noop() {
        let mut agent = workflows_agent(&["wf_old", "wf_new"]);
        let reg = ActionRegistry::defaults();
        let mut effects = Vec::new();
        assert_eq!(agent.workflows_view.detail_run_id, None);

        let out = agent.handle_input(&key(KeyCode::Left), &reg, &mut effects);
        assert!(
            matches!(out, InputOutcome::Changed),
            "Left on the list must stay consumed by the overlay, got {out:?}"
        );
        assert_eq!(agent.workflows_view.detail_run_id, None);
        assert_eq!(agent.workflows_view.selected_run, 0);
        assert!(agent.show_workflows);
    }

    #[test]
    fn run_selection_survives_newest_first_insert() {
        let mut agent = workflows_agent(&["wf_old", "wf_selected"]);
        let reg = ActionRegistry::defaults();
        let mut effects = Vec::new();

        assert!(matches!(
            agent.handle_input(&key(KeyCode::Down), &reg, &mut effects),
            InputOutcome::Changed
        ));
        assert_eq!(
            agent.workflows_view.selected_run_id.as_deref(),
            Some("wf_old")
        );
        agent
            .session
            .workflow_runs
            .push(make_workflow_run("wf_newest"));

        assert!(matches!(
            agent.handle_input(&key(KeyCode::Enter), &reg, &mut effects),
            InputOutcome::Changed
        ));
        assert_eq!(
            agent.workflows_view.detail_run_id.as_deref(),
            Some("wf_old")
        );
    }

    #[test]
    fn enter_in_detail_opens_selected_phase_transcript() {
        let mut agent = workflows_agent(&["wf_run"]);
        let run = agent.session.workflow_runs.last_mut().unwrap();
        run.phases = vec![("Research".to_owned(), "active".to_owned())];
        run.current_phase = Some("Research".to_owned());
        run.agents = vec![
            crate::app::session::WorkflowAgentRowView {
                agent_id: "child-done".to_owned(),
                label: "done".to_owned(),
                phase: Some("Research".to_owned()),
                model: None,
                state: "done".to_owned(),
                tokens_used: 0,
                duration_ms: 0,
            },
            crate::app::session::WorkflowAgentRowView {
                agent_id: "child-running".to_owned(),
                label: "running".to_owned(),
                phase: Some("Research".to_owned()),
                model: None,
                state: "running".to_owned(),
                tokens_used: 0,
                duration_ms: 0,
            },
        ];
        agent
            .subagent_views
            .insert("child-running".to_owned(), Box::new(make_agent()));
        let reg = ActionRegistry::defaults();
        let mut effects = Vec::new();

        assert!(matches!(
            agent.handle_input(&key(KeyCode::Enter), &reg, &mut effects),
            InputOutcome::Changed
        ));
        assert_eq!(agent.active_subagent.as_deref(), Some("child-running"));
        assert!(agent.show_workflows);
    }

    #[test]
    fn modified_local_chars_do_not_trigger_workflow_controls() {
        let mut agent = workflows_agent(&["wf_run"]);
        let reg = ActionRegistry::defaults();
        let mut effects = Vec::new();
        agent.workflows_view.detail_run_id = Some("wf_run".to_string());

        for ch in ['p', 'r', 'x', 's', 'q', 'j', 'k'] {
            agent.show_workflows = true;
            let out = agent.handle_input(
                &modified_key(KeyCode::Char(ch), KeyModifiers::CONTROL),
                &reg,
                &mut effects,
            );
            assert!(agent.show_workflows, "Ctrl+{ch} must not close the modal");
            assert!(
                !matches!(
                    out,
                    InputOutcome::Action(Action::SendSlashCommandPreservingDraft(_))
                ),
                "Ctrl+{ch} must not dispatch a local workflow command"
            );
        }
    }

    #[test]
    fn paused_budget_limited_and_failed_runs_are_resumable_others_fail_closed() {
        let mut agent = workflows_agent(&["wf_run"]);
        let reg = ActionRegistry::defaults();
        let mut effects = Vec::new();
        agent.session.workflow_runs[0].status = "user_paused".to_string();
        agent.workflows_view.detail_run_id = Some("wf_run".to_string());

        let out = agent.handle_input(&key(KeyCode::Char('r')), &reg, &mut effects);
        assert!(matches!(
            out,
            InputOutcome::Action(Action::SendSlashCommandPreservingDraft(ref command))
                if command == "/workflow-run resume deep-research"
        ));

        agent.show_workflows = true;
        agent.session.workflow_runs[0].status = "budget_limited".to_string();
        let out = agent.handle_input(&key(KeyCode::Char('r')), &reg, &mut effects);
        assert!(matches!(
            out,
            InputOutcome::Action(Action::SendSlashCommandPreservingDraft(ref command))
                if command == "/workflow-run resume deep-research"
        ));
        assert!(
            !agent.show_workflows,
            "budget-limited r closes the overlay so the shell reply is visible"
        );

        agent.show_workflows = true;
        agent.session.workflow_runs[0].status = "failed".to_string();
        let out = agent.handle_input(&key(KeyCode::Char('r')), &reg, &mut effects);
        assert!(
            matches!(
                out,
                InputOutcome::Action(Action::SendSlashCommandPreservingDraft(ref command))
                    if command == "/workflow-run resume deep-research"
            ),
            "failed runs resume via journal replay"
        );
        assert!(
            !agent.show_workflows,
            "failed r dispatches a resume and closes the overlay"
        );

        agent.show_workflows = true;
        agent.session.workflow_runs[0].status = "complete".to_string();
        let out = agent.handle_input(&key(KeyCode::Char('r')), &reg, &mut effects);
        assert!(matches!(out, InputOutcome::Changed));
        assert!(agent.show_workflows, "completed runs must not be resumed");

        agent.session.workflow_runs[0].status = "user_paused".to_string();
        agent.session.workflow_runs[0].management_available = false;
        let out = agent.handle_input(&key(KeyCode::Char('r')), &reg, &mut effects);
        assert!(matches!(out, InputOutcome::Changed));
        assert!(agent.show_workflows, "unsupported resume must fail closed");

        agent.session.workflow_runs[0].status = "budget_limited".to_string();
        agent.session.workflow_runs[0].management_available = false;
        let out = agent.handle_input(&key(KeyCode::Char('r')), &reg, &mut effects);
        assert!(matches!(out, InputOutcome::Changed));
        assert!(
            agent.show_workflows,
            "budget-limited without management must not dispatch"
        );
    }

    #[test]
    fn right_on_list_opens_selected_run_detail() {
        let mut agent = workflows_agent(&["wf_old", "wf_new"]);
        let reg = ActionRegistry::defaults();
        let mut effects = Vec::new();

        assert!(matches!(
            agent.handle_input(&key(KeyCode::Right), &reg, &mut effects),
            InputOutcome::Changed
        ));
        assert_eq!(
            agent.workflows_view.detail_run_id.as_deref(),
            Some("wf_new")
        );

        assert!(matches!(
            agent.handle_input(&key(KeyCode::Right), &reg, &mut effects),
            InputOutcome::Changed
        ));
        assert_eq!(
            agent.workflows_view.detail_run_id.as_deref(),
            Some("wf_new")
        );
    }

    fn mouse_down(column: u16, row: u16) -> Event {
        Event::Mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }

    fn rect(x: u16, y: u16, w: u16, h: u16) -> ratatui::layout::Rect {
        ratatui::layout::Rect::new(x, y, w, h)
    }

    #[test]
    fn click_on_roster_agent_opens_transcript_fullscreen_over_overlay() {
        let mut agent = workflows_agent(&["wf_run"]);
        agent.workflows_view.agent_hits = vec![(rect(10, 5, 30, 1), "child-1".to_string())];
        agent
            .subagent_views
            .insert("child-1".to_string(), Box::new(make_agent()));
        let reg = ActionRegistry::defaults();
        let mut effects = Vec::new();

        let out = agent.handle_input(&mouse_down(12, 5), &reg, &mut effects);
        assert!(matches!(out, InputOutcome::Changed));
        assert_eq!(
            agent.active_subagent.as_deref(),
            Some("child-1"),
            "roster click must open the child transcript fullscreen"
        );
        assert!(
            agent.show_workflows,
            "the overlay stays open underneath so closing the transcript returns to it"
        );
    }

    #[test]
    fn click_on_roster_agent_without_local_view_is_consumed_noop() {
        let mut agent = workflows_agent(&["wf_run"]);
        agent.workflows_view.agent_hits = vec![(rect(10, 5, 30, 1), "ghost".to_string())];
        let reg = ActionRegistry::defaults();
        let mut effects = Vec::new();

        let out = agent.handle_input(&mouse_down(12, 5), &reg, &mut effects);
        assert!(matches!(out, InputOutcome::Changed));
        assert_eq!(agent.active_subagent, None);
        assert!(agent.show_workflows);
    }

    #[test]
    fn click_on_phase_row_pins_that_phase() {
        let mut agent = make_agent();
        let mut run = make_workflow_run("wf_run");
        run.phases = vec![
            ("Plan".to_string(), "active".to_string()),
            ("Do".to_string(), "pending".to_string()),
        ];
        agent.session.workflow_runs.push(run);
        agent.show_workflows = true;
        agent.workflows_view.phase_hits = vec![
            (rect(2, 3, 12, 1), "Plan".to_owned()),
            (rect(2, 4, 12, 1), "Do".to_owned()),
        ];
        let reg = ActionRegistry::defaults();
        let mut effects = Vec::new();

        let out = agent.handle_input(&mouse_down(3, 4), &reg, &mut effects);
        assert!(matches!(out, InputOutcome::Changed));
        assert_eq!(agent.workflows_view.selected_phase, 1);
        assert_eq!(
            agent.workflows_view.selected_phase_name.as_deref(),
            Some("Do")
        );
        assert!(
            agent.workflows_view.phase_pinned,
            "a clicked phase must pin (stop following the active phase)"
        );
    }

    #[test]
    fn click_on_run_row_opens_that_run_detail() {
        let mut agent = workflows_agent(&["wf_old", "wf_new"]);
        agent.workflows_view.run_hits = vec![
            (rect(5, 4, 60, 1), "wf_new".to_string()),
            (rect(5, 5, 60, 1), "wf_old".to_string()),
        ];
        let reg = ActionRegistry::defaults();
        let mut effects = Vec::new();

        let out = agent.handle_input(&mouse_down(6, 5), &reg, &mut effects);
        assert!(matches!(out, InputOutcome::Changed));
        assert_eq!(
            agent.workflows_view.detail_run_id.as_deref(),
            Some("wf_old"),
            "clicking a run row opens its detail (mouse mirror of Enter)"
        );
        assert_eq!(agent.workflows_view.selected_run, 1);
        assert!(agent.show_workflows);
    }

    #[test]
    fn click_on_empty_body_space_is_consumed_and_keeps_overlay() {
        let mut agent = workflows_agent(&["wf_old", "wf_new"]);
        agent.workflows_view.run_hits = vec![(rect(5, 4, 60, 1), "wf_new".to_string())];
        agent.workflows_view.window.popup_area = Some(rect(0, 0, 100, 30));
        let reg = ActionRegistry::defaults();
        let mut effects = Vec::new();

        let out = agent.handle_input(&mouse_down(50, 20), &reg, &mut effects);
        assert!(matches!(out, InputOutcome::Changed));
        assert!(agent.show_workflows);
        assert_eq!(agent.workflows_view.detail_run_id, None);
    }
}
