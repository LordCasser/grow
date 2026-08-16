//! Contract tests pinning the built-in tool → `ToolKind` taxonomy.
//!
//! The allowlist table in [`expected_builtin_tool_kinds`] is the test-side
//! source of truth for (a) which tools are built-in and (b) the capability
//! kind each one declares. It is asserted equal to the live registry, so a
//! tool may never be added, removed, or re-kind'd without an explicit edit
//! here. This is deliberately an allowlist: there is no "everything else"
//! catch-all, and kind-less MCP/custom ids are an explicit, separate list
//! (see [`kindless_mcp_custom_exceptions_are_explicit`]).
//!
//! Contract covered:
//! - `ToolConfig::kind` is auto-filled as `Some(kind)` by
//!   `ToolConfig::for_tool::<T>()` for every built-in tool (never `None`).
//! - `ToolRegistryBuilder::new()` registers exactly the allowlisted set.
//! - `ToolConfig::from_id()` leaves `kind: None` for MCP/custom ids, which
//!   restricted capability modes drop fail-closed (enforced by
//!   `workspace::capability` tests).

use std::collections::HashMap;

use crate::implementations::{
    grow_build, grow_build_concise, grow_build_hashline, memory, search_tool, use_tool,
};
use crate::registry::types::{ToolConfig, ToolRegistryBuilder};
use crate::types::tool::ToolKind;
use crate::types::tool_metadata::ToolMetadata;

/// Asserts `for_tool::<T>()` auto-fills `kind` with `expected` (invariant:
/// a built-in tool never carries `kind: None`) and returns the
/// fully-qualified id for the contract table.
fn entry<T>(expected: ToolKind) -> (String, ToolKind)
where
    T: ToolMetadata + tool_runtime::Tool + Default + 'static,
{
    let cfg = ToolConfig::for_tool::<T>();
    assert!(
        cfg.kind.is_some(),
        "built-in tool {} must auto-fill kind via for_tool::<T>(); got None",
        cfg.id
    );
    assert_eq!(
        cfg.kind,
        Some(expected),
        "{} declares a kind different from the contract table",
        cfg.id
    );
    (cfg.id, cfg.kind.expect("checked above"))
}

/// The complete built-in tool allowlist: fully-qualified id → `ToolKind`.
///
/// Kind semantics are pinned to `workspace::capability::kind_allowed`:
/// - Read class (ReadOnly/ReadWrite/Execute): Read, Search, Lsp, ListDir,
///   List, MemoryGet, MemorySearch, GoalRead, GoalPlanSubmit.
/// - Search class: Search, WebFetch.
/// - Edit class (ReadWrite): Edit, Write, Delete, Move, DeployApp.
/// - Execute class (Execute): Execute, BackgroundTaskAction,
///   WaitTasksAction, KillTaskAction, Task, Monitor, Workflow.
/// - Meta (always): Plan, PlanControl, AskUser, Skill, SearchTool,
///   CapabilityRequest.
/// - `Other`: fail-closed — only the All mode keeps it.
///
/// The scheduler tools are intentionally pinned to `Other` (fail-closed in
/// every restricted mode) even though `scheduler_list` only reads and
/// create/delete manage background loops. Reclassifying them would widen
/// the restricted-mode fence (scheduled tasks fire loop subagents), which
/// is a capability decision owned by the architect, not this table.
/// `list_dir` declaring `List` (not `ListDir`) is the documented grow
/// toolset choice; both variants share one capability class.
fn expected_builtin_tool_kinds() -> HashMap<String, ToolKind> {
    [
        // ── Grow file tools ────────────────────────────────────────────────
        entry::<grow_build::ReadFileTool>(ToolKind::Read),
        entry::<grow_build::SearchReplaceTool>(ToolKind::Edit),
        entry::<grow_build::WriteTool>(ToolKind::Write),
        entry::<grow_build::ListDirTool>(ToolKind::List),
        entry::<grow_build::GrepTool>(ToolKind::Search),
        // ── Grow shell / background-task lifecycle ─────────────────────────
        entry::<grow_build::BashTool>(ToolKind::Execute),
        entry::<grow_build::KillTaskTool>(ToolKind::KillTaskAction),
        entry::<grow_build::KillTerminalCommandTool>(ToolKind::KillTaskAction),
        entry::<grow_build::TaskOutputTool>(ToolKind::BackgroundTaskAction),
        entry::<grow_build::GetTerminalCommandOutputTool>(ToolKind::BackgroundTaskAction),
        entry::<grow_build::WaitTasksTool>(ToolKind::WaitTasksAction),
        entry::<grow_build::MonitorTool>(ToolKind::Monitor),
        // ── Grow planning / meta ───────────────────────────────────────────
        entry::<grow_build::TodoWriteTool>(ToolKind::Plan),
        entry::<grow_build::PlanControlTool>(ToolKind::PlanControl),
        entry::<grow_build::AskUserQuestionTool>(ToolKind::AskUser),
        entry::<grow_build::RequestToolAccessTool>(ToolKind::CapabilityRequest),
        // ── Grow goal control plane ────────────────────────────────────────
        entry::<grow_build::GetGoalTool>(ToolKind::GoalRead),
        entry::<grow_build::UpdateGoalProgressTool>(ToolKind::GoalProgressUpdate),
        entry::<grow_build::RequestGoalReplanTool>(ToolKind::GoalReplanRequest),
        entry::<grow_build::UpdateGoalTool>(ToolKind::GoalLifecycleUpdate),
        entry::<grow_build::SubmitGoalPlanSectionTool>(ToolKind::GoalPlanSubmit),
        entry::<grow_build::FinalizeGoalPlanTool>(ToolKind::GoalPlanSubmit),
        // ── Grow orchestration ─────────────────────────────────────────────
        entry::<grow_build::TaskTool>(ToolKind::Task),
        entry::<grow_build::WorkflowTool>(ToolKind::Workflow),
        entry::<grow_build::WebFetchTool>(ToolKind::WebFetch),
        entry::<grow_build::LspTool>(ToolKind::Lsp),
        // ── Grow scheduler (loop control) — fail-closed by design ──────────
        entry::<grow_build::SchedulerCreateTool>(ToolKind::Other),
        entry::<grow_build::SchedulerDeleteTool>(ToolKind::Other),
        entry::<grow_build::SchedulerListTool>(ToolKind::Other),
        // ── Grow integration dispatch ──────────────────────────────────────
        entry::<use_tool::UseTool>(ToolKind::UseTool),
        entry::<search_tool::SearchTool>(ToolKind::SearchTool),
        // ── Grow memory ────────────────────────────────────────────────────
        entry::<memory::MemorySearchImpl>(ToolKind::MemorySearch),
        entry::<memory::MemoryGetImpl>(ToolKind::MemoryGet),
        // ── GrowConcise variants ───────────────────────────────────────────
        entry::<grow_build_concise::ReadFileConciseTool>(ToolKind::Read),
        entry::<grow_build_concise::SearchReplaceConciseTool>(ToolKind::Edit),
        entry::<grow_build_concise::BashConciseTool>(ToolKind::Execute),
        // ── GrowHashline variants ──────────────────────────────────────────
        entry::<grow_build_hashline::HashlineReadTool>(ToolKind::Read),
        entry::<grow_build_hashline::HashlineEditTool>(ToolKind::Edit),
        entry::<grow_build_hashline::HashlineGrepTool>(ToolKind::Search),
    ]
    .into_iter()
    .collect()
}

