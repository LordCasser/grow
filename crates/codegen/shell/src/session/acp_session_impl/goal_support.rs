//! Goal-harness support for `SessionActor`: reminder/directive templates,
//! goal wrappers, availability, and goal token accounting.

use super::*;

/// Number of consecutive non-completing goal-mode turns before the goal
/// auto-pauses with `GoalPauseReason::BackOff`. See `handle_turn_end`.
/// Compile-time constant for v1; remote tunability is a deferred follow-up.
pub(super) const GOAL_CONTINUATION_BACKOFF_THRESHOLD: u32 = 3;

const GOAL_PAUSED_INTERACTION_REMINDER: &str = "The current Goal is paused. Respond to the user's immediate message using the Goal objective and existing context, but do not resume autonomous Goal execution, schedule another Goal cycle, or claim the Goal is complete. The Goal remains paused until the user explicitly resumes it.";

const GOAL_PLAN_MAX_BYTES: u64 = 1024 * 1024;

async fn validate_goal_plan_staging(
    actual: &std::path::Path,
    expected: &std::path::Path,
) -> Result<(Vec<u8>, String), String> {
    if actual != expected {
        return Err(format!(
            "planner returned unexpected staging path: {}",
            actual.display()
        ));
    }
    let metadata = tokio::fs::symlink_metadata(actual)
        .await
        .map_err(|error| format!("cannot stat staged plan: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("staged plan must be a regular non-symlink file".into());
    }
    if metadata.len() == 0 || metadata.len() > GOAL_PLAN_MAX_BYTES {
        return Err(format!(
            "staged plan size {} is outside 1..={GOAL_PLAN_MAX_BYTES}",
            metadata.len()
        ));
    }
    let bytes = tokio::fs::read(actual)
        .await
        .map_err(|error| format!("cannot read staged plan: {error}"))?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| format!("staged plan is not UTF-8: {error}"))?;
    let has_markdown_task = text.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("- [ ] ")
            || line.starts_with("- [x] ")
            || line.starts_with("- [X] ")
            || line.starts_with("* [ ] ")
            || line.starts_with("* [x] ")
            || line.starts_with("* [X] ")
    });
    if !has_markdown_task {
        return Err("staged plan contains no parseable Markdown task".into());
    }
    let content_hash = blake3::hash(&bytes).to_hex().to_string();
    Ok((bytes, content_hash))
}

impl SessionActor {
    /// Move the single live Todo resource into Goal scope while parking the
    /// session plan. The ACP wire remains an ordinary full Plan replacement;
    /// scope is an internal ownership rule.
    pub(super) async fn enter_goal_plan_scope(&self) {
        use crate::tools::todo::TodoState;
        use tools::types::resources::State;

        let bridge = self.agent.borrow().tool_bridge().clone();
        let live_plan = bridge
            .read_resource::<State<TodoState>>()
            .await
            .map(|state| state.0)
            .unwrap_or_default();
        let goal_plan = {
            let mut scope = self.goal_plan_scope.lock();
            match scope.as_mut() {
                Some(scope) if scope.goal_active => return,
                Some(scope) => {
                    scope.session = live_plan;
                    scope.goal_active = true;
                    scope.goal.clone()
                }
                None => {
                    let goal = TodoState::default();
                    *scope = Some(super::super::GoalPlanScope {
                        session: live_plan,
                        goal: goal.clone(),
                        goal_active: true,
                    });
                    goal
                }
            }
        };
        self.replace_live_plan(goal_plan).await;
    }

    /// Switch the live resource back to Session scope without retiring the
    /// paused Goal. A later switch to Goal restores its private Todo state.
    pub(super) async fn deactivate_goal_plan_scope(&self) {
        use crate::tools::todo::TodoState;
        use tools::types::resources::State;

        let bridge = self.agent.borrow().tool_bridge().clone();
        let live_goal = bridge
            .read_resource::<State<TodoState>>()
            .await
            .map(|state| state.0)
            .unwrap_or_default();
        let session_plan = {
            let mut scope = self.goal_plan_scope.lock();
            let Some(scope) = scope.as_mut() else {
                return;
            };
            if !scope.goal_active {
                return;
            }
            scope.goal = live_goal;
            scope.goal_active = false;
            scope.session.clone()
        };
        self.replace_live_plan(session_plan).await;
    }

    async fn replace_live_plan(&self, plan: crate::tools::todo::TodoState) {
        use crate::tools::todo::plan_entry_from_todo_item;
        use tools::types::resources::State;

        self.agent
            .borrow()
            .tool_bridge()
            .update_resource(State(plan.clone()))
            .await;
        let _ = self.notifications.persistence_tx.send(
            crate::session::persistence::PersistenceMsg::PlanState(plan.clone()),
        );
        let entries = plan
            .todo_items()
            .cloned()
            .map(plan_entry_from_todo_item)
            .collect();
        self.send_update(acp::SessionUpdate::Plan(acp::Plan::new(entries)), None)
            .await;
    }

    /// Retire Goal plan ownership and restore the pre-Goal session Todo state.
    /// This is shared by clear and verified completion, so a later Normal turn
    /// cannot persist or rebroadcast Goal leftovers. Only ever called on
    /// terminal Goal transitions, so it also performs the staging GC.
    pub(super) async fn retire_goal_plan_scope(&self) {
        // Terminal Goal transitions remove the transient definition-owned
        // staging drafts; the canonical plan.md / plan.baseline.md audit
        // record is retained (see docs/architecture/input-routing.md). The
        // planner recreates the staging dir on demand, and any still-running
        // planner continuation's result is lease-dropped at commit.
        let staging = self.goal_tracker.lock().plan_staging_dir();
        if let Err(error) = std::fs::remove_dir_all(&staging) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    path = %staging.display(),
                    error = %error,
                    "goal plan staging cleanup failed",
                );
            }
        }
        let Some(scope) = self.goal_plan_scope.lock().take() else {
            return;
        };
        if scope.goal_active {
            self.replace_live_plan(scope.session).await;
        }
    }

    pub(super) fn goal_control_generation(&self) -> u64 {
        self.goal_control_generation
            .load(std::sync::atomic::Ordering::Acquire)
    }

    pub(super) fn push_goal_control_notice(&self, content: String) {
        self.pending_system_reminders
            .lock()
            .push(self.goal_directive_item(
                content,
                sampling_types::SyntheticReason::SystemReminder,
                sampling_types::GoalDirectiveKind::ControlNotice,
            ));
        self.goal_control_generation
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.goal_control_notify.notify_waiters();
    }

    pub(super) async fn wait_goal_control_change(&self, generation: u64) {
        loop {
            let notified = self.goal_control_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.goal_control_generation() != generation {
                return;
            }
            notified.await;
        }
    }

    pub(super) fn current_goal_directive_tag(
        &self,
        kind: sampling_types::GoalDirectiveKind,
    ) -> Option<sampling_types::GoalDirectiveTag> {
        let tracker = self.goal_tracker.lock();
        let goal = tracker.snapshot()?;
        Some(sampling_types::GoalDirectiveTag {
            goal_id: goal.goal_id.clone(),
            definition_revision: goal.definition_revision,
            kind,
        })
    }

    pub(super) fn goal_directive_item(
        &self,
        content: impl Into<String>,
        reason: sampling_types::SyntheticReason,
        kind: sampling_types::GoalDirectiveKind,
    ) -> ConversationItem {
        match self.current_goal_directive_tag(kind) {
            Some(tag) => ConversationItem::goal_directive(content, reason, tag),
            None => ConversationItem::system_reminder(content),
        }
    }

    pub(super) fn push_current_goal_context(&self) {
        let objective = self
            .goal_tracker
            .lock()
            .snapshot()
            .map(|goal| goal.objective.clone());
        let Some(objective) = objective else {
            return;
        };
        self.chat_state_handle
            .push_user_message(self.goal_directive_item(
                format!("<goal-context>\nObjective: {objective}\n</goal-context>"),
                sampling_types::SyntheticReason::SystemReminder,
                sampling_types::GoalDirectiveKind::Context,
            ));
    }

    pub(super) async fn inject_paused_goal_interaction_directive(&self) {
        if self.behavior.lock().behavior() != Some(tool_types::BehaviorId::Goal)
            || !self
                .goal_tracker
                .lock()
                .status()
                .is_some_and(|status| status.is_paused())
        {
            return;
        }
        // One PausedInteraction directive per (goal_id, definition_revision):
        // if the conversation already carries this exact tag, the model has
        // seen it and later paused user turns skip the duplicate. A revised
        // Goal bumps the revision, so its next paused turn re-injects with the
        // fresh tag; after compaction removed the directive from context,
        // re-injecting once is correct again.
        let Some(tag) =
            self.current_goal_directive_tag(sampling_types::GoalDirectiveKind::PausedInteraction)
        else {
            return;
        };
        if self
            .chat_state_handle
            .get_conversation()
            .await
            .iter()
            .any(|item| {
                matches!(
                    item,
                    ConversationItem::User(user) if user.goal_directive.as_ref() == Some(&tag)
                )
            })
        {
            return;
        }
        self.chat_state_handle
            .push_user_message(self.goal_directive_item(
                format!(
                    "<{}>\n{}\n</{}>",
                    self.reminder_wrapper_tag(),
                    GOAL_PAUSED_INTERACTION_REMINDER,
                    self.reminder_wrapper_tag()
                ),
                sampling_types::SyntheticReason::SystemReminder,
                sampling_types::GoalDirectiveKind::PausedInteraction,
            ));
    }

    pub(super) fn project_goal_conversation(
        &self,
        items: Vec<ConversationItem>,
        include_autonomy: bool,
    ) -> Vec<ConversationItem> {
        if self.behavior.lock().behavior() != Some(tool_types::BehaviorId::Goal) {
            return sampling_types::project_conversation_for_goal_scope(
                items,
                sampling_types::GoalConversationProjection::Exclude,
            );
        }
        let definition = self
            .goal_tracker
            .lock()
            .snapshot()
            .map(|goal| (goal.goal_id.clone(), goal.definition_revision, goal.status));
        match definition.as_ref() {
            Some((goal_id, revision, crate::session::goal_tracker::GoalStatus::Active)) => {
                sampling_types::project_conversation_for_goal_scope(
                    items,
                    sampling_types::GoalConversationProjection::Active {
                        goal_id,
                        definition_revision: *revision,
                        include_autonomy,
                    },
                )
            }
            Some((goal_id, revision, status)) if status.is_paused() => {
                sampling_types::project_conversation_for_goal_scope(
                    items,
                    sampling_types::GoalConversationProjection::Paused {
                        goal_id,
                        definition_revision: *revision,
                    },
                )
            }
            _ => sampling_types::project_conversation_for_goal_scope(
                items,
                sampling_types::GoalConversationProjection::Exclude,
            ),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct GoalClassifierPolicy {
    pub enabled: bool,
    pub max_runs: u32,
}

impl DrainSource {
    /// Consume the source, returning `(input, Option<ack_tx>)`.
    /// `None` means the ack was already resolved.
    pub(super) fn into_parts(
        self,
    ) -> (
        tools::implementations::grow_build::update_goal::UpdateGoalInput,
        Option<
            tokio::sync::oneshot::Sender<
                tools::implementations::grow_build::update_goal::UpdateGoalAck,
            >,
        >,
    ) {
        match self {
            Self::Pending(i) => (i, None),
            Self::Channel((i, ack)) => (i, Some(ack)),
        }
    }
}

/// Send an `UpdateGoalAck` only if the ack channel is live (channel
/// source). No-op for `Pending` source — its ack was already resolved.
pub(super) fn try_send_ack(
    ack_tx: Option<
        tokio::sync::oneshot::Sender<
            tools::implementations::grow_build::update_goal::UpdateGoalAck,
        >,
    >,
    ack: tools::implementations::grow_build::update_goal::UpdateGoalAck,
) {
    if let Some(tx) = ack_tx {
        send_ack(tx, ack);
    }
}

pub(super) fn send_ack(
    ack_tx: tokio::sync::oneshot::Sender<
        tools::implementations::grow_build::update_goal::UpdateGoalAck,
    >,
    ack: tools::implementations::grow_build::update_goal::UpdateGoalAck,
) {
    if ack_tx.send(ack).is_err() {
        tracing::debug!("update_goal ack receiver dropped before harness could respond");
    }
}

pub(super) const GOAL_CLASSIFIER_PENDING_QUEUE_CAP: usize = 4;

pub(super) struct InFlightGuard<'a> {
    pub(super) flag: &'a std::sync::atomic::AtomicBool,
}

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        self.flag.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

impl NotAchievedSyntheticReason {
    /// Per-variant Markdown body for the synthetic details file. The
    /// exhaustive match forces a new variant to supply its own prose.
    pub(super) fn details_body(self, attempt: u32, max_runs: u32) -> String {
        match self {
            Self::ConcurrentInFlight => format!(
                "# Goal classifier — Not Achieved (synthetic)\n\n\
                 **Attempt:** {attempt} of {max_runs}\n\
                 **Reason:** ConcurrentInFlight\n\n\
                 The harness short-circuited this `update_goal(completed: true)` call \
                 because a classifier was already in flight for the previous completion \
                 attempt. The second attempt was accounted as Not Achieved (counting \
                 toward `classifier_max_runs`) to bound model retry spam.\n\n\
                 No sampler call was made; no token cost was incurred for this attempt.\n",
            ),
        }
    }

    /// One-line gaps gist inlined into the rejection nudge for the
    /// synthetic (no-sampler) `NotAchieved` path. Mirrors the bullet
    /// shape of `goal_classifier::build_gaps_summary` so the nudge
    /// reads uniformly regardless of which path produced it.
    pub(super) fn gaps_summary(self) -> String {
        match self {
            Self::ConcurrentInFlight => "- a verification was already running for the previous \
                 completion attempt; this duplicate `completed: true` was rejected without \
                 re-running the panel"
                .to_string(),
        }
    }
}

/// Disarm-able tracker scope-guard: runs `on_drop` on scope exit AND when a
/// state can never outlive its run.
///
/// No `GoalUpdated` from `Drop`: the Ctrl+C cancel path emits its own
/// auto-pause update strictly after the turn future is dropped, so cleared
/// state reaches the pager on that emit. `Drop` re-locks the non-reentrant
/// tracker mutex — never let the guard drop while holding that lock.
pub(super) struct TrackerDropGuard<'a, F: FnOnce(&mut crate::session::goal_tracker::GoalTracker)> {
    tracker: &'a parking_lot::Mutex<crate::session::goal_tracker::GoalTracker>,
    on_drop: Option<F>,
}

