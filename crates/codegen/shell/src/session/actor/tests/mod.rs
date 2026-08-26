use super::*;

// Test modules retain access to the actor's private implementation modules,
// matching the visibility they had when they were direct actor children.
use super::{goal_support, turn, updates};

mod build_tool_parse_error_message_tests;
mod chat_history_integrity_tests;
mod client_hooks_tests;
mod compaction_pre_prune_tests;
mod fs_injection_regression_tests;
mod image_input_recovery_tests;
mod interjection_tests;
mod laziness_debug_tests;
mod laziness_detector_tests;
mod laziness_integration_tests;
mod parallel_dispatch_tests;
mod permission_auto_mode_tests;
mod project_instructions_idempotence_tests;
mod prompt_context_persistence_tests;
mod prompt_mode_transition_tests;
mod read_file_image_description_tests;
mod recap_display_only_tests;
mod record_response_token_usage_tests;
mod reminder_policy_tests;
mod reverse_request_session_id_tests;
mod rewind_cross_compaction_tests;
mod rewind_synthetic_turn_tests;
mod session_thread_tests;
mod stop_cancelled_tests;
mod subagent_bash_permission_tests;
mod subagent_usage_fold_tests;
pub(crate) mod support;
mod truncation_recovery_tests;
mod turn_pipeline_v2_tests;
mod usage_categories_tests;
mod workflow_launch_tests;
