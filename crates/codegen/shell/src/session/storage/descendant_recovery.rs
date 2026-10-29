//! Ephemeral ownership projected from one fully validated history frontier.
//! This is display recovery data, never child execution/resume authority.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::SessionUpdate;
use super::jsonl::JsonlStorageAdapter;
use crate::extensions::notification::{SessionNotification, SessionUpdate as GrowUpdate};
use crate::session::history::HistorySnapshot;

pub const DESCENDANT_OWNERSHIP_META_KEY: &str = "grow/descendantOwnership";

/// Exact validated facts used by Pager presentation and the existing
/// cross-entity Seed/Result validators. This remains display data, not a grant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildReplaySpawn {
    pub spawn: chat_state::SubagentSpawnEvent,
    pub parent_spawn_seq: u64,
    pub terminal: Option<chat_state::SubagentTerminalEvent>,
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
        let mut terminals = BTreeMap::new();
        for event in timeline.events() {
            if let chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Ended(
                terminal,
            )) = &event.kind
                && terminal.result_ref.is_some()
            {
                terminals
                    .entry((
                        terminal.subagent_id.as_str(),
                        terminal.child_session_id.as_str(),
                    ))
                    .or_insert(terminal);
            }
        }
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
                if spawn.security_parent_session_id != session_id {
                    return None;
                }
                let terminal = terminals
                    .get(&(spawn.subagent_id.as_str(), spawn.child_session_id.as_str()))
                    .map(|terminal| (**terminal).clone());
                Some((
                    spawn.child_session_id.clone(),
                    ChildReplaySpawn {
                        spawn: (**spawn).clone(),
                        parent_spawn_seq: event.seq.get(),
                        terminal,
                    },
                ))
            })
            .collect();
        Self {
            version: 2,
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
                let spawn_len = serde_json::to_vec(spawn)
                    .expect("validated ownership spawn serializes")
                    .len();
                total
                    .saturating_add(64)
                    .saturating_add(child.len().saturating_mul(6))
                    .saturating_add(spawn_len)
            },
        );
        (bound <= MAX_META_BYTES)
            .then(|| serde_json::to_value(self).expect("ownership snapshot serializes"))
    }

    /// Check the enclosing response identity and wire shape before using a
    /// present payload as authority. An absent or rejected payload falls back
    /// to an independent validated local read, never an assumed empty map.
    pub fn belongs_to(&self, session_id: &str) -> bool {
        self.version == 2
            && !session_id.is_empty()
            && self.session_id == session_id
            && self.hash.len() == 64
            && self.hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            && (self.cut != 0 || self.spawns.is_empty())
            && self.spawns.iter().all(|(child, spawn)| {
                !child.is_empty()
                    && child == &spawn.spawn.child_session_id
                    && !spawn.spawn.subagent_id.is_empty()
                    && spawn.spawn.security_parent_session_id == session_id
                    && spawn.terminal.as_ref().is_none_or(|terminal| {
                        terminal.subagent_id == spawn.spawn.subagent_id
                            && terminal.child_session_id == *child
                            && terminal.result_ref.is_some()
                    })
            })
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
    Ok(Some(project_descendant_replay(session_id, snapshot)?))
}

/// Load a child source's lifecycle presentation only after validating its
/// canonical Seed/Result link against the already-validated owner projection.
/// The child is read once; breadth-first replay does not reopen its parent.
pub fn load_descendant_child_replay_at(
    child_session_id: &str,
    parent_session_id: &str,
    owner_spawn: &ChildReplaySpawn,
    grow_home: &Path,
) -> io::Result<Option<(DescendantOwnership, Vec<SessionNotification>)>> {
    let storage = JsonlStorageAdapter::with_root(grow_home.to_path_buf());
    let Some(child) = storage.open_session_by_id_shared_read(child_session_id)? else {
        return Ok(None);
    };
    let child_snapshot = child.validated_timeline(child_session_id)?;
    validate_child_parent_link(
        child_session_id,
        parent_session_id,
        owner_spawn,
        child.summary(),
        &child_snapshot.timeline,
    )?;
    Ok(Some(project_descendant_replay(
        child_session_id,
        child_snapshot,
    )?))
}

fn project_descendant_replay(
    session_id: &str,
    snapshot: super::jsonl::ValidatedTimeline,
) -> io::Result<(DescendantOwnership, Vec<SessionNotification>)> {
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
    Ok((ownership, lifecycle))
}

fn validate_child_parent_link(
    child_session_id: &str,
    parent_session_id: &str,
    owner_spawn: &ChildReplaySpawn,
    child_summary: &super::Summary,
    child_timeline: &chat_state::Timeline,
) -> io::Result<()> {
    if owner_spawn.spawn.child_session_id != child_session_id
        || owner_spawn.spawn.security_parent_session_id != parent_session_id
        || child_summary.parent_session_id.as_deref() != Some(parent_session_id)
        || !child_summary
            .session_kind
            .as_deref()
            .is_some_and(|kind| kind.starts_with("subagent"))
    {
        return Err(invalid(format!(
            "child '{child_session_id}' summary disagrees with its lifecycle owner"
        )));
    }
    let spawn_seq = chat_state::EventSeq::new(owner_spawn.parent_spawn_seq);
    child_timeline
        .validate_subagent_seed_link(parent_session_id, spawn_seq, &owner_spawn.spawn)
        .map_err(|error| invalid(error.to_string()))?;
    if let Some(terminal) = &owner_spawn.terminal {
        child_timeline
            .validate_subagent_result_link(
                parent_session_id,
                spawn_seq,
                &owner_spawn.spawn,
                terminal,
            )
            .map_err(|error| invalid(error.to_string()))?;
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests;