impl<'a, F: FnOnce(&mut crate::session::goal_tracker::GoalTracker)> TrackerDropGuard<'a, F> {
    pub(super) fn new(
        tracker: &'a parking_lot::Mutex<crate::session::goal_tracker::GoalTracker>,
        on_drop: F,
    ) -> Self {
        Self {
            tracker,
            on_drop: Some(on_drop),
        }
    }

    /// Cancel the drop action — the guarded state was applied normally.
    pub(super) fn disarm(&mut self) {
        self.on_drop = None;
    }
}

impl<F: FnOnce(&mut crate::session::goal_tracker::GoalTracker)> Drop for TrackerDropGuard<'_, F> {
    fn drop(&mut self) {
        if let Some(f) = self.on_drop.take() {
            f(&mut self.tracker.lock());
        }
    }
}

/// Result of [`SessionActor::resume_goal()`] (`/goal resume`).
pub(super) enum GoalResumeOutcome {
    /// Resume/nudge succeeded: run an inference turn seeded with `reminder`
    /// as the turn content; `user_msg` is the slash-command output.
    Inference { reminder: String, user_msg: String },
    /// Terminal/no-op (`AlreadyComplete` | `BudgetLimited` | `NoGoal` |
    /// missing goal state): print this message and end the turn.
    Message(String),
}

/// Goal-only `<task_completion_discipline>` (Rules 1–4); `{TODO_TOOL}` from [`GoalToolNames`].
/// Template must end with `\n` so `{DISCIPLINE_BLOCK}TRACKING:` glues correctly.
pub(super) fn render_goal_task_discipline(names: &GoalToolNames) -> String {
    GOAL_TASK_DISCIPLINE_TEMPLATE.replace("{TODO_TOOL}", &names.todo)
}

/// Render the plan-aware reminder block. `Plan: <abs path>` renders
/// on its own column-0 line — a single line-delimited pointer the
/// model and any downstream consumer (debug log scraper, support
/// tooling) can extract reliably, so keep the format stable.
pub(super) fn render_goal_plan_block(plan_path: &std::path::Path, names: &GoalToolNames) -> String {
    debug_assert!(
        !plan_path.as_os_str().is_empty(),
        "render_goal_plan_block requires a non-empty plan_path; an \
         empty path renders a dangling `Plan:` line that the model \
         cannot follow",
    );
    // Column-0 single-line `Plan: <abs>` contract — see fn docs.
    GOAL_PLAN_BLOCK_TEMPLATE
        .replace("{PLAN_PATH}", &plan_path.display().to_string())
        .replace("{TODO_TOOL}", &names.todo)
}

/// Plan path for the goal-mode reminder, or `None` when no planner artifact
/// exists. `Some` only when the planner is enabled (`GROW_GOAL_PLANNER`) and a
/// plan exists; disabled means no plan block and no dangling `Plan:` line.
/// All three render sites (`setup_goal`, `resume_goal`, continuation nudge)
/// route through this helper so the gate can't drift. Borrows, no alloc.
pub(super) fn goal_reminder_plan_path(
    planner_enabled: bool,
    orchestration: &crate::session::goal_tracker::GoalOrchestration,
) -> Option<&std::path::Path> {
    planner_enabled
        .then_some(orchestration.plan_file.as_deref())
        .flatten()
}

/// Worker rounds a refuted goal may run without re-firing verification
/// before the continuation directive escalates to a forceful "re-verify
/// now" block. A refuted weak model can otherwise churn indefinitely —
/// Override with `GROW_GOAL_REVERIFY_AFTER` (floored at 1).
pub(crate) const GOAL_REVERIFY_AFTER_DEFAULT: u32 = 8;

/// Stable substring present in every rendered continuation directive
/// (from [`GOAL_CONTINUATION_DIRECTIVE_TEMPLATE`]) and in no other
/// reminder. Used to find and drop the prior turn's directive from
/// history so only the latest copy persists.
pub(super) const GOAL_CONTINUATION_SENTINEL: &str =
    "Goal NOT complete — continue working. Next step:";

/// Bail-specific preface substituted into the `{bail_preface}` slot of
/// [`GOAL_CONTINUATION_DIRECTIVE_TEMPLATE`] when the turn-final text
/// matched a [`goal_stop_detector`](super::goal_stop_detector) pattern
/// while pending todos remained. Names the apparent stop and the
/// outstanding work, then lets the unchanged generic body carry the
/// next-step / token / Rule-1/Rule-4 content. The generic flavor
/// substitutes the empty string for this slot.
pub(super) const GOAL_CONTINUATION_BAIL_PREFACE: &str = "You appear to be stopping or handing off, but the goal is NOT complete \
     and todos remain. Do not end the turn here — keep working.\n\n";

