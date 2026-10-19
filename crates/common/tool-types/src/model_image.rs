//! Stable identity for a model runtime's image-input capability.

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelImageInputKey {
    model: String,
    api_backend: String,
    endpoint_fingerprint: String,
}

impl ModelImageInputKey {
    pub fn new(
        model: impl Into<String>,
        api_backend: impl Into<String>,
        endpoint_fingerprint: impl Into<String>,
    ) -> Self {
        Self {
            model: model.into(),
            api_backend: api_backend.into(),
            endpoint_fingerprint: endpoint_fingerprint.into(),
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Stable representation for string-keyed capability persistence.
    pub fn storage_key(&self) -> String {
        serde_json::to_string(self).expect("image input identity contains only strings")
    }

    pub fn is_valid(&self) -> bool {
        [
            self.model.as_str(),
            self.api_backend.as_str(),
            self.endpoint_fingerprint.as_str(),
        ]
        .into_iter()
        .all(|value| !value.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::ModelImageInputKey;

    #[test]
    fn storage_identity_round_trips_escaped_fields_without_collisions() {
        let key = ModelImageInputKey::new("model\"/图", "messages", "fingerprint");
        assert_eq!(
            serde_json::from_str::<ModelImageInputKey>(&key.storage_key()).unwrap(),
            key
        );
        assert_eq!(key.storage_key(), key.clone().storage_key());
        assert_ne!(
            key.storage_key(),
            ModelImageInputKey::new("model", "messages", "fingerprint").storage_key()
        );
    }
}
