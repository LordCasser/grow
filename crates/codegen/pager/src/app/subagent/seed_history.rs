//! A new child's inherited display ends at its committed seed. Live updates
//! can enter the ordinary tracker while this immutable prefix is projected.
use std::{path::PathBuf, sync::Arc};

use crate::app::actions::Effect;
use crate::app::agent_view::AgentView;
use crate::app::root::{ActiveView, AppView};
use crate::app::session::AgentId;
use crate::scrollback::block::RenderBlock;
use crate::scrollback::blocks::NoticeBlock;
use crate::scrollback::state::ScrollbackState;
use crate::transcript_projection::TranscriptProjection;
use shell::session::storage::transcript::TranscriptEvent;

#[derive(Debug, Clone)]
pub struct ChildSeedHistoryRequest {
    root: AgentId,
    root_binding: u32,
    child_binding: u32,
    id: uuid::Uuid,
    pub(crate) child_session_id: String,
    pub(crate) bytes: u64,
    pub(crate) grow_home: PathBuf,
    appearance: Arc<crate::appearance::AppearanceConfig>,
}

#[derive(Debug)]
pub(crate) enum ChildSeedHistory {
    Loading(uuid::Uuid),
    ReadyAfterReload(u64, Result<Box<ScrollbackState>, String>),
    Failed(u64),
    Retry(u64),
}

impl ChildSeedHistory {
    pub(crate) fn is_failed(&self) -> bool {
        matches!(self, Self::Failed(_))
    }

    pub(crate) fn retry_on_open(&mut self) {
        if let Self::Failed(bytes) = *self {
            *self = Self::Retry(bytes);
        }
    }
}

pub(crate) fn start_child_seed_history(
    parent: &mut AgentView,
    child_sid: &str,
    bytes: u64,
) -> Option<Effect> {
    let child = parent.subagent_views.get_mut(child_sid)?;
    let id = uuid::Uuid::now_v7();
    child.child_seed_history = Some(ChildSeedHistory::Loading(id));
    Some(Effect::ReadChildSeedHistory {
        request: ChildSeedHistoryRequest {
            root: parent.session.id,
            root_binding: parent.session_binding_epoch,
            child_binding: child.session_binding_epoch,
            child_session_id: child_sid.to_owned(),
            bytes,
            id,
            grow_home: super::effective_grow_home(),
            appearance: Arc::new(child.scrollback.appearance().clone()),
        },
    })
}

pub(crate) fn take_open_child_seed_retry(app: &mut AppView) -> Option<Effect> {
    let ActiveView::Agent(id) = app.active_view else {
        return None;
    };
    let parent = app.agents.get_mut(&id)?;
    let sid = parent.active_subagent.as_ref()?;
    let Some(ChildSeedHistory::Retry(bytes)) = parent.subagent_views.get(sid)?.child_seed_history
    else {
        return None;
    };
    let sid = sid.clone();
    start_child_seed_history(parent, &sid, bytes)
}

pub(crate) fn read_child_seed_history(
    request: &ChildSeedHistoryRequest,
) -> Result<Box<ScrollbackState>, String> {
    let (mut scrollback, emitted) = {
        let mut projection = TranscriptProjection::default();
        projection
            .scrollback
            .set_appearance((*request.appearance).clone());
        let emitted = shell::session::storage::stream_child_seed_history_at(
            &request.child_session_id,
            &request.grow_home,
            request.bytes,
            |update| {
                projection.apply(TranscriptEvent {
                    update,
                    timestamp_ms: None,
                    simulated_source: false,
                })
            },
        )
        .map_err(|error| error.to_string())?;
        (projection.scrollback, emitted)
    };
    scrollback.finish_all_running();
    for (_, entry) in scrollback.iter_entries() {
        entry.prepare_source_line_widths();
    }
    if emitted == shell::session::storage::ReplayEmission::Emitted {
        crate::memory_release::release_retained_memory_with("subagent-replay");
    }
    Ok(Box::new(scrollback))
}

pub(crate) fn apply_child_seed_history(
    app: &mut AppView,
    request: ChildSeedHistoryRequest,
    result: Result<Box<ScrollbackState>, String>,
) {
    let Some(root) = app.agents.get_mut(&request.root) else {
        return;
    };
    let stashed = root.child_transcript_is_stashed(&request.child_session_id);
    let Some(child) = root.subagent_views.get_mut(&request.child_session_id) else {
        return;
    };
    if !matches!(child.child_seed_history, Some(ChildSeedHistory::Loading(id)) if id == request.id)
    {
        return;
    }
    if root.session_binding_epoch != request.root_binding
        || child.session_binding_epoch != request.child_binding
    {
        // If the same child view survived rebinding, its old request no longer
        // owns a merge. Keep the seed retryable for the new binding.
        child.child_seed_history = Some(
            if root.active_subagent.as_deref() == Some(&request.child_session_id) {
                ChildSeedHistory::Retry(request.bytes)
            } else {
                ChildSeedHistory::Failed(request.bytes)
            },
        );
        return;
    }
    if stashed {
        child.child_seed_history = Some(ChildSeedHistory::ReadyAfterReload(request.bytes, result));
        return;
    }
    install_prefix(child, &request.child_session_id, request.bytes, result);
}

pub(crate) fn install_seed_after_reload(child: &mut AgentView, child_sid: &str) {
    if matches!(
        child.child_seed_history,
        Some(ChildSeedHistory::ReadyAfterReload(..))
    ) && let Some(ChildSeedHistory::ReadyAfterReload(bytes, result)) =
        child.child_seed_history.take()
    {
        install_prefix(child, child_sid, bytes, result);
    }
}

fn install_prefix(
    child: &mut AgentView,
    child_sid: &str,
    bytes: u64,
    result: Result<Box<ScrollbackState>, String>,
) {
    let error_id = format!("child-history-error:{child_sid}");
    match result {
        Ok(prefix) => {
            let error_entry = child.scrollback.iter_entries().find_map(|(id, entry)| {
                (entry.block.immutable_event_id() == Some(error_id.as_str())).then_some(id)
            });
            if let Some(id) = error_entry {
                child.scrollback.remove_entry(id);
            }
            child.scrollback.prepend_child_seed_history(*prefix);
            child.child_seed_history = None;
        }
        Err(error) => {
            child.child_seed_history = Some(ChildSeedHistory::Failed(bytes));
            let message =
                format!("Could not load child history: {error}. Reopen the child to retry.");
            child.show_toast(&message);
            let mut notice = NoticeBlock::new(message);
            notice.event_id = Some(error_id);
            child
                .scrollback
                .push_block_if_absent(RenderBlock::Notice(notice));
        }
    }
}
