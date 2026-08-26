//! Contract tests pinning the built-in tool → `ToolKind` taxonomy.
//!
//! The allowlist table in [`expected_builtin_tool_kinds`] is the test-side
//! source of truth for (a) which tools are built-in and (b) the presentation
//! kind and descriptor access ceiling each one declares. It is asserted equal
//! to the live registry, so a
//! tool may never be added, removed, or re-kind'd without an explicit edit
//! here. This is deliberately an allowlist: there is no "everything else"
//! catch-all, and kind-less MCP/custom ids are an explicit, separate list
//! (see [`kindless_mcp_custom_exceptions_are_explicit`]).
//!
//! Contract covered:
//! - `ToolConfig::kind` is auto-filled as `Some(kind)` by
//!   `ToolConfig::for_tool::<T>()` for every built-in tool (never `None`).
//! - `ToolRegistryBuilder::new()` registers exactly the allowlisted set.
//! - `ToolConfig::from_id()` leaves `kind: None` for opaque MCP/custom ids.
//!   Authorization never derives from this optional presentation field: the
//!   descriptor RWX ceiling, exact actor eligibility, and call-bound permit
//!   are authoritative.

use std::collections::HashMap;

use crate::implementations::{
    context_recall, grow_build, grow_build_concise, grow_build_hashline, memory, search_tool,
    use_tool,
};
use crate::registry::types::{ToolConfig, ToolRegistryBuilder};
use crate::types::tool::ToolKind;
use crate::types::tool_metadata::ToolMetadata;
use tool_protocol::ToolAccess;

/// Asserts `for_tool::<T>()` auto-fills `kind` with `expected` (invariant:
/// a built-in tool never carries `kind: None`) and returns the
/// fully-qualified id for the contract table.
fn entry<T>(expected_kind: ToolKind, expected_access: ToolAccess) -> (String, ToolKind)
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
        Some(expected_kind),
        "{} declares a kind different from the contract table",
        cfg.id
    );
    assert_eq!(
        tool_runtime::Tool::capabilities(&T::default()).max_access,
        expected_access,
        "{} declares an RWX requirement different from the contract table",
        cfg.id,
    );
    (cfg.id, cfg.kind.expect("checked above"))
}

