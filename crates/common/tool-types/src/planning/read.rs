//! Complete, bounded read projections of an already authorized planning snapshot.
//!
//! The owner supplies one canonical, validated snapshot and rechecks access on
//! every call. A cursor only selects a position within that snapshot; it carries
//! no authority and requires no registry or persisted pagination state.

use std::{collections::HashSet, fmt, io};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    DeliveryReview, DependencyKind, LocalPlan, PlanLink, PlanStep, PlanWait, ReplanChange,
    valid_plan_id,
};

const DEFAULT_LIMIT: u32 = 20;
const MAX_LIMIT: u32 = 50;
const MAX_CURSOR_BYTES: usize = 4096;
const MAX_OUTPUT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PlanReadTarget {
    #[default]
    #[serde(rename = "self")]
    SelfPlan,
    #[serde(rename = "parent")]
    Parent,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanReadQuery {
    pub step_id: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

impl PlanReadQuery {
    /// Check syntax and bounds even when the authorized owner has no plan.
    /// Cursor identity, snapshot freshness and offsets require a snapshot and
    /// are checked by `project_plan_read` after the runtime authorizes access.
    pub fn validate(&self) -> Result<(), PlanReadError> {
        if !(1..=MAX_LIMIT).contains(&self.limit.unwrap_or(DEFAULT_LIMIT))
            || self.step_id.as_deref().is_some_and(|id| !valid_plan_id(id))
        {
            return Err(PlanReadError::InvalidQuery);
        }
        if let Some(encoded) = &self.cursor {
            ReadCursor::parse(encoded)?;
        }
        Ok(())
    }
}

/// Flat tool input: the model cannot select an arbitrary session identity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanReadInput {
    #[serde(default)]
    pub target: PlanReadTarget,
    pub step_id: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanDelegation {
    pub parent_session_id: String,
    pub parent_scope_id: String,
    pub parent_step_id: String,
    pub task_id: String,
    pub child_session_id: String,
}

/// Ephemeral owner input, obtained from the same Timeline snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanningReadSnapshot {
    pub timeline_seq: u64,
    pub plan: LocalPlan,
    pub delegation: Option<PlanDelegation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanReadOutput {
    pub target: PlanReadTarget,
    pub plan: Option<PlanReadPage>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanReadCounts {
    pub steps: u32,
    pub links: u32,
    pub waits: u32,
    pub replans: u32,
    pub reviews: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlanReadPage {
    pub session_id: String,
    pub scope_id: String,
    pub revision: u64,
    pub timeline_seq: u64,
    pub delegation: Option<PlanDelegation>,
    pub counts: PlanReadCounts,
    pub matched_counts: PlanReadCounts,
    pub steps: Vec<PlanStep>,
    pub links: Vec<PlanLink>,
    pub waits: Vec<PlanWait>,
    pub replans: Vec<ReplanChange>,
    pub reviews: Vec<DeliveryReview>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanReadError {
    InvalidQuery,
    InvalidCursor,
    CursorMismatch,
    StaleCursor,
    ItemTooLarge,
}

impl PlanReadError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidQuery => "invalid_query",
            Self::InvalidCursor => "invalid_cursor",
            Self::CursorMismatch => "cursor_mismatch",
            Self::StaleCursor => "stale_cursor",
            Self::ItemTooLarge => "item_too_large",
        }
    }
}

impl fmt::Display for PlanReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidQuery => "step_id must be a valid plan identity and limit must be 1..=50",
            Self::InvalidCursor => "cursor is malformed, exceeds 4096 bytes, or is out of range",
            Self::CursorMismatch => {
                "cursor belongs to a different reader, target, source, or filter"
            }
            Self::StaleCursor => "planning snapshot changed; restart reading without a cursor",
            Self::ItemTooLarge => "one complete planning record exceeds the 256 KiB output budget",
        })
    }
}

impl std::error::Error for PlanReadError {}