/// Render the shared goal-rules template with tool names and
/// site-specific blocks substituted in.
///
/// The template is the slim current form: verification is owned by
/// the harness (the adversarial skeptic panel in `goal_classifier.rs`),
/// so this body only carries TRACKING / WORKING / VERIFY / TEST
/// verdict-file path is substituted — `{VERIFIER_ID}` no longer
/// appears in the template; `verifier_id` continues to anchor
/// harness-owned skeptic verdict files inside `goal_classifier.rs`.
///
/// `block_recap` and `goal_state` are inserted verbatim — pass an
/// empty string to omit either section. The trailing newline of the
/// template is preserved so callers can append their closing
/// directive ("Start now." / "Continue working now.") and the
/// closing `</system-reminder>` tag without extra glue.
///
/// `plan_path` `Some` folds the plan-aware preamble into the same block as
/// the discipline; `None` renders the no-plan block byte-for-byte unchanged.
///
/// Legacy artifacts (the deleted COMPLETION AUDIT / canonical
/// verifier blocks / `{VERIFIER_ID}` placeholder) are pinned absent
/// by `goal_rules_template_drops_all_obsolete_verifier_artifacts`.
pub(super) fn render_goal_rules(
    objective: &str,
    names: &GoalToolNames,
    block_recap: &str,
    goal_state: &str,
    plan_path: Option<&std::path::Path>,
    scratch_dir: &str,
    scratch_ready: bool,
) -> String {
    let discipline = render_goal_task_discipline(names);
    let plan_block = match plan_path {
        Some(path) => render_goal_plan_block(path, names),
        None => String::new(),
    };
    GOAL_RULES_TEMPLATE
        .replace("{OBJECTIVE}", objective)
        .replace("{TASK_TOOL}", &names.task)
        .replace("{TODO_TOOL}", &names.todo)
        .replace("{PLAN_BLOCK}", &plan_block)
        .replace("{BLOCK_RECAP}", block_recap)
        .replace("{DISCIPLINE_BLOCK}", &discipline)
        .replace("{GOAL_STATE}", goal_state)
        // literal `{SCRATCH_DIR}` in the objective WOULD be expanded here
        // (harmless, astronomically unlikely). The `{SCRATCH}` placeholder the
        // text references is a different token, left unreplaced.
        .replace("{SCRATCH_DIR}", scratch_dir)
        // Only claim the dir exists when the harness actually created it.
        .replace(
            "{SCRATCH_STATUS}",
            if scratch_ready {
                "The dir has been created for you."
            } else {
                "Create it with `mkdir -p` if it does not already exist."
            },
        )
}

pub(super) fn render_goal_rules_foreground(
    objective: &str,
    names: &GoalToolNames,
    block_recap: &str,
    goal_state: &str,
    plan_path: Option<&std::path::Path>,
    scratch_dir: &str,
    scratch_ready: bool,
) -> String {
    let discipline = render_goal_task_discipline(names);
    let plan_block = match plan_path {
        Some(path) => render_goal_plan_block(path, names),
        None => String::new(),
    };
    GOAL_RULES_TEMPLATE_FOREGROUND
        .replace("{OBJECTIVE}", objective)
        .replace("{GOAL_TOOL}", &names.goal)
        .replace("{TASK_TOOL}", &names.task)
        .replace("{TODO_TOOL}", &names.todo)
        .replace("{PLAN_BLOCK}", &plan_block)
        .replace("{BLOCK_RECAP}", block_recap)
        .replace("{DISCIPLINE_BLOCK}", &discipline)
        .replace("{GOAL_STATE}", goal_state)
        .replace("{SCRATCH_DIR}", scratch_dir)
        .replace(
            "{SCRATCH_STATUS}",
            if scratch_ready {
                "The dir has been created for you."
            } else {
                "Create it with `mkdir -p` if it does not already exist."
            },
        )
}

/// Assemble a goal auto-pause message: a `headline` summarizing why the
/// goal paused, the grouped-by-classification blocker `pause_summary`
/// (already sanitized upstream), and the `details_path` pointer. The
/// summary block is dropped when empty so a degenerate pause stays
/// well-formed.
pub(super) fn format_goal_pause_message(
    headline: &str,
    pause_summary: &str,
    details_path: &str,
) -> String {
    // An empty `details_path` means the harness has no artifact to point at
    // (e.g. the synthetic-details write was skipped on a squatted scratch
    // root); omit the "See …" pointer rather than dangle a bare "See".
    let has_path = !details_path.trim().is_empty();
    match (pause_summary.trim().is_empty(), has_path) {
        (true, true) => format!("{headline} See {details_path}"),
        (true, false) => headline.to_string(),
        (false, true) => format!("{headline}\n{pause_summary}\nSee {details_path}"),
        (false, false) => format!("{headline}\n{pause_summary}"),
    }
}

/// Make `{` / `}` inert in a model-controlled directive-slot value: a
/// later `.replace` pass and spoof a harness slot. A zero-width space
/// inside each brace keeps legitimate braces (code spans, JSON)
/// visually intact while no `{placeholder}` token can match; borrowed
/// pass-through when brace-free, so clean slots cost no allocation.
fn neutralize_directive_braces(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains(['{', '}']) {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        match c {
            '{' => out.push_str("{\u{200b}"),
            '}' => out.push_str("\u{200b}}"),
            _ => out.push(c),
        }
    }
    std::borrow::Cow::Owned(out)
}

/// Full model-slot sanitization: brace neutralization
/// (anti-`{placeholder}` spoofing) plus reminder-tag neutralization
/// (anti-`</system-reminder>` frame escape). Applied uniformly at the
/// renderer so no slot's defense depends on its producer remembering an
/// axis; re-application to producer-neutralized values is a no-op, and
/// the tag pass only allocates when a frame-tag fragment is present.
fn neutralize_directive_slot(text: &str) -> std::borrow::Cow<'_, str> {
    use crate::session::goal_classifier::neutralize_reminder_tags;

    let out = neutralize_directive_braces(text);
    // Every tag the neutralizer breaks ends in one of these fragments.
    if out.contains("system-reminder>") || out.contains("goal-state>") {
        std::borrow::Cow::Owned(neutralize_reminder_tags(out.into_owned()))
    } else {
        out
    }
}

