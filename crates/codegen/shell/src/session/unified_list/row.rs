use serde::Serialize;

use super::envelope::{FacetMap, SessionKind, SessionMetaEnvelope};
use super::facets::{FacetRegistry, NormalizedItem};
use crate::session::listing::SessionListing;

#[derive(Debug, Clone)]
pub struct UnifiedRow {
    pub kind: SessionKind,
    pub session: SessionListing,
    pub updated_at: Option<String>,
    pub facets: FacetMap,
}

impl UnifiedRow {
    fn envelope(kind: SessionKind, facets: FacetMap) -> RowMeta {
        RowMeta {
            session: SessionMetaEnvelope { kind, facets },
        }
    }

    pub fn into_session_list_row(self) -> SessionListRow {
        let UnifiedRow {
            kind,
            session,
            facets,
            updated_at: _,
        } = self;
        SessionListRow {
            session,
            meta: Self::envelope(kind, facets),
        }
    }

    pub fn into_session_info(self) -> SessionInfo {
        let UnifiedRow {
            kind,
            session,
            updated_at,
            facets,
        } = self;
        SessionInfo {
            session_id: session.session_id,
            cwd: session.cwd,
            title: Some(session.title),
            updated_at,
            meta: Self::envelope(kind, facets),
        }
    }

    pub(super) fn sort_timestamp(&self) -> Option<chrono::DateTime<chrono::FixedOffset>> {
        self.updated_at
            .as_deref()
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
    }
}

pub fn session_listing_to_row(session: SessionListing, reg: &FacetRegistry) -> UnifiedRow {
    let facets = reg.extract_all(&NormalizedItem::from_listing(&session));
    let updated_at = effective_local_ts(&session);
    UnifiedRow {
        kind: SessionKind::Build,
        session,
        updated_at,
        facets,
    }
}

fn effective_local_ts(m: &SessionListing) -> Option<String> {
    m.last_active_at
        .as_deref()
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .or_else(|| chrono::DateTime::parse_from_rfc3339(&m.updated_at).ok())
        .map(|dt| dt.to_rfc3339())
}

#[derive(Debug, Clone, Serialize)]
pub struct RowMeta {
    #[serde(rename = "grow/session")]
    pub session: SessionMetaEnvelope,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionListRow {
    #[serde(flatten)]
    pub session: SessionListing,
    #[serde(rename = "_meta")]
    pub meta: RowMeta,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub session_id: String,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(rename = "_meta")]
    pub meta: RowMeta,
}