#[test]
fn registry_registers_exactly_the_builtin_allowlist_with_expected_kinds() {
    let registry = ToolRegistryBuilder::new();
    let actual = registry.known_tool_kinds();
    let expected = expected_builtin_tool_kinds();
    assert_eq!(
        actual, expected,
        "built-in registry drifted from the contract table: a tool was \
         added, removed, or re-kind'd without updating this allowlist"
    );
    // Every entry above went through `for_tool::<T>()`, which asserted the
    // config-level `kind` is `Some(_)`. Belt-and-braces: none of the
    // built-in ids may resolve to a kind-less config.
    for id in actual.keys() {
        let cfg = ToolConfig::from_id(id.clone());
        assert_eq!(
            cfg.kind, None,
            "from_id must never invent a kind for {id}; only the typed \
             for_tool::<T>() path and the registry backfill may supply one"
        );
    }
}

/// The ONLY ids allowed to carry `kind: None` are MCP/custom tools created
/// via `ToolConfig::from_id`. This list is explicit: a new kind-less id
/// must be added here with a reason, never silently absorbed by a
/// "everything else stays None" catch-all.
///
/// Note this covers the *config* layer. Dynamically registered MCP tools in
/// a finalized toolset carry `Some(ToolKind::Other)` (see
/// `FinalizedToolset::register_tool` and `mcp::McpErasedTool`), which
/// restricted capability modes also drop via `kind_allowed` — the second,
/// runtime fail-closed layer. Both layers reject MCP/custom tools outside
/// `All`.
#[test]
fn kindless_mcp_custom_exceptions_are_explicit() {
    let registry = ToolRegistryBuilder::new();
    let known = registry.known_tool_kinds();

    let kindless_exceptions: &[(&str, &str)] = &[
        // (id, reason it stays kind-less)
        (
            "custom:opaque",
            "custom tool without a trusted registration",
        ),
        (
            "mcp__github__search",
            "MCP tool from a live server snapshot",
        ),
        (
            "mcp__linear__save_issue",
            "MCP tool from a live server snapshot",
        ),
    ];

    for (id, reason) in kindless_exceptions {
        let cfg = ToolConfig::from_id(*id);
        assert_eq!(
            cfg.kind, None,
            "{id} must stay kind-less (reason: {reason}); assigning a kind \
             here would silently widen restricted capability modes"
        );
        assert!(
            !known.contains_key(*id),
            "{id} is listed as a kind-less exception but is a built-in \
             registry tool — the exception list and the registry disagree"
        );
    }

    // Restricted-mode fail-closed behavior for these kind-less ids is
    // enforced by `workspace::capability` tests
    // (`capability_mode_kind_none_fails_closed_outside_all`).
}
