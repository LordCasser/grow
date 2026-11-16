use serde::{Deserialize, Serialize};

/// Stable logical Turn identity, encoded as a string to preserve all u64 values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TurnId(#[serde(with = "turn_id_serde")] pub u64);

mod turn_id_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

/// Captured at work admission; prompt IDs alone are client-controlled and reusable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsagePromptOwner {
    pub turn_id: TurnId,
    pub prompt_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_id_retains_string_encoding_and_rejects_invalid_values() {
        for value in [0, 1, u64::MAX] {
            let encoded = serde_json::to_value(TurnId(value)).unwrap();
            assert_eq!(encoded, value.to_string());
            assert_eq!(
                serde_json::from_value::<TurnId>(encoded).unwrap(),
                TurnId(value)
            );
        }
        for invalid in [
            serde_json::json!(1),
            serde_json::json!("-1"),
            serde_json::json!("18446744073709551616"),
            serde_json::json!("bad"),
            serde_json::Value::Null,
        ] {
            assert!(serde_json::from_value::<TurnId>(invalid).is_err());
        }
    }
}
