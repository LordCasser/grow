//! A request-only summary of owner-confirmed planning facts. No reads or writes.

use serde_json::{Value, json};
use tool_types::planning::{
    LocalPlan, PlanStep, PlanStepStatus, PlanningReadSnapshot, ReplanChange, ReplanProgress,
};
use tools::implementations::grow_build::todo::review_binding;

const MAX_CONTEXT_BYTES: usize = 8 * 1024;
const GUIDANCE: &str = "Current planning context (summary; guidance, not permission). Use read_entry with target self when a missing fact affects the task. Follow next_cursor if more facts are needed and restart on stale_cursor. Copy complete review_binding fields for a review and add a decision and evidence; never guess resultId.\n";

/// Inputs are canonical owner snapshots. Optional fields fit the escaped JSON budget.
pub(super) fn render_planning_context(
    own: &PlanningReadSnapshot,
    read_tool_name: Option<&str>,
) -> Result<String, String> {
    let plan = &own.plan;
    let mut context = Context(json!({
        "summary": true,
        "child_session_id": plan.session_id,
        "read_entry": {"availability": "unavailable"},
        "self": {
            "source": source_json(own),
            "counts": counts_json(plan),
            "omitted_counts": counts_json(plan),
            "unresolved_required_delivery_count": plan.unresolved_required_deliveries().len(),
            "unhandled_replan_count": plan.replans.iter().filter(|change| is_unhandled(change)).count(),
            "steps_summary": [], "links_summary": [], "replans_summary": [], "waits_summary": [],
        },
    }));
    if !context.fits() {
        return Err("planning context identities exceed the 8 KiB request budget".into());
    }
    if let Some(name) = read_tool_name {
        // Tool names are indivisible: never turn a shortened alias into a call.
        if !context.update(|root| {
            root["read_entry"] = json!({
                "availability": "available", "tool": name,
            })
        }) {
            context.update(|root| {
                root["read_entry"] = json!({
                    "availability": "omitted", "reason": "resolved name exceeds context budget",
                })
            });
        }
    }
    add_own_obligations(&mut context, own);
    Ok(context.finish())
}

fn add_own_obligations(context: &mut Context, snapshot: &PlanningReadSnapshot) {
    let plan = &snapshot.plan;
    // Incoming controls are obligations even when the child never made local steps.
    let changes = plan.replans.iter().filter(|change| is_unhandled(change));
    for change in changes
        .clone()
        .filter(|change| change.is_stop())
        .chain(changes.filter(|change| !change.is_stop()))
        .take(3)
    {
        context.update(|root| {
            root["self"]["replans_summary"]
                .as_array_mut()
                .unwrap()
                .push(change_json(change));
            decrement(&mut root["self"]["omitted_counts"]["replans"]);
        });
    }
    for link in plan.unresolved_required_deliveries().into_iter().take(3) {
        // An oversized result reference omits the entire actionable binding.
        // It must never become a truncated resultId or a fabricated null.
        let binding = review_binding(plan, link);
        context.update(|root| {
            root["self"]["links_summary"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "task_id": link.id, "step_id": link.parent_step_id,
                    "child_session_id": link.child_session_id,
                    "terminal": link.terminal, "required": link.required,
                    "pending_source_revision": link.pending_source_revision,
                    "review_binding": binding,
                }));
            decrement(&mut root["self"]["omitted_counts"]["links"]);
        });
    }
    for step in plan
        .steps
        .iter()
        .filter(|step| {
            !step.removed
                && matches!(
                    step.status,
                    PlanStepStatus::Pending | PlanStepStatus::InProgress
                )
        })
        .take(4)
    {
        let position = plan
            .steps
            .iter()
            .position(|candidate| candidate.id == step.id)
            .unwrap();
        let index = context.0["self"]["steps_summary"].as_array().unwrap().len();
        if context.update(|root| {
            root["self"]["steps_summary"]
                .as_array_mut()
                .unwrap()
                .push(step_json(step, position));
            decrement(&mut root["self"]["omitted_counts"]["steps"]);
        }) {
            let path = format!("/self/steps_summary/{index}");
            context.text(&format!("{path}/content_summary"), &step.content, 512);
            add_requirements(context, &path, step);
        }
    }
    for wait in plan.waits.iter().take(3) {
        context.update(|root| {
            root["self"]["waits_summary"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "id": wait.id, "condition": wait.condition,
                    "step_count": wait.step_ids.len(), "dependency_count": wait.dependencies.len(),
                }));
            decrement(&mut root["self"]["omitted_counts"]["waits"]);
        });
    }
}

