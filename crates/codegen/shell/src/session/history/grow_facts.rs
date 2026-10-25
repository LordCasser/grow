//! Closed, typed representation of durable Grow presentation facts.
//!
//! This is the payload vocabulary for the physical Timeline ledger. It keeps
//! only Grow updates whose displayed state cannot be reconstructed from
//! existing domain events.

use crate::agent::models::SessionModelState;
use crate::extensions::notification::{
    AutoCompactCancelReason, ControlStateUpdate, DiffContent, ImageCompressedEntry,
    ImageRecoveryOperationState, PromptUsage, RetryState, SessionUpdate, SubagentPermissionOutcome,
    TurnIdentity, UiNotice, WorkflowAgentInfo, WorkflowPhaseInfo,
};
use crate::session::goal_tracker::GoalTokenUsage;
use tools::types::TaskSnapshot;

/// Durable Grow facts retained as standalone Timeline payloads.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "sessionUpdate", deny_unknown_fields)]
pub(crate) enum GrowHistoryFact {
    ImageRecoveryOperation {
        operation_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_id: Option<String>,
        state: ImageRecoveryOperationState,
    },
    UiNotice(UiNotice),
    ControlStateUpdate(ControlStateUpdate),
    RetryState(RetryState),
    AutoCompactStarted {
        tokens_used: u64,
        context_window: u64,
        percentage: u8,
        reason: String,
    },
    AutoCompactCompleted {
        #[serde(default)]
        manual: bool,
        async_compact: bool,
        tokens_before: u64,
        tokens_after: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        elapsed_ms: Option<i64>,
        summary_preview: Option<String>,
    },
    AutoCompactFailed {
        error: String,
    },
    MemoryFlushStarted,
    MemoryFlushCompleted {
        result: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    MemoryDreamCompleted {
        result: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    MemorySessionSaved {
        path: String,
    },
    AutoCompactCancelled {
        reason: AutoCompactCancelReason,
    },
    HooksChanged {
        hooks: Vec<extension_types::HookInfo>,
        project_trusted: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        load_errors: Vec<String>,
    },
    PluginsChanged {
        plugins: Vec<extension_types::PluginInfo>,
    },
    PluginUpdatesInstalled {
        updates: Vec<(String, String, String)>,
    },
    SessionRecap {
        summary: String,
        #[serde(default)]
        auto: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        away_period_id: Option<uuid::Uuid>,
    },
    SessionRecapUnavailable,
    TaskCompleted {
        task_snapshot: TaskSnapshot,
    },
    SubagentSpawned {
        subagent_id: String,
        parent_session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_prompt_id: Option<String>,
        child_session_id: String,
        subagent_type: String,
        description: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effective_context_source: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        context_normalized: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capability_mode: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        permission_mode: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effective_permission_mode: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning_effort: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_state: Option<SessionModelState>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workflow_agent_names: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resumed_from: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workflow_run_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        goal_id: Option<String>,
    },
    SubagentPermissionDecision {
        child_session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subagent_type: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        tool_call_id: String,
        tool_name: String,
        access_kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        access_summary: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        access_detail: Option<String>,
        outcome: SubagentPermissionOutcome,
        source: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        classifier_reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        latency_ms: Option<u64>,
    },
    SubagentProgress {
        subagent_id: String,
        parent_session_id: String,
        child_session_id: String,
        duration_ms: u64,
        turn_count: u32,
        tool_call_count: u32,
        tokens_used: u64,
        context_window_tokens: u64,
        context_usage_pct: u8,
        tools_used: Vec<String>,
        error_count: u32,
    },
    SubagentFinished {
        subagent_id: String,
        child_session_id: String,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        tool_calls: u32,
        turns: u32,
        duration_ms: u64,
        tokens_used: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    TaskBackgrounded {
        tool_call_id: String,
        task_id: String,
        command: String,
        cwd: String,
        output_file: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        monitor_description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    ScheduledTaskCreated {
        task_id: String,
        prompt: String,
        human_schedule: String,
        next_fire_at: Option<String>,
    },
    ScheduledTaskDeleted {
        task_id: String,
    },
    ImageCompressed {
        images: Vec<ImageCompressedEntry>,
        message: String,
    },
    ImageDropped {
        notes: Vec<String>,
    },
    DiffReview {
        content: Vec<DiffContent>,
    },
    WorkflowUpdated {
        run_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        definition_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        definition_scope: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        definition_hash: Option<String>,
        #[serde(default)]
        revision: u64,
        name: String,
        objective: String,
        status: String,
        #[serde(default)]
        foreground: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        phases: Vec<WorkflowPhaseInfo>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        current_phase: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_budget: Option<u64>,
        #[serde(default)]
        agents_used: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agents_remaining: Option<u64>,
        #[serde(default)]
        agent_usage_incomplete: bool,
        elapsed_ms: u64,
        #[serde(default)]
        active_agents: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        current_agent_label: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        agents: Vec<WorkflowAgentInfo>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pause_message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result_summary: Option<String>,
    },
    GoalUpdated {
        goal_id: String,
        objective: String,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        token_budget: Option<i64>,
        #[serde(default)]
        tokens_used: i64,
        #[serde(default)]
        usage_breakdown: GoalTokenUsage,
        #[serde(default)]
        usage_incomplete: bool,
        elapsed_ms: u64,
        created_at: String,
        updated_at: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status_message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        completion_report: Option<String>,
    },
    RewindMarker {
        target_prompt_index: usize,
        created_at: String,
    },
    TurnCompleted {
        prompt_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        identity: Option<TurnIdentity>,
        stop_reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_result: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<PromptUsage>,
    },
}

impl TryFrom<SessionUpdate> for GrowHistoryFact {
    type Error = &'static str;

    fn try_from(update: SessionUpdate) -> Result<Self, Self::Error> {
        Ok(match update {
            SessionUpdate::ImageRecoveryOperation {
                operation_id,
                prompt_id,
                state,
            } => Self::ImageRecoveryOperation {
                operation_id,
                prompt_id,
                state,
            },
            SessionUpdate::UiNotice(value) => Self::UiNotice(value),
            SessionUpdate::ControlStateUpdate(value) => Self::ControlStateUpdate(value),
            SessionUpdate::RetryState(value) => Self::RetryState(value),
            SessionUpdate::AutoCompactStarted {
                tokens_used,
                context_window,
                percentage,
                reason,
            } => Self::AutoCompactStarted {
                tokens_used,
                context_window,
                percentage,
                reason,
            },
            SessionUpdate::AutoCompactCompleted {
                manual,
                async_compact,
                tokens_before,
                tokens_after,
                elapsed_ms,
                summary_preview,
            } => Self::AutoCompactCompleted {
                manual,
                async_compact,
                tokens_before,
                tokens_after,
                elapsed_ms,
                summary_preview,
            },
            SessionUpdate::AutoCompactFailed { error } => Self::AutoCompactFailed { error },
            SessionUpdate::MemoryFlushStarted => Self::MemoryFlushStarted,
            SessionUpdate::MemoryFlushCompleted { result, path } => {
                Self::MemoryFlushCompleted { result, path }
            }
            SessionUpdate::MemoryDreamCompleted { result, path } => {
                Self::MemoryDreamCompleted { result, path }
            }
            SessionUpdate::MemorySessionSaved { path } => Self::MemorySessionSaved { path },
            SessionUpdate::AutoCompactCancelled { reason } => Self::AutoCompactCancelled { reason },
            SessionUpdate::HooksChanged {
                hooks,
                project_trusted,
                load_errors,
            } => Self::HooksChanged {
                hooks,
                project_trusted,
                load_errors,
            },
            SessionUpdate::PluginsChanged { plugins } => Self::PluginsChanged { plugins },
            SessionUpdate::PluginUpdatesInstalled { updates } => {
                Self::PluginUpdatesInstalled { updates }
            }
            SessionUpdate::SessionRecap {
                summary,
                auto,
                away_period_id,
            } => Self::SessionRecap {
                summary,
                auto,
                away_period_id,
            },
            SessionUpdate::SessionRecapUnavailable => Self::SessionRecapUnavailable,
            SessionUpdate::TaskCompleted { task_snapshot } => Self::TaskCompleted { task_snapshot },
            SessionUpdate::SubagentSpawned {
                subagent_id,
                parent_session_id,
                parent_prompt_id,
                child_session_id,
                subagent_type,
                description,
                effective_context_source,
                context_normalized,
                capability_mode,
                permission_mode,
                effective_permission_mode,
                model,
                reasoning_effort,
                model_state,
                workflow_agent_names,
                resumed_from,
                workflow_run_id,
                goal_id,
            } => Self::SubagentSpawned {
                subagent_id,
                parent_session_id,
                parent_prompt_id,
                child_session_id,
                subagent_type,
                description,
                effective_context_source,
                context_normalized,
                capability_mode,
                permission_mode,
                effective_permission_mode,
                model,
                reasoning_effort,
                model_state,
                workflow_agent_names,
                resumed_from,
                workflow_run_id,
                goal_id,
            },
            SessionUpdate::SubagentPermissionDecision {
                child_session_id,
                subagent_type,
                description,
                tool_call_id,
                tool_name,
                access_kind,
                access_summary,
                access_detail,
                outcome,
                source,
                reason,
                classifier_reason,
                latency_ms,
            } => Self::SubagentPermissionDecision {
                child_session_id,
                subagent_type,
                description,
                tool_call_id,
                tool_name,
                access_kind,
                access_summary,
                access_detail,
                outcome,
                source,
                reason,
                classifier_reason,
                latency_ms,
            },
            SessionUpdate::SubagentProgress {
                subagent_id,
                parent_session_id,
                child_session_id,
                duration_ms,
                turn_count,
                tool_call_count,
                tokens_used,
                context_window_tokens,
                context_usage_pct,
                tools_used,
                error_count,
            } => Self::SubagentProgress {
                subagent_id,
                parent_session_id,
                child_session_id,
                duration_ms,
                turn_count,
                tool_call_count,
                tokens_used,
                context_window_tokens,
                context_usage_pct,
                tools_used,
                error_count,
            },
            SessionUpdate::SubagentFinished {
                subagent_id,
                child_session_id,
                status,
                error,
                tool_calls,
                turns,
                duration_ms,
                tokens_used,
                output,
            } => Self::SubagentFinished {
                subagent_id,
                child_session_id,
                status,
                error,
                tool_calls,
                turns,
                duration_ms,
                tokens_used,
                output,
            },
            SessionUpdate::TaskBackgrounded {
                tool_call_id,
                task_id,
                command,
                cwd,
                output_file,
                monitor_description,
                description,
            } => Self::TaskBackgrounded {
                tool_call_id,
                task_id,
                command,
                cwd,
                output_file,
                monitor_description,
                description,
            },
            SessionUpdate::ScheduledTaskCreated {
                task_id,
                prompt,
                human_schedule,
                next_fire_at,
            } => Self::ScheduledTaskCreated {
                task_id,
                prompt,
                human_schedule,
                next_fire_at,
            },
            SessionUpdate::ScheduledTaskDeleted { task_id } => {
                Self::ScheduledTaskDeleted { task_id }
            }
            SessionUpdate::ImageCompressed { images, message } => {
                Self::ImageCompressed { images, message }
            }
            SessionUpdate::ImageDropped { notes } => Self::ImageDropped { notes },
            SessionUpdate::DiffReview { content } => Self::DiffReview { content },
            SessionUpdate::WorkflowUpdated {
                run_id,
                definition_id,
                definition_scope,
                definition_hash,
                revision,
                name,
                objective,
                status,
                foreground,
                phases,
                current_phase,
                agent_budget,
                agents_used,
                agents_remaining,
                agent_usage_incomplete,
                elapsed_ms,
                active_agents,
                current_agent_label,
                agents,
                pause_message,
                result_summary,
            } => Self::WorkflowUpdated {
                run_id,
                definition_id,
                definition_scope,
                definition_hash,
                revision,
                name,
                objective,
                status,
                foreground,
                phases,
                current_phase,
                agent_budget,
                agents_used,
                agents_remaining,
                agent_usage_incomplete,
                elapsed_ms,
                active_agents,
                current_agent_label,
                agents,
                pause_message,
                result_summary,
            },
            SessionUpdate::GoalUpdated {
                goal_id,
                objective,
                status,
                token_budget,
                tokens_used,
                usage_breakdown,
                usage_incomplete,
                elapsed_ms,
                created_at,
                updated_at,
                status_message,
                completion_report,
            } => Self::GoalUpdated {
                goal_id,
                objective,
                status,
                token_budget,
                tokens_used,
                usage_breakdown,
                usage_incomplete,
                elapsed_ms,
                created_at,
                updated_at,
                status_message,
                completion_report,
            },
            SessionUpdate::RewindMarker {
                target_prompt_index,
                created_at,
            } => Self::RewindMarker {
                target_prompt_index,
                created_at,
            },
            SessionUpdate::TurnCompleted {
                prompt_id,
                identity,
                stop_reason,
                agent_result,
                usage,
            } => Self::TurnCompleted {
                prompt_id,
                identity,
                stop_reason,
                agent_result,
                usage,
            },
            SessionUpdate::SamplingAttempt { .. }
            | SessionUpdate::HookExecution { .. }
            | SessionUpdate::ScheduledTaskFired { .. }
            | SessionUpdate::MonitorEvent { .. }
            | SessionUpdate::ModelAutoSwitched { .. }
            | SessionUpdate::ModelChanged { .. }
            | SessionUpdate::AgentChanged { .. }
            | SessionUpdate::ToolCallDeltaChunk { .. }
            | SessionUpdate::ImageProjected { .. }
            | SessionUpdate::ImageProcessing { .. }
            | SessionUpdate::MemoryFiles { .. }
            | SessionUpdate::PendingInteraction { .. }
            | SessionUpdate::InteractionResolved { .. }
            | SessionUpdate::ResponseStarted { .. }
            | SessionUpdate::ReasoningCompleted { .. }
            | SessionUpdate::ResponseCompleted { .. } => {
                return Err("session update is not a durable Grow history fact");
            }
        })
    }
}

impl From<GrowHistoryFact> for SessionUpdate {
    fn from(fact: GrowHistoryFact) -> Self {
        match fact {
            GrowHistoryFact::ImageRecoveryOperation {
                operation_id,
                prompt_id,
                state,
            } => Self::ImageRecoveryOperation {
                operation_id,
                prompt_id,
                state,
            },
            GrowHistoryFact::UiNotice(value) => Self::UiNotice(value),
            GrowHistoryFact::ControlStateUpdate(value) => Self::ControlStateUpdate(value),
            GrowHistoryFact::RetryState(value) => Self::RetryState(value),
            GrowHistoryFact::AutoCompactStarted {
                tokens_used,
                context_window,
                percentage,
                reason,
            } => Self::AutoCompactStarted {
                tokens_used,
                context_window,
                percentage,
                reason,
            },
            GrowHistoryFact::AutoCompactCompleted {
                manual,
                async_compact,
                tokens_before,
                tokens_after,
                elapsed_ms,
                summary_preview,
            } => Self::AutoCompactCompleted {
                manual,
                async_compact,
                tokens_before,
                tokens_after,
                elapsed_ms,
                summary_preview,
            },
            GrowHistoryFact::AutoCompactFailed { error } => Self::AutoCompactFailed { error },
            GrowHistoryFact::MemoryFlushStarted => Self::MemoryFlushStarted,
            GrowHistoryFact::MemoryFlushCompleted { result, path } => {
                Self::MemoryFlushCompleted { result, path }
            }
            GrowHistoryFact::MemoryDreamCompleted { result, path } => {
                Self::MemoryDreamCompleted { result, path }
            }
            GrowHistoryFact::MemorySessionSaved { path } => Self::MemorySessionSaved { path },
            GrowHistoryFact::AutoCompactCancelled { reason } => {
                Self::AutoCompactCancelled { reason }
            }
            GrowHistoryFact::HooksChanged {
                hooks,
                project_trusted,
                load_errors,
            } => Self::HooksChanged {
                hooks,
                project_trusted,
                load_errors,
            },
            GrowHistoryFact::PluginsChanged { plugins } => Self::PluginsChanged { plugins },
            GrowHistoryFact::PluginUpdatesInstalled { updates } => {
                Self::PluginUpdatesInstalled { updates }
            }
            GrowHistoryFact::SessionRecap {
                summary,
                auto,
                away_period_id,
            } => Self::SessionRecap {
                summary,
                auto,
                away_period_id,
            },
            GrowHistoryFact::SessionRecapUnavailable => Self::SessionRecapUnavailable,
            GrowHistoryFact::TaskCompleted { task_snapshot } => {
                Self::TaskCompleted { task_snapshot }
            }
            GrowHistoryFact::SubagentSpawned {
                subagent_id,
                parent_session_id,
                parent_prompt_id,
                child_session_id,
                subagent_type,
                description,
                effective_context_source,
                context_normalized,
                capability_mode,
                permission_mode,
                effective_permission_mode,
                model,
                reasoning_effort,
                model_state,
                workflow_agent_names,
                resumed_from,
                workflow_run_id,
                goal_id,
            } => Self::SubagentSpawned {
                subagent_id,
                parent_session_id,
                parent_prompt_id,
                child_session_id,
                subagent_type,
                description,
                effective_context_source,
                context_normalized,
                capability_mode,
                permission_mode,
                effective_permission_mode,
                model,
                reasoning_effort,
                model_state,
                workflow_agent_names,
                resumed_from,
                workflow_run_id,
                goal_id,
            },
            GrowHistoryFact::SubagentPermissionDecision {
                child_session_id,
                subagent_type,
                description,
                tool_call_id,
                tool_name,
                access_kind,
                access_summary,
                access_detail,
                outcome,
                source,
                reason,
                classifier_reason,
                latency_ms,
            } => Self::SubagentPermissionDecision {
                child_session_id,
                subagent_type,
                description,
                tool_call_id,
                tool_name,
                access_kind,
                access_summary,
                access_detail,
                outcome,
                source,
                reason,
                classifier_reason,
                latency_ms,
            },
            GrowHistoryFact::SubagentProgress {
                subagent_id,
                parent_session_id,
                child_session_id,
                duration_ms,
                turn_count,
                tool_call_count,
                tokens_used,
                context_window_tokens,
                context_usage_pct,
                tools_used,
                error_count,
            } => Self::SubagentProgress {
                subagent_id,
                parent_session_id,
                child_session_id,
                duration_ms,
                turn_count,
                tool_call_count,
                tokens_used,
                context_window_tokens,
                context_usage_pct,
                tools_used,
                error_count,
            },
            GrowHistoryFact::SubagentFinished {
                subagent_id,
                child_session_id,
                status,
                error,
                tool_calls,
                turns,
                duration_ms,
                tokens_used,
                output,
            } => Self::SubagentFinished {
                subagent_id,
                child_session_id,
                status,
                error,
                tool_calls,
                turns,
                duration_ms,
                tokens_used,
                output,
            },
            GrowHistoryFact::TaskBackgrounded {
                tool_call_id,
                task_id,
                command,
                cwd,
                output_file,
                monitor_description,
                description,
            } => Self::TaskBackgrounded {
                tool_call_id,
                task_id,
                command,
                cwd,
                output_file,
                monitor_description,
                description,
            },
            GrowHistoryFact::ScheduledTaskCreated {
                task_id,
                prompt,
                human_schedule,
                next_fire_at,
            } => Self::ScheduledTaskCreated {
                task_id,
                prompt,
                human_schedule,
                next_fire_at,
            },
            GrowHistoryFact::ScheduledTaskDeleted { task_id } => {
                Self::ScheduledTaskDeleted { task_id }
            }
            GrowHistoryFact::ImageCompressed { images, message } => {
                Self::ImageCompressed { images, message }
            }
            GrowHistoryFact::ImageDropped { notes } => Self::ImageDropped { notes },
            GrowHistoryFact::DiffReview { content } => Self::DiffReview { content },
            GrowHistoryFact::WorkflowUpdated {
                run_id,
                definition_id,
                definition_scope,
                definition_hash,
                revision,
                name,
                objective,
                status,
                foreground,
                phases,
                current_phase,
                agent_budget,
                agents_used,
                agents_remaining,
                agent_usage_incomplete,
                elapsed_ms,
                active_agents,
                current_agent_label,
                agents,
                pause_message,
                result_summary,
            } => Self::WorkflowUpdated {
                run_id,
                definition_id,
                definition_scope,
                definition_hash,
                revision,
                name,
                objective,
                status,
                foreground,
                phases,
                current_phase,
                agent_budget,
                agents_used,
                agents_remaining,
                agent_usage_incomplete,
                elapsed_ms,
                active_agents,
                current_agent_label,
                agents,
                pause_message,
                result_summary,
            },
            GrowHistoryFact::GoalUpdated {
                goal_id,
                objective,
                status,
                token_budget,
                tokens_used,
                usage_breakdown,
                usage_incomplete,
                elapsed_ms,
                created_at,
                updated_at,
                status_message,
                completion_report,
            } => Self::GoalUpdated {
                goal_id,
                objective,
                status,
                token_budget,
                tokens_used,
                usage_breakdown,
                usage_incomplete,
                elapsed_ms,
                created_at,
                updated_at,
                status_message,
                completion_report,
            },
            GrowHistoryFact::RewindMarker {
                target_prompt_index,
                created_at,
            } => Self::RewindMarker {
                target_prompt_index,
                created_at,
            },
            GrowHistoryFact::TurnCompleted {
                prompt_id,
                identity,
                stop_reason,
                agent_result,
                usage,
            } => Self::TurnCompleted {
                prompt_id,
                identity,
                stop_reason,
                agent_result,
                usage,
            },
        }
    }
}
