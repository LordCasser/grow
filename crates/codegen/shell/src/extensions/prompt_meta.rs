use serde::{Deserialize, Serialize};

/// Typed metadata for a prompt `TextContent._meta` field.
///
/// Replaces ad-hoc `serde_json::json!()` construction on the sender side
/// and manual `.get()` parsing on the receiver side.
///
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptBlockMeta {
    /// Direct bash command to execute (bypasses agent loop).
    pub bash_command: String,
}

impl PromptBlockMeta {
    /// Create meta for a direct bash command.
    pub fn bash(command: impl Into<String>) -> Self {
        Self {
            bash_command: command.into(),
        }
    }

    /// Try to parse from a freeform `_meta` map.
    pub fn from_value(value: &agent_client_protocol::schema::v1::Meta) -> Option<Self> {
        serde_json::from_value(serde_json::Value::Object(value.clone())).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_roundtrip_serde() {
        let meta = PromptBlockMeta::bash("ls -la");
        let json = serde_json::to_value(&meta).unwrap();
        let parsed: PromptBlockMeta = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.bash_command, "ls -la");
    }

    #[test]
    fn from_value_parses_canonical_shape() {
        let val = serde_json::json!({"bash_command": "ls"});
        let meta = PromptBlockMeta::from_value(val.as_object().unwrap()).unwrap();
        assert_eq!(meta.bash_command, "ls");
    }

    #[test]
    fn from_value_unrelated_meta() {
        let val = serde_json::json!({"other": 1});
        let meta = PromptBlockMeta::from_value(val.as_object().unwrap());
        assert!(meta.is_none());
    }

    #[test]
    fn from_value_empty_object() {
        let val = serde_json::json!({});
        let meta = PromptBlockMeta::from_value(val.as_object().unwrap());
        assert!(meta.is_none());
    }

    #[test]
    fn from_value_rejects_unknown_fields() {
        let val = serde_json::json!({"bash_command":"ls","other":1});
        assert!(PromptBlockMeta::from_value(val.as_object().unwrap()).is_none());
    }
}
