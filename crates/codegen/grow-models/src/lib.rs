//! Model configuration constants shared across crates.
//!
//! Runtime model configuration comes exclusively from
//! `[provider.<id>.models.<id>]`; this crate deliberately ships no catalog.

/// No compiled model catalog is shipped.
pub const DEFAULT_MODELS_JSON: &str = r#"{"models":[]}"#;

pub fn default_model() -> &'static str {
    ""
}

pub fn default_image_description_model() -> &'static str {
    ""
}

pub fn default_session_summary_model() -> &'static str {
    ""
}
