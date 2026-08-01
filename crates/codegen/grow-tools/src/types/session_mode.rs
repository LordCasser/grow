//! Canonical session-mode enum shared between the agent and pager.
//!
//! ACP carries the mode as an opaque [`acp::SessionModeId`] (`Arc<str>`).
//! This enum is the typed counterpart both crates parse into / serialize
//! out of, so plan-mode state is driven by the closed set of variants
//! instead of by ad-hoc string matching at each boundary.

/// Wire representation is the snake-cased variant name via [`strum`]
/// (`ask`, `plan`, `workflow`, `deep_research`, `goal`); `Default` uses the
/// per-variant override `normal` (its variant name `default` is not a valid
/// wire id and is strictly rejected by [`Self::try_from_id`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum SessionMode {
    #[strum(serialize = "normal")]
    Default,
    Plan,
    Ask,
    Workflow,
    DeepResearch,
    Goal,
}

impl SessionMode {
    /// Parse a Behavior wire id without conflating an unknown id with Normal.
    pub fn try_from_id(id: &str) -> Option<Self> {
        id.parse().ok()
    }

    /// Parse from the wire id. Unknown ids fall back to [`SessionMode::Default`].
    /// UI display code may use this for stale remote state; transition gateways
    /// must use [`Self::try_from_id`] and reject unknown ids.
    pub fn from_id(id: &str) -> Self {
        Self::try_from_id(id).unwrap_or(Self::Default)
    }

    /// The canonical wire id for this mode (snake_case).
    pub fn as_id(&self) -> &'static str {
        self.into()
    }

    /// Human-readable display label, matching the pager's Behavior picker
    /// labels. Wire ids (`normal`, `deep_research`, …) are protocol
    /// identifiers; user-facing messages must use this label so "Normal"
    /// never reads as "normal".
    pub fn display_label(&self) -> &'static str {
        match self {
            Self::Default => "Normal",
            Self::Plan => "Plan",
            Self::Ask => "Clarify",
            Self::Workflow => "Static Workflow",
            Self::DeepResearch => "Deep Research",
            Self::Goal => "Goal",
        }
    }

    pub fn is_plan(&self) -> bool {
        matches!(self, Self::Plan)
    }

    pub fn behavior(&self) -> Option<xai_tool_types::BehaviorId> {
        match self {
            Self::Default => None,
            Self::Ask => Some(xai_tool_types::BehaviorId::Clarify),
            Self::Plan => Some(xai_tool_types::BehaviorId::Plan),
            Self::Workflow => Some(xai_tool_types::BehaviorId::Workflow),
            Self::DeepResearch => Some(xai_tool_types::BehaviorId::DeepResearch),
            Self::Goal => Some(xai_tool_types::BehaviorId::Goal),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_known_ids() {
        for &id in &["normal", "plan", "ask", "workflow", "deep_research", "goal"] {
            let mode = SessionMode::from_id(id);
            assert_eq!(mode.as_id(), id, "round-trip failed for {id}");
        }
    }

    #[test]
    fn unknown_id_falls_back_to_default() {
        assert_eq!(SessionMode::from_id("browser_use"), SessionMode::Default);
        assert_eq!(SessionMode::from_id(""), SessionMode::Default);
        assert_eq!(SessionMode::from_id("PLAN"), SessionMode::Default); // case-sensitive
        // "default" was the pre-unification wire id; the display layer still
        // falls back to Normal for stale remote state, but it is not parseable.
        assert_eq!(SessionMode::from_id("default"), SessionMode::Default);
    }

    #[test]
    fn transition_parser_rejects_unknown_ids() {
        assert_eq!(SessionMode::try_from_id("goal"), Some(SessionMode::Goal));
        assert_eq!(SessionMode::try_from_id("browser_use"), None);
        assert_eq!(SessionMode::try_from_id("default"), None); // old id, strictly rejected
    }

    #[test]
    fn is_plan_only_for_plan_variant() {
        assert!(SessionMode::Plan.is_plan());
        assert!(!SessionMode::Default.is_plan());
        assert!(!SessionMode::Ask.is_plan());
        assert!(!SessionMode::Workflow.is_plan());
        assert!(!SessionMode::DeepResearch.is_plan());
        assert!(!SessionMode::Goal.is_plan());
    }

    #[test]
    fn display_labels_match_pager_picker_labels() {
        assert_eq!(SessionMode::Default.display_label(), "Normal");
        assert_eq!(SessionMode::Plan.display_label(), "Plan");
        assert_eq!(SessionMode::Ask.display_label(), "Clarify");
        assert_eq!(SessionMode::Workflow.display_label(), "Static Workflow");
        assert_eq!(SessionMode::DeepResearch.display_label(), "Deep Research");
        assert_eq!(SessionMode::Goal.display_label(), "Goal");
    }
}
