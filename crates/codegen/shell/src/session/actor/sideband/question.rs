//! Build tool-free questions from a bounded selection of one acknowledged Surface.

use super::{SessionActor, SidebandImageSources, SidebandRun, SidebandRunError, SidebandSource};
use chat_state::{
    SidebandBudgetPolicy, SidebandOutcome, SidebandPurpose, SidebandRoute, SurfaceId,
    TimelineMaterialization,
};
use sampling_types::{ApiBackend, ConversationItem, ConversationRequest};

pub(in crate::session::actor) struct FrozenQuestion {
    pub(in crate::session::actor) request: ConversationRequest,
    pub(in crate::session::actor) context_ids: Vec<SurfaceId>,
}

pub(super) fn input_budget_error(estimated: u64, limit: u64) -> SidebandRunError {
    SidebandRunError::Admission(format!(
        "question input budget exceeded: estimated {estimated} tokens, limit {limit}"
    ))
}

impl FrozenQuestion {
    fn select(
        snapshot: &TimelineMaterialization,
        prompt: String,
        model: String,
        limit: u64,
    ) -> Result<Self, SidebandRunError> {
        let question = ConversationItem::user(prompt);
        let projected = sampling_types::project_portable_history_with_indices(&snapshot.surface);
        let mut mandatory = chat_state::estimate_item_tokens(&question);
        let mut total = mandatory;
        for (_, item) in &projected {
            let cost = chat_state::estimate_item_tokens(item);
            if matches!(item, ConversationItem::System(_)) {
                mandatory += cost;
            }
            total += cost;
        }
        if mandatory > limit {
            return Err(input_budget_error(mandatory, limit));
        }

        // Drop oldest units until the suffix fits. An assistant's paired results
        // and any model skill facts attached to them leave the window together.
        let mut start = 0;
        while total > limit {
            let (source, item) = &projected[start];
            start += 1;
            if matches!(item, ConversationItem::System(_)) {
                continue;
            }
            total -= chat_state::estimate_item_tokens(item);
            if let ConversationItem::Assistant(assistant) = item
                && !assistant.tool_calls.is_empty()
            {
                // Skill metadata may have moved behind later results, including
                // metadata for a call that portable projection filtered out.
                let batch_end = (source + 1..snapshot.surface.len())
                    .find(|&i| {
                        !matches!(snapshot.surface[i], ConversationItem::ToolResult(_))
                            && !sampling_types::is_model_skill_result_metadata(&snapshot.surface, i)
                    })
                    .unwrap_or(snapshot.surface.len());
                while let Some((index, next)) = projected.get(start)
                    && (source + 1..batch_end).contains(index)
                {
                    total -= chat_state::estimate_item_tokens(next);
                    start += 1;
                }
            }
        }

        let (mut items, context_ids): (Vec<_>, Vec<_>) = projected
            .into_iter()
            .enumerate()
            .filter(|(position, (_, item))| {
                *position >= start || matches!(item, ConversationItem::System(_))
            })
            .map(|(_, (index, item))| (item, snapshot.surface_ids[index]))
            .unzip();
        items.push(question);
        Ok(Self {
            request: ConversationRequest {
                items,
                model: Some(model),
                ..Default::default()
            },
            context_ids,
        })
    }

    pub(in crate::session::actor) async fn attempt(
        &self,
        run: &mut SidebandRun,
        snapshot: &TimelineMaterialization,
        backend: ApiBackend,
        feedback: Option<String>,
    ) -> Result<(), SidebandRunError> {
        let result = run
            .attempt_selected(
                &self.request,
                backend,
                vec![snapshot.input_ref.clone()],
                Some(snapshot.surface_revision),
                self.context_ids.clone(),
                Vec::new(),
                "portable-question-window",
                feedback,
            )
            .await;
        if let Err(error @ SidebandRunError::Admission(_)) = &result {
            run.fail(SidebandOutcome::Failed, error.to_string()).await?;
        }
        result
    }

    pub(in crate::session::actor) async fn prepare_images(
        &mut self,
        actor: &SessionActor,
        run: &mut SidebandRun,
        revision: u64,
        key: &sampling_types::ModelImageInputKey,
    ) -> Result<(), SidebandRunError> {
        if let Err(detail) = actor
            .prepare_sideband_images(
                &mut self.request.items,
                SidebandImageSources::Selection(&self.context_ids, revision),
                key,
            )
            .await
        {
            if actor.sideband_cancel.is_cancelled() {
                return Err(SidebandRunError::Cancelled);
            }
            run.fail(SidebandOutcome::Failed, &detail).await?;
            return Err(SidebandRunError::Admission(detail));
        }
        Ok(())
    }
}

impl SessionActor {
    pub(in crate::session::actor) async fn prepare_frozen_question(
        &self,
        snapshot: &TimelineMaterialization,
        prompt: String,
        purpose: SidebandPurpose,
        route: SidebandRoute,
        limit: u64,
        max_attempts: u32,
    ) -> Result<(SidebandRun, FrozenQuestion), SidebandRunError> {
        let model = route.model.clone();
        let mut run = self
            .begin_sideband(
                purpose,
                prompt.clone(),
                SidebandSource::Frozen(vec![snapshot.input_ref.clone()]),
                SidebandBudgetPolicy {
                    max_attempts,
                    max_input_tokens_per_attempt: limit,
                    max_output_tokens_per_attempt: None,
                },
                route,
                None,
            )
            .await?;
        match FrozenQuestion::select(snapshot, prompt, model, limit) {
            Ok(question) => Ok((run, question)),
            Err(error) => {
                run.fail(SidebandOutcome::Failed, error.to_string()).await?;
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests;
