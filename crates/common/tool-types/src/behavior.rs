use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The user-facing protocol a primary Agent follows to advance a goal.
///
/// Behaviors are session state. They do not select an Agent role, grant tools,
/// or propagate to delegated child Agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorId {
    #[default]
    Normal,
    Clarify,
    Plan,
    Workflow,
    DeepResearch,
    Goal,
}

/// Shell-authored availability of one Behavior transition target.
///
/// This is a control-plane projection, not a second state machine. Clients
/// may use it to render choices, but the Session actor remains authoritative
/// and revalidates every requested transition against a fresh snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorAvailabilityEntry {
    pub behavior: BehaviorId,
    /// Unsupported Behaviors are hidden rather than rendered as a permanently
    /// disabled choice. Runtime conflicts keep this `true` and explain why the
    /// otherwise-supported choice is temporarily unavailable.
    pub supported: bool,
    pub disposition: BehaviorAvailabilityDisposition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorAvailabilityDisposition {
    Available,
    ConfirmationRequired,
    Unavailable,
}

/// Atomic Behavior control projection published by the Shell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorAvailability {
    pub current: BehaviorId,
    pub choices: Vec<BehaviorAvailabilityEntry>,
}

impl BehaviorAvailability {
    pub fn choice(&self, behavior: BehaviorId) -> Option<&BehaviorAvailabilityEntry> {
        self.choices.iter().find(|entry| entry.behavior == behavior)
    }
}

impl BehaviorId {
    /// Behaviors with session-owned runtimes that conflict with public
    /// Workflow runs.
    pub const fn owns_special_runtime(self) -> bool {
        matches!(self, Self::Plan | Self::DeepResearch | Self::Goal)
    }
    pub fn try_from_id(id: &str) -> Option<Self> {
        Some(match id {
            "normal" => Self::Normal,
            "ask" => Self::Clarify,
            "plan" => Self::Plan,
            "workflow" => Self::Workflow,
            "deep_research" => Self::DeepResearch,
            "goal" => Self::Goal,
            _ => return None,
        })
    }

    pub fn as_id(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Clarify => "ask",
            Self::Plan => "plan",
            Self::Workflow => "workflow",
            Self::DeepResearch => "deep_research",
            Self::Goal => "goal",
        }
    }

    pub fn display_label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Clarify => "Clarify",
            Self::Plan => "Plan",
            Self::Workflow => "Workflow",
            Self::DeepResearch => "Deep Research",
            Self::Goal => "Goal",
        }
    }

    pub fn is_plan(self) -> bool {
        self == Self::Plan
    }
}

impl std::fmt::Display for BehaviorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_id())
    }
}
