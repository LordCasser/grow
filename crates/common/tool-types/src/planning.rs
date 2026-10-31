//! Serializable facts and pure transitions for session-local plans.
//!
//! This module deliberately does not validate session ownership or dependency
//! access. The runtime owns those checks; these types only preserve the facts
//! that the owner has already validated.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

const MAX_PLAN_ITEMS: usize = 4096;
const MAX_PLAN_ID_BYTES: usize = 128;
const MAX_REPLAN_ID_BYTES: usize = MAX_PLAN_ID_BYTES * 2 + 22;
const MAX_PLAN_TEXT_BYTES: usize = 64 * 1024;
const MAX_PLAN_DEPENDENCIES: usize = 256;
const MAX_SUMMARY_REFERENCE_BYTES: usize = 1024;

/// A plan belongs to one session and one explicitly selected work scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalPlan {
    pub session_id: String,
    pub scope_id: String,
    /// Advances for any effective plan update. Replan work uses the revision
    /// of the source change, not a completion-only update.
    pub revision: u64,
    pub steps: Vec<PlanStep>,
    pub links: Vec<PlanLink>,
    pub replans: Vec<ReplanChange>,
    pub waits: Vec<PlanWait>,
    pub reviews: Vec<DeliveryReview>,
}

impl LocalPlan {
    pub fn new(session_id: impl Into<String>, scope_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            scope_id: scope_id.into(),
            revision: 0,
            steps: Vec::new(),
            links: Vec::new(),
            replans: Vec::new(),
            waits: Vec::new(),
            reviews: Vec::new(),
        }
    }

    /// Validate the local identity graph before a planning snapshot becomes
    /// authoritative. Ownership access checks remain the runtime's job.
    pub fn validate(&self) -> Result<(), String> {
        if !valid_plan_id(&self.session_id) || !valid_plan_id(&self.scope_id) {
            return Err("plan session and scope identities must be nonempty and bounded".into());
        }
        if [
            self.steps.len(),
            self.links.len(),
            self.replans.len(),
            self.waits.len(),
            self.reviews.len(),
        ]
        .into_iter()
        .any(|len| len > MAX_PLAN_ITEMS)
        {
            return Err("plan collection exceeds its item limit".into());
        }

        let mut step_ids = std::collections::HashSet::new();
        for step in &self.steps {
            if !valid_plan_id(&step.id)
                || !step_ids.insert(step.id.as_str())
                || !bounded_plan_text(&step.content)
                || step.requirements.as_ref().is_some_and(|items| {
                    items.len() > MAX_PLAN_DEPENDENCIES
                        || items.iter().any(|item| !bounded_plan_text(item))
                })
            {
                return Err("plan step identity, text, or requirements are invalid".into());
            }
        }

        let mut links_by_id = std::collections::HashMap::new();
        for link in &self.links {
            if !valid_plan_id(&link.id)
                || !valid_plan_id(&link.parent_step_id)
                || !valid_plan_id(&link.child_session_id)
                || link.child_session_id == self.session_id
                || !step_ids.contains(link.parent_step_id.as_str())
                || link.review_required_revision > self.revision
                || link
                    .pending_source_revision
                    .is_some_and(|revision| revision > self.revision)
                || (link.terminal && link.pending_source_revision.is_some())
                || link
                    .result_id
                    .as_deref()
                    .is_some_and(|result| !bounded_plan_text(result))
                || link
                    .dispatch_failure
                    .as_deref()
                    .is_some_and(|reason| !bounded_plan_text(reason) || reason.trim().is_empty())
                || (link.result_id.is_some() && !link.terminal)
                || (link.dispatch_failure.is_some() && (link.terminal || link.result_id.is_some()))
                || links_by_id.insert(link.id.as_str(), link).is_some()
            {
                return Err("plan link identity or source reference is invalid".into());
            }
        }

        let mut replan_ids = std::collections::HashSet::new();
        let mut replan_sources = std::collections::HashSet::new();
        for replan in &self.replans {
            if !valid_replan_id(&replan.id)
                || !valid_plan_id(&replan.link_id)
                || !valid_plan_id(&replan.child_session_id)
                || !valid_plan_id(&replan.step_id)
                || !replan_ids.insert(replan.id.as_str())
                || !replan_sources.insert((
                    replan.link_id.as_str(),
                    replan.child_session_id.as_str(),
                    replan.step_id.as_str(),
                    replan.source_revision,
                ))
                || !bounded_plan_text(&replan.content)
                || replan.source_revision == 0
                || replan.requirements.as_ref().is_some_and(|items| {
                    items.len() > MAX_PLAN_DEPENDENCIES
                        || items.iter().any(|item| !bounded_plan_text(item))
                })
            {
                return Err("replan identity or payload is invalid".into());
            }
            if replan.child_session_id == self.session_id {
                // Incoming records describe the parent-owned step and link,
                // which are intentionally outside this local plan graph.
            } else {
                match links_by_id.get(replan.link_id.as_str()) {
                    Some(link)
                        if link.child_session_id == replan.child_session_id
                            && link.parent_step_id == replan.step_id
                            && replan.source_revision <= self.revision => {}
                    Some(_) => {
                        return Err("replan does not match its local delegation link".into());
                    }
                    None => {
                        return Err("replan has no local link or receiving-session owner".into());
                    }
                }
            }
            match &replan.progress {
                ReplanProgress::Handled {
                    outcome:
                        ReplanOutcome::NoChange { reason } | ReplanOutcome::NotApplied { reason },
                }
                | ReplanProgress::Unapplied { reason }
                    if !valid_plan_text(reason) =>
                {
                    return Err("terminal replan outcome needs a bounded reason".into());
                }
                ReplanProgress::Handled {
                    outcome: ReplanOutcome::Updated { plan_revision },
                } if replan.child_session_id == self.session_id
                    && *plan_revision > self.revision =>
                {
                    return Err("handled replan cannot cite a future local plan revision".into());
                }
                _ => {}
            }
        }

        for link in &self.links {
            let expected_pending = self
                .replans
                .iter()
                .filter(|replan| {
                    replan.link_id == link.id
                        && replan.child_session_id == link.child_session_id
                        && !replan.progress.is_settled()
                })
                .map(|replan| replan.source_revision)
                .max();
            if link.pending_source_revision != expected_pending {
                return Err("link pending revision does not match its open replans".into());
            }
        }

        let mut wait_ids = std::collections::HashSet::new();
        for wait in &self.waits {
            if !valid_plan_id(&wait.id)
                || !wait_ids.insert(wait.id.as_str())
                || wait.dependencies.is_empty()
                || wait.dependencies.len() > MAX_PLAN_DEPENDENCIES
                || wait.step_ids.len() > MAX_PLAN_ITEMS
                || wait
                    .step_ids
                    .iter()
                    .any(|id| !valid_plan_id(id) || !step_ids.contains(id.as_str()))
                || wait
                    .dependencies
                    .iter()
                    .any(|dependency| !valid_plan_id(&dependency.id))
            {
                return Err("plan wait identity or dependency reference is invalid".into());
            }
        }

        for review in &self.reviews {
            let Some(link) = links_by_id.get(review.link_id.as_str()) else {
                return Err("delivery review references an unknown link".into());
            };
            if review.reviewed_source_revision > self.revision
                || review.result_id != link.result_id
                || review
                    .reason
                    .as_deref()
                    .is_some_and(|reason| !bounded_plan_text(reason))
                || review.verification_refs.len() > MAX_PLAN_DEPENDENCIES
                || review
                    .verification_refs
                    .iter()
                    .any(|reference| !bounded_plan_text(reference))
            {
                return Err(
                    "delivery review is not bound to the current link result/revision".into(),
                );
            }
            match review.decision {
                ReviewDecision::Accepted | ReviewDecision::NeedsRevision
                    if !link.terminal || review.result_id.is_none() =>
                {
                    return Err("result review requires the exact terminal delivery".into());
                }
                ReviewDecision::Released
                    if review
                        .reason
                        .as_deref()
                        .is_none_or(|reason| reason.trim().is_empty()) =>
                {
                    return Err("releasing a delivery requires a reason".into());
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Validate the durable transition from the previous snapshot. Pending
    /// replans may be replaced by a newer coalesced pending batch; once claimed
    /// or received, their identity and payload remain fixed.
    pub fn validate_transition_from(&self, previous: &Self) -> Result<(), String> {
        self.validate()?;
        previous.validate()?;
        if self.session_id != previous.session_id {
            return Err("planning session ownership changed".into());
        }
        if self.scope_id != previous.scope_id {
            if !previous.waits.is_empty()
                || previous
                    .links
                    .iter()
                    .any(|link| !link.terminal && link.dispatch_failure.is_none())
                || !previous.unresolved_required_deliveries().is_empty()
                || previous
                    .replans
                    .iter()
                    .any(|replan| !replan.progress.is_settled())
            {
                return Err("work scope changed before its obligations were disposed".into());
            }
            return Ok(());
        }
        if self.revision < previous.revision
            || (self.steps != previous.steps && self.revision <= previous.revision)
        {
            return Err("planning revision did not advance with an effective step change".into());
        }

        let current_steps: std::collections::HashMap<_, _> = self
            .steps
            .iter()
            .map(|step| (step.id.as_str(), step))
            .collect();
        for old in &previous.steps {
            let Some(current) = current_steps.get(old.id.as_str()) else {
                return Err("planning transition erased a step identity".into());
            };
            if old.removed && *current != old {
                return Err("planning transition changed a tombstoned step".into());
            }
        }

        let current_links: std::collections::HashMap<_, _> = self
            .links
            .iter()
            .map(|link| (link.id.as_str(), link))
            .collect();
        for old in &previous.links {
            let Some(current) = current_links.get(old.id.as_str()) else {
                return Err("planning transition erased a delegation link".into());
            };
            if current.parent_step_id != old.parent_step_id
                || current.child_session_id != old.child_session_id
                || current.required != old.required
                || (old.terminal && !current.terminal)
                || (old.result_id.is_some() && current.result_id != old.result_id)
                || (old.dispatch_failure.is_some()
                    && current.dispatch_failure != old.dispatch_failure)
                || current.review_required_revision < old.review_required_revision
            {
                return Err("planning transition rewrote a delegation link identity".into());
            }
        }
        if self.reviews.len() < previous.reviews.len()
            || self.reviews[..previous.reviews.len()] != previous.reviews[..]
        {
            return Err("planning transition erased or rewrote a delivery review".into());
        }

        let current_replans: std::collections::HashMap<_, _> = self
            .replans
            .iter()
            .map(|replan| (replan.id.as_str(), replan))
            .collect();
        for old in &previous.replans {
            match current_replans.get(old.id.as_str()) {
                Some(current) => {
                    let terminalized =
                        matches!(&current.progress, ReplanProgress::Unapplied { .. })
                            && !old.progress.is_settled()
                            && current_links.get(old.link_id.as_str()).is_some_and(|link| {
                                link.terminal || link.dispatch_failure.is_some()
                            });
                    if !old.same_identity_payload(current)
                        || (!old.progress.can_advance_to(&current.progress) && !terminalized)
                    {
                        return Err("planning transition rewrote or regressed a replan".into());
                    }
                }
                None if old.progress == ReplanProgress::Pending => {
                    let coalesced = self.replans.iter().any(|current| {
                        current.link_id == old.link_id
                            && current.child_session_id == old.child_session_id
                            && current.step_id == old.step_id
                            && current.source_revision > old.source_revision
                            && current.progress == ReplanProgress::Pending
                    });
                    if !coalesced {
                        return Err("planning transition erased a pending replan".into());
                    }
                }
                None => return Err("planning transition erased an immutable replan receipt".into()),
            }
        }
        Ok(())
    }

    /// Apply a full-step merge or replacement and report only effective
    /// changes. In replace mode, omitted live steps become tombstones.
    pub fn update_steps(
        &mut self,
        mode: PlanUpdateMode,
        incoming: Vec<PlanStep>,
    ) -> Result<PlanUpdate, PlanUpdateError> {
        validate_incoming(&incoming)?;
        for step in &incoming {
            if let Some(existing) = self.steps.iter().find(|old| old.id == step.id)
                && existing.removed
            {
                return Err(PlanUpdateError::TombstonedIdReuse(step.id.clone()));
            }
        }

        let before = self.steps.clone();
        let mut changes = Vec::new();

        match mode {
            PlanUpdateMode::Merge => {
                for step in incoming {
                    match self.steps.iter_mut().find(|old| old.id == step.id) {
                        Some(old) => {
                            let change = compare_step(old, &step);
                            if let Some(change) = change {
                                *old = step;
                                changes.push(change);
                            }
                        }
                        None => {
                            changes.push(StepChange::added(&step));
                            self.steps.push(step);
                        }
                    }
                }
            }
            PlanUpdateMode::Replace => {
                let retained: Vec<String> = incoming.iter().map(|step| step.id.clone()).collect();
                for old in &mut self.steps {
                    if !old.removed && !retained.iter().any(|id| id == &old.id) {
                        old.removed = true;
                        changes.push(StepChange::removed(old));
                    }
                }
                for step in incoming {
                    match self.steps.iter_mut().find(|old| old.id == step.id) {
                        Some(old) => {
                            let change = compare_step(old, &step);
                            if let Some(change) = change {
                                *old = step;
                                changes.push(change);
                            }
                        }
                        None => {
                            changes.push(StepChange::added(&step));
                            self.steps.push(step);
                        }
                    }
                }
            }
        }

        if self.steps != before {
            let Some(next_revision) = self.revision.checked_add(1) else {
                self.steps = before;
                return Err(PlanUpdateError::RevisionExhausted);
            };
            self.revision = next_revision;
        }
        let source_changes: Vec<_> = changes
            .iter()
            .filter(|change| change.replans)
            .cloned()
            .collect();
        for change in &source_changes {
            let payload = self.steps.iter().find(|step| step.id == change.step_id);
            let (content, requirements, priority, status, removed) = match payload {
                Some(step) => (
                    step.content.clone(),
                    step.requirements.clone(),
                    step.priority,
                    step.status,
                    step.removed,
                ),
                None => continue,
            };
            let source_revision = self.revision;
            let links: Vec<_> =
                self.links
                    .iter_mut()
                    .filter(|link| link.parent_step_id == change.step_id)
                    .map(|link| {
                        let unapplied_reason = if link.terminal {
                            Some("child is already terminal".to_owned())
                        } else {
                            link.dispatch_failure.clone()
                        };
                        if unapplied_reason.is_none() {
                            link.pending_source_revision = Some(source_revision);
                        }
                        if change.fields.iter().any(|field| {
                            matches!(field, StepField::Content | StepField::Requirements)
                        }) {
                            link.review_required_revision = source_revision;
                        }
                        (
                            link.id.clone(),
                            link.child_session_id.clone(),
                            unapplied_reason,
                        )
                    })
                    .collect();
            for (link_id, child_session_id, unapplied_reason) in links {
                let id = format!("{link_id}:{}:{source_revision}", change.step_id);
                let record = ReplanChange {
                    id,
                    link_id,
                    child_session_id,
                    step_id: change.step_id.clone(),
                    source_revision,
                    content: content.clone(),
                    requirements: requirements.clone(),
                    priority,
                    status,
                    removed,
                    progress: unapplied_reason.map_or(ReplanProgress::Pending, |reason| {
                        ReplanProgress::Unapplied { reason }
                    }),
                };
                if record.progress.is_settled() {
                    self.replans.push(record);
                } else if let Some(index) = self.replans.iter().position(|existing| {
                    existing.link_id == record.link_id
                        && existing.step_id == record.step_id
                        && existing.progress == ReplanProgress::Pending
                }) {
                    // Pending has not been claimed for dispatch, so it may
                    // coalesce. Dispatching and Received payloads are immutable.
                    self.replans[index] = record;
                } else {
                    self.replans.push(record);
                }
            }
        }

        Ok(PlanUpdate {
            revision: self.revision,
            changes,
            source_changes,
        })
    }

    /// Advance one immutable replan identity. An old acknowledgement only
    /// updates its own record; pending source revision is recomputed from all
    /// still-open records for the link.
    pub fn advance_replan(&mut self, id: &str, next: ReplanProgress) -> Result<(), ReplanError> {
        if let ReplanProgress::Handled {
            outcome: ReplanOutcome::NoChange { reason },
        } = &next
            && reason.trim().is_empty()
        {
            return Err(ReplanError::EmptyNoChangeReason);
        }
        let record = self
            .replans
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or_else(|| ReplanError::UnknownId(id.to_owned()))?;
        if !record.progress.can_advance_to(&next) {
            return Err(ReplanError::InvalidProgress);
        }
        record.progress = next;
        let link_id = record.link_id.clone();
        let pending = self
            .replans
            .iter()
            .filter(|record| record.link_id == link_id && !record.progress.is_settled())
            .map(|record| record.source_revision)
            .max();
        if let Some(link) = self.links.iter_mut().find(|link| link.id == link_id) {
            link.pending_source_revision = pending;
        }
        Ok(())
    }

    /// Deduplicate an incoming delivery by immutable identity and payload.
    /// Replaying a receipt never rewinds an already consumed or handled item.
    pub fn receive_replan(&mut self, incoming: ReplanChange) -> Result<bool, ReplanError> {
        if incoming.progress != ReplanProgress::Received
            || !valid_replan_id(&incoming.id)
            || !valid_plan_id(&incoming.link_id)
            || !valid_plan_id(&incoming.child_session_id)
            || !valid_plan_id(&incoming.step_id)
            || !bounded_plan_text(&incoming.content)
            || incoming.requirements.as_ref().is_some_and(|items| {
                items.len() > MAX_PLAN_DEPENDENCIES
                    || items.iter().any(|item| !bounded_plan_text(item))
            })
        {
            return Err(ReplanError::InvalidProgress);
        }
        if let Some(existing) = self.replans.iter().find(|item| item.id == incoming.id) {
            if !existing.same_identity_payload(&incoming) {
                return Err(ReplanError::IdentityPayloadConflict(incoming.id));
            }
            return Ok(false);
        }
        self.replans.push(incoming);
        Ok(true)
    }

    /// Record a confirmed terminal child without leaving its unfinished
    /// replan responsibility pending or reopening the child.
    pub fn mark_link_terminal(
        &mut self,
        link_id: &str,
        reason: impl Into<String>,
    ) -> Result<(), ReplanError> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(ReplanError::EmptyUnappliedReason);
        }
        let link = self
            .links
            .iter_mut()
            .find(|link| link.id == link_id)
            .ok_or_else(|| ReplanError::UnknownLink(link_id.to_owned()))?;
        if link.dispatch_failure.is_some() {
            return Err(ReplanError::InvalidProgress);
        }
        link.terminal = true;
        link.pending_source_revision = None;
        for record in self
            .replans
            .iter_mut()
            .filter(|record| record.link_id == link_id && !record.progress.is_settled())
        {
            record.progress = ReplanProgress::Unapplied {
                reason: reason.clone(),
            };
        }
        Ok(())
    }

    /// Record an explicit coordinator rejection before the child was admitted.
    /// This never marks the link terminal or creates a child result.
    pub fn mark_link_undispatched(
        &mut self,
        link_id: &str,
        reason: impl Into<String>,
    ) -> Result<bool, ReplanError> {
        let reason = reason.into();
        if reason.trim().is_empty() || !bounded_plan_text(&reason) {
            return Err(ReplanError::EmptyUnappliedReason);
        }
        let link = self
            .links
            .iter_mut()
            .find(|link| link.id == link_id)
            .ok_or_else(|| ReplanError::UnknownLink(link_id.to_owned()))?;
        if link.terminal || link.result_id.is_some() {
            return Err(ReplanError::InvalidProgress);
        }
        if let Some(existing) = &link.dispatch_failure
            && existing != &reason
        {
            return Err(ReplanError::DispatchFailureConflict);
        }

        let mut changed = link.dispatch_failure.is_none();
        link.dispatch_failure = Some(reason.clone());
        changed |= link.pending_source_revision.take().is_some();
        for record in self
            .replans
            .iter_mut()
            .filter(|record| record.link_id == link_id && !record.progress.is_settled())
        {
            record.progress = ReplanProgress::Unapplied {
                reason: reason.clone(),
            };
            changed = true;
        }
        Ok(changed)
    }

    /// Required links alone participate in this explicit delivery gate.
    pub fn unresolved_required_deliveries(&self) -> Vec<&PlanLink> {
        self.links
            .iter()
            .filter(|link| link.required && !self.delivery_is_closed(&link.id))
            .collect()
    }

    fn delivery_is_closed(&self, link_id: &str) -> bool {
        let Some(link) = self.links.iter().find(|link| link.id == link_id) else {
            return false;
        };
        self.reviews.iter().any(|review| {
            review.link_id == link_id
                && match review.decision {
                    ReviewDecision::Released => {
                        review.reason.as_ref().is_some_and(|r| !r.trim().is_empty())
                    }
                    ReviewDecision::NeedsRevision => false,
                    ReviewDecision::Accepted => {
                        let Some(result_id) = &link.result_id else {
                            return false;
                        };
                        link.terminal
                            && review.result_id.as_ref() == Some(result_id)
                            && review.reviewed_source_revision >= link.review_required_revision
                    }
                }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    /// Stable within `(session_id, scope_id)`; a tombstoned ID is never reused.
    pub id: String,
    pub content: String,
    pub priority: PlanPriority,
    pub status: PlanStepStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requirements: Option<Vec<String>>,
    #[serde(default)]
    pub removed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanPriority {
    Low,
    #[default]
    Normal,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanUpdateMode {
    Merge,
    Replace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StepChange {
    pub step_id: String,
    pub fields: Vec<StepField>,
    /// True for non-completion task-intent changes. Completion-only changes
    /// remain in `changes` but do not request replanning.
    pub replans: bool,
}

impl StepChange {
    fn added(step: &PlanStep) -> Self {
        Self {
            step_id: step.id.clone(),
            fields: vec![StepField::Added],
            replans: true,
        }
    }

    fn removed(step: &PlanStep) -> Self {
        Self {
            step_id: step.id.clone(),
            fields: vec![StepField::Removed],
            replans: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepField {
    Added,
    Content,
    Requirements,
    Priority,
    Status,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanUpdate {
    pub revision: u64,
    pub changes: Vec<StepChange>,
    pub source_changes: Vec<StepChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanUpdateError {
    EmptyStepId,
    DuplicateStepId(String),
    TombstonedIdReuse(String),
    RevisionExhausted,
}

fn valid_plan_id(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_PLAN_ID_BYTES
        && !value.chars().any(char::is_control)
}

fn valid_replan_id(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_REPLAN_ID_BYTES
        && !value.chars().any(char::is_control)
}

fn bounded_plan_text(value: &str) -> bool {
    value.len() <= MAX_PLAN_TEXT_BYTES
}

fn valid_plan_text(value: &str) -> bool {
    !value.trim().is_empty() && bounded_plan_text(value)
}

fn validate_incoming(steps: &[PlanStep]) -> Result<(), PlanUpdateError> {
    let mut ids = std::collections::HashSet::new();
    for step in steps {
        if step.id.trim().is_empty() {
            return Err(PlanUpdateError::EmptyStepId);
        }
        if !ids.insert(&step.id) {
            return Err(PlanUpdateError::DuplicateStepId(step.id.clone()));
        }
    }
    Ok(())
}

fn compare_step(old: &PlanStep, new: &PlanStep) -> Option<StepChange> {
    let mut fields = Vec::new();
    if old.content != new.content {
        fields.push(StepField::Content);
    }
    if old.requirements != new.requirements {
        fields.push(StepField::Requirements);
    }
    if old.priority != new.priority {
        fields.push(StepField::Priority);
    }
    if old.status != new.status {
        fields.push(StepField::Status);
    }
    if old.removed != new.removed {
        fields.push(StepField::Removed);
    }
    (!fields.is_empty()).then(|| StepChange {
        step_id: old.id.clone(),
        replans: fields.iter().any(|field| {
            !matches!(field, StepField::Status) || new.status != PlanStepStatus::Completed
        }),
        fields,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanLink {
    pub id: String,
    pub parent_step_id: String,
    pub child_session_id: String,
    pub terminal: bool,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_source_revision: Option<u64>,
    /// Latest source revision whose content/requirements still needs explicit
    /// review of this link's delivered result.
    #[serde(default)]
    pub review_required_revision: u64,
    /// Latest immutable delivered result reference, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_id: Option<String>,
    /// Explicit evidence that coordinator admission rejected this task before
    /// execution began. This is not a child terminal result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatch_failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReplanChange {
    pub id: String,
    pub link_id: String,
    pub child_session_id: String,
    pub step_id: String,
    pub source_revision: u64,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requirements: Option<Vec<String>>,
    pub priority: PlanPriority,
    pub status: PlanStepStatus,
    pub removed: bool,
    pub progress: ReplanProgress,
}

impl ReplanChange {
    fn same_identity_payload(&self, other: &Self) -> bool {
        self.id == other.id
            && self.link_id == other.link_id
            && self.child_session_id == other.child_session_id
            && self.step_id == other.step_id
            && self.source_revision == other.source_revision
            && self.content == other.content
            && self.requirements == other.requirements
            && self.priority == other.priority
            && self.status == other.status
            && self.removed == other.removed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReplanProgress {
    Pending,
    /// Claimed by the source worker before sending; the payload is now fixed.
    Dispatching,
    Received,
    Consumed,
    Handled {
        outcome: ReplanOutcome,
    },
    /// The referenced child was already terminal when the source changed.
    /// This preserves the unapplied fact without reopening the child.
    Unapplied {
        reason: String,
    },
}

impl ReplanProgress {
    fn is_settled(&self) -> bool {
        matches!(self, Self::Handled { .. } | Self::Unapplied { .. })
    }

    fn can_advance_to(&self, next: &Self) -> bool {
        match (self, next) {
            (Self::Pending, Self::Pending | Self::Dispatching | Self::Received)
            | (Self::Dispatching, Self::Dispatching | Self::Received)
            | (Self::Received, Self::Received | Self::Consumed)
            | (Self::Consumed, Self::Consumed) => true,
            (Self::Consumed, Self::Handled { .. }) => true,
            (Self::Handled { outcome: old }, Self::Handled { outcome: new }) => old == new,
            (Self::Unapplied { reason: old }, Self::Unapplied { reason: new }) => old == new,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReplanOutcome {
    Updated { plan_revision: u64 },
    NoChange { reason: String },
    NotApplied { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplanError {
    UnknownId(String),
    UnknownLink(String),
    InvalidProgress,
    EmptyNoChangeReason,
    EmptyUnappliedReason,
    IdentityPayloadConflict(String),
    DispatchFailureConflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanWait {
    pub id: String,
    /// Empty means the entire local work is waiting. Otherwise only these
    /// steps wait, leaving other local steps available to advance.
    #[serde(default)]
    pub step_ids: Vec<String>,
    pub dependencies: Vec<WaitDependency>,
    pub condition: WaitCondition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WaitDependency {
    pub kind: DependencyKind,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    Task,
    Command,
    Question,
    Permission,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WaitCondition {
    Any,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryReview {
    pub link_id: String,
    /// `None` permits an explicit release before any result exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_id: Option<String>,
    /// Source revision whose content/requirements this decision reviewed.
    pub reviewed_source_revision: u64,
    pub decision: ReviewDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verification_refs: Vec<String>,
}

/// Bounded local planning facts attached to a canonical child result for its
/// direct parent. It is a projection of the child's plan, not copied plan
/// state or execution authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubagentPlanningSummary {
    pub planning_seq: u64,
    pub scope_id: String,
    pub revision: u64,
    pub reviews: Vec<SubagentPlanningReview>,
    pub review_count: u32,
    pub reviews_truncated: u32,
    pub unresolved_required_deliveries: Vec<SubagentRequiredDelivery>,
    pub unresolved_required_delivery_count: u32,
    pub unresolved_required_deliveries_truncated: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubagentPlanningReview {
    pub task_id: String,
    pub result_ref: Option<String>,
    pub requirements_revision: u64,
    pub decision: ReviewDecision,
    pub verification_refs: Vec<String>,
    pub verification_ref_count: u32,
    pub verification_refs_truncated: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubagentRequiredDelivery {
    pub link_id: String,
    pub parent_step_id: String,
    pub parent_step_status: PlanStepStatus,
    pub parent_step_removed: bool,
}

impl SubagentPlanningSummary {
    pub const MAX_ITEMS: usize = 8;
    pub const MAX_VERIFICATION_REFS: usize = 8;

    pub fn from_plan(planning_seq: u64, plan: &LocalPlan) -> Result<Self, String> {
        plan.validate()?;
        let reviews = plan
            .reviews
            .iter()
            .take(Self::MAX_ITEMS)
            .map(|review| {
                let link = plan
                    .links
                    .iter()
                    .find(|link| link.id == review.link_id)
                    .expect("validated review has a link");
                SubagentPlanningReview {
                    task_id: review.link_id.clone(),
                    result_ref: review.result_id.clone(),
                    requirements_revision: link.review_required_revision,
                    decision: review.decision,
                    verification_refs: review
                        .verification_refs
                        .iter()
                        .take(Self::MAX_VERIFICATION_REFS)
                        .cloned()
                        .collect(),
                    verification_ref_count: review.verification_refs.len() as u32,
                    verification_refs_truncated: review
                        .verification_refs
                        .len()
                        .saturating_sub(Self::MAX_VERIFICATION_REFS)
                        as u32,
                }
            })
            .collect::<Vec<_>>();
        let required = plan.unresolved_required_deliveries();
        let unresolved_required_deliveries = required
            .iter()
            .take(Self::MAX_ITEMS)
            .map(|link| {
                let step = plan
                    .steps
                    .iter()
                    .find(|step| step.id == link.parent_step_id)
                    .expect("validated link has a parent step");
                SubagentRequiredDelivery {
                    link_id: link.id.clone(),
                    parent_step_id: step.id.clone(),
                    parent_step_status: step.status,
                    parent_step_removed: step.removed,
                }
            })
            .collect::<Vec<_>>();
        let summary = Self {
            planning_seq,
            scope_id: plan.scope_id.clone(),
            revision: plan.revision,
            reviews,
            review_count: u32::try_from(plan.reviews.len()).map_err(|_| "review count overflow")?,
            reviews_truncated: u32::try_from(plan.reviews.len().saturating_sub(Self::MAX_ITEMS))
                .map_err(|_| "review truncation count overflow")?,
            unresolved_required_deliveries,
            unresolved_required_delivery_count: u32::try_from(required.len())
                .map_err(|_| "delivery count overflow")?,
            unresolved_required_deliveries_truncated: u32::try_from(
                required.len().saturating_sub(Self::MAX_ITEMS),
            )
            .map_err(|_| "delivery truncation count overflow")?,
        };
        summary.validate()?;
        Ok(summary)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !valid_plan_id(&self.scope_id)
            || self.reviews.len() > Self::MAX_ITEMS
            || self.unresolved_required_deliveries.len() > Self::MAX_ITEMS
            || self.reviews.iter().any(|review| {
                !valid_plan_id(&review.task_id)
                    || review.result_ref.as_deref().is_some_and(|value| {
                        !bounded_plan_text(value) || value.len() > MAX_SUMMARY_REFERENCE_BYTES
                    })
                    || review.verification_refs.len() > Self::MAX_VERIFICATION_REFS
                    || review.verification_refs.iter().any(|value| {
                        !bounded_plan_text(value) || value.len() > MAX_SUMMARY_REFERENCE_BYTES
                    })
                    || u64::from(review.verification_ref_count)
                        != review.verification_refs.len() as u64
                            + u64::from(review.verification_refs_truncated)
                    || review.verification_ref_count as usize > MAX_PLAN_DEPENDENCIES
                    || review.verification_refs.len()
                        != review
                            .verification_ref_count
                            .min(Self::MAX_VERIFICATION_REFS as u32)
                            as usize
            })
            || self
                .unresolved_required_deliveries
                .iter()
                .any(|item| !valid_plan_id(&item.link_id) || !valid_plan_id(&item.parent_step_id))
            || u64::from(self.review_count)
                != self.reviews.len() as u64 + u64::from(self.reviews_truncated)
            || self.review_count as usize > MAX_PLAN_ITEMS
            || self.reviews.len() != self.review_count.min(Self::MAX_ITEMS as u32) as usize
            || u64::from(self.unresolved_required_delivery_count)
                != self.unresolved_required_deliveries.len() as u64
                    + u64::from(self.unresolved_required_deliveries_truncated)
            || self.unresolved_required_delivery_count as usize > MAX_PLAN_ITEMS
            || self.unresolved_required_deliveries.len()
                != self
                    .unresolved_required_delivery_count
                    .min(Self::MAX_ITEMS as u32) as usize
        {
            return Err("child planning summary is invalid or exceeds its bounds".into());
        }
        Ok(())
    }

    pub fn render_bounded(&self) -> String {
        let mut rendered = format!(
            "Planning scope {} revision {} (snapshot {}): {} review decisions, {} unresolved required deliveries.",
            self.scope_id,
            self.revision,
            self.planning_seq,
            self.review_count,
            self.unresolved_required_delivery_count,
        );
        for review in &self.reviews {
            let decision = match review.decision {
                ReviewDecision::Accepted => "accepted",
                ReviewDecision::NeedsRevision => "needs_revision",
                ReviewDecision::Released => "released",
            };
            let _ = write!(
                rendered,
                "\nReview {}: {} at requirements revision {}; result={:?}; verification_refs={:?}.",
                review.task_id,
                decision,
                review.requirements_revision,
                review.result_ref,
                review.verification_refs,
            );
        }
        for delivery in &self.unresolved_required_deliveries {
            let status = match delivery.parent_step_status {
                PlanStepStatus::Pending => "pending",
                PlanStepStatus::InProgress => "in_progress",
                PlanStepStatus::Completed => "completed",
                PlanStepStatus::Cancelled => "cancelled",
            };
            let _ = write!(
                rendered,
                "\nRequired delivery {} for step {} ({status}{}).",
                delivery.link_id,
                delivery.parent_step_id,
                if delivery.parent_step_removed {
                    ", removed"
                } else {
                    ""
                }
            );
        }
        rendered
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    Accepted,
    NeedsRevision,
    Released,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: &str, content: &str, status: PlanStepStatus) -> PlanStep {
        PlanStep {
            id: id.into(),
            content: content.into(),
            priority: PlanPriority::Normal,
            status,
            requirements: None,
            removed: false,
        }
    }

    fn linked_plan() -> LocalPlan {
        let mut plan = LocalPlan::new("parent", "work");
        plan.steps
            .push(step("s1", "first", PlanStepStatus::Pending));
        plan.links.push(PlanLink {
            id: "l1".into(),
            parent_step_id: "s1".into(),
            child_session_id: "child".into(),
            terminal: false,
            required: true,
            pending_source_revision: None,
            review_required_revision: 0,
            result_id: None,
            dispatch_failure: None,
        });
        plan
    }

    #[test]
    fn validation_preserves_tombstone_references_and_accepts_incoming_replans() {
        let mut parent = linked_plan();
        parent
            .update_steps(PlanUpdateMode::Replace, Vec::new())
            .unwrap();
        parent.validate().unwrap();
        assert!(
            parent
                .steps
                .iter()
                .any(|step| step.id == "s1" && step.removed)
        );
        assert_eq!(parent.links[0].parent_step_id, "s1");
        assert!(parent.replans[0].removed);

        let mut child = LocalPlan::new("child", "child-work");
        let mut incoming = parent.replans[0].clone();
        incoming.progress = ReplanProgress::Received;
        child.receive_replan(incoming).unwrap();
        child.validate().unwrap();
    }

    #[test]
    fn validation_rejects_duplicate_or_dangling_identities_and_future_reviews() {
        let mut duplicate = linked_plan();
        duplicate
            .steps
            .push(step("s1", "duplicate", PlanStepStatus::Pending));
        assert!(duplicate.validate().is_err());

        let mut dangling = linked_plan();
        dangling.links[0].parent_step_id = "missing".into();
        assert!(dangling.validate().is_err());

        let mut future = linked_plan();
        future.links[0].terminal = true;
        future.links[0].result_id = Some("result-1".into());
        future.reviews.push(DeliveryReview {
            link_id: "l1".into(),
            result_id: Some("result-1".into()),
            reviewed_source_revision: 1,
            decision: ReviewDecision::Accepted,
            reason: None,
            verification_refs: Vec::new(),
        });
        assert!(future.validate().is_err());

        let mut wrong_result = linked_plan();
        wrong_result.links[0].terminal = true;
        wrong_result.links[0].result_id = Some("result-1".into());
        wrong_result.reviews.push(DeliveryReview {
            link_id: "l1".into(),
            result_id: Some("another-result".into()),
            reviewed_source_revision: 0,
            decision: ReviewDecision::Accepted,
            reason: None,
            verification_refs: Vec::new(),
        });
        assert!(wrong_result.validate().is_err());
    }

    #[test]
    fn transition_keeps_stale_handled_and_completion_only_is_valid() {
        let mut plan = linked_plan();
        plan.update_steps(
            PlanUpdateMode::Merge,
            vec![step("s1", "v2", PlanStepStatus::Pending)],
        )
        .unwrap();
        let old_id = plan.replans[0].id.clone();
        plan.advance_replan(&old_id, ReplanProgress::Received)
            .unwrap();
        plan.advance_replan(&old_id, ReplanProgress::Consumed)
            .unwrap();

        plan.update_steps(
            PlanUpdateMode::Merge,
            vec![step("s1", "v3", PlanStepStatus::Pending)],
        )
        .unwrap();
        let previous = plan.clone();
        plan.advance_replan(
            &old_id,
            ReplanProgress::Handled {
                outcome: ReplanOutcome::NoChange {
                    reason: "the existing work already covers this change".into(),
                },
            },
        )
        .unwrap();
        plan.validate_transition_from(&previous).unwrap();
        assert_eq!(plan.links[0].pending_source_revision, Some(2));

        let previous = plan.clone();
        let completion = plan
            .update_steps(
                PlanUpdateMode::Merge,
                vec![step("s1", "v3", PlanStepStatus::Completed)],
            )
            .unwrap();
        assert!(completion.source_changes.is_empty());
        plan.validate_transition_from(&previous).unwrap();
    }

    #[test]
    fn transition_rejects_dispatching_payload_rewrite_but_accepts_coalescing() {
        let mut plan = linked_plan();
        plan.update_steps(
            PlanUpdateMode::Merge,
            vec![step("s1", "v2", PlanStepStatus::Pending)],
        )
        .unwrap();
        let id = plan.replans[0].id.clone();
        plan.advance_replan(&id, ReplanProgress::Dispatching)
            .unwrap();
        let dispatching = plan.clone();

        let mut tampered = dispatching.clone();
        tampered.replans[0].content = "rewritten after dispatch".into();
        assert!(tampered.validate_transition_from(&dispatching).is_err());

        plan.update_steps(
            PlanUpdateMode::Merge,
            vec![step("s1", "v3", PlanStepStatus::Pending)],
        )
        .unwrap();
        plan.validate_transition_from(&dispatching).unwrap();

        let mut pending = linked_plan();
        pending
            .update_steps(
                PlanUpdateMode::Merge,
                vec![step("s1", "v2", PlanStepStatus::Pending)],
            )
            .unwrap();
        let previous = pending.clone();
        pending
            .update_steps(
                PlanUpdateMode::Merge,
                vec![step("s1", "v3", PlanStepStatus::Pending)],
            )
            .unwrap();
        pending.validate_transition_from(&previous).unwrap();
        assert_eq!(pending.replans.len(), 1);
        assert_eq!(pending.replans[0].content, "v3");
    }

    #[test]
    fn scope_change_requires_open_work_disposal_but_keeps_terminal_history_valid() {
        let current = LocalPlan::new("parent", "new-scope");
        assert!(current.validate_transition_from(&linked_plan()).is_err());

        let mut old_scope = linked_plan();
        old_scope
            .update_steps(
                PlanUpdateMode::Merge,
                vec![step("s1", "changed", PlanStepStatus::Pending)],
            )
            .unwrap();
        old_scope
            .mark_link_terminal("l1", "child finished before handling the change")
            .unwrap();
        old_scope.reviews.push(DeliveryReview {
            link_id: "l1".into(),
            result_id: None,
            reviewed_source_revision: old_scope.revision,
            decision: ReviewDecision::Released,
            reason: Some("the old scope no longer needs this child result".into()),
            verification_refs: Vec::new(),
        });
        old_scope.validate().unwrap();
        assert!(matches!(
            old_scope.replans[0].progress,
            ReplanProgress::Unapplied { .. }
        ));
        current.validate_transition_from(&old_scope).unwrap();
    }

    #[test]
    fn revision_exhaustion_does_not_partially_apply_a_step_update() {
        let mut plan = linked_plan();
        plan.revision = u64::MAX;
        let before = plan.steps.clone();
        assert_eq!(
            plan.update_steps(
                PlanUpdateMode::Merge,
                vec![step("s1", "cannot advance", PlanStepStatus::Pending)],
            ),
            Err(PlanUpdateError::RevisionExhausted)
        );
        assert_eq!(plan.steps, before);
        assert_eq!(plan.revision, u64::MAX);
        assert!(plan.replans.is_empty());
    }

    #[test]
    fn delivery_reviews_are_append_only_within_a_scope() {
        let mut previous = linked_plan();
        previous.links[0].terminal = true;
        previous.links[0].result_id = Some("result-1".into());
        previous.reviews.push(DeliveryReview {
            link_id: "l1".into(),
            result_id: Some("result-1".into()),
            reviewed_source_revision: 0,
            decision: ReviewDecision::Accepted,
            reason: None,
            verification_refs: vec!["check-1".into()],
        });
        previous.validate().unwrap();

        let mut rewritten = previous.clone();
        rewritten.reviews.clear();
        assert!(rewritten.validate_transition_from(&previous).is_err());

        let mut appended = previous.clone();
        appended.reviews.push(DeliveryReview {
            link_id: "l1".into(),
            result_id: Some("result-1".into()),
            reviewed_source_revision: 0,
            decision: ReviewDecision::NeedsRevision,
            reason: Some("one more check is needed".into()),
            verification_refs: Vec::new(),
        });
        appended.validate_transition_from(&previous).unwrap();
    }

    #[test]
    fn effective_diff_only_replans_for_intent_changes() {
        let mut plan = linked_plan();
        let completion = plan
            .update_steps(
                PlanUpdateMode::Merge,
                vec![step("s1", "first", PlanStepStatus::Completed)],
            )
            .unwrap();
        assert!(completion.source_changes.is_empty());
        assert!(plan.replans.is_empty());

        let edit = plan
            .update_steps(
                PlanUpdateMode::Merge,
                vec![step("s1", "revised", PlanStepStatus::Completed)],
            )
            .unwrap();
        assert_eq!(edit.source_changes.len(), 1);
        assert_eq!(plan.links[0].pending_source_revision, Some(edit.revision));

        let noop = plan
            .update_steps(
                PlanUpdateMode::Merge,
                vec![step("s1", "revised", PlanStepStatus::Completed)],
            )
            .unwrap();
        assert!(noop.changes.is_empty());
        assert_eq!(plan.replans.len(), 1);
    }

    #[test]
    fn requirements_priority_noncompletion_status_and_remove_are_triggers() {
        let mut plan = linked_plan();
        let mut changed = step("s1", "first", PlanStepStatus::Pending);
        changed.requirements = Some(vec!["must pass".into()]);
        changed.priority = PlanPriority::High;
        let update = plan
            .update_steps(PlanUpdateMode::Merge, vec![changed.clone()])
            .unwrap();
        assert_eq!(update.source_changes.len(), 1);
        changed.status = PlanStepStatus::Cancelled;
        assert_eq!(
            plan.update_steps(PlanUpdateMode::Merge, vec![changed.clone()])
                .unwrap()
                .source_changes
                .len(),
            1
        );
        changed.removed = true;
        assert_eq!(
            plan.update_steps(PlanUpdateMode::Merge, vec![changed])
                .unwrap()
                .source_changes
                .len(),
            1
        );
    }

    #[test]
    fn replace_tombstones_omitted_steps_and_never_reuses_ids() {
        let mut plan = linked_plan();
        plan.steps
            .push(step("s2", "second", PlanStepStatus::Pending));
        let update = plan
            .update_steps(
                PlanUpdateMode::Replace,
                vec![step("s2", "second", PlanStepStatus::Pending)],
            )
            .unwrap();
        assert!(plan.steps.iter().any(|s| s.id == "s1" && s.removed));
        assert_eq!(update.source_changes.len(), 1);
        assert!(matches!(
            plan.update_steps(PlanUpdateMode::Merge, vec![step("s1", "reused", PlanStepStatus::Pending)]),
            Err(PlanUpdateError::TombstonedIdReuse(id)) if id == "s1"
        ));
    }

    #[test]
    fn child_handled_revision_is_checked_only_by_its_local_owner() {
        let mut parent = linked_plan();
        parent
            .update_steps(
                PlanUpdateMode::Merge,
                vec![step("s1", "changed", PlanStepStatus::Pending)],
            )
            .unwrap();
        let id = parent.replans[0].id.clone();
        parent
            .advance_replan(&id, ReplanProgress::Received)
            .unwrap();
        parent
            .advance_replan(&id, ReplanProgress::Consumed)
            .unwrap();
        parent
            .advance_replan(
                &id,
                ReplanProgress::Handled {
                    outcome: ReplanOutcome::Updated { plan_revision: 7 },
                },
            )
            .unwrap();
        assert!(parent.revision < 7);
        parent
            .validate()
            .expect("child and parent revisions are independent");

        let mut child = LocalPlan::new(parent.replans[0].child_session_id.clone(), "child-work");
        child.replans.push(parent.replans[0].clone());
        assert!(
            child.validate().is_err(),
            "the actual local owner rejects a future revision"
        );
        child.revision = 7;
        child
            .validate()
            .expect("the same receipt is valid at the child's actual revision");
    }

    #[test]
    fn old_replan_ack_does_not_clear_newer_pending_revision() {
        let mut plan = linked_plan();
        plan.update_steps(
            PlanUpdateMode::Merge,
            vec![step("s1", "v2", PlanStepStatus::Pending)],
        )
        .unwrap();
        let old_id = plan.replans[0].id.clone();
        plan.advance_replan(&old_id, ReplanProgress::Received)
            .unwrap();
        plan.advance_replan(&old_id, ReplanProgress::Consumed)
            .unwrap();
        plan.update_steps(
            PlanUpdateMode::Merge,
            vec![step("s1", "v3", PlanStepStatus::Pending)],
        )
        .unwrap();
        let latest = plan.links[0].pending_source_revision;
        plan.advance_replan(
            &old_id,
            ReplanProgress::Handled {
                outcome: ReplanOutcome::NoChange {
                    reason: "already covered".into(),
                },
            },
        )
        .unwrap();
        assert_eq!(plan.links[0].pending_source_revision, latest);
        assert_eq!(plan.replans[1].progress, ReplanProgress::Pending);
    }

    #[test]
    fn pending_replans_coalesce_but_received_payloads_remain_immutable() {
        let mut plan = linked_plan();
        plan.update_steps(
            PlanUpdateMode::Merge,
            vec![step("s1", "v2", PlanStepStatus::Pending)],
        )
        .unwrap();
        plan.update_steps(
            PlanUpdateMode::Merge,
            vec![step("s1", "v3", PlanStepStatus::Pending)],
        )
        .unwrap();
        assert_eq!(plan.replans.len(), 1);
        assert_eq!(plan.replans[0].content, "v3");
        assert_eq!(plan.replans[0].progress, ReplanProgress::Pending);

        let received_id = plan.replans[0].id.clone();
        plan.advance_replan(&received_id, ReplanProgress::Received)
            .unwrap();
        plan.update_steps(
            PlanUpdateMode::Merge,
            vec![step("s1", "v4", PlanStepStatus::Pending)],
        )
        .unwrap();
        assert_eq!(plan.replans.len(), 2);
        assert_eq!(plan.replans[0].content, "v3");
        assert_eq!(plan.replans[0].progress, ReplanProgress::Received);
        assert_eq!(plan.replans[1].content, "v4");
        assert_eq!(plan.replans[1].progress, ReplanProgress::Pending);
    }

    #[test]
    fn dispatching_replan_payload_is_immutable_after_a_new_source_change() {
        let mut plan = linked_plan();
        plan.update_steps(
            PlanUpdateMode::Merge,
            vec![step("s1", "v2", PlanStepStatus::Pending)],
        )
        .unwrap();
        let claimed_id = plan.replans[0].id.clone();
        plan.advance_replan(&claimed_id, ReplanProgress::Dispatching)
            .unwrap();

        plan.update_steps(
            PlanUpdateMode::Merge,
            vec![step("s1", "v3", PlanStepStatus::Pending)],
        )
        .unwrap();

        assert_eq!(plan.replans.len(), 2);
        assert_eq!(plan.replans[0].id, claimed_id);
        assert_eq!(plan.replans[0].content, "v2");
        assert_eq!(plan.replans[0].progress, ReplanProgress::Dispatching);
        assert_eq!(plan.replans[1].content, "v3");
        assert_eq!(plan.replans[1].progress, ReplanProgress::Pending);
    }

    #[test]
    fn terminal_child_keeps_unapplied_changes_without_pending_work() {
        let mut plan = linked_plan();
        plan.update_steps(
            PlanUpdateMode::Merge,
            vec![step("s1", "v2", PlanStepStatus::Pending)],
        )
        .unwrap();
        plan.mark_link_terminal("l1", "child completed before handling change")
            .unwrap();
        assert_eq!(plan.links[0].pending_source_revision, None);
        assert!(matches!(
            plan.replans[0].progress,
            ReplanProgress::Unapplied { .. }
        ));

        plan.update_steps(
            PlanUpdateMode::Merge,
            vec![step("s1", "v3", PlanStepStatus::Pending)],
        )
        .unwrap();
        assert_eq!(plan.links[0].pending_source_revision, None);
        assert!(matches!(
            plan.replans[1].progress,
            ReplanProgress::Unapplied { .. }
        ));
        assert!(plan.mark_link_terminal("l1", " ").is_err());
    }

    #[test]
    fn replan_identity_cannot_change_payload_and_no_change_needs_a_reason() {
        let mut plan = linked_plan();
        plan.update_steps(
            PlanUpdateMode::Merge,
            vec![step("s1", "v2", PlanStepStatus::Pending)],
        )
        .unwrap();
        let mut incoming = plan.replans[0].clone();
        incoming.progress = ReplanProgress::Received;
        let mut child = LocalPlan::new("child", "child-work");
        assert!(child.receive_replan(incoming.clone()).unwrap());
        assert!(!child.receive_replan(incoming.clone()).unwrap());
        let mut altered = incoming.clone();
        altered.content = "different payload".into();
        assert!(matches!(
            child.receive_replan(altered),
            Err(ReplanError::IdentityPayloadConflict(_))
        ));
        child
            .advance_replan(&incoming.id, ReplanProgress::Consumed)
            .unwrap();
        assert_eq!(
            child.advance_replan(
                &incoming.id,
                ReplanProgress::Handled {
                    outcome: ReplanOutcome::NoChange {
                        reason: "  ".into()
                    }
                }
            ),
            Err(ReplanError::EmptyNoChangeReason)
        );
    }

    #[test]
    fn required_delivery_gate_ignores_optional_links_and_needs_review() {
        let mut plan = linked_plan();
        plan.links.push(PlanLink {
            id: "optional".into(),
            parent_step_id: "s1".into(),
            child_session_id: "other".into(),
            terminal: true,
            required: false,
            pending_source_revision: None,
            review_required_revision: 0,
            result_id: None,
            dispatch_failure: None,
        });
        assert_eq!(plan.unresolved_required_deliveries().len(), 1);
        plan.links[0].terminal = true;
        plan.links[0].result_id = Some("result-1".into());
        assert_eq!(plan.unresolved_required_deliveries().len(), 1);
        plan.reviews.push(DeliveryReview {
            link_id: "l1".into(),
            result_id: Some("result-1".into()),
            reviewed_source_revision: 0,
            decision: ReviewDecision::Accepted,
            reason: None,
            verification_refs: vec!["test-run".into()],
        });
        assert!(plan.unresolved_required_deliveries().is_empty());

        let mut changed = step("s1", "first", PlanStepStatus::Pending);
        changed.requirements = Some(vec!["new criterion".into()]);
        plan.update_steps(PlanUpdateMode::Merge, vec![changed.clone()])
            .unwrap();
        assert_eq!(plan.links[0].review_required_revision, 1);
        assert_eq!(plan.unresolved_required_deliveries().len(), 1);

        changed.priority = PlanPriority::High;
        plan.update_steps(PlanUpdateMode::Merge, vec![changed])
            .unwrap();
        assert_eq!(plan.links[0].review_required_revision, 1);
        plan.reviews.push(DeliveryReview {
            link_id: "l1".into(),
            result_id: Some("result-1".into()),
            reviewed_source_revision: 1,
            decision: ReviewDecision::Accepted,
            reason: None,
            verification_refs: vec!["rechecked-old-result".into()],
        });
        assert!(plan.unresolved_required_deliveries().is_empty());
    }

    #[test]
    fn reasoned_release_closes_gate_before_result_without_ending_child() {
        let mut plan = linked_plan();
        plan.reviews.push(DeliveryReview {
            link_id: "l1".into(),
            result_id: None,
            reviewed_source_revision: 0,
            decision: ReviewDecision::Released,
            reason: Some("parent no longer requires this execution".into()),
            verification_refs: Vec::new(),
        });
        assert!(plan.unresolved_required_deliveries().is_empty());
        assert!(!plan.links[0].terminal);
    }

    #[test]
    fn undispatched_link_is_idempotent_unapplies_replans_and_requires_release() {
        let mut plan = linked_plan();
        plan.update_steps(
            PlanUpdateMode::Merge,
            vec![step("s1", "changed", PlanStepStatus::Pending)],
        )
        .unwrap();
        let previous = plan.clone();
        assert!(
            plan.mark_link_undispatched("l1", "coordinator admission closed")
                .unwrap()
        );
        assert!(
            !plan
                .mark_link_undispatched("l1", "coordinator admission closed")
                .unwrap()
        );
        assert!(
            plan.mark_link_undispatched("l1", "different rejection")
                .is_err()
        );
        assert_eq!(
            plan.links[0].dispatch_failure.as_deref(),
            Some("coordinator admission closed")
        );
        assert!(!plan.links[0].terminal);
        assert!(plan.links[0].result_id.is_none());
        assert!(plan.links[0].pending_source_revision.is_none());
        assert!(matches!(
            plan.replans[0].progress,
            ReplanProgress::Unapplied { ref reason }
                if reason == "coordinator admission closed"
        ));
        plan.validate_transition_from(&previous).unwrap();
        plan.validate().unwrap();

        plan.reviews.push(DeliveryReview {
            link_id: "l1".into(),
            result_id: None,
            reviewed_source_revision: plan.revision,
            decision: ReviewDecision::Released,
            reason: Some("the required delivery was explicitly released".into()),
            verification_refs: Vec::new(),
        });
        assert!(plan.unresolved_required_deliveries().is_empty());
        plan.validate().unwrap();
        assert!(
            plan.mark_link_terminal("l1", "there was no child execution")
                .is_err()
        );
    }

    #[test]
    fn edits_after_undispatched_link_do_not_reopen_child_replanning() {
        for required in [false, true] {
            for released in [false, true] {
                let mut plan = linked_plan();
                plan.links[0].required = required;
                plan.mark_link_undispatched("l1", "startup failed").unwrap();
                if released {
                    plan.reviews.push(DeliveryReview {
                        link_id: "l1".into(),
                        result_id: None,
                        reviewed_source_revision: plan.revision,
                        decision: ReviewDecision::Released,
                        reason: Some("release the required delivery".into()),
                        verification_refs: Vec::new(),
                    });
                }

                plan.update_steps(
                    PlanUpdateMode::Merge,
                    vec![step("s1", "changed after failure", PlanStepStatus::Pending)],
                )
                .unwrap();

                let link = &plan.links[0];
                assert_eq!(link.pending_source_revision, None);
                assert!(!link.terminal);
                assert_eq!(link.result_id, None);
                assert_eq!(link.dispatch_failure.as_deref(), Some("startup failed"));
                assert_eq!(plan.replans.len(), 1);
                assert!(plan.replans.iter().all(|replan| matches!(
                    &replan.progress,
                    ReplanProgress::Unapplied { reason } if reason == "startup failed"
                )));
                assert_eq!(
                    !plan.unresolved_required_deliveries().is_empty(),
                    required && !released
                );
                plan.validate().unwrap();
            }
        }

        let mut active = linked_plan();
        active
            .update_steps(
                PlanUpdateMode::Merge,
                vec![step("s1", "changed while active", PlanStepStatus::Pending)],
            )
            .unwrap();
        assert_eq!(
            active.links[0].pending_source_revision,
            Some(active.revision)
        );
        assert_eq!(active.replans[0].progress, ReplanProgress::Pending);
    }

    #[test]
    fn repeated_undispatched_ack_closes_legacy_unsettled_replans() {
        for progress in [
            ReplanProgress::Pending,
            ReplanProgress::Dispatching,
            ReplanProgress::Received,
            ReplanProgress::Consumed,
        ] {
            let mut plan = linked_plan();
            plan.update_steps(
                PlanUpdateMode::Merge,
                vec![step(
                    "s1",
                    "changed before failure",
                    PlanStepStatus::Pending,
                )],
            )
            .unwrap();
            plan.replans[0].progress = progress;
            plan.links[0].dispatch_failure = Some("startup failed".into());
            plan.reviews.push(DeliveryReview {
                link_id: "l1".into(),
                result_id: None,
                reviewed_source_revision: plan.revision,
                decision: ReviewDecision::Released,
                reason: Some("release the required delivery".into()),
                verification_refs: Vec::new(),
            });
            let before = plan.clone();
            before.validate().unwrap();
            let identity = plan.replans[0].clone();

            assert!(plan.mark_link_undispatched("l1", "startup failed").unwrap());
            assert!(matches!(
                &plan.replans[0].progress,
                ReplanProgress::Unapplied { reason } if reason == "startup failed"
            ));
            assert!(identity.same_identity_payload(&plan.replans[0]));
            assert_eq!(
                plan.links[0].dispatch_failure.as_deref(),
                Some("startup failed")
            );
            assert!(!plan.links[0].terminal);
            assert_eq!(plan.links[0].result_id, None);
            assert_eq!(plan.links[0].pending_source_revision, None);
            plan.validate_transition_from(&before).unwrap();
            plan.validate().unwrap();

            let closed = plan.clone();
            assert!(!plan.mark_link_undispatched("l1", "startup failed").unwrap());
            assert_eq!(plan, closed);
            assert!(
                plan.mark_link_undispatched("l1", "different failure")
                    .is_err()
            );
            assert_eq!(plan, closed);

            let mut next_scope = plan.clone();
            next_scope.scope_id = "next-work".into();
            next_scope.validate_transition_from(&plan).unwrap();
        }
    }

    #[test]
    fn needs_revision_keeps_result_and_reason_without_closing_gate() {
        let mut plan = linked_plan();
        plan.links[0].terminal = true;
        plan.links[0].result_id = Some("result-1".into());
        plan.reviews.push(DeliveryReview {
            link_id: "l1".into(),
            result_id: Some("result-1".into()),
            reviewed_source_revision: 0,
            decision: ReviewDecision::NeedsRevision,
            reason: Some("the required integration check is missing".into()),
            verification_refs: vec!["review-1".into()],
        });
        assert_eq!(plan.unresolved_required_deliveries().len(), 1);
        assert_eq!(plan.reviews[0].result_id.as_deref(), Some("result-1"));
        assert_eq!(
            plan.reviews[0].reason.as_deref(),
            Some("the required integration check is missing")
        );
    }

    #[test]
    fn child_planning_summary_is_bounded_and_keeps_snapshot_identity() {
        let mut plan = LocalPlan::new("child", "scope-7");
        for index in 0..10 {
            let step_id = format!("step-{index}");
            let link_id = format!("link-{index}");
            plan.steps
                .push(step(&step_id, "work", PlanStepStatus::InProgress));
            plan.links.push(PlanLink {
                id: link_id,
                parent_step_id: step_id,
                child_session_id: format!("grandchild-{index}"),
                terminal: false,
                required: true,
                pending_source_revision: None,
                review_required_revision: 0,
                result_id: None,
                dispatch_failure: None,
            });
        }
        plan.revision = 2;
        plan.links[0].terminal = true;
        plan.links[0].result_id = Some("result-ref".into());
        plan.links[0].review_required_revision = 2;
        plan.reviews.push(DeliveryReview {
            link_id: "link-0".into(),
            result_id: Some("result-ref".into()),
            reviewed_source_revision: 2,
            decision: ReviewDecision::Accepted,
            reason: None,
            verification_refs: vec!["verification:ci".into()],
        });
        let summary = SubagentPlanningSummary::from_plan(42, &plan).unwrap();
        assert_eq!((summary.planning_seq, summary.revision), (42, 2));
        assert_eq!(summary.scope_id, "scope-7");
        assert_eq!(summary.review_count, 1);
        assert_eq!(summary.reviews[0].task_id, "link-0");
        assert_eq!(summary.reviews[0].result_ref.as_deref(), Some("result-ref"));
        assert_eq!(summary.reviews[0].requirements_revision, 2);
        assert_eq!(summary.reviews[0].verification_ref_count, 1);
        assert_eq!(summary.unresolved_required_delivery_count, 9);
        assert_eq!(summary.unresolved_required_deliveries.len(), 8);
        assert_eq!(summary.unresolved_required_deliveries_truncated, 1);
        assert_eq!(
            summary.unresolved_required_deliveries[0].parent_step_status,
            PlanStepStatus::InProgress
        );
        let rendered = summary.render_bounded();
        assert!(rendered.contains("snapshot 42"));
        assert!(rendered.contains("result=Some(\"result-ref\")"));
        assert!(rendered.contains("verification:ci"));
        summary.validate().unwrap();

        let mut malformed = summary;
        malformed.unresolved_required_delivery_count += 1;
        assert!(malformed.validate().is_err());
    }
}
