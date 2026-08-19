//! Pager snapshot of the canonical bundled Agent and Skill catalog.

use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct BundleState {
    pub has_cache: bool,
    pub version: String,
    pub agents: Vec<String>,
    pub skills: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BundleStatusResult {
    pub has_cache: bool,
    pub version: Option<String>,
    pub agents: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryGetResult {
    pub kind: String,
    pub name: String,
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_schema_is_exact() {
        let status: BundleStatusResult = serde_json::from_str(
            r#"{"hasCache":true,"version":"v2","agents":["review"],"skills":["commit"]}"#,
        )
        .unwrap();
        assert_eq!(status.agents, vec!["review"]);
        assert_eq!(status.skills, vec!["commit"]);
        assert!(
            serde_json::from_str::<BundleStatusResult>(
                r#"{"hasCache":true,"version":"v2","agents":[],"skills":[],"legacy":[]}"#
            )
            .is_err()
        );
    }

    #[test]
    fn agent_entry_schema_is_exact() {
        let entry: EntryGetResult =
            serde_json::from_str(r##"{"kind":"agent","name":"review","content":"# Review"}"##)
                .unwrap();
        assert_eq!(entry.kind, "agent");
    }
}
