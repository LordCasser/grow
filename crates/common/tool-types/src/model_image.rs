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
