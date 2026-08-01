//! Session-owned extensions that react to lifecycle edges.

#[path = "extensions/idle_prompt.rs"]
mod idle_prompt;

pub(crate) use idle_prompt::IdlePromptExtension;
