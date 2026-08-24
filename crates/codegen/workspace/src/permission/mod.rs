pub mod auto_mode;
pub mod bash_command_splitting;
mod exec_risk;
mod gate_preflight;
mod manager;
mod policy;
mod prompter;
pub mod resolution;
pub mod rules;
mod shell_access;
mod state;
pub mod types;

pub use auto_mode::{
    AUTO_MODE_CLASSIFIER_SYSTEM_PROMPT, AutoFastPath, CLASSIFIER_TURN_MAX_LEN, ClassifierContext,
    ClassifierFailure, ClassifierMessage, ClassifierMessageRole, ClassifierOutcome,
    ClassifierPromptType, ClassifierSource, ClassifierTurn, ClassifierVerdict, ClassifyTextChannel,
    ClassifyTextFn, FixedClassifier, HeuristicPermissionClassifier, LlmPermissionClassifier,
    PermissionClassifier, PermissionJudgmentRequest, SharedClassifier,
    access_requires_user_interaction, auto_mode_fast_path, build_classifier_messages,
    build_primary_context_judgment_message, classifier_output_json_schema,
    default_auto_mode_classifier, is_auto_mode_allowlisted_access,
    is_auto_mode_allowlisted_tool_name, parse_classifier_model_output, parse_classifier_model_text,
    permission_decision_args, primary_context_judgment_system_prompt,
};

use prometheus::{HistogramVec, IntCounterVec, register_histogram_vec, register_int_counter_vec};

static SUBAGENT_PERMISSION_JUDGMENT_TOTAL: std::sync::LazyLock<IntCounterVec> =
    std::sync::LazyLock::new(|| {
        register_int_counter_vec!(
            "grow_subagent_permission_judgment_total",
            "Primary-context permission judgments for subagents, by structured outcome",
            &["outcome"]
        )
        .unwrap()
    });

static SUBAGENT_PERMISSION_JUDGMENT_DURATION: std::sync::LazyLock<HistogramVec> =
    std::sync::LazyLock::new(|| {
        register_histogram_vec!(
            "grow_subagent_permission_judgment_duration_seconds",
            "Primary-context subagent permission judgment latency",
            &["outcome"],
            vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0]
        )
        .unwrap()
    });

static PERMISSION_PROMPT_DURATION: std::sync::LazyLock<HistogramVec> =
    std::sync::LazyLock::new(|| {
        register_histogram_vec!(
            "grow_permission_prompt_duration_seconds",
            "Interactive permission prompt latency by requester and outcome",
            &["requester", "outcome"],
            vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0]
        )
        .unwrap()
    });

static SUBAGENT_PERMISSION_NONTERMINAL_TOTAL: std::sync::LazyLock<IntCounterVec> =
    std::sync::LazyLock::new(|| {
        register_int_counter_vec!(
            "grow_subagent_permission_nonterminal_total",
            "Subagent permission failures returned as non-terminal tool results",
            &["outcome"]
        )
        .unwrap()
    });

pub(crate) fn record_subagent_permission_judgment(
    verdict: ClassifierVerdict,
    duration_seconds: f64,
) {
    let outcome = match verdict {
        ClassifierVerdict::Allow => "allow",
        ClassifierVerdict::Block => "deny",
        ClassifierVerdict::Unavailable => "unavailable",
    };
    SUBAGENT_PERMISSION_JUDGMENT_TOTAL
        .with_label_values(&[outcome])
        .inc();
    SUBAGENT_PERMISSION_JUDGMENT_DURATION
        .with_label_values(&[outcome])
        .observe(duration_seconds);
}

pub(crate) fn observe_permission_prompt(child: bool, outcome: &str, duration_seconds: f64) {
    let outcome = match outcome {
        "allow_once"
        | "allow_always"
        | "allow_always_bash"
        | "allow_always_domain"
        | "allow_always_mcp_tool"
        | "allow_always_mcp_server"
        | "allow_edits_for_session" => "allow",
        "reject_once" | "reject_always_bash" => "deny",
        "timed_out" => "timed_out",
        "cancelled" => "cancelled",
        "followup" => "followup",
        _ => "error",
    };
    PERMISSION_PROMPT_DURATION
        .with_label_values(&[if child { "subagent" } else { "primary" }, outcome])
        .observe(duration_seconds);
}

pub(crate) fn record_subagent_nonterminal_permission(outcome: &str) {
    SUBAGENT_PERMISSION_NONTERMINAL_TOTAL
        .with_label_values(&[outcome])
        .inc();
}

/// Zero-init this module's metric families. See [`crate::init_metrics`].
pub(crate) fn init_metrics() {
    for outcome in ["allow", "deny", "unavailable"] {
        SUBAGENT_PERMISSION_JUDGMENT_TOTAL
            .with_label_values(&[outcome])
            .inc_by(0);
        let _ = SUBAGENT_PERMISSION_JUDGMENT_DURATION.with_label_values(&[outcome]);
    }
    for requester in ["primary", "subagent"] {
        for outcome in [
            "allow",
            "deny",
            "timed_out",
            "cancelled",
            "followup",
            "error",
        ] {
            let _ = PERMISSION_PROMPT_DURATION.with_label_values(&[requester, outcome]);
        }
    }
    for outcome in ["denied", "timed_out"] {
        SUBAGENT_PERMISSION_NONTERMINAL_TOTAL
            .with_label_values(&[outcome])
            .inc_by(0);
    }
}
pub use manager::{
    PermissionHandle, command_is_known_observational, default_always_allow_scope,
    spawn_permission_manager,
};
pub use policy::CompiledPolicy;
pub use prompter::{
    ALLOW_EDITS_SESSION_OPTION_ID, AcpPrompter, BashCommandPermission, BashCommandSelectedTerms,
    ENABLE_ALWAYS_APPROVE_OPTION_ID, McpScopeSelection, McpToolPermission, PromptOutcome,
    is_enable_always_approve_option, mcp_pretty_name_if_qualified, mcp_titleize_segment,
    mcp_tool_action, mcp_tool_display_name,
};
pub use shell_access::{
    ProtectedEditPermission, ProtectedEditReason, command_write_paths_in_tree,
    command_write_paths_with_cwd_in_tree, tree_has_opaque_shell,
};
pub use state::PermissionState;
pub use state::cleanup_stale_permission_state;
pub use types::{AccessKind, ClientType, Decision, PermissionCommand, PermissionEvent};
