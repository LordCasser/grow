//! Validated identifiers shared by Grow's in-process tool contracts.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum IdError {
    #[error("identifier must not be empty")]
    Empty,
    #[error("identifier {value:?} has invalid format")]
    InvalidFormat { value: String },
}

fn ensure_non_empty(value: &str) -> Result<(), IdError> {
    if value.is_empty() {
        Err(IdError::Empty)
    } else {
        Ok(())
    }
}

macro_rules! opaque_id {
    ($(#[$meta:meta])* $name:ident $(, extra_validator = $validator:path)?) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, IdError> {
                let value = value.into();
                ensure_non_empty(&value)?;
                $($validator(&value)?;)?
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = IdError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(deserializer)?;
                Self::new(raw).map_err(serde::de::Error::custom)
            }
        }
    };
}

opaque_id!(
    /// End-to-end identifier for one local tool invocation.
    ToolCallId
);

impl ToolCallId {
    pub fn new_v7() -> Self {
        Self(uuid::Uuid::now_v7().to_string())
    }
}

fn validate_tool_id(value: &str) -> Result<(), IdError> {
    let mut parts = value.splitn(3, ':');
    let Some(first) = parts.next() else {
        return Err(IdError::InvalidFormat {
            value: value.to_owned(),
        });
    };
    let valid_segment = |segment: &str| {
        !segment.is_empty()
            && segment
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    };
    let valid = match (parts.next(), parts.next()) {
        (None, _) => valid_segment(first),
        (Some(second), None) => valid_segment(first) && valid_segment(second),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(IdError::InvalidFormat {
            value: value.to_owned(),
        })
    }
}

opaque_id!(
    /// Tool identifier in `{namespace}:{name}` or `{name}` form.
    ToolId,
    extra_validator = validate_tool_id
);
