//! One owned transcript request across terminal handoff and retries.
use crate::app::agent_view::AgentView;
use crate::app::root::{ActiveView, AppView};
use crate::app::session::AgentId;
use crate::scrollback::block::RenderBlock;

pub(crate) struct PendingPager {
    pub path: tempfile::TempPath,
    pub ansi: bool,
    root: AgentId,
    agent: AgentId,
    session: Option<acp_transport::protocol::SessionId>,
}

impl PendingPager {
    pub fn new(path: tempfile::TempPath, ansi: bool, root: AgentId, agent: &AgentView) -> Self {
        Self {
            path,
            ansi,
            root,
            agent: agent.session.id,
            session: agent.session.session_id.clone(),
        }
    }

    /// Returns false when an additional application-level notice is needed.
    /// Never appends feedback to a different or rebound conversation.
    pub fn report(&self, app: &mut AppView, message: &str) -> bool {
        let active = app.active_view == ActiveView::Agent(self.root);
        let minimal = app.screen_mode.is_minimal();
        let Some(root) = app.agents.get_mut(&self.root) else {
            return false;
        };
        let visible_id = if !minimal && root.permission_queue.is_empty() {
            root.active_subagent
                .as_ref()
                .and_then(|key| root.subagent_views.get(key))
                .map(|child| child.session.id)
                .unwrap_or(root.session.id)
        } else {
            root.session.id
        };
        let visible = active && visible_id == self.agent;
        let target = if root.session.id == self.agent {
            Some(root)
        } else {
            root.subagent_views
                .values_mut()
                .find(|child| child.session.id == self.agent)
                .map(|child| &mut **child)
        };
        let Some(target) = target.filter(|target| target.session.session_id == self.session) else {
            return false;
        };
        if visible && !minimal {
            target.show_toast(message);
        } else {
            target.scrollback.push_block(RenderBlock::notice(message));
        }
        visible
    }
}