/// Render the per-turn directive continuation nudge.
///
/// Uses chained `.replace` calls rather than `format!` because the
/// template carries literal `{...}` examples inside backticks that
/// `format!` would force-escape. Placeholders are lowercase (see the
/// doc on [`GOAL_CONTINUATION_DIRECTIVE_TEMPLATE`]).
///
/// `objective` is load-bearing and guarded by `debug_assert!`: an
/// empty value renders an `Objective:` line the agent cannot resolve
/// to a goal. `next_step` is NOT guarded because the production
/// caller's `unwrap_or_else` already substitutes a non-empty fallback
/// (`Check your \`{todo_tool}\` list for next steps.`); guarding here
/// would only catch a future refactor that drops that fallback.
///
/// `plan_pointer` is inlined verbatim; `bail_preface`, `verifier_gaps`
/// and `next_step` pass through [`neutralize_directive_slot`] first but
/// are otherwise inlined as passed — pass [`GOAL_CONTINUATION_BAIL_PREFACE`] for
/// `bail_preface` when the stop-detector fired (and the empty string
/// otherwise); pass the empty string for `plan_pointer` when no plan is
/// available; pass [`render_verifier_gaps_block`]'s output for
/// `verifier_gaps` (empty string when the latest verdict carries no
/// gaps) so the freshest verifier findings render above the next-step
/// line; pass the caller's chosen fallback for `next_step` when the
/// plan yields no concrete step.
///
/// Model-controlled slots (`bail_preface`, `verifier_gaps`,
/// `strategist_note`, `next_step`) are sanitized via
/// [`neutralize_directive_slot`] before substitution — inert under any
/// `.replace` order and unable to close the surrounding
/// `<system-reminder>` frame. `objective` is user-authored and stays
/// verbatim. Pinned by
/// `render_goal_continuation_directive_order_dependent_substitution_pinned`.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_goal_continuation_directive(
    objective: &str,
    tokens: u64,
    elapsed: &str,
    bail_preface: &str,
    plan_pointer: &str,
    verifier_gaps: &str,
    strategist_note: &str,
    reverify_block: &str,
    next_step: &str,
    todo_tool: &str,
    scratch_dir: &str,
    scratch_ready: bool,
) -> String {
    debug_assert!(
        !objective.is_empty(),
        "render_goal_continuation_directive requires a non-empty objective; \
         an empty value leaves the Objective: line dangling in the nudge",
    );
    // Every model-controlled slot is sanitized (bail_preface is a
    // harness const today but sits in the same slot class).
    let bail_preface = neutralize_directive_slot(bail_preface);
    let verifier_gaps = neutralize_directive_slot(verifier_gaps);
    let strategist_note = neutralize_directive_slot(strategist_note);
    let next_step = neutralize_directive_slot(next_step);
    GOAL_CONTINUATION_DIRECTIVE_TEMPLATE
        .replace("{objective}", objective)
        .replace("{tokens}", &tokens.to_string())
        .replace("{elapsed}", elapsed)
        .replace("{bail_preface}", &bail_preface)
        .replace("{plan_pointer}", plan_pointer)
        .replace("{verifier_gaps}", &verifier_gaps)
        .replace("{reverify_block}", reverify_block)
        .replace("{next_step}", &next_step)
        .replace("{todo_tool}", todo_tool)
        // `{SCRATCH}` is left literal — the model resolves it to this dir.
        .replace("{scratch_dir}", scratch_dir)
        // Only claim the dir exists when the harness actually created it.
        .replace(
            "{scratch_status}",
            if scratch_ready {
                "(created for you)"
            } else {
                "(create with `mkdir -p` if missing)"
            },
        )
        .replace("{strategist_note}", &strategist_note)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_goal_continuation_directive_foreground(
    objective: &str,
    tokens: u64,
    elapsed: &str,
    bail_preface: &str,
    plan_pointer: &str,
    verifier_gaps: &str,
    strategist_note: &str,
    reverify_block: &str,
    next_step: &str,
    todo_tool: &str,
    goal_tool: &str,
    scratch_dir: &str,
    scratch_ready: bool,
) -> String {
    debug_assert!(
        !objective.is_empty(),
        "render_goal_continuation_directive_foreground requires a non-empty objective; \
         an empty value leaves the Objective: line dangling in the nudge",
    );
    let bail_preface = neutralize_directive_slot(bail_preface);
    let verifier_gaps = neutralize_directive_slot(verifier_gaps);
    let strategist_note = neutralize_directive_slot(strategist_note);
    let next_step = neutralize_directive_slot(next_step);
    GOAL_CONTINUATION_DIRECTIVE_TEMPLATE_FOREGROUND
        .replace("{objective}", objective)
        .replace("{tokens}", &tokens.to_string())
        .replace("{elapsed}", elapsed)
        .replace("{bail_preface}", &bail_preface)
        .replace("{plan_pointer}", plan_pointer)
        .replace("{verifier_gaps}", &verifier_gaps)
        .replace("{reverify_block}", reverify_block)
        .replace("{next_step}", &next_step)
        .replace("{todo_tool}", todo_tool)
        .replace("{goal_tool}", goal_tool)
        .replace("{scratch_dir}", scratch_dir)
        .replace(
            "{scratch_status}",
            if scratch_ready {
                "(created for you)"
            } else {
                "(create with `mkdir -p` if missing)"
            },
        )
        .replace("{strategist_note}", &strategist_note)
}

/// Render the `{reverify_block}` slot. Empty unless the goal has been
/// refuted at least once AND has run `>= threshold` rounds since the last
/// verification fired. Reframes the loop (passing verification is the ONLY
/// way to finish) and inlines the live count; the lead hardens past
/// `3 * threshold`.
pub(super) fn render_goal_reverify_block(
    rounds_since_verify: u32,
    refuted: bool,
    threshold: u32,
) -> String {
    if !refuted || rounds_since_verify < threshold {
        return String::new();
    }
    let lead = if rounds_since_verify >= threshold.saturating_mul(3) {
        "STOP DRIFTING — VERIFY THE REMAINING GAP NOW."
    } else {
        "Re-run the verification plan before continuing."
    };
    format!(
        "{lead} You have run {rounds_since_verify} rounds since the last \
         adversarial rejection. The host evaluates completion after every round. \
         Fix the single concrete gap rather than making cosmetic changes.\n\n"
    )
}

pub(super) fn render_goal_reverify_block_foreground(
    rounds_since_verify: u32,
    refuted: bool,
    threshold: u32,
    goal_tool: &str,
) -> String {
    if !refuted || rounds_since_verify < threshold {
        return String::new();
    }
    let lead = if rounds_since_verify >= threshold.saturating_mul(3) {
        "STOP DRIFTING — RE-VERIFY NOW."
    } else {
        "Re-verify before continuing."
    };
    format!(
        "{lead} You have run {rounds_since_verify} rounds since your last \
         verification without calling `{goal_tool}(completed: true)`. The ONLY \
         way to finish this goal is to PASS verification — not to keep editing. \
         If the plan's `## Verification plan` steps now hold, call \
         `{goal_tool}(completed: true)` THIS round to re-trigger the skeptic \
         panel. If they do NOT, name the SINGLE concrete gap still blocking it \
         and fix exactly that — do not make cosmetic changes to look busy.\n\n"
    )
}

/// Render the strategist-note block for the continuation directive.
/// `recommendation` is the capped, model-authored snippet read back from
/// the strategy note (persisted as `last_strategy_recommendation`); empty
/// ⇒ empty string, collapsing the `{strategist_note}` slot (same
/// empty-slot convention as `verifier_gaps`). Otherwise it renders a
/// narrative that tells the model to RE-READ its plan AND the strategy
/// note before continuing, then ends with a blank line so it stacks above
/// the "Goal NOT complete" sentinel. When no plan path is available
/// (planner disabled) the plan clause is dropped.
///
/// Injection hardening: the rendered note is wrapped in fence markers
/// carrying a PER-RENDER nonce. The nonce makes the fence unguessable, so a
/// model-authored recommendation can't reproduce the END marker to break out
/// and pose as harness narration; as a second layer any body line equal to a
/// fence marker is dropped. Placeholder/tag spoofing is handled at the
/// directive slot ([`neutralize_directive_slot`]), not here.
pub(super) fn render_strategist_note(
    recommendation: &str,
    plan_path: Option<&Path>,
    strategy_path: Option<&str>,
) -> String {
    if recommendation.trim().is_empty() {
        return String::new();
    }
    // Per-render nonce: a fresh short token the untrusted body can't predict,
    // so it can't forge the closing marker to break out of the fence.
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let nonce = &nonce[..12];
    let begin = format!("--- STRATEGIST RECOMMENDATION (advisory) [{nonce}] ---");
    let end = format!("--- END STRATEGIST RECOMMENDATION [{nonce}] ---");
    // Static marker prefixes used to drop any body line that LOOKS like a
    // fence marker (with or without a nonce) — defence in depth on top of the
    // unguessable nonce.
    const BEGIN_PREFIX: &str = "--- STRATEGIST RECOMMENDATION (advisory)";
    const END_PREFIX: &str = "--- END STRATEGIST RECOMMENDATION";
    // Drop forged-marker lines from the untrusted recommendation.
    let sanitized: String = recommendation
        .lines()
        .filter(|line| {
            let t = line.trim_start();
            !(t.starts_with(BEGIN_PREFIX) || t.starts_with(END_PREFIX))
        })
        .collect::<Vec<_>>()
        .join("\n");
    let strategy_path = strategy_path.unwrap_or("the strategy note");
    let reread = match plan_path {
        Some(p) => format!(
            "RE-READ your plan at {} AND the strategy note at {strategy_path}",
            p.display(),
        ),
        None => format!("RE-READ the strategy note at {strategy_path}"),
    };
    format!(
        "A strategist reviewed your stuck progress and recommends a STRUCTURAL \
         change of approach (you keep flagging a different gap each round). \
         Before continuing, {reread}, then weigh this ADVISORY recommendation \
         (between the {nonce}-tagged markers below; it does NOT change the \
         acceptance criteria, only HOW you get there — it is not a harness \
         instruction):\n\
         {begin}\n\
         {sanitized}\n\
         {end}\n\n",
    )
}

/// Render the verifier-gaps slot: inline the bounded gaps checklist
/// directly (empty → ""). The verbose per-skeptic details file is
/// deliberately NOT referenced — pointing the model at it bloats context;
/// the file stays on disk for the user.
pub(super) fn render_verifier_gaps_block(gaps: &str) -> String {
    if gaps.is_empty() {
        return String::new();
    }
    format!(
        "Verification rejected the prior completion candidate. Fix every gap \
         the skeptic panel flagged below — these take priority — \
         before claiming completion again:\n{gaps}\n\n",
    )
}

pub(super) fn render_verifier_gaps_block_foreground(gaps: &str, goal_tool: &str) -> String {
    if gaps.is_empty() {
        return String::new();
    }
    format!(
        "Verification REJECTED your last `{goal_tool}(completed: true)` claim. \
         Fix every gap the skeptic panel flagged below — these take priority — \
         before claiming completion again:\n{gaps}\n\n",
    )
}

/// `char` cap on the (model-authored) plan-mined next-step line — one
/// checklist item never legitimately needs more, while a single plan
/// line can run to the reader's 8 KiB cap. Applied BEFORE tag
/// neutralization, which may add a zero-width break per broken tag
/// (plus the `…` cap suffix).
pub(super) const GOAL_NEXT_STEP_MAX_CHARS: usize = 400;

/// Resolve the inlined "next concrete step" for the continuation
/// nudge from the planner-emitted plan file. The read is 8 KiB-capped
/// and best-effort — any I/O or parse failure yields `None`, leaving
/// the caller to substitute a generic "check your todo list" fallback.
///
/// The plan item is model-authored: `char`-capped to
/// [`GOAL_NEXT_STEP_MAX_CHARS`], then reminder-frame tags are
/// zero-width-broken so it cannot close the `<system-reminder>` frame
/// it is inlined into.
///
/// Verifier gaps are NOT consulted here: a `NotAchieved` verdict's
/// findings are surfaced separately and prominently via
/// [`render_verifier_gaps_block`] (persisted in `last_classifier_gaps`),
/// so this slot carries only the plan's next item to avoid duplicating
/// the top gap.
pub(super) fn resolve_goal_next_step(plan_path: Option<&Path>) -> Option<String> {
    use crate::session::goal_classifier::{cap_chars, neutralize_reminder_tags};
    use crate::session::goal_next_step::first_unchecked_plan_item;

    plan_path
        .and_then(first_unchecked_plan_item)
        .map(|item| neutralize_reminder_tags(cap_chars(&item, GOAL_NEXT_STEP_MAX_CHARS)))
}

pub(super) fn format_blocked_chat_notification(reason: &str, detail: Option<&str>) -> String {
    let mut out = String::with_capacity(reason.len() + detail.map_or(0, str::len) + 96);
    out.push_str("Goal paused — verification blocked.\nReason: ");
    out.push_str(reason);
    out.push('\n');
    if let Some(detail) = detail {
        out.push_str(detail);
        out.push('\n');
    }
    out.push_str("\nType /goal resume to continue after you've addressed it.");
    out
}

/// Per-subagent token state; the goal-scoped marginal is
/// `last_cumulative_reported - resume_anchor_cumulative`.
///
/// Child reports carry the child's CONTEXT token total, so a child
/// compaction freezes the ratchet at its pre-compaction max until the
/// child's context regrows past it.
#[derive(Debug)]
pub(crate) struct SubagentTokenRecord {
    /// `None` for subagents spawned outside any active goal.
    pub goal_id: Option<String>,
    /// Parent's `last_cumulative_reported` at spawn; 0 for fresh spawns.
    pub resume_anchor_cumulative: u64,
    /// Monotonic high-water mark, ratcheted on `SubagentProgress` ticks
    /// and sealed by `SubagentFinished`.
    pub last_cumulative_reported: u64,
    /// Effective model id captured from `SubagentSpawned.model` at spawn
    /// time. Captured here (not at aggregation time) so attribution is
    /// pinned to the model the subagent actually ran on, even if the user
    /// switches the session model mid-goal. `None` (or empty) only when the
    /// wire field was absent; such records fold under the current model id
    /// as a best-effort fallback during aggregation.
    pub model: Option<String>,
    /// Set by `SubagentFinished`; later (stale or spoofed) progress ticks
    /// for the subagent are ignored so they can't move the ratchet.
    pub finished: bool,
}

impl SubagentTokenRecord {
    /// Goal-scoped marginal cost: `last_cumulative_reported -
    /// resume_anchor_cumulative`, saturating so a stale/out-of-order
    /// report below the anchor yields 0 instead of underflowing. Shared by
    /// the single-line total ([`SessionActor::goal_tokens`]) and the
    /// per-model breakdown ([`fold_tokens_by_model`]) so they can't drift.
    pub fn marginal(&self) -> u64 {
        self.last_cumulative_reported
            .saturating_sub(self.resume_anchor_cumulative)
    }
}

/// Fold subagent token records into a `model_id -> marginal_tokens`
/// breakdown for `goal_id`, sorted by tokens descending (ties broken by
/// model id for determinism). Marginal cost per record is
/// [`SubagentTokenRecord::marginal`]. Records whose `model` was absent or
/// empty/whitespace-only on the wire fold under `current_model_id`.
/// Zero-marginal records are skipped.
fn fold_tokens_by_model<'a>(
    records: impl IntoIterator<Item = &'a SubagentTokenRecord>,
    goal_id: &'a str,
    current_model_id: &'a str,
) -> Vec<(String, u64)> {
    let mut by_model: HashMap<&'a str, u64> = HashMap::new();
    for r in records {
        if r.goal_id.as_deref() != Some(goal_id) {
            continue;
        }
        let marginal = r.marginal();
        if marginal == 0 {
            continue;
        }
        // A missing OR empty/whitespace-only captured id folds under the
        // current model so we never create a blank-id bucket.
        let model = r
            .model
            .as_deref()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or(current_model_id);
        let entry = by_model.entry(model).or_insert(0);
        *entry = entry.saturating_add(marginal);
    }
    let mut out: Vec<(String, u64)> = by_model
        .into_iter()
        .map(|(m, t)| (m.to_owned(), t))
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

#[cfg(test)]
mod fold_tokens_by_model_tests {
    use super::{SubagentTokenRecord, fold_tokens_by_model};

    fn rec(goal: Option<&str>, anchor: u64, last: u64, model: Option<&str>) -> SubagentTokenRecord {
        SubagentTokenRecord {
            goal_id: goal.map(str::to_owned),
            resume_anchor_cumulative: anchor,
            last_cumulative_reported: last,
            model: model.map(str::to_owned),
            finished: false,
        }
    }

    #[test]
    fn empty_records_yields_empty() {
        let records: Vec<SubagentTokenRecord> = Vec::new();
        assert!(fold_tokens_by_model(&records, "g1", "cur").is_empty());
    }

    #[test]
    fn mixed_models_sum_marginals_sorted_desc() {
        let records = vec![
            rec(Some("g1"), 0, 100, Some("grow-3")),
            rec(Some("g1"), 100, 500, Some("grow-4")), // marginal 400
            rec(Some("g1"), 0, 50, Some("grow-3")),    // grow-3 total 150
        ];
        let out = fold_tokens_by_model(&records, "g1", "cur");
        assert_eq!(
            out,
            vec![("grow-4".to_owned(), 400), ("grow-3".to_owned(), 150)]
        );
    }

    #[test]
    fn ties_break_by_model_id_ascending() {
        let records = vec![
            rec(Some("g1"), 0, 100, Some("zeta")),
            rec(Some("g1"), 0, 100, Some("alpha")),
        ];
        let out = fold_tokens_by_model(&records, "g1", "cur");
        assert_eq!(
            out,
            vec![("alpha".to_owned(), 100), ("zeta".to_owned(), 100)]
        );
    }

    #[test]
    fn none_model_folds_under_current() {
        let records = vec![rec(Some("g1"), 0, 100, None), rec(Some("g1"), 0, 200, None)];
        let out = fold_tokens_by_model(&records, "g1", "cur-model");
        assert_eq!(out, vec![("cur-model".to_owned(), 300)]);
    }

    #[test]
    fn single_distinct_model_collapses_to_one_entry() {
        let records = vec![
            rec(Some("g1"), 0, 100, Some("grow-4")),
            rec(Some("g1"), 0, 200, None), // folds under current = grow-4
        ];
        let out = fold_tokens_by_model(&records, "g1", "grow-4");
        assert_eq!(out, vec![("grow-4".to_owned(), 300)]);
    }

    #[test]
    fn other_goal_records_excluded() {
        let records = vec![
            rec(Some("g1"), 0, 100, Some("grow-4")),
            rec(Some("g2"), 0, 999, Some("grow-4")),
            rec(None, 0, 999, Some("grow-4")),
        ];
        let out = fold_tokens_by_model(&records, "g1", "cur");
        assert_eq!(out, vec![("grow-4".to_owned(), 100)]);
    }

    #[test]
    fn last_below_anchor_does_not_underflow() {
        let records = vec![rec(Some("g1"), 500, 100, Some("grow-4"))];
        // marginal saturates to 0 -> skipped as a zero-token entry.
        assert!(fold_tokens_by_model(&records, "g1", "cur").is_empty());
    }

    #[test]
    fn captured_model_survives_mid_goal_current_model_switch() {
        // A record captured `grow-4` at spawn keeps it even though the
        // current model at aggregation time is `grow-3`.
        let records = vec![rec(Some("g1"), 0, 100, Some("grow-4"))];
        let out = fold_tokens_by_model(&records, "g1", "grow-3");
        assert_eq!(out, vec![("grow-4".to_owned(), 100)]);
    }

    #[test]
    fn empty_or_whitespace_model_folds_under_current() {
        // `Some("")` and `Some("  ")` must NOT create a blank-id bucket;
        // they fold under the current model exactly like `None`.
        let records = vec![
            rec(Some("g1"), 0, 100, Some("")),
            rec(Some("g1"), 0, 200, Some("   ")),
            rec(Some("g1"), 0, 50, None),
        ];
        let out = fold_tokens_by_model(&records, "g1", "cur-model");
        assert_eq!(out, vec![("cur-model".to_owned(), 350)]);
    }

    #[test]
    fn empty_model_merges_with_current_model_bucket() {
        // An empty id folds into the SAME bucket as records that
        // explicitly captured the current model id.
        let records = vec![
            rec(Some("g1"), 0, 100, Some("grow-4")),
            rec(Some("g1"), 0, 200, Some("")),
        ];
        let out = fold_tokens_by_model(&records, "g1", "grow-4");
        assert_eq!(out, vec![("grow-4".to_owned(), 300)]);
    }
}

/// Resolved per-role `/goal` model selection, cached on the actor.
///
/// `Default` (every role `InheritCurrent`, empty skeptic pool) reproduces
/// today's behavior — `runtime_overrides.model = None` + the role's default
/// `subagent_type`. The kill-switch (`goal_use_current_model_only`) collapses
/// all three to `InheritCurrent` at resolution time, so consumers never need
/// to re-check it.
#[derive(Debug, Clone, Default)]
pub(crate) struct GoalRoleModelConfig {
    /// Planner role choice (single pair or inherit).
    pub(crate) planner: crate::agent::config::GoalRoleModelChoice,
    /// Strategist role choice (single pair or inherit).
    pub(crate) strategist: crate::agent::config::GoalRoleModelChoice,
    /// Ordered skeptic pool; `pool[0]` is skeptic-0's model, the rest are
    /// assigned round-robin by index. Empty ⇒ all skeptics inherit.
    pub(crate) skeptic_pool: Vec<crate::util::config::GoalRoleModel>,
}

pub(crate) fn planner_failure_pause_message() -> String {
    "Planning failed; resume with /goal to retry.".to_string()
}

pub(crate) fn goal_slash_and_harness_available(goal_enabled: bool, tool_names: &[String]) -> bool {
    use tools::implementations::grow_build::UPDATE_GOAL_TOOL_NAME;
    goal_enabled && tool_names.iter().any(|n| n == UPDATE_GOAL_TOOL_NAME)
}

/// Active `/goal` session with goal harness enabled (laziness, continuation, TodoGate goal arm).
pub(crate) fn laziness_injection_active(
    goal_harness_enabled: bool,
    goal_status: Option<crate::session::goal_tracker::GoalStatus>,
) -> bool {
    goal_harness_enabled && goal_status == Some(crate::session::goal_tracker::GoalStatus::Active)
}

impl SessionActor {
    pub(super) fn goal_harness_enabled(&self) -> bool {
        self.goal_harness_enabled
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// True while a `/goal` autonomous run is actively driving the turn
    /// (goal harness enabled and the durable status is `Active`). Used to
    /// tag background tasks spawned during the goal turn as goal-turn-origin.
    pub(super) fn goal_loop_active(&self) -> bool {
        self.goal_harness_enabled()
            && self.goal_tracker.lock().status()
                == Some(crate::session::goal_tracker::GoalStatus::Active)
    }

    /// Tag `task_id`s the goal model spawned itself during the goal turn as
    /// goal-turn origin (see [`Self::goal_turn_task_ids`]). No-op when the goal
    /// loop isn't active.
    pub(super) fn record_goal_turn_task_ids(&self, task_ids: impl IntoIterator<Item = String>) {
        if !self.goal_loop_active() {
            return;
        }
        let mut set = self.goal_turn_task_ids.lock();
        set.extend(task_ids);
    }

    /// Tag `task_id`s reparented from a harness verifier/planner subagent as
    /// goal-turn origin. Gated on the goal harness being enabled — stable across
    /// status flips — NOT on `Active`, so a final-round skeptic exiting as the
    /// goal flips `Active → Blocked` is still suppressed. The caller already
    /// filters to harness-internal children (`surface_completion: false`).
    pub(super) fn record_reparented_goal_turn_task_ids(
        &self,
        task_ids: impl IntoIterator<Item = String>,
    ) {
        if !self.goal_harness_enabled() {
            return;
        }
        let mut set = self.goal_turn_task_ids.lock();
        set.extend(task_ids);
    }

    pub(super) fn sync_goal_harness(&self) -> bool {
        let enabled = self.goal_enabled && self.goal_classifier_enabled;
        self.goal_harness_enabled
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
        enabled
    }

    pub(super) fn sync_goal_harness_from_tools(&self, tool_names: &[String]) -> bool {
        let enabled = goal_slash_and_harness_available(
            self.goal_enabled && self.goal_classifier_enabled,
            tool_names,
        );
        self.goal_harness_enabled
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
        enabled
    }

    pub(super) async fn refresh_goal_harness_enabled(&self) {
        if self.goal_runs_on_workflow_engine() {
            self.sync_goal_harness();
        } else {
            let tool_names = self.registered_tool_names().await;
            self.sync_goal_harness_from_tools(&tool_names);
        }
    }

    pub(super) async fn maybe_reconcile_active_goal_without_harness(&self) {
        if self
            .goal_harness_availability_reconciled
            .swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            return;
        }
        if self.goal_harness_enabled() {
            return;
        }
        if self.goal_tracker.lock().status()
            != Some(crate::session::goal_tracker::GoalStatus::Active)
        {
            return;
        }
        let _ = self
            .auto_pause_goal_if_active_with_message(
                crate::session::goal_tracker::GoalPauseReason::User,
                "Goal paused: `update_goal` is not available in this session's toolset. \
                 Resume with /goal after the tool is registered."
                    .into(),
            )
            .await;
    }

    /// Load-time safety net: an Active goal restored with `plan_file == None`
    /// (foreground snapshot) is paused with the canonical message.
    /// One-shot via `goal_plan_reconciled`; the mid-session retry path lives
    /// in `resume_goal`, not here.
    pub(super) async fn maybe_reconcile_active_goal_without_plan(&self) {
        if self
            .goal_plan_reconciled
            .swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            return;
        }
        if !self.goal_planner_enabled || !self.goal_harness_enabled() {
            return;
        }
        let needs_pause = {
            let tracker = self.goal_tracker.lock();
            match tracker.snapshot() {
                Some(o) => {
                    o.status == crate::session::goal_tracker::GoalStatus::Active
                        && o.plan_file.is_none()
                }
                None => false,
            }
        };
        if !needs_pause {
            return;
        }
        let _ = self
            .auto_pause_goal_if_active_with_message(
                crate::session::goal_tracker::GoalPauseReason::User,
                planner_failure_pause_message(),
            )
            .await;
    }

    pub(super) fn prune_subagent_records_for_active_goal(&self) {
        let goal_id = match self.goal_tracker.lock().snapshot() {
            Some(o) => o.goal_id.clone(),
            None => return,
        };
        self.subagent_token_records
            .lock()
            .retain(|_, r| r.goal_id.as_deref() != Some(goal_id.as_str()));
    }

    /// Run the planner subagent for a goal that has no plan yet.
    /// Called from `setup_goal` (fresh goal) and from `resume_goal`
    /// (post-pause retry, honouring the canonical "Planning failed;
    /// resume with /goal to retry." message). No-op when the planner
    /// is disabled, when the coordinator is not plumbed (tests /
    /// compatible template), or when `plan_file` is already populated.
    /// On `FailClosed` the goal is paused with the canonical reason.
    pub(super) async fn maybe_run_goal_planner(self: &std::sync::Arc<Self>, objective: &str) {
        if !self.goal_planner_enabled {
            return;
        }
        let Some(event_tx) = self.tool_context.subagent_event_tx.clone() else {
            tracing::debug!("goal planner: no subagent coordinator channel; skipping");
            return;
        };
        let (plan_file, definition_key, autonomy_generation) = {
            let tracker = self.goal_tracker.lock();
            match tracker.snapshot() {
                Some(o) if o.plan_file.is_some() => {
                    tracing::debug!("goal planner: plan already present; skipping");
                    return;
                }
                Some(o) => (
                    tracker.plan_staging_path(&o.goal_id, o.definition_revision),
                    (o.goal_id.clone(), o.definition_revision),
                    o.autonomy_generation,
                ),
                None => return,
            }
        };

        // `GOAL_PLANNER_MAX_RUNS` is diagnostics-only; resume retries are unbounded.
        let attempt = 1u32;

        let model_id = self
            .chat_state_handle
            .get_sampling_config()
            .await
            .map(|c| c.model)
            .unwrap_or_default();
        // Fork owns history; fail-open stays OBJECTIVE-only (no last-assistant CONTEXT).
        let context = String::new();

        // Tag the planner with the goal-creation turn's prompt id so its
        // `subagent.json` / parent `subagents_spawned` ref link to this turn,
        // matching how model-spawned subagents attach to their parent.
        let parent_prompt_id = self
            .current_prompt_id
            .lock()
            .expect("current_prompt_id mutex poisoned")
            .clone();
        // The planner is a verbatim mirror-child fork: its request prefix matches
        // the parent and the radix cache is per-model, so it must run on the
        // parent session model. Any configured planner role model is intentionally
        // ignored on this path (breadcrumb below for observability parity with the
        // strategist, which still resolves a role model).
        let role_override = crate::session::goal_planner::RoleSpawnOverride::default();
        if !matches!(
            self.goal_role_models.planner,
            crate::agent::config::GoalRoleModelChoice::InheritCurrent
        ) {
            tracing::info!(
                "goal planner: configured role model ignored — forced to parent model for verbatim-fork cache reuse"
            );
        }
        // Render planner-prompt tool-name placeholders from the PARENT's resolved
        // toolset (the exact tools the verbatim mirror sends) — honoring renames
        // and the Write->Edit fallback — instead of static defaults.
        let tool_names = self.resolve_inherit_role_tool_names().await;
        let inherit_tool_names = tool_names.clone();
        let spawner: std::sync::Arc<dyn crate::session::goal_planner::GoalPlannerSpawner> =
            std::sync::Arc::new(crate::session::goal_planner::ChannelSpawner {
                event_tx,
                foreground_wait: Some(crate::tools::tool_context::subagent_foreground_wait(
                    self.tool_context.blocking_wait_depth.clone(),
                )),
                parent_session_id: self.session_id_string(),
                parent_prompt_id,
                cwd: Some(self.tool_context.cwd.as_str().to_owned()),
                role_override,
            });

        // Surface the "planning…" badge while the subagent runs. Cleared
        // unconditionally below so it can never get stuck on screen.
        let current_tokens = self.chat_state_handle.get_total_tokens().await as i64;
        self.emit_goal_planning(current_tokens);

        let objective = objective.to_owned();
        let effective_model_id =
            crate::session::goal_planner::effective_role_model_id(None, &model_id).to_owned();
        let foreground_generation = self.completion_delivery.generation();
        let control_generation = self.goal_control_generation();
        let mut planner_task = tokio::task::spawn_local(async move {
            crate::session::goal_planner::run_goal_planner(
                spawner,
                crate::session::goal_planner::GoalPlannerInputs {
                    objective: &objective,
                    context: &context,
                    plan_file: &plan_file,
                    attempt,
                    model_id: &effective_model_id,
                    tool_names: &tool_names,
                    inherit_tool_names: &inherit_tool_names,
                },
            )
            .await
        });
        let background_definition = definition_key.clone();
        enum PlannerWait {
            Displaced(&'static str),
            Finished(
                Result<crate::session::goal_planner::GoalPlannerOutcome, tokio::task::JoinError>,
            ),
        }
        let wait = tokio::select! {
            biased;
            _ = super::tool_calls::wait_for_pending_interjection(
                &self.pending_interjections,
            ) => PlannerWait::Displaced("user steering"),
            _ = self
                .completion_delivery
                .wait_generation_change(foreground_generation) => {
                PlannerWait::Displaced("a foreground completion")
            },
            _ = self.wait_goal_control_change(control_generation) => {
                PlannerWait::Displaced("a Goal control change")
            },
            result = &mut planner_task => PlannerWait::Finished(result),
        };
        let outcome = match wait {
            PlannerWait::Displaced(displaced_by) => {
                // Transfer the planner wait to a continuation. The child and
                // its waiter remain alive; only the Goal's foreground setup
                // stops blocking so the main agent can process the foreground.
                self.push_system_reminder(&format!(
                    "The Goal planner is still running in the background after {displaced_by}. Process the foreground information now, but do not begin plan-dependent execution until the planner completion reminder arrives."
                ));
                let actor = std::sync::Arc::clone(self);
                tokio::task::spawn_local(async move {
                    match planner_task.await {
                        Ok(outcome) => {
                            let applied = actor
                                .apply_goal_planner_outcome(
                                    &background_definition,
                                    autonomy_generation,
                                    outcome,
                                )
                                .await;
                            if applied {
                                actor.completion_delivery.queue_ready(
                                    format!(
                                        "goal-planner:{}:{}",
                                        background_definition.0, background_definition.1
                                    ),
                                    "The Goal planner finished in the background. Its plan is now available; incorporate it before continuing execution."
                                        .to_string(),
                                );
                                let _ = actor.event_tx.send(SessionEvent::ForegroundWake);
                            }
                        }
                        Err(error) => tracing::warn!(
                            error = %error,
                            "goal planner continuation task failed"
                        ),
                    }
                });
                return;
            }
            PlannerWait::Finished(result) => match result {
                Ok(outcome) => outcome,
                Err(error) => {
                    tracing::warn!(error = %error, "goal planner task failed");
                    crate::session::goal_planner::GoalPlannerOutcome::FailClosed {
                        reason: crate::session::events::GoalPlannerFailClosedReason::Aborted,
                        latency_ms: 0,
                    }
                }
            },
        };

        self.apply_goal_planner_outcome(&definition_key, autonomy_generation, outcome)
            .await;
    }

    async fn apply_goal_planner_outcome(
        &self,
        definition_key: &(String, u64),
        autonomy_generation: u64,
        outcome: crate::session::goal_planner::GoalPlannerOutcome,
    ) -> bool {
        let lease_is_current = |tracker: &crate::session::goal_tracker::GoalTracker| {
            tracker.definition_is_current(&definition_key.0, definition_key.1)
                && tracker.autonomy_generation() == Some(autonomy_generation)
                && tracker.status() == Some(crate::session::goal_tracker::GoalStatus::Active)
        };
        if !lease_is_current(&self.goal_tracker.lock()) {
            tracing::info!("goal planner: discarded stale result after goal revision");
            return false;
        }

        let plan_available = match outcome {
            crate::session::goal_planner::GoalPlannerOutcome::Planned { plan_file, .. } => {
                let (expected_staging, canonical_plan, baseline_plan) = {
                    let tracker = self.goal_tracker.lock();
                    (
                        tracker.plan_staging_path(&definition_key.0, definition_key.1),
                        tracker.plan_path(),
                        tracker.plan_baseline_path(),
                    )
                };
                let (plan_bytes, content_hash) =
                    match validate_goal_plan_staging(&plan_file, &expected_staging).await {
                        Ok(validated) => validated,
                        Err(error) => {
                            tracing::warn!(%error, "goal planner: rejected staging artifact");
                            let _ = tokio::fs::remove_file(&plan_file).await;
                            return false;
                        }
                    };
                // Publication and lease validation are one actor-owned commit.
                // The artifact is capped at 1 MiB, so the small synchronous
                // rename/write while holding the tracker lock is preferable to
                // releasing the lease between filesystem awaits: otherwise a
                // concurrent `/goal set`, pause, or steering event could make a
                // stale planner overwrite the canonical plan after validation.
                let mut tracker = self.goal_tracker.lock();
                if !lease_is_current(&tracker) {
                    drop(tracker);
                    let _ = tokio::fs::remove_file(&plan_file).await;
                    return false;
                }
                if let Err(error) = std::fs::rename(&plan_file, &canonical_plan) {
                    tracing::warn!(%error, "goal planner: failed to publish canonical plan");
                    return false;
                }
                if let Err(error) = std::fs::write(&baseline_plan, &plan_bytes) {
                    tracing::warn!(%error, "goal planner: failed to publish plan baseline");
                    let _ = std::fs::remove_file(&canonical_plan);
                    return false;
                }
                if let Some(o) = tracker.snapshot_mut() {
                    o.plan_file = Some(canonical_plan);
                    o.plan_content_hash = Some(content_hash);
                    o.plan_baseline_file = Some(baseline_plan);
                }
                true
            }
            crate::session::goal_planner::GoalPlannerOutcome::FailClosed { .. } => {
                let _ = self
                    .auto_pause_goal_if_active_with_message(
                        crate::session::goal_tracker::GoalPauseReason::User,
                        planner_failure_pause_message(),
                    )
                    .await;
                false
            }
        };

        // Reset the latch on every exit path (success, fail-closed,
        // mid-run status change) and emit so the "planning…" badge turns
        // off. Unconditional here because `setup_goal` has no later emit;
        // no-op if the orchestration has since vanished.
        let current_tokens = self.chat_state_handle.get_total_tokens().await as i64;
        let (tokens_used, finished_marginal) = self.goal_tokens(current_tokens);
        let mut tracker = self.goal_tracker.lock();
        if !lease_is_current(&tracker) {
            tracing::info!("goal planner: discarded result after goal changed during commit");
            return false;
        }
        if let Some(o) = tracker.snapshot_mut() {
            o.planning_in_flight = false;
        }
        self.goal_notify_sender()
            .emit_goal_updated(&mut tracker, tokens_used, finished_marginal);
        plan_available
    }

    /// Schedule the stall-triggered strategist subagent as a background Goal
    /// stage (best-effort, fail-OPEN). The fire claim (cap bonus + marker)
    /// is made by the caller in the mailbox; the model work runs in the
    /// spawned stage task and its outcome is committed by the mailbox via
    /// `SessionEvent::GoalStageCompleted`. `apply_classifier_outcome` never
    /// awaits model work, so the mailbox can call this directly.
    pub(super) fn maybe_run_goal_strategist(
        self: &std::sync::Arc<Self>,
        attempt: u32,
        consecutive_failures: u32,
    ) {
        let Some(definition_key) = self.goal_tracker.lock().definition_key() else {
            return;
        };
        let autonomy_generation = self
            .goal_tracker
            .lock()
            .autonomy_generation()
            .unwrap_or_default();
        if self.tool_context.subagent_event_tx.is_none() {
            // Mirror the old no-coordinator exit (guard dropped without a
            // run): the strategist delivered nothing, so the cap bonus the
            // caller claimed is revoked — only while the same definition
            // still owns the orchestration.
            let mut tracker = self.goal_tracker.lock();
            if tracker.definition_is_current(&definition_key.0, definition_key.1) {
                tracker.revoke_strategist_cap_bonus();
            }
            return;
        }
        let stage_id = self
            .goal_stage_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let foreground_generation = self.completion_delivery.generation();
        let actor = std::sync::Arc::clone(self);
        tokio::task::spawn_local(async move {
            let outcome = actor
                .run_goal_strategist_stage(attempt, consecutive_failures)
                .await;
            let _ = actor.event_tx.send(SessionEvent::GoalStageCompleted(
                crate::session::replay_events::GoalStageCompletion {
                    stage_id,
                    goal_id: definition_key.0.clone(),
                    definition_revision: definition_key.1,
                    autonomy_generation,
                    foreground_generation,
                    kind: crate::session::replay_events::GoalStageKind::Strategist {
                        attempt,
                        consecutive_failures,
                        outcome,
                    },
                },
            ));
        });
    }

    /// Stage-task body for the strategist: assemble inputs and run the
    /// subagent off the mailbox. Returns the raw outcome; the mailbox
    /// commit (`commit_strategist_stage`) decides apply vs. discard and owns
    /// the cap-bonus bookkeeping.
    async fn run_goal_strategist_stage(
        &self,
        attempt: u32,
        consecutive_failures: u32,
    ) -> Result<crate::session::goal_strategist::GoalStrategistOutcome, String> {
        let Some(event_tx) = self.tool_context.subagent_event_tx.clone() else {
            return Err("goal strategist: no subagent coordinator channel".to_string());
        };

        // Assemble inputs under one scoped lock, then drop it before the
        // spawn await. `plan_path` / `strategy_path` are derived from the
        // tracker while we hold the lock (cheap path joins).
        let (objective, plan_file, strategy_file, verifier_id) = {
            let tracker = self.goal_tracker.lock();
            let Some(o) = tracker.snapshot() else {
                return Err("goal strategist: orchestration vanished before stage ran".to_string());
            };
            (
                o.objective.clone(),
                tracker.plan_path(),
                tracker.strategy_path(),
                o.verifier_id.clone(),
            )
        };

        // The strategist reads the run's traces itself (transcript + verdict
        // history) instead of a pre-assembled gaps/diff packet.
        let session_traces_dir = crate::session::persistence::session_dir(&self.session_info);
        let scratch_root = crate::session::goal_tracker::goal_scratch_root(&verifier_id);

        let model_id = self
            .chat_state_handle
            .get_sampling_config()
            .await
            .map(|c| c.model)
            .unwrap_or_default();

        let parent_prompt_id = self
            .current_prompt_id
            .lock()
            .expect("current_prompt_id mutex poisoned")
            .clone();
        // Resolve the strategist role override: entitlement +
        // toolset capability gate, per-role fail-open. Borrows `event_tx`
        // for the describe round-trip; `event_tx` moves into the spawner.
        let (role_override, tool_names, inherit_tool_names) = self
            .resolve_goal_single_role_override(
                "strategist",
                &self.goal_role_models.strategist,
                goal::RoleCapability::Strategist,
                &event_tx,
            )
            .await;
        // Cloned for diagnostics before `role_override` moves into the spawner.
        let strategist_model_override = role_override.model.clone();
        let spawner: std::sync::Arc<dyn crate::session::goal_strategist::GoalStrategistSpawner> =
            std::sync::Arc::new(crate::session::goal_strategist::ChannelSpawner {
                event_tx,
                foreground_wait: Some(crate::tools::tool_context::subagent_foreground_wait(
                    self.tool_context.blocking_wait_depth.clone(),
                )),
                parent_session_id: self.session_id_string(),
                parent_prompt_id,
                cwd: Some(self.tool_context.cwd.as_str().to_owned()),
                role_override,
            });

        let outcome = crate::session::goal_strategist::run_goal_strategist(
            spawner,
            crate::session::goal_strategist::GoalStrategistInputs {
                objective: &objective,
                plan_file: &plan_file,
                strategy_file: &strategy_file,
                session_traces_dir: &session_traces_dir,
                scratch_root: &scratch_root,
                attempt,
                consecutive_failures,
                every: self.goal_strategist_every,
                model_id: crate::session::goal_planner::effective_role_model_id(
                    strategist_model_override.as_deref(),
                    &model_id,
                ),
                tool_names: &tool_names,
                inherit_tool_names: &inherit_tool_names,
            },
        )
        .await;
        Ok(outcome)
    }

    /// Schedule the ONE closing user-facing summarizer after a goal is
    /// verified-achieved, as a background Goal stage (fail-OPEN). The
    /// closing summary is surfaced by the mailbox commit
    /// (`commit_summarizer_stage`) — goal completion is never blocked,
    /// paused, or un-achieved (the goal is already Complete when this runs).
    pub(super) fn maybe_run_goal_summarizer(self: &std::sync::Arc<Self>, attempt: u32) {
        if !self.goal_summary_enabled {
            return;
        }
        let Some(definition_key) = self.goal_tracker.lock().definition_key() else {
            return;
        };
        let autonomy_generation = self
            .goal_tracker
            .lock()
            .autonomy_generation()
            .unwrap_or_default();
        if self.tool_context.subagent_event_tx.is_none() {
            tracing::debug!("goal summarizer: no subagent coordinator channel; skipping");
            return;
        }
        let stage_id = self
            .goal_stage_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let foreground_generation = self.completion_delivery.generation();
        let actor = std::sync::Arc::clone(self);
        tokio::task::spawn_local(async move {
            let outcome = actor.run_goal_summarizer_stage(attempt).await;
            let _ = actor.event_tx.send(SessionEvent::GoalStageCompleted(
                crate::session::replay_events::GoalStageCompletion {
                    stage_id,
                    goal_id: definition_key.0.clone(),
                    definition_revision: definition_key.1,
                    autonomy_generation,
                    foreground_generation,
                    kind: crate::session::replay_events::GoalStageKind::Summarizer {
                        attempt,
                        outcome,
                    },
                },
            ));
        });
    }

    /// Stage-task body for the summarizer: assemble inputs and run the
    /// subagent off the mailbox. Returns the raw outcome; the mailbox
    /// commit (`commit_summarizer_stage`) surfaces the summary.
    async fn run_goal_summarizer_stage(
        &self,
        attempt: u32,
    ) -> Result<crate::session::goal_summarizer::GoalSummarizerOutcome, String> {
        let Some(event_tx) = self.tool_context.subagent_event_tx.clone() else {
            return Err("goal summarizer: no subagent coordinator channel".to_string());
        };

        // Snapshot inputs under one scoped lock, dropped before the awaits.
        let (objective, plan_file, details_file) = {
            let tracker = self.goal_tracker.lock();
            let Some(o) = tracker.snapshot() else {
                return Err("goal summarizer: orchestration vanished before stage ran".to_string());
            };
            (
                o.objective.clone(),
                tracker.plan_path(),
                o.last_classifier_details_path.clone(),
            )
        };

        let session_traces_dir = crate::session::persistence::session_dir(&self.session_info);
        let model_id = self
            .chat_state_handle
            .get_sampling_config()
            .await
            .map(|c| c.model)
            .unwrap_or_default();
        let parent_prompt_id = self
            .current_prompt_id
            .lock()
            .expect("current_prompt_id mutex poisoned")
            .clone();
        // The summarizer always inherits the current model (no per-role key);
        // its §7 prompt names the parent toolset's tools.
        let tool_names = self.resolve_inherit_role_tool_names().await;

        let spawner: std::sync::Arc<dyn crate::session::goal_summarizer::GoalSummarizerSpawner> =
            std::sync::Arc::new(crate::session::goal_summarizer::ChannelSpawner {
                event_tx,
                foreground_wait: Some(crate::tools::tool_context::subagent_foreground_wait(
                    self.tool_context.blocking_wait_depth.clone(),
                )),
                parent_session_id: self.session_id_string(),
                parent_prompt_id,
                cwd: Some(self.tool_context.cwd.as_str().to_owned()),
            });

        let outcome = crate::session::goal_summarizer::run_goal_summarizer(
            spawner,
            crate::session::goal_summarizer::GoalSummarizerInputs {
                objective: &objective,
                plan_file: &plan_file,
                details_file: details_file.as_deref(),
                session_traces_dir: &session_traces_dir,
                attempt,
                model_id: &model_id,
                tool_names: &tool_names,
            },
        )
        .await;
        Ok(outcome)
    }

    /// Build a [`GoalNotifySender`] for the goal orchestrator.
    ///
    /// The sender can emit notifications and push system reminders
    /// independently of `SessionActor` (used by the `spawn_local`
    /// orchestrator task).
    pub(crate) fn goal_notify_sender(&self) -> crate::session::goal_orchestrator::GoalNotifySender {
        crate::session::goal_orchestrator::GoalNotifySender::new(
            self.session_info.id.clone(),
            self.notifications.gateway.clone(),
            self.notifications.persistence_tx.clone(),
        )
    }

    /// Returns `(ratcheted_total, finished_subagent_marginal_sum)` for the
    /// active goal, or `(0, 0)` if no orchestration is loaded.
    ///
    /// The ratcheted total folds in EVERY goal-scoped subagent's marginal
    /// (finished + in-flight) so the displayed/enforced spend tracks live
    /// progress. The second value is the wire `finished_subagent_tokens`
    /// and intentionally folds ONLY sealed (`finished`) records: the pager
    /// adds its own live active-subagent sum on top of that field
    /// (`GoalDisplayState::live_tokens_used`), so including an in-flight
    /// subagent here would double-count it in the live display / budget bar.
    ///
    /// Parent usage is accumulated as a monotonic spend counter
    /// (`parent_tokens_spent`): only POSITIVE deltas of the session token
    /// total are added, anchored at `last_session_tokens_seen`. A
    /// compaction that shrinks the context total merely re-anchors, so
    /// the count can neither decrease nor freeze until context regrows
    /// past a prior peak. Best-effort sampling: growth fully consumed by
    /// a compaction between two calls is unobserved.
    ///
    /// Side-effect: advances the spend accumulator and ratchets
    /// `tokens_used_high_water` monotonically; idempotent under stable
    /// inputs.
    pub(crate) fn goal_tokens(&self, current_session_tokens: i64) -> (i64, i64) {
        let goal_id = {
            let tracker = self.goal_tracker.lock();
            match tracker.snapshot() {
                Some(o) => o.goal_id.clone(),
                None => return (0, 0),
            }
        };
        // `subagent_sum` folds every goal-scoped record (finished + in-flight)
        // into the ratcheted total; `finished_subagent_sum` folds only sealed
        // records and is what ships on the wire as `finished_subagent_tokens`.
        // Keeping them distinct preserves the pager contract — the pager sums
        // running subagents itself and adds them to `finished_subagent_tokens`,
        // so an in-flight marginal must NOT appear in the wire field.
        let (subagent_sum, finished_subagent_sum) = {
            let records = self.subagent_token_records.lock();
            records
                .values()
                .filter(|r| r.goal_id.as_deref() == Some(goal_id.as_str()))
                .fold((0i64, 0i64), |(all, finished), r| {
                    let d = r.marginal();
                    if i64::try_from(d).is_err() {
                        static WARNED: std::sync::Once = std::sync::Once::new();
                        WARNED.call_once(|| {
                            tracing::warn!(
                                marginal = d,
                                "subagent token marginal exceeds i64::MAX; saturating"
                            );
                        });
                    }
                    let d = i64::try_from(d).unwrap_or(i64::MAX);
                    let all = all.saturating_add(d);
                    let finished = if r.finished {
                        finished.saturating_add(d)
                    } else {
                        finished
                    };
                    (all, finished)
                })
        };
        let ratcheted = {
            let mut tracker = self.goal_tracker.lock();
            let o = tracker
                .snapshot_mut()
                .expect("orchestration vanished between locks on single-threaded actor");
            // Legacy snapshots carry no anchor; seed from the goal baseline.
            let last_seen = o.last_session_tokens_seen.unwrap_or(o.token_baseline);
            if current_session_tokens > last_seen {
                o.parent_tokens_spent = o
                    .parent_tokens_spent
                    .saturating_add(current_session_tokens - last_seen);
            }
            o.last_session_tokens_seen = Some(current_session_tokens);
            let raw = o.parent_tokens_spent.saturating_add(subagent_sum);
            o.tokens_used_high_water = o.tokens_used_high_water.max(raw);
            o.tokens_used_high_water
        };
        (ratcheted, finished_subagent_sum)
    }

    /// Thin wrapper around [`Self::goal_tokens`] returning only the
    /// ratcheted total. Inherits the high-water-mark side-effect.
    pub(crate) fn goal_tokens_used(&self, current_session_tokens: i64) -> i64 {
        self.goal_tokens(current_session_tokens).0
    }

    /// Per-model marginal-token breakdown for the active goal's LIVE
    /// active-subagent window, sorted by tokens descending. Empty when no
    /// goal is loaded. Records with no captured model fold under
    /// `current_model_id` (best-effort). See [`fold_tokens_by_model`] for the
    /// fold contract. Fed into `update_live_progress` by the
    /// `SubagentProgress` handler.
    ///
    /// Sealed (`finished`) records are excluded: this feeds
    /// `live_tokens_by_model`, which is rendered under the running subagent's
    /// live block, so spend from earlier finished subagents must not leak in.
    /// This is the per-model analogue of the finished/in-flight split in
    /// [`Self::goal_tokens`]; the ratcheted total path is unaffected.
    pub(crate) fn goal_tokens_by_model(&self, current_model_id: &str) -> Vec<(String, u64)> {
        let goal_id = match self.goal_tracker.lock().snapshot() {
            Some(o) => o.goal_id.clone(),
            None => return Vec::new(),
        };
        let records = self.subagent_token_records.lock();
        fold_tokens_by_model(
            records.values().filter(|r| !r.finished),
            &goal_id,
            current_model_id,
        )
    }

    /// Push the tool-layer `GoalLoopActive` flag so per-tool-call bg-task /
    /// subagent completion reminders suppress themselves while the goal loop
    /// drives the turn. Mirrors the `CurrentPromptIdResource` push.
    ///
    /// Also mirrors the value into `tool_context.goal_loop_active_gate`, the
    /// shared `Arc<AtomicBool>` the notification bridge (bash auto-wake) and
    /// subagent spawn contexts (subagent auto-wake) read to suppress synthetic
    /// completion prompts mid-goal. Writing both from this one chokepoint keeps
    /// the gate from *persistently* drifting from the resource; the two writes
    /// are sequential (gate store, then async `update_resource`), so a transient
    /// window exists — benign, since those consumers read only the gate.
    pub(super) async fn set_goal_loop_active_resource(&self, active: bool) {
        self.tool_context
            .goal_loop_active_gate
            .store(active, std::sync::atomic::Ordering::Relaxed);
        self.agent
            .borrow()
            .tool_bridge()
            .update_resource(
                tools::implementations::grow_build::task::types::GoalLoopActive(active),
            )
            .await;
    }
}
