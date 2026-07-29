use std::cmp::Reverse;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use super::row::UnifiedRow;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct Cursor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    boundary: Option<BoundaryKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BoundaryKey {
    updated_at: String,
    session_id: String,
}

impl Cursor {
    pub fn decode(raw: Option<&str>) -> Self {
        raw.filter(|value| !value.is_empty())
            .and_then(|value| {
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(value)
                    .ok()
            })
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn encode(&self) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(self).unwrap_or_default())
    }
}

pub(super) fn paginate(
    rows: Vec<UnifiedRow>,
    cursor: &Cursor,
    limit: usize,
) -> (Vec<UnifiedRow>, Option<Cursor>) {
    let mut keyed: Vec<_> = rows
        .into_iter()
        .map(|row| (row_sort_key(&row), row))
        .collect();
    if let Some(boundary) = &cursor.boundary {
        let boundary = boundary_sort_key(boundary);
        keyed.retain(|(key, _)| key > &boundary);
    }
    keyed.sort_by(|(left, _), (right, _)| left.cmp(right));

    let has_more = keyed.len() > limit;
    keyed.truncate(limit);
    let rows: Vec<_> = keyed.into_iter().map(|(_, row)| row).collect();
    let next = has_more.then(|| Cursor {
        boundary: rows.last().map(boundary_of),
    });
    (rows, next)
}

type SortKey = (
    Reverse<Option<chrono::DateTime<chrono::FixedOffset>>>,
    String,
);

fn row_sort_key(row: &UnifiedRow) -> SortKey {
    (Reverse(row.sort_timestamp()), row.legacy.session_id.clone())
}

fn boundary_sort_key(boundary: &BoundaryKey) -> SortKey {
    (
        Reverse(chrono::DateTime::parse_from_rfc3339(&boundary.updated_at).ok()),
        boundary.session_id.clone(),
    )
}

fn boundary_of(row: &UnifiedRow) -> BoundaryKey {
    BoundaryKey {
        updated_at: row.updated_at.clone().unwrap_or_default(),
        session_id: row.legacy.session_id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::merge::MergedSession;
    use crate::session::unified_list::{facet_registry, merged_session_to_row};

    fn row(id: &str, timestamp: &str) -> UnifiedRow {
        merged_session_to_row(
            MergedSession {
                session_id: id.to_owned(),
                summary: id.to_owned(),
                updated_at: timestamp.to_owned(),
                created_at: timestamp.to_owned(),
                cwd: "/repo".to_owned(),
                source: "local".to_owned(),
                num_messages: 1,
                last_active_at: Some(timestamp.to_owned()),
                ..MergedSession::default()
            },
            facet_registry(),
        )
    }

    #[test]
    fn cursor_walk_is_stable_and_has_no_duplicates() {
        let input = vec![
            row("new", "2026-06-03T00:00:00Z"),
            row("mid", "2026-06-02T00:00:00Z"),
            row("old", "2026-06-01T00:00:00Z"),
        ];
        let (first, next) = paginate(input.clone(), &Cursor::default(), 2);
        assert_eq!(
            first
                .iter()
                .map(|row| row.legacy.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["new", "mid"]
        );
        let (second, next) = paginate(input, &next.unwrap(), 2);
        assert_eq!(
            second
                .iter()
                .map(|row| row.legacy.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["old"]
        );
        assert!(next.is_none());
    }
}
