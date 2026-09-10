//! Observed provider metadata. This is never a host completion decision.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderTerminal {
    ChatCompletions {
        finish_reason: String,
    },
    Messages {
        stop_reason: String,
        stop_sequence: Option<String>,
    },
    Responses {
        event: String,
        status: String,
        incomplete_reason: Option<String>,
    },
}

impl ProviderTerminal {
    /// A display label only; control flow uses the independent typed StopReason.
    pub fn display_reason(&self) -> String {
        match self {
            Self::ChatCompletions { finish_reason } => finish_reason.clone(),
            Self::Messages { stop_reason, .. } => stop_reason.clone(),
            Self::Responses {
                status,
                incomplete_reason,
                ..
            } => match incomplete_reason {
                Some(reason) => format!("{status}:{reason}"),
                None => status.clone(),
            },
        }
    }

    pub fn stop_sequence(&self) -> Option<&str> {
        match self {
            Self::Messages { stop_sequence, .. } => stop_sequence.as_deref(),
            _ => None,
        }
    }
}
