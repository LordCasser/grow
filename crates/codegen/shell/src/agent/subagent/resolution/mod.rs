//! Subagent definition, runtime, prompt, and resume resolution.
//!
//! Given a spawn request and a resolution context, this module resolves:
//!
//! - Effective runtime config (model, capability mode, isolation).
//! - Resume identity validation (agent type; model is soft-ignored).
//!
//! Definition discovery, gating, prompt context, runtime defaults, and
//! capability/depth tool policy are shared here. Model catalog selection and
//! workspace materialization remain host adapters.

pub(crate) mod context;
pub(crate) mod definition;
pub(crate) mod resume;
pub(crate) mod types;

pub(crate) use definition::{
    DefinitionResolutionContext, DefinitionValidationContext, HarnessToolsetContext,
    apply_child_tool_policy, apply_goal_object_tool_policy, apply_harness_toolset,
    available_agent_names, discover_agent_definition, gate_agent_definition,
    resolve_agent_definition, resolve_goal_stage_definition, resolve_runtime_config,
    subagent_harness_flavor_is_representable, validate_agent_name,
};
pub(crate) use resume::validate_resume_identity;
pub(crate) use types::{ResolutionError, ResumeSourceData};