/// Project complete records in stable steps/links/waits/replans/reviews order.
///
/// `snapshot` must be a canonical validated plan. The runtime authorizes both
/// the reader and target independently of the cursor. This function performs no
/// I/O, creates no scope, and never mutates or summarizes its borrowed input.
/// The 256 KiB budget includes serialization of the `PlanReadOutput` envelope.
pub fn project_plan_read(
    reader_session_id: &str,
    target: PlanReadTarget,
    snapshot: &PlanningReadSnapshot,
    query: &PlanReadQuery,
) -> Result<PlanReadPage, PlanReadError> {
    query.validate()?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    let mut cursor = ReadCursor {
        reader_session_id: reader_session_id.to_owned(),
        target,
        session_id: snapshot.plan.session_id.clone(),
        scope_id: snapshot.plan.scope_id.clone(),
        timeline_seq: snapshot.timeline_seq,
        revision: snapshot.plan.revision,
        step_id: query.step_id.clone(),
        offset: 0,
    };
    if let Some(encoded) = &query.cursor {
        let supplied = ReadCursor::parse(encoded)?;
        if supplied.reader_session_id != cursor.reader_session_id
            || supplied.target != cursor.target
            || supplied.session_id != cursor.session_id
            || supplied.scope_id != cursor.scope_id
            || supplied.step_id != cursor.step_id
        {
            return Err(PlanReadError::CursorMismatch);
        }
        if supplied.timeline_seq != cursor.timeline_seq || supplied.revision != cursor.revision {
            return Err(PlanReadError::StaleCursor);
        }
        cursor.offset = supplied.offset;
    }

    let records = selected_records(&snapshot.plan, query.step_id.as_deref());
    if query.cursor.is_some() && cursor.offset >= records.len() {
        return Err(PlanReadError::InvalidCursor);
    }
    let mut matched_counts = PlanReadCounts::default();
    for record in &records {
        match record {
            ReadRecord::Step(_) => matched_counts.steps += 1,
            ReadRecord::Link(_) => matched_counts.links += 1,
            ReadRecord::Wait(_) => matched_counts.waits += 1,
            ReadRecord::Replan(_) => matched_counts.replans += 1,
            ReadRecord::Review(_) => matched_counts.reviews += 1,
        }
    }
    let plan = &snapshot.plan;
    let mut output = PlanReadOutput {
        target,
        plan: Some(PlanReadPage {
            session_id: plan.session_id.clone(),
            scope_id: plan.scope_id.clone(),
            revision: plan.revision,
            timeline_seq: snapshot.timeline_seq,
            delegation: snapshot.delegation.clone(),
            // Canonical LocalPlan collections are bounded to 4096 records each.
            counts: PlanReadCounts {
                steps: plan.steps.len() as u32,
                links: plan.links.len() as u32,
                waits: plan.waits.len() as u32,
                replans: plan.replans.len() as u32,
                reviews: plan.reviews.len() as u32,
            },
            matched_counts,
            steps: Vec::new(),
            links: Vec::new(),
            waits: Vec::new(),
            replans: Vec::new(),
            reviews: Vec::new(),
            next_cursor: None,
        }),
    };
    if serialized_bytes(&output).is_none() {
        return Err(PlanReadError::ItemTooLarge);
    }
    let start = cursor.offset;
    for (index, record) in records.iter().enumerate().skip(start).take(limit as usize) {
        output.plan.as_mut().expect("page exists").next_cursor =
            cursor.next(index + 1, records.len())?;
        let envelope_bytes = serialized_bytes(&output);
        let record_bytes = serialized_bytes(record);
        let page = output.plan.as_mut().expect("page exists");
        let fits = envelope_bytes
            .zip(record_bytes)
            .is_some_and(|(base, item)| {
                base + item + usize::from(record.has_previous(page)) <= MAX_OUTPUT_BYTES
            });
        if !fits {
            if index == start {
                return Err(PlanReadError::ItemTooLarge);
            }
            // This record remains unread; the prior accepted page already fit
            // with this exact continuation position in its envelope.
            page.next_cursor = cursor.next(index, records.len())?;
            break;
        }
        record.append_to(page);
    }
    Ok(output.plan.expect("projection owns its page"))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadCursor {
    reader_session_id: String,
    target: PlanReadTarget,
    session_id: String,
    scope_id: String,
    timeline_seq: u64,
    revision: u64,
    step_id: Option<String>,
    offset: usize,
}

impl ReadCursor {
    fn parse(encoded: &str) -> Result<Self, PlanReadError> {
        if encoded.len() > MAX_CURSOR_BYTES {
            return Err(PlanReadError::InvalidCursor);
        }
        serde_json::from_str(encoded).map_err(|_| PlanReadError::InvalidCursor)
    }

    fn next(&mut self, offset: usize, total: usize) -> Result<Option<String>, PlanReadError> {
        if offset == total {
            return Ok(None);
        }
        self.offset = offset;
        let encoded = serde_json::to_string(self).map_err(|_| PlanReadError::InvalidCursor)?;
        if encoded.len() > MAX_CURSOR_BYTES {
            return Err(PlanReadError::InvalidCursor);
        }
        Ok(Some(encoded))
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum ReadRecord<'a> {
    Step(&'a PlanStep),
    Link(&'a PlanLink),
    Wait(&'a PlanWait),
    Replan(&'a ReplanChange),
    Review(&'a DeliveryReview),
}

impl ReadRecord<'_> {
    fn has_previous(&self, page: &PlanReadPage) -> bool {
        match self {
            Self::Step(_) => !page.steps.is_empty(),
            Self::Link(_) => !page.links.is_empty(),
            Self::Wait(_) => !page.waits.is_empty(),
            Self::Replan(_) => !page.replans.is_empty(),
            Self::Review(_) => !page.reviews.is_empty(),
        }
    }

    fn append_to(&self, page: &mut PlanReadPage) {
        match self {
            Self::Step(value) => page.steps.push((*value).clone()),
            Self::Link(value) => page.links.push((*value).clone()),
            Self::Wait(value) => page.waits.push((*value).clone()),
            Self::Replan(value) => page.replans.push((*value).clone()),
            Self::Review(value) => page.reviews.push((*value).clone()),
        }
    }
}

fn selected_records<'a>(plan: &'a LocalPlan, step_id: Option<&str>) -> Vec<ReadRecord<'a>> {
    if step_id.is_some_and(|id| !plan.steps.iter().any(|step| step.id == id)) {
        return Vec::new();
    }
    let selected_links: HashSet<_> = plan
        .links
        .iter()
        .filter(|link| step_id.is_none_or(|id| link.parent_step_id == id))
        .map(|link| link.id.as_str())
        .collect();
    plan.steps
        .iter()
        .filter(|step| step_id.is_none_or(|id| step.id == id))
        .map(ReadRecord::Step)
        .chain(
            plan.links
                .iter()
                .filter(|link| selected_links.contains(link.id.as_str()))
                .map(ReadRecord::Link),
        )
        .chain(
            plan.waits
                .iter()
                .filter(|wait| {
                    step_id.is_none_or(|id| {
                        wait.step_ids.is_empty()
                            || wait.step_ids.iter().any(|step| step == id)
                            || wait.dependencies.iter().any(|dependency| {
                                dependency.kind == DependencyKind::Task
                                    && selected_links.contains(dependency.id.as_str())
                            })
                    })
                })
                .map(ReadRecord::Wait),
        )
        .chain(
            plan.replans
                .iter()
                .filter(|replan| step_id.is_none_or(|id| replan.step_id == id))
                .map(ReadRecord::Replan),
        )
        .chain(
            plan.reviews
                .iter()
                .filter(|review| selected_links.contains(review.link_id.as_str()))
                .map(ReadRecord::Review),
        )
        .collect()
}

/// Measure the actual JSON representation, including UTF-8 and escaping, without
/// allocating a second serialized copy of a potentially oversized record.
fn serialized_bytes(value: &impl Serialize) -> Option<usize> {
    struct ByteCount(usize);

    impl io::Write for ByteCount {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 += bytes.len();
            if self.0 > MAX_OUTPUT_BYTES {
                return Err(io::Error::other("planning output byte limit exceeded"));
            }
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let mut count = ByteCount(0);
    serde_json::to_writer(&mut count, value).ok()?;
    Some(count.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::{
        PlanPriority, PlanStepStatus, ReplanOutcome, ReplanProgress, ReviewDecision, WaitCondition,
        WaitDependency,
    };
    use serde_json::{Value, json};

    fn step(id: &str) -> PlanStep {
        PlanStep {
            id: id.into(),
            content: format!("Full content for {id}: 文, quotes \" and newline\n"),
            priority: PlanPriority::High,
            status: PlanStepStatus::InProgress,
            requirements: Some(vec![
                "Keep every decision field".into(),
                "Verify UTF-8 文".into(),
            ]),
            removed: false,
        }
    }

    fn link(id: &str, step_id: &str) -> PlanLink {
        PlanLink {
            id: id.into(),
            parent_step_id: step_id.into(),
            child_session_id: format!("child-{id}"),
            terminal: false,
            required: true,
            pending_source_revision: None,
            review_required_revision: 3,
            result_id: None,
            dispatch_failure: None,
        }
    }

    fn replan(
        link: &PlanLink,
        step: &PlanStep,
        revision: u64,
        progress: ReplanProgress,
    ) -> ReplanChange {
        ReplanChange {
            id: format!("{}:{}:{revision}", link.id, step.id),
            link_id: link.id.clone(),
            child_session_id: link.child_session_id.clone(),
            step_id: step.id.clone(),
            source_revision: revision,
            content: step.content.clone(),
            requirements: step.requirements.clone(),
            priority: step.priority,
            status: step.status,
            removed: step.removed,
            progress,
        }
    }

    fn wait(id: &str, steps: &[&str], kind: DependencyKind, dependency: &str) -> PlanWait {
        PlanWait {
            id: id.into(),
            step_ids: steps.iter().map(|id| (*id).into()).collect(),
            dependencies: vec![WaitDependency {
                kind,
                id: dependency.into(),
            }],
            condition: WaitCondition::All,
        }
    }

    fn snapshot() -> PlanningReadSnapshot {
        let mut plan = LocalPlan::new("parent-session", "scope-文");
        plan.revision = 7;
        plan.steps = vec![step("live"), step("removed"), step("other")];
        plan.steps[1].removed = true;
        plan.steps[1].status = PlanStepStatus::Cancelled;
        let mut delivered = link("link-z", "removed");
        delivered.terminal = true;
        delivered.result_id =
            Some(r#"{"timeline_id":"child-link-z","first_seq":42,"last_seq":42}"#.into());
        delivered.review_required_revision = 5;
        let mut active = link("link-a", "live");
        active.pending_source_revision = Some(6);
        let mut undispatched = link("link-m", "removed");
        undispatched.dispatch_failure =
            Some("coordinator admission failed; no child started".into());
        let mut unrelated = link("link-other", "other");
        unrelated.terminal = true;
        unrelated.result_id =
            Some(r#"{"timeline_id":"child-link-other","first_seq":9,"last_seq":9}"#.into());
        plan.links = vec![delivered, active, undispatched, unrelated];
        plan.replans = vec![
            replan(
                &plan.links[0],
                &plan.steps[1],
                5,
                ReplanProgress::Unapplied {
                    reason: "child already terminal".into(),
                },
            ),
            replan(&plan.links[1], &plan.steps[0], 6, ReplanProgress::Received),
            replan(
                &plan.links[2],
                &plan.steps[1],
                4,
                ReplanProgress::Unapplied {
                    reason: plan.links[2].dispatch_failure.clone().unwrap(),
                },
            ),
            replan(
                &plan.links[1],
                &plan.steps[0],
                2,
                ReplanProgress::Handled {
                    outcome: ReplanOutcome::Updated { plan_revision: 500 },
                },
            ),
            replan(&plan.links[1], &plan.steps[0], 4, ReplanProgress::Consumed),
        ];
        plan.waits = vec![
            wait("scope", &[], DependencyKind::Permission, "permission-1"),
            wait("direct", &["removed"], DependencyKind::Command, "command-1"),
            wait("by-task", &["other"], DependencyKind::Task, "link-z"),
            wait("wrong-kind", &["other"], DependencyKind::Command, "link-z"),
            wait("unrelated", &["other"], DependencyKind::Task, "link-other"),
            wait("live", &["live"], DependencyKind::Question, "question-1"),
            wait(
                "undispatched-task",
                &["other"],
                DependencyKind::Task,
                "link-m",
            ),
        ];
        plan.waits[2].condition = WaitCondition::Any;
        plan.reviews = vec![
            DeliveryReview {
                link_id: "link-z".into(),
                result_id: plan.links[0].result_id.clone(),
                reviewed_source_revision: 5,
                decision: ReviewDecision::Accepted,
                reason: Some("Full result reference verified".into()),
                verification_refs: vec!["checks/文:42".into(), "test \"with quotes\"".into()],
            },
            DeliveryReview {
                link_id: "link-m".into(),
                result_id: None,
                reviewed_source_revision: 4,
                decision: ReviewDecision::Released,
                reason: Some("execution was never admitted".into()),
                verification_refs: vec!["startup-failure".into()],
            },
            DeliveryReview {
                link_id: "link-other".into(),
                result_id: plan.links[3].result_id.clone(),
                reviewed_source_revision: 3,
                decision: ReviewDecision::NeedsRevision,
                reason: Some("one more check".into()),
                verification_refs: vec!["check-a".into()],
            },
            DeliveryReview {
                link_id: "link-z".into(),
                result_id: None,
                reviewed_source_revision: 1,
                decision: ReviewDecision::Released,
                reason: Some("historical release preceded canonical delivery".into()),
                verification_refs: Vec::new(),
            },
        ];
        plan.validate().unwrap();
        PlanningReadSnapshot {
            timeline_seq: 123,
            plan,
            delegation: Some(PlanDelegation {
                parent_session_id: "parent-session".into(),
                parent_scope_id: "scope-文".into(),
                parent_step_id: "removed".into(),
                task_id: "link-z".into(),
                child_session_id: "child-link-z".into(),
            }),
        }
    }

    fn project(
        snapshot: &PlanningReadSnapshot,
        query: &PlanReadQuery,
    ) -> Result<PlanReadPage, PlanReadError> {
        project_plan_read("child-link-z", PlanReadTarget::Parent, snapshot, query)
    }

    fn output_bytes(page: &PlanReadPage) -> usize {
        serde_json::to_vec(&PlanReadOutput {
            target: PlanReadTarget::Parent,
            plan: Some(page.clone()),
        })
        .unwrap()
        .len()
    }

    fn page_count(page: &PlanReadPage) -> usize {
        page.steps.len()
            + page.links.len()
            + page.waits.len()
            + page.replans.len()
            + page.reviews.len()
    }

    /// Concatenate only public page results, checking progress, class order and
    /// the actual output representation rather than reproducing projection logic.
    fn collect_pages(
        snapshot: &PlanningReadSnapshot,
        step_id: Option<&'_ str>,
        limits: &[u32],
    ) -> (LocalPlan, Vec<PlanReadPage>) {
        let before = serde_json::to_vec(snapshot).unwrap();
        let mut combined = LocalPlan::new(&snapshot.plan.session_id, &snapshot.plan.scope_id);
        combined.revision = snapshot.plan.revision;
        let mut cursor = None;
        let mut pages = Vec::new();
        let mut last_class = 0;
        loop {
            assert!(pages.len() < 100, "pagination must make progress");
            let limit = limits[pages.len() % limits.len()];
            let page = project(
                snapshot,
                &PlanReadQuery {
                    step_id: step_id.map(str::to_owned),
                    cursor: cursor.clone(),
                    limit: Some(limit),
                },
            )
            .unwrap();
            assert!(page_count(&page) <= limit as usize);
            assert!(output_bytes(&page) <= 256 * 1024);
            assert_eq!(page.session_id, snapshot.plan.session_id);
            assert_eq!(page.scope_id, snapshot.plan.scope_id);
            assert_eq!(page.revision, snapshot.plan.revision);
            assert_eq!(page.timeline_seq, snapshot.timeline_seq);
            assert_eq!(page.delegation, snapshot.delegation);
            for (class, count) in [
                page.steps.len(),
                page.links.len(),
                page.waits.len(),
                page.replans.len(),
                page.reviews.len(),
            ]
            .into_iter()
            .enumerate()
            {
                if count != 0 {
                    assert!(
                        class >= last_class,
                        "record classes must be globally ordered"
                    );
                    last_class = class;
                }
            }
            combined.steps.extend(page.steps.clone());
            combined.links.extend(page.links.clone());
            combined.waits.extend(page.waits.clone());
            combined.replans.extend(page.replans.clone());
            combined.reviews.extend(page.reviews.clone());
            assert!(page.next_cursor.is_none() || page_count(&page) > 0);
            if let Some(next) = &page.next_cursor {
                assert!(next.len() <= 4096);
                assert_ne!(cursor.as_ref(), Some(next));
            }
            cursor = page.next_cursor.clone();
            pages.push(page);
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(serde_json::to_vec(snapshot).unwrap(), before);
        (combined, pages)
    }

    fn first_cursor(snapshot: &PlanningReadSnapshot, step_id: Option<&str>) -> String {
        project(
            snapshot,
            &PlanReadQuery {
                step_id: step_id.map(str::to_owned),
                limit: Some(1),
                cursor: None,
            },
        )
        .unwrap()
        .next_cursor
        .unwrap()
    }

    #[test]
    fn mixed_pages_preserve_all_facts_and_order_with_changing_limits() {
        let snapshot = snapshot();
        let (combined, pages) = collect_pages(&snapshot, None, &[4, 2, 3, 1, 5]);
        assert_eq!(combined, snapshot.plan);
        assert!(pages.len() > 5);
        assert_eq!(pages[0].steps, snapshot.plan.steps);
        assert_eq!(pages[0].links, snapshot.plan.links[..1]);
        assert_eq!(pages[1].links, snapshot.plan.links[1..3]);
        assert_eq!(pages[2].links, snapshot.plan.links[3..]);
        assert_eq!(pages[2].waits, snapshot.plan.waits[..2]);
        for page in pages {
            assert_eq!(
                page.counts,
                PlanReadCounts {
                    steps: 3,
                    links: 4,
                    waits: 7,
                    replans: 5,
                    reviews: 4
                }
            );
            assert_eq!(page.matched_counts, page.counts);
        }
    }

    #[test]
    fn step_filter_retains_tombstone_related_waits_and_exact_review_bindings() {
        let snapshot = snapshot();
        let (combined, pages) = collect_pages(&snapshot, Some("removed"), &[2, 1, 4]);
        assert_eq!(combined.steps, snapshot.plan.steps[1..2]);
        assert!(combined.steps[0].removed);
        assert_eq!(
            combined.links,
            vec![
                snapshot.plan.links[0].clone(),
                snapshot.plan.links[2].clone()
            ]
        );
        assert_eq!(
            combined.waits,
            vec![
                snapshot.plan.waits[0].clone(),
                snapshot.plan.waits[1].clone(),
                snapshot.plan.waits[2].clone(),
                snapshot.plan.waits[6].clone()
            ]
        );
        assert_eq!(
            combined.replans,
            vec![
                snapshot.plan.replans[0].clone(),
                snapshot.plan.replans[2].clone()
            ]
        );
        assert_eq!(
            combined.reviews,
            vec![
                snapshot.plan.reviews[0].clone(),
                snapshot.plan.reviews[1].clone(),
                snapshot.plan.reviews[3].clone()
            ]
        );
        assert_ne!(
            combined.reviews[0].result_id.as_deref(),
            Some(combined.reviews[0].link_id.as_str())
        );
        assert_eq!(
            combined.reviews[2].result_id, None,
            "historical release is not rewritten"
        );
        for page in pages {
            assert_eq!(
                page.counts,
                PlanReadCounts {
                    steps: 3,
                    links: 4,
                    waits: 7,
                    replans: 5,
                    reviews: 4
                }
            );
            assert_eq!(
                page.matched_counts,
                PlanReadCounts {
                    steps: 1,
                    links: 2,
                    waits: 4,
                    replans: 2,
                    reviews: 3
                }
            );
        }
    }

    #[test]
    fn empty_and_unknown_step_return_no_records_without_creating_state() {
        let snapshot = snapshot();
        for filter in ["missing", "removed ", "REMOVED"] {
            let (combined, pages) = collect_pages(&snapshot, Some(filter), &[1]);
            assert_eq!(
                combined.steps.len()
                    + combined.links.len()
                    + combined.waits.len()
                    + combined.replans.len()
                    + combined.reviews.len(),
                0
            );
            assert_eq!(pages.len(), 1);
            assert_eq!(pages[0].matched_counts, PlanReadCounts::default());
            assert_eq!(
                pages[0].counts.waits, 7,
                "unknown steps do not select scope-wide waits"
            );
        }
        let empty = PlanningReadSnapshot {
            timeline_seq: 0,
            plan: LocalPlan::new("reader", "empty-scope"),
            delegation: None,
        };
        for filter in [None, Some("missing")] {
            let (combined, pages) = collect_pages(&empty, filter, &[20]);
            assert_eq!(combined, empty.plan);
            assert_eq!(pages.len(), 1);
            assert_eq!(pages[0].counts, PlanReadCounts::default());
            assert_eq!(pages[0].matched_counts, PlanReadCounts::default());
        }
    }

    #[test]
    fn wire_schema_is_flat_strict_and_preserves_nested_fact_names() {
        let default: PlanReadInput = serde_json::from_value(json!({})).unwrap();
        assert_eq!(default, PlanReadInput::default());
        assert_eq!(default.target, PlanReadTarget::SelfPlan);
        let input = PlanReadInput {
            target: PlanReadTarget::Parent,
            step_id: Some("removed".into()),
            cursor: Some("opaque".into()),
            limit: Some(3),
        };
        assert_eq!(
            serde_json::to_value(&input).unwrap(),
            json!({"target":"parent","step_id":"removed","cursor":"opaque","limit":3})
        );
        assert_eq!(
            serde_json::from_value::<PlanReadInput>(serde_json::to_value(&input).unwrap()).unwrap(),
            input
        );
        for invalid in [
            json!({"session_id":"other"}),
            json!({"subagent_id":"other"}),
            json!({"query":{}}),
            json!({"stepId":"removed"}),
            json!({"target":"SelfPlan"}),
            json!({"target":"root"}),
            json!({"limit":-1}),
            json!({"limit":1.5}),
        ] {
            assert!(
                serde_json::from_value::<PlanReadInput>(invalid.clone()).is_err(),
                "{invalid}"
            );
        }
        assert!(serde_json::from_value::<PlanReadQuery>(json!({"session_id":"other"})).is_err());
        let schema = serde_json::to_value(schemars::schema_for!(PlanReadInput)).unwrap();
        assert_eq!(schema["additionalProperties"], false);
        assert!(schema["properties"].get("step_id").is_some());
        assert!(schema["properties"].get("query").is_none());
        let snapshot = snapshot();
        let page = project(
            &snapshot,
            &PlanReadQuery {
                limit: Some(50),
                ..Default::default()
            },
        )
        .unwrap();
        let output = PlanReadOutput {
            target: PlanReadTarget::Parent,
            plan: Some(page),
        };
        let wire = serde_json::to_value(&output).unwrap();
        assert_eq!(wire["target"], "parent");
        assert_eq!(wire["plan"]["session_id"], "parent-session");
        assert_eq!(wire["plan"]["timeline_seq"], 123);
        assert_eq!(wire["plan"]["delegation"]["task_id"], "link-z");
        assert_eq!(wire["plan"]["links"][0]["parentStepId"], "removed");
        assert!(wire["plan"]["links"][0].get("parent_step_id").is_none());
        assert_eq!(
            wire["plan"]["reviews"][0]["resultId"],
            snapshot.plan.reviews[0].result_id.clone().unwrap()
        );
        assert_eq!(wire["plan"]["reviews"][0]["reviewedSourceRevision"], 5);
        assert_eq!(
            wire["plan"]["reviews"][0]["verificationRefs"],
            json!(snapshot.plan.reviews[0].verification_refs)
        );
        assert_eq!(
            serde_json::from_value::<PlanReadOutput>(wire).unwrap(),
            output
        );
        assert_eq!(
            serde_json::to_value(PlanReadOutput {
                target: PlanReadTarget::SelfPlan,
                plan: None
            })
            .unwrap(),
            json!({"target":"self","plan":null})
        );
    }

    #[test]
    fn cursor_binds_reader_target_source_session_scope_and_exact_filter() {
        let snapshot = snapshot();
        let cursor = first_cursor(&snapshot, None);
        let query = PlanReadQuery {
            cursor: Some(cursor),
            limit: Some(2),
            step_id: None,
        };
        assert_eq!(
            project_plan_read("another-reader", PlanReadTarget::Parent, &snapshot, &query),
            Err(PlanReadError::CursorMismatch)
        );
        assert_eq!(
            project_plan_read("child-link-z", PlanReadTarget::SelfPlan, &snapshot, &query),
            Err(PlanReadError::CursorMismatch)
        );
        let mut another_session = snapshot.clone();
        another_session.plan.session_id = "another-source".into();
        assert_eq!(
            project(&another_session, &query),
            Err(PlanReadError::CursorMismatch)
        );
        let mut another_scope = snapshot.clone();
        another_scope.plan.scope_id = "another-scope".into();
        assert_eq!(
            project(&another_scope, &query),
            Err(PlanReadError::CursorMismatch)
        );
        let filtered = PlanReadQuery {
            step_id: Some("removed".into()),
            ..query.clone()
        };
        assert_eq!(
            project(&snapshot, &filtered),
            Err(PlanReadError::CursorMismatch)
        );
        let removed_cursor = first_cursor(&snapshot, Some("removed"));
        for step_id in [None, Some("live"), Some("removed ")] {
            assert_eq!(
                project(
                    &snapshot,
                    &PlanReadQuery {
                        step_id: step_id.map(str::to_owned),
                        cursor: Some(removed_cursor.clone()),
                        limit: None
                    }
                ),
                Err(PlanReadError::CursorMismatch)
            );
        }
        assert!(
            project(&snapshot, &query).is_ok(),
            "a cursor may be replayed for the same authorized snapshot"
        );
    }

    #[test]
    fn cursor_rejects_changed_planning_sequence_or_revision_as_stale() {
        let snapshot = snapshot();
        let query = PlanReadQuery {
            cursor: Some(first_cursor(&snapshot, None)),
            ..Default::default()
        };
        let mut changed_sequence = snapshot.clone();
        changed_sequence.timeline_seq += 1;
        assert_eq!(
            project(&changed_sequence, &query),
            Err(PlanReadError::StaleCursor)
        );
        let mut changed_revision = snapshot.clone();
        changed_revision.plan.revision += 1;
        assert_eq!(
            project(&changed_revision, &query),
            Err(PlanReadError::StaleCursor)
        );
        assert!(project(&snapshot, &query).is_ok());
    }

    #[test]
    fn query_rejects_invalid_ids_and_limits_without_normalizing_identity() {
        let snapshot = snapshot();
        for limit in [0, 51, u32::MAX] {
            assert_eq!(
                project(
                    &snapshot,
                    &PlanReadQuery {
                        limit: Some(limit),
                        ..Default::default()
                    }
                ),
                Err(PlanReadError::InvalidQuery)
            );
        }
        for step_id in [
            String::new(),
            "   ".into(),
            "a\nb".into(),
            "a\0b".into(),
            "x".repeat(129),
            "é".repeat(65),
        ] {
            assert_eq!(
                project(
                    &snapshot,
                    &PlanReadQuery {
                        step_id: Some(step_id),
                        ..Default::default()
                    }
                ),
                Err(PlanReadError::InvalidQuery)
            );
        }
        let mut bounded = PlanningReadSnapshot {
            timeline_seq: 1,
            plan: LocalPlan::new("reader", "scope"),
            delegation: None,
        };
        let boundary_id = "é".repeat(64);
        bounded.plan.steps = vec![step(&boundary_id), step(" spaced ")];
        bounded.plan.validate().unwrap();
        for id in [boundary_id.as_str(), " spaced "] {
            let page = project(
                &bounded,
                &PlanReadQuery {
                    step_id: Some(id.into()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(page.steps.len(), 1);
            assert_eq!(page.steps[0].id, id);
        }
        assert!(
            project(
                &bounded,
                &PlanReadQuery {
                    step_id: Some("spaced".into()),
                    ..Default::default()
                }
            )
            .unwrap()
            .steps
            .is_empty()
        );
    }

    #[test]
    fn cursor_rejects_malformed_oversized_and_out_of_range_positions() {
        let snapshot = snapshot();
        let before = serde_json::to_vec(&snapshot).unwrap();
        for cursor in [
            String::new(),
            "not json".into(),
            "[]".into(),
            "null".into(),
            "{}".into(),
            "é".repeat(2049),
        ] {
            assert_eq!(
                project(
                    &snapshot,
                    &PlanReadQuery {
                        cursor: Some(cursor),
                        ..Default::default()
                    }
                ),
                Err(PlanReadError::InvalidCursor)
            );
        }
        let original: Value = serde_json::from_str(&first_cursor(&snapshot, None)).unwrap();
        let total = snapshot.plan.steps.len()
            + snapshot.plan.links.len()
            + snapshot.plan.waits.len()
            + snapshot.plan.replans.len()
            + snapshot.plan.reviews.len();
        for offset in [
            json!(-1),
            json!(total),
            json!(total + 1),
            json!(u64::MAX),
            json!("1"),
            Value::Null,
        ] {
            let mut invalid = original.clone();
            invalid["offset"] = offset;
            assert_eq!(
                project(
                    &snapshot,
                    &PlanReadQuery {
                        cursor: Some(invalid.to_string()),
                        ..Default::default()
                    }
                ),
                Err(PlanReadError::InvalidCursor)
            );
        }
        let mut missing_offset = original.clone();
        missing_offset.as_object_mut().unwrap().remove("offset");
        let mut unknown_field = original;
        unknown_field["extra"] = json!(true);
        for invalid in [missing_offset, unknown_field] {
            assert_eq!(
                project(
                    &snapshot,
                    &PlanReadQuery {
                        cursor: Some(invalid.to_string()),
                        ..Default::default()
                    }
                ),
                Err(PlanReadError::InvalidCursor)
            );
        }
        assert_eq!(serde_json::to_vec(&snapshot).unwrap(), before);
    }

    #[test]
    fn query_validation_is_shared_and_does_not_need_a_snapshot() {
        PlanReadQuery::default().validate().unwrap();
        for query in [
            PlanReadQuery {
                limit: Some(0),
                ..Default::default()
            },
            PlanReadQuery {
                limit: Some(51),
                ..Default::default()
            },
            PlanReadQuery {
                step_id: Some(String::new()),
                ..Default::default()
            },
            PlanReadQuery {
                step_id: Some("x".repeat(129)),
                ..Default::default()
            },
            PlanReadQuery {
                step_id: Some("a\nb".into()),
                ..Default::default()
            },
        ] {
            let error = query.validate().unwrap_err();
            assert_eq!(error, PlanReadError::InvalidQuery);
            assert_eq!(error.code(), "invalid_query");
            assert!(!error.to_string().is_empty());
        }
        for cursor in [String::new(), "{}".into(), "é".repeat(2049)] {
            let error = PlanReadQuery {
                cursor: Some(cursor),
                ..Default::default()
            }
            .validate()
            .unwrap_err();
            assert_eq!(error, PlanReadError::InvalidCursor);
            assert_eq!(error.code(), "invalid_cursor");
        }
        let snapshot = snapshot();
        let cursor = first_cursor(&snapshot, None);
        let mut query = PlanReadQuery {
            cursor: Some(cursor.clone()),
            ..Default::default()
        };
        query.validate().unwrap();
        let mut out_of_range: Value = serde_json::from_str(&cursor).unwrap();
        out_of_range["offset"] = json!(u64::MAX);
        query.cursor = Some(out_of_range.to_string());
        query
            .validate()
            .expect("syntax validation does not select a snapshot offset");
        assert_eq!(
            project(&snapshot, &query),
            Err(PlanReadError::InvalidCursor)
        );
        query.cursor = Some(cursor);
        query.step_id = Some("other".into());
        query
            .validate()
            .expect("syntax validation does not authorize a filter");
        assert_eq!(
            project(&snapshot, &query),
            Err(PlanReadError::CursorMismatch)
        );
    }

    #[test]
    fn default_and_max_limits_apply_across_the_whole_page() {
        let mut snapshot = PlanningReadSnapshot {
            timeline_seq: 8,
            plan: LocalPlan::new("reader", "scope"),
            delegation: None,
        };
        snapshot.plan.steps = (0..60)
            .map(|index| step(&format!("step-{index}")))
            .collect();
        snapshot.plan.validate().unwrap();
        let default = project(&snapshot, &PlanReadQuery::default()).unwrap();
        assert_eq!(page_count(&default), 20);
        assert!(default.next_cursor.is_some());
        let max = project(
            &snapshot,
            &PlanReadQuery {
                limit: Some(50),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page_count(&max), 50);
        assert!(max.next_cursor.is_some());
        let (combined, pages) = collect_pages(&snapshot, None, &[50, 1, 3]);
        assert_eq!(combined, snapshot.plan);
        assert_eq!(page_count(&pages[0]), 50);
        assert_eq!(page_count(&pages[1]), 1);
        let mixed = self::snapshot();
        let page = project(&mixed, &PlanReadQuery::default()).unwrap();
        assert_eq!(page_count(&page), 20);
        assert_eq!(page.reviews, mixed.plan.reviews[..1]);
    }

    #[test]
    fn byte_limited_pages_keep_utf8_escaped_requirements_and_reach_the_end() {
        let chunk = "文😀\\\"\n".repeat(4000);
        assert!(chunk.len() <= 64 * 1024);
        assert!(serde_json::to_vec(&chunk).unwrap().len() > chunk.len());
        let mut snapshot = PlanningReadSnapshot {
            timeline_seq: 99,
            plan: LocalPlan::new("reader-文", "scope-\\\""),
            delegation: None,
        };
        snapshot.plan.steps = (0..5)
            .map(|index| {
                let mut item = step(&format!("large-{index}"));
                item.requirements = Some(vec![chunk.clone(), chunk.clone()]);
                item
            })
            .collect();
        snapshot.plan.validate().unwrap();
        let (combined, pages) = collect_pages(&snapshot, None, &[50, 49]);
        assert_eq!(combined, snapshot.plan);
        assert_eq!(
            pages.iter().map(page_count).collect::<Vec<_>>(),
            vec![2, 2, 1]
        );
        assert!(pages[0].next_cursor.is_some());
        assert!(pages[1].next_cursor.is_some());
        assert!(pages[2].next_cursor.is_none());
        assert_eq!(pages[2].steps[0].id, "large-4");
    }

    #[test]
    fn exact_byte_boundary_accounts_for_output_envelope_and_next_cursor() {
        let mut snapshot = PlanningReadSnapshot {
            timeline_seq: 1,
            plan: LocalPlan::new("reader", "scope"),
            delegation: None,
        };
        let mut item = step("boundary");
        item.requirements = Some(vec![
            "x".repeat(64 * 1024),
            "x".repeat(64 * 1024),
            "x".repeat(64 * 1024),
            String::new(),
        ]);
        snapshot.plan.steps.push(item);
        let baseline = project(&snapshot, &PlanReadQuery::default()).unwrap();
        let padding = 256 * 1024 - output_bytes(&baseline);
        assert!(padding < 64 * 1024);
        snapshot.plan.steps[0].requirements.as_mut().unwrap()[3] = "x".repeat(padding);
        snapshot.plan.validate().unwrap();
        let exact = project(&snapshot, &PlanReadQuery::default()).unwrap();
        assert_eq!(output_bytes(&exact), 256 * 1024);
        assert_eq!(exact.steps, snapshot.plan.steps);
        snapshot.plan.steps[0].requirements.as_mut().unwrap()[3].push('x');
        assert_eq!(
            project(&snapshot, &PlanReadQuery::default()),
            Err(PlanReadError::ItemTooLarge)
        );
        snapshot.plan.steps[0].requirements.as_mut().unwrap()[3].pop();
        snapshot.plan.steps.push(step("second"));
        snapshot.plan.validate().unwrap();
        assert_eq!(
            project(
                &snapshot,
                &PlanReadQuery {
                    limit: Some(1),
                    ..Default::default()
                }
            ),
            Err(PlanReadError::ItemTooLarge),
            "a continuation cursor must fit too"
        );
    }

    #[test]
    fn oversized_record_errors_without_skipping_truncating_or_mutating_facts() {
        let mut snapshot = PlanningReadSnapshot {
            timeline_seq: 10,
            plan: LocalPlan::new("reader", "scope"),
            delegation: None,
        };
        let mut oversized = step("oversized");
        oversized.requirements = Some(vec!["x".repeat(64 * 1024); 5]);
        snapshot.plan.steps = vec![step("first"), oversized, step("last")];
        snapshot.plan.validate().unwrap();
        let before = serde_json::to_vec(&snapshot).unwrap();
        let first = project(
            &snapshot,
            &PlanReadQuery {
                limit: Some(50),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(first.steps, snapshot.plan.steps[..1]);
        assert_eq!(first.counts.steps, 3);
        let query = PlanReadQuery {
            cursor: first.next_cursor.clone(),
            limit: Some(50),
            step_id: None,
        };
        assert!(query.cursor.is_some());
        assert_eq!(project(&snapshot, &query), Err(PlanReadError::ItemTooLarge));
        assert_eq!(
            project(&snapshot, &query),
            Err(PlanReadError::ItemTooLarge),
            "retry cannot silently advance past an oversized record"
        );
        let repeated = project(
            &snapshot,
            &PlanReadQuery {
                limit: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(repeated, first);
        assert_eq!(serde_json::to_vec(&snapshot).unwrap(), before);
    }
}
