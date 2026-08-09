//! Subagent definition, runtime, prompt, and resume resolution.
//!
//! Given a spawn request and a resolution context (roles, personas, parent
//! state), this module resolves:
//!
//! - Effective runtime config (model, persona, capability mode, isolation)
//!   via precedence: explicit override > role > persona > parent.
//! - Persona instruction loading (inline `instructions` + `instructions_file`).
//! - Role prompt file loading.
//! - Resume identity validation (type/persona match checks; model is soft-ignored).
//!
//! Definition discovery, gating, prompt context, runtime defaults, and
//! capability/depth tool policy are shared here. Model catalog selection and
//! workspace materialization remain host adapters.

pub(crate) mod config;
pub(crate) mod context;
pub(crate) mod definition;
pub(crate) mod overrides;
pub(crate) mod resume;
pub(crate) mod types;

pub(crate) use definition::{
    DefinitionResolutionContext, DefinitionValidationContext, HarnessToolsetContext,
    apply_child_tool_policy, apply_goal_object_tool_policy, apply_harness_toolset,
    available_agent_names, discover_agent_definition, gate_agent_definition,
    resolve_agent_definition, resolve_goal_stage_definition, resolve_runtime_config,
    subagent_harness_flavor_is_representable, validate_agent_name,
};
pub(crate) use overrides::{intersect_capability_modes, resolve_effective_overrides};
pub(crate) use resume::validate_resume_identity;
pub(crate) use types::{ResolutionError, ResumeSourceData};