fn source_json(snapshot: &PlanningReadSnapshot) -> Value {
    json!({"session_id": snapshot.plan.session_id, "scope_id": snapshot.plan.scope_id,
        "revision": snapshot.plan.revision, "timeline_seq": snapshot.timeline_seq})
}

fn counts_json(plan: &LocalPlan) -> Value {
    json!({"steps": plan.steps.len(), "links": plan.links.len(), "waits": plan.waits.len(),
        "replans": plan.replans.len(), "reviews": plan.reviews.len()})
}

fn step_json(step: &PlanStep, position: usize) -> Value {
    json!({
        "id": step.id, "position": position + 1, "status": step.status,
        "priority": step.priority, "removed": step.removed,
        "content_summary": {"text": "", "omitted_bytes": step.content.len()},
        "requirements_summary": {
            "count": step.requirements.as_ref().map_or(0, Vec::len),
            "omitted_count": step.requirements.as_ref().map_or(0, Vec::len), "items": [],
        },
    })
}

fn add_requirements(context: &mut Context, path: &str, step: &PlanStep) {
    for requirement in step.requirements.iter().flatten().take(2) {
        let pointer = format!("{path}/requirements_summary");
        let index = context.0.pointer(&pointer).unwrap()["items"]
            .as_array()
            .unwrap()
            .len();
        if context.update(|root| {
            let summary = root.pointer_mut(&pointer).unwrap();
            summary["items"].as_array_mut().unwrap().push(json!({
                "text": "", "omitted_bytes": requirement.len(),
            }));
            decrement(&mut summary["omitted_count"]);
        }) {
            context.text(&format!("{pointer}/items/{index}"), requirement, 512);
        }
    }
}

fn is_unhandled(change: &ReplanChange) -> bool {
    !matches!(
        change.progress,
        ReplanProgress::Handled { .. } | ReplanProgress::Unapplied { .. }
    )
}

fn change_json(change: &ReplanChange) -> Value {
    let progress = match change.progress {
        ReplanProgress::Pending => "pending",
        ReplanProgress::Dispatching => "dispatching",
        ReplanProgress::Received => "received",
        ReplanProgress::Consumed => "consumed",
        ReplanProgress::Handled { .. } => "handled",
        ReplanProgress::Unapplied { .. } => "unapplied",
    };
    json!({
        "id": change.id, "task_id": change.link_id, "step_id": change.step_id,
        "child_session_id": change.child_session_id,
        "source_revision": change.source_revision, "progress": progress,
        "directive": if change.is_stop() { "stop_and_wrap_up" } else { "replan" },
    })
}

fn decrement(value: &mut Value) {
    *value = json!(value.as_u64().unwrap().saturating_sub(1));
}

/// At most 8 KiB of accepted JSON is cloned; large optional records are discarded
/// as units. No operation truncates the final JSON string or an identity.
struct Context(Value);

impl Context {
    fn fits(&self) -> bool {
        GUIDANCE.len() + self.0.to_string().len() <= MAX_CONTEXT_BYTES
    }

    fn update(&mut self, update: impl FnOnce(&mut Value)) -> bool {
        let mut candidate = Self(self.0.clone());
        update(&mut candidate.0);
        if !candidate.fits() {
            return false;
        }
        *self = candidate;
        true
    }

    fn text(&mut self, pointer: &str, text: &str, preferred_bytes: usize) {
        let mut low = 0;
        let mut high = text.len().min(preferred_bytes);
        while low < high {
            let middle = low + (high - low).div_ceil(2);
            let mut end = middle;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            if self.update(|root| {
                *root.pointer_mut(pointer).expect("summary slot exists") = json!({
                    "text": &text[..end], "omitted_bytes": text.len() - end,
                });
            }) {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
    }

    fn finish(self) -> String {
        let text = format!("{GUIDANCE}{}", self.0);
        debug_assert!(text.len() <= MAX_CONTEXT_BYTES);
        text
    }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
