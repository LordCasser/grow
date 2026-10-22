//! Ephemeral ownership projected from one fully validated history frontier.
//! This is display recovery data, never child execution/resume authority.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::SessionUpdate;
use crate::extensions::notification::{SessionNotification, SessionUpdate as GrowUpdate};
use crate::session::history::HistorySnapshot;

pub const DESCENDANT_OWNERSHIP_META_KEY: &str = "grow/descendantOwnership";

/// Only fields consumed by the Pager's spawn enrichment. In particular this
/// does not duplicate execution recipes, model bodies or unrelated spawn data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildReplaySpawn {
    pub subagent_id: String,
    pub prompt: String,
    pub child_cwd: String,
    pub worktree_path: Option<String>,
}

/// Owned by one load response (or one local read), not cached across reloads.
/// The cut/hash identify its validated source, not a promise that disk cannot
/// change afterward. The ACP server is the authority for response metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DescendantOwnership {
    version: u8,
    session_id: String,
    cut: u64,
    hash: String,
    spawns: BTreeMap<String, ChildReplaySpawn>,
}

impl DescendantOwnership {
    pub(crate) fn from_validated(
        session_id: &str,
        timeline: &chat_state::Timeline,
        history: &HistorySnapshot,
    ) -> Self {
        let spawns = timeline
            .events()
            .iter()
            .filter_map(|event| {
                let chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) = &event.kind
                else {
                    return None;
                };
                Some((
                    spawn.child_session_id.clone(),
                    ChildReplaySpawn {
                        subagent_id: spawn.subagent_id.clone(),
                        prompt: spawn.prompt.clone(),
                        child_cwd: spawn.child_cwd.clone(),
                        worktree_path: spawn.worktree_path.clone(),
                    },
                ))
            })
            .collect();
        Self {
            version: 1,
            session_id: session_id.to_owned(),
            cut: history.sources.len() as u64,
            hash: history.prefix_hash.clone(),
            spawns,
        }
    }

    /// Bound this optional optimization before adding it to a single ACP
    /// response (the transport has a 64 MiB line limit). Six bytes per source
    /// byte covers JSON escaping; fixed per-row/envelope costs are conservative.
    /// Larger roots use the independently validated local fallback.
    pub(crate) fn into_load_meta(self) -> Option<serde_json::Value> {
        const MAX_META_BYTES: usize = 1024 * 1024;
        let bound = self.spawns.iter().fold(
            512usize.saturating_add(self.session_id.len().saturating_mul(6)),
            |total, (child, spawn)| {
                [
                    child.as_str(),
                    spawn.subagent_id.as_str(),
                    spawn.prompt.as_str(),
                    spawn.child_cwd.as_str(),
                    spawn.worktree_path.as_deref().unwrap_or_default(),
                ]
                .into_iter()
                .fold(total.saturating_add(128), |bound, text| {
                    bound.saturating_add(text.len().saturating_mul(6))
                })
            },
        );
        (bound <= MAX_META_BYTES)
            .then(|| serde_json::to_value(self).expect("ownership snapshot serializes"))
    }

    /// Check the enclosing response identity and wire shape before using a
    /// present payload as authority. An absent or rejected payload falls back
    /// to an independent validated local read, never an assumed empty map.
    pub fn belongs_to(&self, session_id: &str) -> bool {
        self.version == 1
            && !session_id.is_empty()
            && self.session_id == session_id
            && self.hash.len() == 64
            && self.hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            && (self.cut != 0 || self.spawns.is_empty())
            && self
                .spawns
                .iter()
                .all(|(child, spawn)| !child.is_empty() && !spawn.subagent_id.is_empty())
    }

    pub fn into_spawns(self) -> BTreeMap<String, ChildReplaySpawn> {
        self.spawns
    }
}

/// Independently validate the fallback root instead of trusting UI rows.
/// Most ordinary loads supply the already validated ownership in their result.
pub fn load_descendant_ownership_at(
    session_id: &str,
    grow_home: &Path,
) -> io::Result<Option<DescendantOwnership>> {
    Ok(
        super::load_validated_session_at(session_id, grow_home)?.map(|snapshot| {
            DescendantOwnership::from_validated(session_id, &snapshot.timeline, &snapshot.history)
        }),
    )
}

/// One identity-pinned, fully validated read supplies both ownership and the
/// ordered child lifecycle/control projection. The full projector and its
/// rewind/cursor rules are shared with all existing history consumers.
pub fn load_descendant_replay_at(
    session_id: &str,
    grow_home: &Path,
) -> io::Result<Option<(DescendantOwnership, Vec<SessionNotification>)>> {
    let Some(snapshot) = super::load_validated_session_at(session_id, grow_home)? else {
        return Ok(None);
    };
    let ownership =
        DescendantOwnership::from_validated(session_id, &snapshot.timeline, &snapshot.history);
    let prepared = snapshot.history.into_prepare(
        &acp_transport::protocol::SessionId::new(session_id.to_owned()),
        &snapshot.timeline,
        None,
    )?;
    let lifecycle = prepared
        .updates
        .into_iter()
        .filter_map(|update| match update {
            SessionUpdate::Grow(notification)
                if matches!(
                    &notification.update,
                    GrowUpdate::SubagentSpawned { .. }
                        | GrowUpdate::SubagentProgress { .. }
                        | GrowUpdate::SubagentFinished { .. }
                        | GrowUpdate::ModelChanged { .. }
                        | GrowUpdate::AgentChanged { .. }
                ) =>
            {
                Some(*notification)
            }
            _ => None,
        })
        .collect();
    Ok(Some((ownership, lifecycle)))
}

#[cfg(test)]
mod tests;
