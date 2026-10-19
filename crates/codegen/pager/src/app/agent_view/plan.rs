//! Plan surfaces: plan chip/preview, plan approval + feedback, and casual
//! plan commenting (incl. the casual-commenting test fixture).
use super::AgentView;
#[cfg(test)]
use super::{ActivePane, InputMode, test_fixtures};
#[cfg(test)]
use crate::actions::ActionRegistry;
use crate::app::actions::Action;
use crate::app::root::InputOutcome;
use crate::views::file_search::line_viewer::LineViewerState;
use crate::views::list_pane::ListItem;
use crate::views::plan_approval_view::{
    PendingPlanApprovalDecision, PlanApprovalFocus, PlanApprovalViewState, PlanComment,
};
use crate::views::prompt_widget::{EnterOutcome, PromptEvent};
#[cfg(test)]
use crossterm::event::KeyModifiers;
use crossterm::event::{KeyCode, KeyEvent};

#[derive(Debug, Clone)]
pub(crate) struct PlanArtifactPreview {
    pub session_id: String,
    pub binding_epoch: u32,
    pub submission_id: u64,
    pub retained: bool,
    pub artifact_revision: u64,
    pub artifact_hash: String,
    pub plan_content: String,
}

impl PlanArtifactPreview {
    pub(crate) fn matches_submission(&self, other: &Self) -> bool {
        self.session_id == other.session_id
            && self.binding_epoch == other.binding_epoch
            && self.submission_id == other.submission_id
            && self.artifact_revision == other.artifact_revision
            && self.artifact_hash == other.artifact_hash
    }
}

fn preserved_plan_comments_note(
    source: Option<&PlanArtifactPreview>,
    comments: &[PlanComment],
) -> String {
    let body = crate::views::plan_approval_view::format_plan_comments(
        comments,
        source.map(|artifact| artifact.plan_content.as_str()),
    );
    match source {
        Some(artifact) => format!(
            "Unsent Plan notes from submission {} (revision {}, hash {}):\n\n{}",
            artifact.submission_id, artifact.artifact_revision, artifact.artifact_hash, body
        ),
        None => format!("Unsent Plan notes from an unbound earlier submission:\n\n{body}"),
    }
}

fn append_draft_text(text: &mut String, cursor: &mut usize, addition: &str) {
    if !text.is_empty() {
        text.push_str("\n\n");
    }
    text.push_str(addition);
    *cursor = text.len();
}

impl AgentView {
    /// Whether the current line viewer is showing a plan preview.
    pub(super) fn is_plan_viewer(&self) -> bool {
        self.line_viewer.as_ref().is_some_and(|v| {
            v.kind == crate::views::file_search::line_viewer::LineViewerKind::PlanPreview
        })
    }
    /// Whether the user is currently composing a comment via the prompt
    /// input inside the *casual* plan preview (the modal opened with no
    /// `plan_approval_view`). Mirrors the `pav.focus == Commenting`
    /// check used by the plan-approval path so the prompt/footer
    /// behaves identically across both modes.
    pub(super) fn is_casual_commenting(&self) -> bool {
        self.plan_approval_view.is_none()
            && self.is_plan_viewer()
            && self.casual_commenting_range.is_some()
    }
    /// Whether the prompt "auto" (LLM classifier mode) flag should render.
    /// Extracted for unit testing the precedence: auto shows only when the
    /// session is in auto mode and always-approve is not active. Behavior and permission
    /// policy are independent, so Plan does not hide the permission indicator.
    pub(super) fn auto_flag_visible(&self, _effective_plan: bool) -> bool {
        self.session.is_auto() && !self.session.is_always_approve()
    }
    /// Whether plan content is available for preview.
    fn plan_preview_available(&self) -> bool {
        self.plan_body_for_preview().is_some()
    }
    /// Whether the Plan status-bar entry should be rendered.
    ///
    /// Active Plan state remains inspectable before a body is cached. Outside
    /// Plan, retained/configured previews still require a valid cached body.
    pub(super) fn should_show_plan_chip(
        &self,
        appearance: &crate::appearance::AppearanceConfig,
    ) -> bool {
        self.session.plan_mode_active
            || ((appearance.show_plan_chip
                || self
                    .plan_artifact_preview
                    .as_ref()
                    .is_some_and(|preview| preview.retained))
                && self.plan_preview_available())
    }

