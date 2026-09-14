/// Identifies the type of client connecting to the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum ClientType {
    #[default]
    #[serde(rename = "generic")]
    Generic,
    #[serde(rename = "grow-tui")]
    GrowTUI,
    #[serde(rename = "grow-web")]
    GrowWeb,
    #[serde(rename = "nebula")]
    Nebula,
    #[serde(rename = "extension")]
    Extension,
    #[serde(rename = "grow-pager")]
    GrowPager,
}

impl ClientType {
    pub fn user_agent_label(&self) -> &'static str {
        match self {
            Self::Generic => "grow-shell",
            Self::GrowTUI => "grow-tui",
            Self::GrowWeb => "grow-web",
            Self::Nebula => "nebula",
            Self::Extension => "grow-code-extension",
            Self::GrowPager => "grow-pager",
        }
    }

    pub fn from_client_identifier(id: Option<&str>) -> Self {
        match id {
            Some("grow-web") => Self::GrowWeb,
            Some("nebula") => Self::Nebula,
            Some("grow-code-extension") => Self::Extension,
            Some("grow-pager") => Self::GrowPager,
            _ => Self::Generic,
        }
    }

    pub fn feedback_label(&self) -> &'static str {
        match self {
            Self::GrowTUI | Self::GrowPager => "tui",
            Self::GrowWeb => "web",
            Self::Nebula => "nebula",
            Self::Extension => "extension",
            Self::Generic => "agent",
        }
    }
}

/// Identity of the client that originated a request, used for User-Agent rendering.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OriginClientInfo {
    pub product: String,
    pub version: Option<String>,
}
