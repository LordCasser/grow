//! Agent builder, definition parsing, and system prompt assembly.
//!
//! This crate extracts a first-class `Agent` type from `shell`.
//! An `Agent` binds one definition to rendered prompt layers and a finalized
//! tool bridge. Provider, permission, reminders, and compaction remain host
//! session concerns.

pub mod agent;
pub mod builder;
pub mod config;
pub mod discovery;
pub mod error;
pub mod plugins;
pub mod prompt;
pub mod repo;
mod resource_roots;
pub mod timing;

pub use agent::Agent;
pub use builder::AgentBuilder;
pub use config::AgentDefinition;
pub use config::preset_names;
pub use config::toolset_for_preset;
pub use config::workspace_grow_build_toolset;
pub use error::AgentBuildError;
pub use prompt::context::{DEFAULT_SYSTEM_PROMPT_LABEL, PromptContext};