    pub(super) fn plan_status_label(&self) -> &'static str {
        if !self.session.plan_mode_active {
            return "[Plan]";
        }
        match self.session.plan_phase.as_deref() {
            Some("awaiting_approval") => "[Plan: Review]",
            Some("executing") => "[Plan: Executing]",
            Some("amending") => "[Plan: Amending]",
            _ => "[Plan: Drafting]",
        }
    }
    fn inline_plan_content(&self) -> Option<&str> {
        self.plan_approval_view
            .as_ref()
            .map(|p| p.plan_content.as_str())
            .filter(|s| !s.trim().is_empty())
    }
    /// Resolve the plan body for the line-viewer preview.
    ///
    /// Prefers content carried on the live approval request, then uses the
    /// latest artifact resolved by the Shell's canonical read API.
    pub(super) fn plan_body_for_preview(&self) -> Option<String> {
        if let Some(content) = self
            .plan_approval_view
            .as_ref()
            .map(|p| p.plan_content.as_str())
            .filter(|s| !s.trim().is_empty())
        {
            return Some(content.to_owned());
        }
        let session_id = self.session.session_id.as_ref()?.0.as_ref();
        self.plan_artifact_preview
            .as_ref()
            .filter(|preview| {
                preview.session_id == session_id
                    && preview.binding_epoch == self.session_binding_epoch
                    && preview.artifact_revision > 0
                    && preview.artifact_hash.len() == 64
            })
            .map(|preview| preview.plan_content.clone())
            .filter(|content| !content.trim().is_empty())
    }
    /// Open the plan preview when content exists.
    pub(crate) fn show_plan_preview_if_available(&mut self) {
        if self.plan_preview_available() {
            self.show_plan_preview();
        }
    }
    /// Show the plan in the line viewer overlay or a "no plan" toast.
    pub fn show_plan_preview(&mut self) {
        let body = self.plan_body_for_preview();
        let Some(content) = body else {
            self.show_toast("No plan written yet.");
            return;
        };
        self.open_plan_preview_content(content);
    }

    pub(crate) fn resolve_plan_preview(
        &mut self,
        response: tools::implementations::grow_build::plan_control::PlanArtifactQueryResponse,
    ) -> Result<(), &'static str> {
        let expected_session_id = self
            .session
            .session_id
            .as_ref()
            .ok_or("No active session")?
            .0
            .as_ref();
        if response.session_id != expected_session_id {
            return Err("Plan preview belongs to a different session");
        }
        if response.artifact_revision == 0
            || response.artifact_hash.len() != 64
            || !response
                .artifact_hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || blake3::hash(response.plan_content.as_bytes())
                .to_hex()
                .as_str()
                != response.artifact_hash
            || response.plan_content.trim().is_empty()
        {
            return Err("Plan artifact identity or content is invalid");
        }
        let preview = PlanArtifactPreview {
            session_id: response.session_id,
            binding_epoch: self.session_binding_epoch,
            submission_id: response.submission_id,
            retained: response.retained,
            artifact_revision: response.artifact_revision,
            artifact_hash: response.artifact_hash,
            plan_content: response.plan_content,
        };
        self.plan_artifact_preview = Some(preview.clone());
        self.open_plan_preview_content(preview.plan_content);
        if let Some(viewer) = self.line_viewer.as_mut() {
            viewer.title_override =
                Some(format!("plan.md · revision {}", preview.artifact_revision));
        }
        Ok(())
    }

    fn open_plan_preview_content(&mut self, content: String) {
        let Some(mut viewer) = LineViewerState::open_markdown_content("plan.md", content, None)
        else {
            self.show_toast("Plan preview is unavailable.");
            return;
        };
        viewer.kind = crate::views::file_search::line_viewer::LineViewerKind::PlanPreview;
        viewer.title_override = Some("plan.md".to_string());
        viewer.fullscreen = true;
        {
            let plan = viewer.plan_mut();
            plan.show_action_buttons = self.plan_approval_view.is_none();
            plan.feedback_active = self.plan_approval_view.is_some();
            plan.show_review_button = self.plan_approval_view.is_none()
                && self
                    .plan_artifact_preview
                    .as_ref()
                    .is_some_and(|preview| preview.retained);
            plan.decision_pending = self.pending_plan_approval_decision.is_some();
        }
        if let Some(ref pav) = self.plan_approval_view
            && !pav.comments.is_empty()
        {
            viewer.rebuild_with_comments(&pav.comments);
        } else if self.plan_approval_view.is_none()
            && !self.plan_comments.is_empty()
            && self
                .plan_comments_artifact
                .as_ref()
                .is_some_and(|artifact| {
                    self.plan_artifact_preview.as_ref().is_some_and(|preview| {
                        preview.matches_submission(artifact)
                            && preview.binding_epoch == self.session_binding_epoch
                            && self.session.session_id.as_ref().is_some_and(|session_id| {
                                preview.session_id == session_id.0.as_ref()
                            })
                    })
                })
        {
            viewer.rebuild_with_comments(&self.plan_comments);
        } else if !self.plan_comments.is_empty() {
            self.show_toast("Saved comments belong to an earlier plan revision.");
        }
        self.line_viewer = Some(viewer);
    }
    /// Test fixture: drive the agent into casual-commenting state
    /// (line viewer open in plan-preview mode + `casual_commenting_range`
    /// armed) so the `Event::Paste` plan-feedback arm at ~1539 is
    /// reachable from a unit test without spawning the real
    /// keystroke pipeline. Consolidates three field mutations into
    /// one helper so a future refactor of casual-commenting state
    /// only has to update this fixture rather than every test that
    /// reaches into the fields by name.
    #[cfg(test)]
    pub(crate) fn enter_casual_commenting_for_test(&mut self) {
        let mut viewer =
            crate::views::file_search::line_viewer::LineViewerState::open_markdown_content(
                "test.md",
                "hello\n".to_owned(),
                None,
            )
            .expect("fixture must open the line viewer");
        viewer.kind = crate::views::file_search::line_viewer::LineViewerKind::PlanPreview;
        self.line_viewer = Some(viewer);
        self.casual_commenting_range = Some(0..1);
    }
    pub(crate) fn approve_plan(&mut self) -> InputOutcome {
        if let Some(outcome) = self.pending_plan_decision_outcome() {
            return outcome;
        }
        let Some(pav) = self.plan_approval_view.as_mut() else {
            return InputOutcome::Changed;
        };
        let review_feedback = if !pav.comments.is_empty() {
            let formatted = pav.format_feedback(None);
            if formatted.trim().is_empty() {
                None
            } else {
                Some(formatted)
            }
        } else {
            None
        };
        let response = pav.send_approved(review_feedback);
        self.capture_pending_plan_decision(response);
        {
            use diagnostics::events::PlanSubmit;
            use diagnostics::session_ctx::log_event;
            log_event(PlanSubmit {
                action: "build".to_string(),
            });
        }
        InputOutcome::Action(Action::CheckPlanApprovalDecision)
    }
    pub(crate) fn abandon_plan(&mut self) -> InputOutcome {
        if let Some(outcome) = self.pending_plan_decision_outcome() {
            return outcome;
        }
        let Some(pav) = self.plan_approval_view.as_mut() else {
            return InputOutcome::Changed;
        };
        let response = pav.send_abandoned();
        self.capture_pending_plan_decision(response);
        {
            use diagnostics::events::PlanSubmit;
            use diagnostics::session_ctx::log_event;
            log_event(PlanSubmit {
                action: "abandon".to_string(),
            });
        }
        InputOutcome::Action(Action::CheckPlanApprovalDecision)
    }
    pub(crate) fn defer_plan(&mut self) -> InputOutcome {
        if let Some(outcome) = self.pending_plan_decision_outcome() {
            return outcome;
        }
        let Some(pav) = self.plan_approval_view.as_mut() else {
            return InputOutcome::Changed;
        };
        let freeform = self.prompt.text().to_owned();
        let formatted = pav.format_feedback(Some(&freeform));
        let feedback = (!formatted.trim().is_empty()).then_some(formatted);
        if crate::app::minimal_mode_active()
            && let Some(msg) = feedback.as_deref().map(str::trim).filter(|s| !s.is_empty())
        {
            self.scrollback
                .push_block(crate::scrollback::RenderBlock::user_prompt(msg.to_string()));
        }
        let response = pav.send_deferred(feedback);
        self.capture_pending_plan_decision(response);
        {
            use diagnostics::events::PlanSubmit;
            use diagnostics::session_ctx::log_event;
            log_event(PlanSubmit {
                action: "defer".to_string(),
            });
        }
        InputOutcome::Action(Action::CheckPlanApprovalDecision)
    }
    fn send_plan_feedback(&mut self, feedback: Option<String>) -> InputOutcome {
        if let Some(outcome) = self.pending_plan_decision_outcome() {
            return outcome;
        }
        let Some(pav) = self.plan_approval_view.as_mut() else {
            return InputOutcome::Changed;
        };
        let formatted = pav.format_feedback(feedback.as_deref());
        let to_send = if formatted.trim().is_empty() {
            feedback
        } else {
            Some(formatted)
        };
        if crate::app::minimal_mode_active()
            && let Some(msg) = to_send.as_deref().map(str::trim).filter(|s| !s.is_empty())
        {
            self.scrollback
                .push_block(crate::scrollback::RenderBlock::user_prompt(msg.to_string()));
        }
        let response = pav.send_cancelled(to_send);
        self.capture_pending_plan_decision(response);
        {
            use diagnostics::events::PlanSubmit;
            use diagnostics::session_ctx::log_event;
            log_event(PlanSubmit {
                action: "revise".to_string(),
            });
        }
        InputOutcome::Action(Action::CheckPlanApprovalDecision)
    }

    fn pending_plan_decision_outcome(&mut self) -> Option<InputOutcome> {
        let pending = self.pending_plan_approval_decision.as_ref()?;
        let current = self.plan_approval_view.as_ref();
        if current.is_some_and(PlanApprovalViewState::decision_pending) {
            return Some(InputOutcome::Action(Action::CheckPlanApprovalDecision));
        }
        if current.is_some_and(|pav| pav.submission_id != pending.response.submission_id) {
            self.show_toast(
                "Resolve the pending Plan decision before deciding on another submission.",
            );
        } else {
            self.show_toast("This Plan decision is still awaiting confirmation.");
        }
        Some(InputOutcome::Action(Action::CheckPlanApprovalDecision))
    }

    fn capture_pending_plan_decision(
        &mut self,
        response: crate::views::plan_approval_view::PlanApprovalExtResponse,
    ) {
        let Some(pav) = self.plan_approval_view.as_ref() else {
            return;
        };
        let target = PlanArtifactPreview {
            session_id: pav.source_session_id.clone(),
            binding_epoch: self.session_binding_epoch,
            submission_id: response.submission_id,
            retained: true,
            artifact_revision: pav.artifact_revision,
            artifact_hash: pav.artifact_hash.clone(),
            plan_content: pav.plan_content.clone(),
        };
        let comments_match = self.plan_comments.is_empty()
            || self
                .plan_comments_artifact
                .as_ref()
                .is_some_and(|source| target.matches_submission(source));
        if !comments_match {
            let note = preserved_plan_comments_note(
                self.plan_comments_artifact.as_ref(),
                &self.plan_comments,
            );
            if let Some(pav) = self.plan_approval_view.as_mut() {
                append_draft_text(
                    &mut pav.stashed_prompt.text,
                    &mut pav.stashed_prompt.cursor,
                    &note,
                );
            }
            self.plan_comments.clear();
            self.plan_comments_artifact = None;
            self.plan_next_comment_id = 0;
        }
        let Some(pav) = self.plan_approval_view.as_mut() else {
            return;
        };
        let (session_id, artifact_revision, artifact_hash, comments, next_comment_id) = (
            pav.source_session_id.clone(),
            pav.artifact_revision,
            pav.artifact_hash.clone(),
            pav.comments.clone(),
            pav.next_comment_id,
        );
        pav.focus = PlanApprovalFocus::Preview;
        self.plan_next_comment_id = next_comment_id;
        self.plan_comments = comments.clone();
        self.plan_comments_artifact = (!comments.is_empty()).then(|| PlanArtifactPreview {
            session_id: session_id.clone(),
            binding_epoch: self.session_binding_epoch,
            submission_id: response.submission_id,
            retained: true,
            artifact_revision,
            artifact_hash: artifact_hash.clone(),
            plan_content: pav.plan_content.clone(),
        });
        self.pending_plan_approval_decision = Some(PendingPlanApprovalDecision {
            session_id,
            binding_epoch: self.session_binding_epoch,
            response,
            artifact_revision,
            artifact_hash,
            comments,
        });
        if let Some(viewer) = self.line_viewer.as_mut() {
            let plan = viewer.plan_mut();
            plan.feedback_active = true;
            plan.decision_pending = true;
        }
        self.show_toast("Plan decision sent; waiting for confirmation.");
    }
    pub(crate) fn reopen_plan_approval(&mut self) {
        if let Some(ref mut pav) = self.plan_approval_view {
            pav.focus = PlanApprovalFocus::Preview;
        } else {
            self.prompt.set_text("");
        }
        self.show_plan_preview_if_available();
        if self.line_viewer.is_none() {
            if let Some(ref mut pav) = self.plan_approval_view {
                pav.focus = PlanApprovalFocus::Prompt;
            }
        } else if let Some(ref mut viewer) = self.line_viewer {
            viewer.plan_mut().feedback_active = true;
        }
    }
    /// Discard an in-progress comment draft: clear the prompt text and
    /// drop the selected line range + pending edit + stashed feedback.
    /// Used whenever focus leaves the prompt without an explicit save
    /// or cancel (e.g. Tab back to Preview, click into the modal).
    fn discard_in_progress_comment(&mut self) {
        if let Some(ref mut pav) = self.plan_approval_view {
            pav.commenting_range = None;
            pav.editing_comment_id = None;
            pav.stashed_feedback_prompt = None;
        }
        self.prompt.set_text("");
    }
    pub(super) fn handle_plan_feedback_key(&mut self, key: &KeyEvent) -> InputOutcome {
        let is_commenting = self
            .plan_approval_view
            .as_ref()
            .is_some_and(|pav| pav.focus == PlanApprovalFocus::Commenting);
        if self.pending_plan_approval_decision.is_some() {
            if key.code == KeyCode::Char('r')
                && key.modifiers.is_empty()
                && !self.prompt.file_search_visible()
            {
                return InputOutcome::Action(Action::CheckPlanApprovalDecision);
            }
            if key.code == KeyCode::Esc {
                if self.prompt.file_search_visible() {
                    self.prompt.file_search.clear_context();
                } else {
                    self.cancel_line_viewer();
                }
            }
            return InputOutcome::Changed;
        }
        if key.code == KeyCode::Tab && key.modifiers.is_empty() {
            let focus = self.plan_approval_view.as_ref().map(|p| p.focus);
            match focus {
                Some(PlanApprovalFocus::Prompt) | Some(PlanApprovalFocus::Commenting) => {
                    if self.line_viewer.is_none() {
                        self.show_plan_preview_if_available();
                    }
                    if let Some(ref mut pav) = self.plan_approval_view {
                        pav.focus = PlanApprovalFocus::Preview;
                    }
                    if let Some(ref mut viewer) = self.line_viewer {
                        viewer.plan_mut().feedback_active = true;
                    }
                }
                Some(PlanApprovalFocus::Preview) => {
                    if let Some(ref mut pav) = self.plan_approval_view {
                        pav.focus = PlanApprovalFocus::Prompt;
                    }
                }
                None => {}
            }
            if is_commenting {
                self.discard_in_progress_comment();
            }
            return InputOutcome::Changed;
        }
        if key.code == KeyCode::Esc {
            if self.prompt.file_search_visible() {
                self.prompt.file_search.clear_context();
                return InputOutcome::Changed;
            }
            if is_commenting {
                let stashed = if let Some(ref mut pav) = self.plan_approval_view {
                    pav.focus = PlanApprovalFocus::Preview;
                    pav.editing_comment_id = None;
                    pav.commenting_range = None;
                    pav.stashed_feedback_prompt.take()
                } else {
                    None
                };
                if let Some(stashed) = stashed {
                    self.prompt.restore(stashed);
                } else {
                    self.prompt.set_text("");
                }
                return InputOutcome::Changed;
            }
            self.cancel_line_viewer();
            return InputOutcome::Changed;
        }
        if !is_commenting
            && key.code == KeyCode::Char('a')
            && key.modifiers.is_empty()
            && self.prompt.text().trim().is_empty()
            && !self.prompt.file_search_visible()
            && self
                .plan_approval_view
                .as_ref()
                .is_some_and(|pav| pav.comments.is_empty())
        {
            return self.approve_plan();
        }
        match self.prompt.route_enter(key) {
            EnterOutcome::NewlineInserted => return InputOutcome::Changed,
            EnterOutcome::Submit => {
                if is_commenting {
                    return self.save_plan_comment();
                }
                let text = self.prompt.text().to_string();
                let has_comments = self
                    .plan_approval_view
                    .as_ref()
                    .is_some_and(|pav| !pav.comments.is_empty());
                let prompt_focused = self
                    .plan_approval_view
                    .as_ref()
                    .is_some_and(|pav| pav.focus == PlanApprovalFocus::Prompt);
                if prompt_focused {
                    if text.trim().is_empty() && !has_comments {
                        self.show_toast("Type revision notes, or press a to approve.");
                        return InputOutcome::Changed;
                    }
                    let freeform = if text.trim().is_empty() {
                        None
                    } else {
                        Some(text)
                    };
                    return self.send_plan_feedback(freeform);
                }
                return InputOutcome::Changed;
            }
            EnterOutcome::PassThrough => {}
        }
        match self.prompt.handle_key(key) {
            PromptEvent::Edited => {
                if let Some(req) = self.prompt.pending_viewer_request.take() {
                    self.open_line_viewer(&req.path, req.initial_range);
                }
                InputOutcome::Changed
            }
            PromptEvent::Ignored => InputOutcome::Changed,
        }
    }
    pub(super) fn enter_plan_commenting(&mut self) -> InputOutcome {
        if self
            .plan_approval_view
            .as_ref()
            .is_some_and(PlanApprovalViewState::decision_pending)
        {
            self.show_toast("Plan decision is awaiting confirmation.");
            return InputOutcome::Changed;
        }
        let viewer = match self.line_viewer.as_mut() {
            Some(v) => v,
            None => return InputOutcome::Changed,
        };
        if let Some(vi) = viewer.list_state.selected_index() {
            let pi = viewer.list_state.to_physical(vi);
            if let Some(comment_id) = viewer.lines.get(pi).and_then(|item| item.comment_id())
                && let Some(pav) = self.plan_approval_view.as_mut()
                && let Some(comment) = pav.comments.iter().find(|c| c.id == comment_id)
            {
                let comment_text = comment.text.clone();
                let comment_range = comment.line_range.clone();
                pav.stashed_feedback_prompt = Some(self.prompt.stash());
                pav.editing_comment_id = Some(comment_id);
                pav.commenting_range = Some(comment_range);
                pav.focus = PlanApprovalFocus::Commenting;
                self.prompt.set_text(&comment_text);
                return InputOutcome::Changed;
            }
        }
        let range = viewer.selected_line_range();
        let Some(range) = range else {
            return InputOutcome::Changed;
        };
        if viewer.list_state.visual_mode {
            let start_vi = viewer.list_state.multi_range().map(|r| r.start);
            if let Some(start_vi) = start_vi {
                let start_pi = viewer.list_state.to_physical(start_vi);
                let start_id = viewer.lines.get(start_pi).map(|l| l.stable_id());
                viewer.list_state.exit_visual_mode();
                if let Some(id) = start_id {
                    viewer.list_state.select_by_id(id);
                }
            } else {
                viewer.list_state.exit_visual_mode();
            }
        }
        if let Some(ref mut pav) = self.plan_approval_view {
            pav.stashed_feedback_prompt = Some(self.prompt.stash());
            pav.commenting_range = Some(range);
            pav.editing_comment_id = None;
            pav.focus = PlanApprovalFocus::Commenting;
        }
        self.prompt.set_text("");
        InputOutcome::Changed
    }
    fn save_plan_comment(&mut self) -> InputOutcome {
        let text = self.prompt.text().to_string();
        if text.trim().is_empty() {
            return InputOutcome::Changed;
        }
        let pav = match self.plan_approval_view.as_mut() {
            Some(pav) => pav,
            None => return InputOutcome::Changed,
        };
        let range = match pav.commenting_range.take() {
            Some(r) => r,
            None => return InputOutcome::Changed,
        };
        if let Some(edit_id) = pav.editing_comment_id.take() {
            if let Some(comment) = pav.comments.iter_mut().find(|c| c.id == edit_id) {
                comment.text = text;
                comment.line_range = range;
            }
        } else {
            let id = pav.next_comment_id;
            pav.next_comment_id += 1;
            pav.comments.push(PlanComment {
                id,
                line_range: range,
                text,
            });
        }
        pav.focus = PlanApprovalFocus::Preview;
        let comments = pav.comments.clone();
        if let Some(ref mut viewer) = self.line_viewer {
            viewer.rebuild_with_comments(&comments);
        }
        if let Some(stashed) = pav.stashed_feedback_prompt.take() {
            self.prompt.restore(stashed);
        } else {
            self.prompt.set_text("");
        }
        InputOutcome::Changed
    }
    pub(super) fn delete_plan_comment_at_cursor(&mut self) -> InputOutcome {
        let viewer = match self.line_viewer.as_ref() {
            Some(v) => v,
            None => return InputOutcome::Changed,
        };
        let vi = match viewer.list_state.selected_index() {
            Some(vi) => vi,
            None => return InputOutcome::Changed,
        };
        let pi = viewer.list_state.to_physical(vi);
        let comment_id = match viewer.lines.get(pi).and_then(|item| item.comment_id()) {
            Some(id) => id,
            None => return InputOutcome::Changed,
        };
        if let Some(ref mut pav) = self.plan_approval_view {
            pav.comments.retain(|c| c.id != comment_id);
            let comments = pav.comments.clone();
            if let Some(ref mut viewer) = self.line_viewer {
                viewer.rebuild_with_comments(&comments);
            }
        }
        InputOutcome::Changed
    }
    /// Enter casual commenting mode from the plan preview.
    ///
    /// If the cursor is on a comment line, enter edit mode for that comment.
    /// If the cursor is on a source line, capture the line range and enter
    /// new-comment mode.
    pub(super) fn enter_casual_plan_commenting(&mut self) -> InputOutcome {
        let viewer = match self.line_viewer.as_mut() {
            Some(v) => v,
            None => return InputOutcome::Changed,
        };
        if let Some(vi) = viewer.list_state.selected_index() {
            let pi = viewer.list_state.to_physical(vi);
            if let Some(comment_id) = viewer.lines.get(pi).and_then(|item| item.comment_id())
                && let Some(comment) = self.plan_comments.iter().find(|c| c.id == comment_id)
            {
                let comment_text = comment.text.clone();
                let comment_range = comment.line_range.clone();
                if self.casual_stashed_prompt.is_none() {
                    self.casual_stashed_prompt = Some(self.prompt.stash());
                }
                self.casual_editing_comment_id = Some(comment_id);
                self.casual_commenting_range = Some(comment_range);
                self.prompt.set_text(&comment_text);
                return InputOutcome::Changed;
            }
        }
        let range = viewer.selected_line_range();
        let Some(range) = range else {
            return InputOutcome::Changed;
        };
        if viewer.list_state.visual_mode {
            let start_vi = viewer.list_state.multi_range().map(|r| r.start);
            if let Some(start_vi) = start_vi {
                let start_pi = viewer.list_state.to_physical(start_vi);
                let start_id = viewer.lines.get(start_pi).map(|l| l.stable_id());
                viewer.list_state.exit_visual_mode();
                if let Some(id) = start_id {
                    viewer.list_state.select_by_id(id);
                }
            } else {
                viewer.list_state.exit_visual_mode();
            }
        }
        let same_artifact = self
            .plan_comments_artifact
            .as_ref()
            .zip(self.plan_artifact_preview.as_ref())
            .is_some_and(|(comments, preview)| {
                preview.matches_submission(comments)
                    && preview.binding_epoch == self.session_binding_epoch
                    && self
                        .session
                        .session_id
                        .as_ref()
                        .is_some_and(|session_id| preview.session_id == session_id.0.as_ref())
            });
        if !self.plan_comments.is_empty() && !same_artifact {
            let note = preserved_plan_comments_note(
                self.plan_comments_artifact.as_ref(),
                &self.plan_comments,
            );
            let addition = if self.prompt.text().is_empty() {
                note
            } else {
                format!("\n\n{note}")
            };
            self.prompt.append_text(&addition);
            self.plan_comments.clear();
            self.plan_comments_artifact = None;
            self.plan_next_comment_id = 0;
        }
        if self.casual_stashed_prompt.is_none() {
            self.casual_stashed_prompt = Some(self.prompt.stash());
        }
        self.casual_commenting_range = Some(range);
        self.casual_editing_comment_id = None;
        self.prompt.set_text("");
        InputOutcome::Changed
    }
    /// Save the current casual comment (new or edited) and rebuild the viewer.
    pub(super) fn save_casual_plan_comment(&mut self) -> InputOutcome {
        let text = self.prompt.text().to_owned();
        if text.trim().is_empty() {
            return self.cancel_casual_plan_commenting();
        }
        let range = match self.casual_commenting_range.take() {
            Some(r) => r,
            None => return self.cancel_casual_plan_commenting(),
        };
        if let Some(edit_id) = self.casual_editing_comment_id.take() {
            if let Some(comment) = self.plan_comments.iter_mut().find(|c| c.id == edit_id) {
                comment.text = text;
                comment.line_range = range;
            }
        } else {
            if self.plan_comments.is_empty() {
                self.plan_comments_artifact = self.plan_artifact_preview.as_ref().cloned();
            }
            let id = self.plan_next_comment_id;
            self.plan_next_comment_id += 1;
            self.plan_comments.push(PlanComment {
                id,
                line_range: range,
                text,
            });
        }
        if let Some(stashed) = self.casual_stashed_prompt.take() {
            self.prompt.restore(stashed);
        } else {
            self.prompt.set_text("");
        }
        let comments = self.plan_comments.clone();
        if let Some(ref mut viewer) = self.line_viewer {
            viewer.rebuild_with_comments(&comments);
        }
        InputOutcome::Changed
    }
    /// Cancel casual plan commenting without saving.
    pub(super) fn cancel_casual_plan_commenting(&mut self) -> InputOutcome {
        self.casual_commenting_range = None;
        self.casual_editing_comment_id = None;
        if let Some(stashed) = self.casual_stashed_prompt.take() {
            self.prompt.restore(stashed);
        } else {
            self.prompt.set_text("");
        }
        InputOutcome::Changed
    }
    /// Key handler used while the user is composing a casual plan
    /// comment via the prompt input. Mirrors `handle_plan_feedback_key`
    /// (which serves the plan-approval Commenting focus) so the UX is
    /// identical: Enter saves, Esc cancels, Tab cancels back to the
    /// modal, and everything else routes to the prompt textarea.
    pub(super) fn handle_casual_plan_feedback_key(&mut self, key: &KeyEvent) -> InputOutcome {
        if key.code == KeyCode::Esc {
            if self.prompt.file_search_visible() {
                self.prompt.file_search.clear_context();
                return InputOutcome::Changed;
            }
            return self.cancel_casual_plan_commenting();
        }
        match self.prompt.route_enter(key) {
            EnterOutcome::NewlineInserted => return InputOutcome::Changed,
            EnterOutcome::Submit => return self.save_casual_plan_comment(),
            EnterOutcome::PassThrough => {}
        }
        if key.code == KeyCode::Tab && key.modifiers.is_empty() {
            return self.cancel_casual_plan_commenting();
        }
        match self.prompt.handle_key(key) {
            PromptEvent::Edited => {
                if let Some(req) = self.prompt.pending_viewer_request.take() {
                    self.open_line_viewer(&req.path, req.initial_range);
                }
                InputOutcome::Changed
            }
            PromptEvent::Ignored => InputOutcome::Changed,
        }
    }
    /// Delete the casual comment under the cursor in the plan preview.
    pub(super) fn delete_casual_plan_comment_at_cursor(&mut self) -> InputOutcome {
        let viewer = match self.line_viewer.as_ref() {
            Some(v) => v,
            None => return InputOutcome::Unchanged,
        };
        let vi = match viewer.list_state.selected_index() {
            Some(vi) => vi,
            None => return InputOutcome::Unchanged,
        };
        let pi = viewer.list_state.to_physical(vi);
        let comment_id = match viewer.lines.get(pi).and_then(|item| item.comment_id()) {
            Some(id) => id,
            None => return InputOutcome::Unchanged,
        };
        self.plan_comments.retain(|c| c.id != comment_id);
        if self.plan_comments.is_empty() {
            self.plan_comments_artifact = None;
        }
        let comments = self.plan_comments.clone();
        if let Some(ref mut viewer) = self.line_viewer {
            viewer.rebuild_with_comments(&comments);
        }
        InputOutcome::Changed
    }
    pub(super) fn send_casual_plan_comments(&mut self) -> InputOutcome {
        if self.plan_comments.is_empty() {
            self.show_toast("No comments to send.");
            return InputOutcome::Changed;
        }
        if self
            .plan_comments_artifact
            .as_ref()
            .is_some_and(|artifact| {
                !self.plan_artifact_preview.as_ref().is_some_and(|preview| {
                    preview.matches_submission(artifact)
                        && preview.binding_epoch == self.session_binding_epoch
                        && self
                            .session
                            .session_id
                            .as_ref()
                            .is_some_and(|session_id| preview.session_id == session_id.0.as_ref())
                })
            })
        {
            self.show_toast(
                "Comments belong to an earlier plan revision; review them before sending.",
            );
            return InputOutcome::Changed;
        }
        let plan_content = self
            .inline_plan_content()
            .map(str::to_owned)
            .or_else(|| self.plan_body_for_preview());
        let body = crate::views::plan_approval_view::format_plan_comments(
            &self.plan_comments,
            plan_content.as_deref(),
        );
        let text = format!("Plan feedback:\n\n{body}");
        self.plan_comments.clear();
        self.plan_comments_artifact = None;
        self.plan_next_comment_id = 0;
        self.cancel_line_viewer();
        self.show_toast("Plan feedback sent.");
        InputOutcome::Action(Action::SendPrompt(text))
    }
}
#[cfg(test)]
mod prompt_flag_tests {
    use super::test_fixtures::make_agent;
    /// The prompt "auto" flag is independent of Behavior; only the stronger
    /// always-approve permission policy suppresses it.
    #[test]
    fn auto_flag_visible_precedence() {
        let mut agent = make_agent();
        assert!(!agent.auto_flag_visible(false));
        agent.session.permission_mode = shell::util::config::PermissionMode::Auto;
        assert!(agent.auto_flag_visible(false));
        assert!(agent.auto_flag_visible(true));
        agent.session.permission_mode = shell::util::config::PermissionMode::AlwaysApprove;
        assert!(!agent.auto_flag_visible(false));
        agent.session.permission_mode = shell::util::config::PermissionMode::Ask;
        assert!(!agent.auto_flag_visible(false));
    }
}
#[cfg(test)]
mod plan_chip_tests {
    use super::*;
    use crate::acp::model_state::ModelState;
    use crate::app::session::{AgentId, AgentSession};
    use crate::appearance::AppearanceConfig;
    use crate::scrollback::state::ScrollbackState;
    fn make_agent() -> AgentView {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        AgentView::new(
            {
                let session = AgentSession::new(
                    AgentId(0),
                    tx,
                    None,
                    ModelState::default(),
                    std::path::PathBuf::from("/tmp"),
                    shell::util::config::PermissionMode::Ask,
                );
                session
            },
            ScrollbackState::new(),
        )
    }
    #[test]
    fn canonical_preview_cache_is_bound_to_its_session_and_revision() {
        let mut agent = make_agent();
        agent.session.session_id = Some(acp_transport::protocol::SessionId::new("plan-session"));
        agent.plan_artifact_preview = Some(PlanArtifactPreview {
            submission_id: 1,
            retained: false,
            session_id: "plan-session".into(),
            binding_epoch: agent.session_binding_epoch,
            artifact_revision: 2,
            artifact_hash: blake3::hash(b"# immutable plan").to_hex().to_string(),
            plan_content: "# immutable plan".into(),
        });
        assert_eq!(
            agent.plan_body_for_preview().as_deref(),
            Some("# immutable plan")
        );
        agent.session.session_id = Some(acp_transport::protocol::SessionId::new("other-session"));
        assert!(agent.plan_body_for_preview().is_none());
    }
    #[test]
    fn canonical_query_rejects_hash_mismatch_and_opens_exact_revision() {
        let mut agent = make_agent();
        agent.session.session_id = Some(acp_transport::protocol::SessionId::new("plan-session"));
        let response =
            tools::implementations::grow_build::plan_control::PlanArtifactQueryResponse {
                submission_id: 1,
                retained: false,
                session_id: "plan-session".into(),
                artifact_revision: 3,
                artifact_hash: "0".repeat(64),
                plan_content: "# immutable plan".into(),
            };
        assert!(agent.resolve_plan_preview(response).is_err());
        assert!(agent.line_viewer.is_none());

        let content = "# immutable plan".to_string();
        let response =
            tools::implementations::grow_build::plan_control::PlanArtifactQueryResponse {
                submission_id: 1,
                retained: false,
                session_id: "plan-session".into(),
                artifact_revision: 3,
                artifact_hash: blake3::hash(content.as_bytes()).to_hex().to_string(),
                plan_content: content,
            };
        agent.resolve_plan_preview(response).unwrap();
        assert_eq!(
            agent
                .line_viewer
                .as_ref()
                .unwrap()
                .title_override
                .as_deref(),
            Some("plan.md · revision 3")
        );
    }
    #[test]
    fn starting_comments_for_a_new_submission_preserves_prior_notes_in_the_draft() {
        let mut agent = make_agent();
        agent.session.session_id = Some(acp_transport::protocol::SessionId::new("plan-session"));
        let content = "# same plan\n\nStep 1: do something";
        let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
        agent.plan_artifact_preview = Some(PlanArtifactPreview {
            session_id: "plan-session".into(),
            binding_epoch: agent.session_binding_epoch,
            submission_id: 9,
            retained: false,
            artifact_revision: 4,
            artifact_hash: hash.clone(),
            plan_content: content.into(),
        });
        agent.plan_comments_artifact = Some(PlanArtifactPreview {
            session_id: "plan-session".into(),
            binding_epoch: agent.session_binding_epoch,
            submission_id: 8,
            retained: false,
            artifact_revision: 4,
            artifact_hash: hash,
            plan_content: content.into(),
        });
        agent.plan_comments.push(PlanComment {
            id: 0,
            line_range: 0..1,
            text: "keep this earlier note".into(),
        });
        agent.prompt.set_text("existing composer draft");
        agent.show_plan_preview();
        let viewer = agent.line_viewer.as_mut().unwrap();
        viewer.prepare_layout(80, 24);
        let first_line = viewer.lines[0].stable_id();
        viewer.list_state.select_by_id(first_line);
        viewer.prepare_layout(80, 24);

        assert!(matches!(
            agent.enter_casual_plan_commenting(),
            InputOutcome::Changed
        ));
        let stashed = agent.casual_stashed_prompt.as_ref().unwrap();
        assert!(stashed.text.starts_with("existing composer draft\n\n"));
        assert!(stashed.text.contains("submission 8 (revision 4, hash "));
        assert!(stashed.text.contains("keep this earlier note"));
        assert!(agent.plan_comments.is_empty());
        assert!(agent.plan_comments_artifact.is_none());
    }
    #[test]
    fn plan_chip_hidden_after_exit_by_default() {
        let mut agent = make_agent();
        agent.session.plan_mode_active = false;
        let appearance = AppearanceConfig::default();
        assert!(!appearance.show_plan_chip);
        assert!(!agent.should_show_plan_chip(&appearance));
    }
    #[test]
    fn plan_chip_visible_while_plan_mode_active() {
        let mut agent = make_agent();
        agent.session.plan_mode_active = true;
        let appearance = AppearanceConfig::default();
        assert!(agent.should_show_plan_chip(&appearance));
    }
    #[test]
    fn plan_chip_visible_when_config_overrides() {
        let mut agent = make_agent();
        agent.session.plan_mode_active = false;
        let appearance = AppearanceConfig {
            show_plan_chip: true,
            ..Default::default()
        };
        assert!(!agent.should_show_plan_chip(&appearance));
    }
    #[test]
    fn set_input_mode_vim_empty_prompt_switches_to_scrollback_and_j_selects_next() {
        crate::appearance::cache::set_simple_mode(true);
        let mut agent = make_agent();
        agent.vim_mode = true;
        agent.force_active_pane(ActivePane::Prompt);
        let mut effects: Vec<crate::app::actions::Effect> = Vec::new();
        agent.set_input_mode(InputMode::Vim, &mut effects);
        assert_eq!(agent.active_pane, ActivePane::Scrollback);
        assert!(!agent.is_simple_mode());
        let registry = ActionRegistry::defaults();
        let j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        let outcome = agent.handle_scrollback_key(&j, &registry, &mut effects);
        assert!(matches!(outcome, InputOutcome::Action(Action::SelectNext)));
    }
    #[test]
    fn set_input_mode_vim_nonempty_prompt_keeps_pane() {
        let mut agent = make_agent();
        agent.force_active_pane(ActivePane::Prompt);
        agent.prompt.set_text("draft");
        let mut effects: Vec<crate::app::actions::Effect> = Vec::new();
        agent.set_input_mode(InputMode::Vim, &mut effects);
        assert_eq!(agent.active_pane, ActivePane::Prompt);
    }
    #[test]
    fn set_input_mode_simple_from_scrollback_leaves_pane_unchanged() {
        let mut agent = make_agent();
        agent.vim_mode = true;
        agent.force_active_pane(ActivePane::Scrollback);
        let mut effects: Vec<crate::app::actions::Effect> = Vec::new();
        agent.set_input_mode(InputMode::Simple, &mut effects);
        assert_eq!(agent.active_pane, ActivePane::Scrollback);
        assert!(agent.is_simple_mode());
        let registry = ActionRegistry::defaults();
        let x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        let outcome = agent.handle_scrollback_key(&x, &registry, &mut effects);
        assert_eq!(agent.active_pane, ActivePane::Scrollback);
        assert!(matches!(outcome, InputOutcome::Unchanged));
    }
    #[test]
    fn new_agent_respects_persisted_simple_mode_for_mode_and_pane() {
        crate::appearance::cache::set_simple_mode(true);
        let a1 = make_agent();
        assert!(a1.is_simple_mode());
        assert_eq!(a1.active_pane, ActivePane::Prompt);
        crate::appearance::cache::set_simple_mode(false);
        let a2 = make_agent();
        assert!(!a2.is_simple_mode());
        assert_eq!(a2.active_pane, ActivePane::Scrollback);
    }
    #[test]
    fn set_input_mode_reconciles_pane_orthogonal_to_active_modal_field() {
        let mut agent = make_agent();
        agent.force_active_pane(ActivePane::Prompt);
        agent.active_modal = None;
        let mut effects: Vec<crate::app::actions::Effect> = Vec::new();
        agent.set_input_mode(InputMode::Vim, &mut effects);
        assert_eq!(agent.active_pane, ActivePane::Scrollback);
        assert!(agent.active_modal.is_none());
    }
    #[test]
    fn scrollback_j_with_vim_mode_off_forwards_to_prompt() {
        crate::appearance::cache::set_vim_mode(false);
        let mut agent = make_agent();
        agent.vim_mode = false;
        agent.force_active_pane(ActivePane::Scrollback);
        let registry = ActionRegistry::defaults();
        let j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        let mut effects: Vec<crate::app::actions::Effect> = Vec::new();
        let outcome = agent.handle_scrollback_key(&j, &registry, &mut effects);
        assert!(
            matches!(
                outcome,
                InputOutcome::ActionThenForward(Action::FocusPrompt)
            ),
            "vim-off: bare 'j' in scrollback must forward to prompt; got {outcome:?}"
        );
    }
    #[test]
    fn scrollback_j_with_vim_mode_on_selects_next() {
        crate::appearance::cache::set_vim_mode(true);
        let mut agent = make_agent();
        agent.vim_mode = true;
        agent.force_active_pane(ActivePane::Scrollback);
        let registry = ActionRegistry::defaults();
        let j = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        let mut effects: Vec<crate::app::actions::Effect> = Vec::new();
        let outcome = agent.handle_scrollback_key(&j, &registry, &mut effects);
        assert!(
            matches!(outcome, InputOutcome::Action(Action::SelectNext)),
            "vim-on: bare 'j' in scrollback must dispatch SelectNext; got {outcome:?}"
        );
    }
    #[test]
    fn scrollback_arrow_down_works_in_both_modes() {
        let registry = ActionRegistry::defaults();
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let mut effects: Vec<crate::app::actions::Effect> = Vec::new();
        let mut a_off = make_agent();
        a_off.vim_mode = false;
        a_off.force_active_pane(ActivePane::Scrollback);
        assert!(matches!(
            a_off.handle_scrollback_key(&down, &registry, &mut effects),
            InputOutcome::Action(Action::SelectNext)
        ));
        let mut a_on = make_agent();
        a_on.vim_mode = true;
        a_on.force_active_pane(ActivePane::Scrollback);
        assert!(matches!(
            a_on.handle_scrollback_key(&down, &registry, &mut effects),
            InputOutcome::Action(Action::SelectNext)
        ));
    }
}
#[cfg(test)]
mod plan_approval_enter_tests {
    use super::test_fixtures::make_agent;
    use super::*;
    use crate::views::plan_approval_view::PlanApprovalFocus;
    fn enter_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    }
    fn agent_with_revise_prompt() -> AgentView {
        let mut agent = make_agent();
        let (tx, _rx) = tokio::sync::oneshot::channel();
        let request = crate::views::plan_approval_view::PlanApprovalExtRequest {
            session_id: "test-session".into(),
            tool_call_id: "call-1".into(),
            plan_content: "# Plan\n\n## Step 1\nDo something".into(),
            submission_id: 1,
            artifact_revision: 2,
            artifact_hash: "a".repeat(64),
        };
        let mut pav = crate::views::plan_approval_view::PlanApprovalViewState::new(
            request,
            crate::views::prompt_widget::StashedPrompt {
                text: String::new(),
                cursor: 0,
                images: Vec::new(),
                chip_elements: Vec::new(),
                image_counter: 0,
                image_undo_stash: Vec::new(),
            },
            tx,
        );
        pav.focus = PlanApprovalFocus::Prompt;
        agent.plan_approval_view = Some(pav);
        agent.prompt.set_text("");
        agent
    }
    #[test]
    fn empty_enter_on_revise_prompt_does_not_approve() {
        let mut agent = agent_with_revise_prompt();
        let outcome = agent.handle_plan_feedback_key(&enter_key());
        assert!(matches!(outcome, InputOutcome::Changed));
        assert!(
            agent.plan_approval_view.is_some(),
            "empty Enter must leave plan approval open"
        );
        assert_eq!(
            agent.toast.as_ref().map(|(msg, _)| msg.as_str()),
            Some("Type revision notes, or press a to approve.")
        );
    }
    #[test]
    fn enter_with_revision_text_requests_changes() {
        let mut agent = agent_with_revise_prompt();
        agent.prompt.set_text("please use auth middleware");
        let outcome = agent.handle_plan_feedback_key(&enter_key());
        assert!(matches!(
            outcome,
            InputOutcome::Action(Action::CheckPlanApprovalDecision)
        ));
        assert!(
            agent
                .plan_approval_view
                .as_ref()
                .unwrap()
                .decision_pending()
        );
        assert_eq!(
            agent.toast.as_ref().map(|(msg, _)| msg.as_str()),
            Some("Plan decision sent; waiting for confirmation.")
        );
    }
    #[test]
    fn empty_enter_with_pending_comments_still_requests_changes() {
        let mut agent = agent_with_revise_prompt();
        if let Some(ref mut pav) = agent.plan_approval_view {
            pav.comments.push(PlanComment {
                id: 1,
                line_range: 0..1,
                text: "nit".into(),
            });
        }
        let outcome = agent.handle_plan_feedback_key(&enter_key());
        assert!(matches!(
            outcome,
            InputOutcome::Action(Action::CheckPlanApprovalDecision)
        ));
        assert!(
            agent
                .plan_approval_view
                .as_ref()
                .unwrap()
                .decision_pending()
        );
        assert_eq!(
            agent.toast.as_ref().map(|(msg, _)| msg.as_str()),
            Some("Plan decision sent; waiting for confirmation.")
        );
    }
    #[test]
    fn a_on_empty_revise_prompt_approves() {
        let mut agent = agent_with_revise_prompt();
        let a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        let outcome = agent.handle_plan_feedback_key(&a);
        assert!(matches!(
            outcome,
            InputOutcome::Action(Action::CheckPlanApprovalDecision)
        ));
        assert!(
            agent
                .plan_approval_view
                .as_ref()
                .unwrap()
                .decision_pending()
        );
        assert_ne!(
            agent.toast.as_ref().map(|(msg, _)| msg.as_str()),
            Some("Plan revision sent.")
        );
    }

    #[test]
    fn approving_with_comments_carries_feedback_in_the_approval_response() {
        let mut agent = make_agent();
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let request = crate::views::plan_approval_view::PlanApprovalExtRequest {
            session_id: "test-session".into(),
            tool_call_id: "call-with-comments".into(),
            plan_content: "# Plan\n\nDo something".into(),
            submission_id: 1,
            artifact_revision: 2,
            artifact_hash: "a".repeat(64),
        };
        let mut pav = crate::views::plan_approval_view::PlanApprovalViewState::new(
            request,
            crate::views::prompt_widget::StashedPrompt {
                text: String::new(),
                cursor: 0,
                images: Vec::new(),
                chip_elements: Vec::new(),
                image_counter: 0,
                image_undo_stash: Vec::new(),
            },
            tx,
        );
        pav.comments.push(PlanComment {
            id: 1,
            line_range: 2..3,
            text: "keep the public API stable".into(),
        });
        agent.plan_approval_view = Some(pav);

        assert!(matches!(
            agent.approve_plan(),
            InputOutcome::Action(Action::CheckPlanApprovalDecision)
        ));
        assert!(
            agent
                .plan_approval_view
                .as_ref()
                .unwrap()
                .decision_pending()
        );
        assert_eq!(
            agent
                .pending_plan_approval_decision
                .as_ref()
                .unwrap()
                .artifact_hash,
            "a".repeat(64)
        );
        let response = rx
            .try_recv()
            .expect("approval response should be sent synchronously")
            .expect("approval response should succeed");
        let parsed: crate::views::plan_approval_view::PlanApprovalExtResponse =
            serde_json::from_str(response.0.get()).expect("typed approval response");
        assert_eq!(
            parsed.outcome,
            crate::views::plan_approval_view::PlanApprovalOutcome::Approved
        );
        assert!(
            parsed
                .feedback
                .as_deref()
                .is_some_and(|feedback| feedback.contains("keep the public API stable"))
        );
        let pending_decision = agent.pending_plan_approval_decision.as_ref().unwrap();
        assert_eq!(pending_decision.response.decision_id, parsed.decision_id);
        let retry = agent.approve_plan();
        assert!(matches!(
            retry,
            InputOutcome::Action(Action::CheckPlanApprovalDecision)
        ));
        assert!(
            rx.try_recv().is_err(),
            "recheck must not resend the reverse response"
        );
    }

    #[test]
    fn approval_moves_different_submission_notes_to_stashed_draft_not_feedback() {
        let mut agent = agent_with_revise_prompt();
        let old_content = "# Same plan body";
        let hash = blake3::hash(old_content.as_bytes()).to_hex().to_string();
        let (tx, _rx) = tokio::sync::oneshot::channel();
        let mut pav = crate::views::plan_approval_view::PlanApprovalViewState::new(
            crate::views::plan_approval_view::PlanApprovalExtRequest {
                session_id: "test-session".into(),
                tool_call_id: "new-submission".into(),
                plan_content: old_content.into(),
                submission_id: 2,
                artifact_revision: 5,
                artifact_hash: hash.clone(),
            },
            crate::views::prompt_widget::StashedPrompt {
                text: "original restored draft".into(),
                cursor: 7,
                images: Vec::new(),
                chip_elements: vec![crate::app::session::ChipElement {
                    range: 0..8,
                    kind: crate::views::prompt_widget::KIND_PASTE,
                    display: None,
                }],
                image_counter: 0,
                image_undo_stash: Vec::new(),
            },
            tx,
        );
        pav.comments.push(PlanComment {
            id: 0,
            line_range: 0..1,
            text: "new approval feedback".into(),
        });
        agent.plan_approval_view = Some(pav);
        agent.plan_comments_artifact = Some(PlanArtifactPreview {
            session_id: "test-session".into(),
            binding_epoch: agent.session_binding_epoch,
            submission_id: 1,
            retained: false,
            artifact_revision: 5,
            artifact_hash: hash,
            plan_content: old_content.into(),
        });
        agent.plan_comments.push(PlanComment {
            id: 3,
            line_range: 0..1,
            text: "old submission note".into(),
        });

        assert!(matches!(
            agent.approve_plan(),
            InputOutcome::Action(Action::CheckPlanApprovalDecision)
        ));
        let stashed = &agent.plan_approval_view.as_ref().unwrap().stashed_prompt;
        assert!(stashed.text.starts_with("original restored draft\n\n"));
        assert!(stashed.text.contains("submission 1 (revision 5, hash "));
        assert!(stashed.text.contains("old submission note"));
        assert_eq!(stashed.cursor, stashed.text.len());
        assert_eq!(stashed.chip_elements[0].range, 0..8);
        let pending = agent.pending_plan_approval_decision.as_ref().unwrap();
        assert_eq!(pending.comments.len(), 1);
        assert_eq!(pending.comments[0].text, "new approval feedback");
        assert!(
            !pending
                .response
                .feedback
                .as_deref()
                .unwrap_or_default()
                .contains("old submission note")
        );
        assert_eq!(agent.plan_comments.len(), 1);
        assert_eq!(agent.plan_comments[0].text, "new approval feedback");
        assert_eq!(
            agent.plan_comments_artifact.as_ref().unwrap().submission_id,
            2
        );
    }

    #[test]
    fn deferring_keeps_comment_identity_until_receipt_confirmation() {
        let mut agent = make_agent();
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let request = crate::views::plan_approval_view::PlanApprovalExtRequest {
            session_id: "test-session".into(),
            tool_call_id: "call-defer".into(),
            plan_content: "# Plan\n\nDo something".into(),
            submission_id: 17,
            artifact_revision: 23,
            artifact_hash: "b".repeat(64),
        };
        let mut pav = crate::views::plan_approval_view::PlanApprovalViewState::new(
            request,
            crate::views::prompt_widget::StashedPrompt {
                text: "original prompt".into(),
                cursor: 15,
                images: Vec::new(),
                chip_elements: Vec::new(),
                image_counter: 0,
                image_undo_stash: Vec::new(),
            },
            tx,
        );
        pav.next_comment_id = 2;
        pav.comments.push(PlanComment {
            id: 1,
            line_range: 2..3,
            text: "preserve this note".into(),
        });
        agent.plan_approval_view = Some(pav);
        agent.prompt.set_text("extra feedback");

        assert!(matches!(
            agent.defer_plan(),
            InputOutcome::Action(Action::CheckPlanApprovalDecision)
        ));
        assert!(
            agent
                .plan_approval_view
                .as_ref()
                .unwrap()
                .decision_pending()
        );
        assert!(agent.line_viewer.is_none());
        assert_eq!(agent.prompt.text(), "extra feedback");
        assert_eq!(agent.plan_next_comment_id, 2);
        assert_eq!(agent.plan_comments.len(), 1);
        assert_eq!(agent.plan_comments[0].text, "preserve this note");
        let comment_source = agent.plan_comments_artifact.as_ref().unwrap();
        assert_eq!(comment_source.submission_id, 17);
        assert_eq!(comment_source.artifact_hash, "b".repeat(64));
        assert_eq!(comment_source.artifact_revision, 23);
        let pending = agent.pending_plan_approval_decision.as_ref().unwrap();
        assert_eq!(pending.response.submission_id, 17);
        assert_eq!(pending.artifact_revision, 23);
        assert_eq!(pending.artifact_hash, "b".repeat(64));
        assert_eq!(pending.comments.len(), 1);
        assert!(
            pending
                .response
                .feedback
                .as_deref()
                .unwrap()
                .contains("extra feedback")
        );

        let response = rx
            .try_recv()
            .expect("deferred response should be sent")
            .expect("response channel should succeed");
        let parsed: crate::views::plan_approval_view::PlanApprovalExtResponse =
            serde_json::from_str(response.0.get()).expect("typed deferred response");
        assert_eq!(
            parsed.outcome,
            crate::views::plan_approval_view::PlanApprovalOutcome::Deferred
        );
        assert_eq!(parsed.submission_id, 17);
        let feedback = parsed.feedback.expect("comments and freeform feedback");
        assert!(feedback.contains("preserve this note"));
        assert!(feedback.contains("extra feedback"));
    }

    #[test]
    fn a_with_pending_comments_does_not_approve() {
        let mut agent = agent_with_revise_prompt();
        if let Some(ref mut pav) = agent.plan_approval_view {
            pav.comments.push(PlanComment {
                id: 1,
                line_range: 0..1,
                text: "nit".into(),
            });
        }
        let a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        let _ = agent.handle_plan_feedback_key(&a);
        assert!(
            agent.plan_approval_view.is_some(),
            "`a` with pending comments must type, not approve"
        );
        assert_eq!(agent.prompt.text(), "a");
    }
}
