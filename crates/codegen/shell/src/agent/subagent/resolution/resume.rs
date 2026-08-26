//! Resume identity validation for subagents.
//!
//! The source agent type is stable across resume. Model overrides are
//! soft-ignored because the source model is pinned by the host.

use super::types::ResumeSourceData;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResumeValidationError {
    #[error(
        "Cannot resume with subagent_type '{requested}': source subagent was '{source_value}'. \
         Resumed sessions must use the same subagent type as the source."
    )]
    TypeMismatch {
        requested: String,
        source_value: String,
    },
}

pub fn validate_resume_identity(
    requested_type: &str,
    source: &ResumeSourceData,
) -> Result<(), ResumeValidationError> {
    if requested_type != source.subagent_type {
        return Err(ResumeValidationError::TypeMismatch {
            requested: requested_type.to_owned(),
            source_value: source.subagent_type.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(subagent_type: &str) -> ResumeSourceData {
        ResumeSourceData {
            subagent_id: "source-id".into(),
            subagent_type: subagent_type.into(),
            model_id: "grow-3".into(),
            model_transport_key: sampling_types::ModelImageInputKey::new(
                "grow-3",
                "responses",
                "test-endpoint",
            ),
            reasoning_effort: None,
            child_cwd: "/workspace".into(),
            worktree_path: None,
            snapshot_ref: None,
            child_session_id: "child-session".into(),
        }
    }

    #[test]
    fn matching_type_is_valid() {
        assert!(validate_resume_identity("explore", &source("explore")).is_ok());
    }

    #[test]
    fn mismatched_type_is_rejected() {
        let error = validate_resume_identity("explore", &source("general-purpose"))
            .expect_err("type mismatch");
        assert!(matches!(error, ResumeValidationError::TypeMismatch { .. }));
    }
}