/// The complete built-in tool allowlist: fully-qualified id → `ToolKind`, with
/// a separately asserted descriptor ceiling. `ToolKind` is presentation and
/// discovery metadata only; authorization must never derive from it.
///
/// Scheduler tools remain `Other` because eligibility is owner/depth policy,
/// not an RWX inference. Their explicit ceilings prevent `Other` from becoming
/// an implicit authorization fallback.
/// `list_dir` declaring `List` (not `ListDir`) is the documented grow
/// toolset choice; both variants share one capability class.
fn expected_builtin_tool_kinds() -> HashMap<String, ToolKind> {
    [
        // ── Grow file tools ────────────────────────────────────────────────
        entry::<grow_build::ReadFileTool>(ToolKind::Read, ToolAccess::Read),
        entry::<grow_build::SearchReplaceTool>(ToolKind::Edit, ToolAccess::ReadWrite),
        entry::<grow_build::WriteTool>(ToolKind::Write, ToolAccess::Write),
        entry::<grow_build::ListDirTool>(ToolKind::List, ToolAccess::Read),
        entry::<grow_build::GrepTool>(ToolKind::Search, ToolAccess::Read),
        // ── Grow shell / background-task lifecycle ─────────────────────────
        entry::<grow_build::BashTool>(ToolKind::Execute, ToolAccess::All),
        entry::<grow_build::KillTaskTool>(ToolKind::KillTaskAction, ToolAccess::None),
        entry::<grow_build::KillTerminalCommandTool>(ToolKind::KillTaskAction, ToolAccess::None),
        entry::<grow_build::TaskOutputTool>(ToolKind::BackgroundTaskAction, ToolAccess::Read),
        entry::<grow_build::GetTerminalCommandOutputTool>(
            ToolKind::BackgroundTaskAction,
            ToolAccess::Read,
        ),
        entry::<grow_build::MonitorTool>(ToolKind::Monitor, ToolAccess::All),
        // ── Grow planning / meta ───────────────────────────────────────────
        entry::<grow_build::TodoWriteTool>(ToolKind::Plan, ToolAccess::None),
        entry::<grow_build::PlanControlTool>(ToolKind::PlanControl, ToolAccess::None),
        entry::<grow_build::AskUserQuestionTool>(ToolKind::AskUser, ToolAccess::None),
        // ── Grow goal control plane ────────────────────────────────────────
        entry::<grow_build::CreateGoalTool>(
            ToolKind::GoalLifecycleUpdate,
            ToolAccess::WriteExecute,
        ),
        entry::<grow_build::GetGoalTool>(ToolKind::GoalRead, ToolAccess::Read),
        entry::<grow_build::UpdateGoalTool>(
            ToolKind::GoalLifecycleUpdate,
            ToolAccess::WriteExecute,
        ),
        // ── Grow orchestration ─────────────────────────────────────────────
        entry::<grow_build::TaskTool>(ToolKind::Task, ToolAccess::All),
        entry::<grow_build::WorkflowTool>(ToolKind::Workflow, ToolAccess::All),
        entry::<grow_build::WebFetchTool>(ToolKind::WebFetch, ToolAccess::ReadWrite),
        entry::<grow_build::LspTool>(ToolKind::Lsp, ToolAccess::Read),
        // ── Grow scheduler (loop control) — fail-closed by design ──────────
        entry::<grow_build::SchedulerCreateTool>(ToolKind::Other, ToolAccess::WriteExecute),
        entry::<grow_build::SchedulerDeleteTool>(ToolKind::Other, ToolAccess::WriteExecute),
        entry::<grow_build::SchedulerListTool>(ToolKind::Other, ToolAccess::Read),
        // ── Grow integration dispatch ──────────────────────────────────────
        entry::<use_tool::UseTool>(ToolKind::UseTool, ToolAccess::All),
        entry::<search_tool::SearchTool>(ToolKind::SearchTool, ToolAccess::None),
        // ── Grow memory ────────────────────────────────────────────────────
        entry::<memory::MemorySearchImpl>(ToolKind::MemorySearch, ToolAccess::Read),
        entry::<memory::MemoryGetImpl>(ToolKind::MemoryGet, ToolAccess::Read),
        entry::<context_recall::ContextRecallImpl>(ToolKind::ContextRecall, ToolAccess::Read),
        // ── GrowConcise variants ───────────────────────────────────────────
        entry::<grow_build_concise::ReadFileConciseTool>(ToolKind::Read, ToolAccess::Read),
        entry::<grow_build_concise::SearchReplaceConciseTool>(
            ToolKind::Edit,
            ToolAccess::ReadWrite,
        ),
        entry::<grow_build_concise::BashConciseTool>(ToolKind::Execute, ToolAccess::All),
        // ── GrowHashline variants ──────────────────────────────────────────
        entry::<grow_build_hashline::HashlineReadTool>(ToolKind::Read, ToolAccess::Read),
        entry::<grow_build_hashline::HashlineEditTool>(ToolKind::Edit, ToolAccess::ReadWrite),
        entry::<grow_build_hashline::HashlineGrepTool>(ToolKind::Search, ToolAccess::Read),
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

/// The pinned examples allowed to carry `kind: None` are MCP/custom tools created
/// via `ToolConfig::from_id`. This list is explicit: a new kind-less id
/// must be added here with a reason, never silently absorbed by a
/// "everything else stays None" catch-all.
///
/// Note this covers the *config* layer. Dynamically registered MCP tools in a
/// finalized toolset carry `Some(ToolKind::Other)` for display/discovery (see
/// `FinalizedToolset::register_tool` and `mcp::McpErasedTool`). Their runtime
/// authority instead comes from the server trust-domain mask and transport
/// generation binding.
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
            "{id} must stay kind-less (reason: {reason}); opaque config ids \
             must not invent presentation metadata"
        );
        assert!(
            !known.contains_key(*id),
            "{id} is listed as a kind-less exception but is a built-in \
             registry tool — the exception list and the registry disagree"
        );
    }
}
