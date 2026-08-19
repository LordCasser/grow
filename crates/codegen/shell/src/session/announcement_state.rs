//! Announcement tracking projected from the canonical session Timeline.
//!
//! Tracks which MCP servers and skills have already been announced
//! via `<system-reminder>` messages so that resumed sessions don't
//! re-inject duplicate listings.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const TIMELINE_ANNOUNCEMENT_VERSION: u32 = 1;
const TIMELINE_ANNOUNCEMENT_SCOPE: &str = "runtime";
const TIMELINE_ANNOUNCEMENT_NAME: &str = "announcement_snapshot";

#[derive(Serialize, Deserialize)]
struct TimelineAnnouncementSnapshot {
    version: u32,
    state: AnnouncementState,
}

/// Runtime announcement tracking state.
///
/// The latest matching Timeline observation is the only recovery source. The
/// existing delta/fingerprint comparison logic then handles changes without
/// re-injecting unchanged MCP or skill listings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnouncementState {
    /// Fingerprints of MCP servers that have been announced.
    /// Maps server_name → `McpServerFingerprint`.
    ///
    /// The hash values use FNV-1a (deterministic, portable) so that
    /// persisted fingerprints remain valid across Rust versions, build
    /// profiles, and CPU architectures.
    pub mcp_server_fingerprints: HashMap<String, McpServerFingerprint>,

    /// Names of skills already announced via system-reminder.
    /// Uses the skill's `dedup_key()` (which is the skill name).
    pub announced_skill_names: HashSet<String>,
}

impl AnnouncementState {
    pub fn timeline_kind(&self) -> std::io::Result<chat_state::TimelineEventKind> {
        let data = serde_json::to_value(TimelineAnnouncementSnapshot {
            version: TIMELINE_ANNOUNCEMENT_VERSION,
            state: self.clone(),
        })
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        Ok(chat_state::TimelineEventKind::Observation(
            chat_state::ObservationEvent {
                scope: TIMELINE_ANNOUNCEMENT_SCOPE.into(),
                name: TIMELINE_ANNOUNCEMENT_NAME.into(),
                turn: None,
                step: None,
                data: Some(data),
            },
        ))
    }

    pub fn latest_from_timeline(
        events: &[chat_state::TimelineEvent],
    ) -> std::io::Result<Option<Self>> {
        let mut latest = None;
        for observation in events.iter().filter_map(|event| match &event.kind {
            chat_state::TimelineEventKind::Observation(observation)
                if observation.scope == TIMELINE_ANNOUNCEMENT_SCOPE
                    && observation.name == TIMELINE_ANNOUNCEMENT_NAME =>
            {
                Some(observation)
            }
            _ => None,
        }) {
            let snapshot: TimelineAnnouncementSnapshot =
                serde_json::from_value(observation.data.clone().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Timeline announcement observation has no payload",
                    )
                })?)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
            if snapshot.version != TIMELINE_ANNOUNCEMENT_VERSION {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "unsupported Timeline announcement version {}; expected {}",
                        snapshot.version, TIMELINE_ANNOUNCEMENT_VERSION
                    ),
                ));
            }
            latest = Some(snapshot.state);
        }
        Ok(latest)
    }
}

/// Serializable MCP server fingerprint for persistence.
///
/// This is the serializable counterpart of the in-memory
/// `ServerFingerprint` type alias `(usize, u64, u64)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerFingerprint {
    pub tool_count: usize,
    pub description_hash: u64,
    pub tool_names_hash: u64,
}

/// Convert from in-memory fingerprint map to persistable map.
pub fn to_persisted_fingerprints(
    in_memory: &HashMap<String, (usize, u64, u64)>,
) -> HashMap<String, McpServerFingerprint> {
    in_memory
        .iter()
        .map(|(name, &(tc, dh, tnh))| {
            (
                name.clone(),
                McpServerFingerprint {
                    tool_count: tc,
                    description_hash: dh,
                    tool_names_hash: tnh,
                },
            )
        })
        .collect()
}

/// Convert from persisted fingerprint map to in-memory map.
pub fn from_persisted_fingerprints(
    persisted: &HashMap<String, McpServerFingerprint>,
) -> HashMap<String, (usize, u64, u64)> {
    persisted
        .iter()
        .map(|(name, fp)| {
            (
                name.clone(),
                (fp.tool_count, fp.description_hash, fp.tool_names_hash),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_round_trip_uses_latest_snapshot() {
        let state = AnnouncementState {
            mcp_server_fingerprints: HashMap::from([(
                "github".to_string(),
                McpServerFingerprint {
                    tool_count: 5,
                    description_hash: 12345678,
                    tool_names_hash: 87654321,
                },
            )]),
            announced_skill_names: HashSet::from(["commit".to_string(), "review".to_string()]),
        };
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(AnnouncementState::default().timeline_kind().unwrap())
            .unwrap();
        timeline.record(state.timeline_kind().unwrap()).unwrap();

        let loaded = AnnouncementState::latest_from_timeline(timeline.events())
            .unwrap()
            .unwrap();
        assert_eq!(loaded, state);
        let fp = &loaded.mcp_server_fingerprints["github"];
        assert_eq!(fp.tool_count, 5);
        assert_eq!(fp.description_hash, 12345678);
        assert_eq!(fp.tool_names_hash, 87654321);
    }

    #[test]
    fn unsupported_timeline_snapshot_version_fails_closed() {
        let mut kind = AnnouncementState::default().timeline_kind().unwrap();
        let chat_state::TimelineEventKind::Observation(observation) = &mut kind else {
            unreachable!();
        };
        observation.data.as_mut().unwrap()["version"] = serde_json::json!(2);
        let mut timeline = chat_state::Timeline::default();
        timeline.record(kind).unwrap();

        let error = AnnouncementState::latest_from_timeline(timeline.events()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn fingerprint_conversion_round_trip() {
        let in_memory: HashMap<String, (usize, u64, u64)> =
            HashMap::from([("srv".to_string(), (3, 111, 222))]);
        let persisted = to_persisted_fingerprints(&in_memory);
        let back = from_persisted_fingerprints(&persisted);
        assert_eq!(in_memory, back);
    }
}
