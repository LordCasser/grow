//! Model-assisted recall over the calling agent's immutable branch history.

use std::collections::BTreeSet;
use std::sync::{Arc, Weak};
use std::time::Duration;

use sampling_types::{ConversationItem, ConversationRequest};
use tokio::sync::{mpsc, oneshot};
use tools::implementations::context_recall::ContextRecallBackend;

use crate::session::SessionActor;
use crate::session::sideband::{SidebandInput, sideband_backend, sideband_finish, sideband_usage};

const CONTEXT_RECALL_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_ARCHIVE_ITEM_CHARS: usize = 12_000;
const MAX_ARCHIVE_BUDGET_TOKENS: u64 = 120_000;
const MAX_RECALL_OUTPUT_TOKENS: u64 = 2_048;
const MIN_RECALL_OUTPUT_TOKENS: u64 = 256;
const MIN_RECALL_ARCHIVE_TOKENS: u64 = 2_000;

const CONTEXT_RECALL_SYSTEM_PROMPT: &str = "You are a read-only context recall sideband. Search the supplied archived session excerpts for the requested fact, decision, constraint, or prior work. Return a concise, evidence-based recollection in the same language as the request. Do not continue the task, call tools, invent missing details, or describe the compaction mechanism. If the requested information is absent, say that it was not found.";

pub(crate) struct ContextRecallRequest {
    query: String,
    reply: oneshot::Sender<Result<String, String>>,
}

pub(crate) type ContextRecallReceiver = mpsc::UnboundedReceiver<ContextRecallRequest>;

pub(crate) struct ShellContextRecallBackend {
    sender: mpsc::UnboundedSender<ContextRecallRequest>,
}

pub(crate) fn context_recall_channel() -> (Arc<dyn ContextRecallBackend>, ContextRecallReceiver) {
    let (sender, receiver) = mpsc::unbounded_channel();
    (Arc::new(ShellContextRecallBackend { sender }), receiver)
}

#[async_trait::async_trait]
impl ContextRecallBackend for ShellContextRecallBackend {
    async fn recall(
        &self,
        query: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let (reply, result) = oneshot::channel();
        self.sender
            .send(ContextRecallRequest {
                query: query.to_owned(),
                reply,
            })
            .map_err(|_| std::io::Error::other("context recall service is unavailable"))?;
        result
            .await
            .map_err(|_| std::io::Error::other("context recall service stopped"))?
            .map_err(|error| std::io::Error::other(error).into())
    }
}

/// Tool execution can run on a Send worker, while `SessionActor` is LocalSet
/// owned. The channel is the deliberate boundary between the two runtimes.
pub(crate) fn serve_context_recall(
    session: &Arc<SessionActor>,
    mut receiver: ContextRecallReceiver,
) {
    let session = Arc::downgrade(session);
    tokio::task::spawn_local(async move {
        while let Some(request) = receiver.recv().await {
            let result = match Weak::upgrade(&session) {
                Some(session) => session.run_context_recall(&request.query).await,
                None => Err("calling session no longer exists".into()),
            };
            let _ = request.reply.send(result);
        }
    });
}

impl SessionActor {
    async fn run_context_recall(&self, query: &str) -> Result<String, String> {
        let (input_ref, transcript) = self
            .chat_state_handle
            .materialize_branch_transcript(self.session_info.id.to_string())
            .await
            .ok_or_else(|| "chat-state actor is unavailable".to_string())?;
        if transcript.is_empty() {
            return Err("the current Timeline branch has no conversation context".into());
        }

        let sampling_client = self
            .prepare_chat_completion(false)
            .await
            .map_err(|error| error.to_string())?;
        let sampling_config = self
            .chat_state_handle
            .get_sampling_config()
            .await
            .ok_or_else(|| "sampling configuration is unavailable".to_string())?;
        let context_window = sampling_config.context_window.get();
        let parent_tokens = self.chat_state_handle.get_estimated_total_tokens().await;
        let result_wrapper_tokens = chat_state::estimate_item_tokens(&ConversationItem::user(
            format!("Recalled topic: {query}\n\nRecalled content:\n"),
        ));
        let output_budget =
            context_recall_output_budget(context_window, parent_tokens, result_wrapper_tokens)
                .ok_or_else(|| {
                    "the calling session has insufficient context headroom for a safe recall result"
                        .to_string()
                })?;
        let fixed_request_tokens = chat_state::estimate_conversation_tokens(&[
            ConversationItem::system(CONTEXT_RECALL_SYSTEM_PROMPT),
            ConversationItem::user(format!(
                "Recall request:\n{query}\n\n<archived-session-context>\n</archived-session-context>"
            )),
        ]);
        let archive_budget = context_recall_archive_budget(
            context_window,
            fixed_request_tokens,
            u64::from(output_budget),
        )
        .ok_or_else(|| {
            "the recall sideband has insufficient context headroom for archived evidence"
                .to_string()
        })?;
        let archive = select_recall_archive(transcript, query, archive_budget);
        if archive.is_empty() {
            return Err("the current Timeline branch has no readable archived text".into());
        }

        let sideband_prompt = format!(
            "Recall from the calling agent's frozen Timeline branch.\nquery: {query}\narchive_budget_tokens: {archive_budget}"
        );
        let mut sideband = self
            .begin_sideband(
                chat_state::SidebandPurpose::ContextRecall,
                sideband_prompt,
                SidebandInput::Frozen(vec![input_ref]),
                chat_state::SidebandRoute {
                    model: sampling_config.model.clone(),
                    backend: sideband_backend(sampling_client.api_backend()).into(),
                },
                None,
            )
            .await
            .map_err(|error| error.to_string())?;
        sideband
            .attempt(None)
            .await
            .map_err(|error| error.to_string())?;

        let request = ConversationRequest {
            items: vec![
                ConversationItem::system(CONTEXT_RECALL_SYSTEM_PROMPT),
                ConversationItem::user(format!(
                    "Recall request:\n{query}\n\n<archived-session-context>\n{archive}\n</archived-session-context>"
                )),
            ],
            tools: vec![],
            tool_choice: None,
            model: Some(sampling_config.model),
            temperature: None,
            max_output_tokens: Some(output_budget),
            ..ConversationRequest::default()
        };
        let response = match tokio::time::timeout(
            CONTEXT_RECALL_TIMEOUT,
            sampling_client.conversation_collect(request),
        )
        .await
        {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => {
                let message = error.to_string();
                sideband
                    .fail(chat_state::SidebandOutcome::Failed, message.clone())
                    .await
                    .map_err(|record_error| record_error.to_string())?;
                return Err(message);
            }
            Err(_) => {
                let message = "context recall sideband timed out".to_string();
                sideband
                    .fail(chat_state::SidebandOutcome::Cancelled, message.clone())
                    .await
                    .map_err(|record_error| record_error.to_string())?;
                return Err(message);
            }
        };
        let content = response.assistant_text().trim().to_owned();
        if content.is_empty() {
            let message = "context recall sideband returned an empty response".to_string();
            sideband
                .fail(chat_state::SidebandOutcome::Failed, message.clone())
                .await
                .map_err(|record_error| record_error.to_string())?;
            return Err(message);
        }

        let usage = sideband_usage(&response);
        let finish = sideband_finish(&response);
        sideband
            .complete(content.clone(), None, usage, finish)
            .await
            .map_err(|error| error.to_string())?;
        Ok(format!(
            "Recalled topic: {query}\n\nRecalled content:\n{content}"
        ))
    }
}

fn context_recall_output_budget(
    context_window: u64,
    parent_tokens: u64,
    wrapper_tokens: u64,
) -> Option<u32> {
    let next_turn_reserve = context_window.saturating_div(20).clamp(2_048, 16_384);
    let budget = context_window
        .saturating_sub(parent_tokens)
        .saturating_sub(next_turn_reserve)
        .saturating_sub(wrapper_tokens)
        .min(MAX_RECALL_OUTPUT_TOKENS);
    (budget >= MIN_RECALL_OUTPUT_TOKENS).then(|| budget as u32)
}

fn context_recall_archive_budget(
    context_window: u64,
    fixed_request_tokens: u64,
    output_budget: u64,
) -> Option<u64> {
    let provider_reserve = context_window.saturating_div(20).clamp(1_024, 8_192);
    let budget = context_window
        .saturating_sub(fixed_request_tokens)
        .saturating_sub(output_budget)
        .saturating_sub(provider_reserve)
        .min(MAX_ARCHIVE_BUDGET_TOKENS);
    (budget >= MIN_RECALL_ARCHIVE_TOKENS).then_some(budget)
}

fn select_recall_archive(
    transcript: Vec<ConversationItem>,
    query: &str,
    token_budget: u64,
) -> String {
    let terms = recall_terms(query);
    let exact = query.trim().to_lowercase();
    let entries = transcript
        .iter()
        .enumerate()
        .filter_map(|(index, item)| render_archive_item(index, item))
        .map(|text| {
            let lowered = text.to_lowercase();
            let score = u64::from(!exact.is_empty() && lowered.contains(&exact)) * 100
                + terms
                    .iter()
                    .filter(|term| lowered.contains(term.as_str()))
                    .count() as u64
                    * 10;
            let tokens = chat_state::estimate_item_tokens(&ConversationItem::user(&text));
            (text, score, tokens)
        })
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return String::new();
    }

    let total_tokens = entries.iter().map(|(_, _, tokens)| *tokens).sum::<u64>();
    if total_tokens <= token_budget {
        return entries
            .into_iter()
            .map(|(text, _, _)| text)
            .collect::<Vec<_>>()
            .join("\n\n");
    }

    let mut ranked = entries
        .iter()
        .enumerate()
        .filter(|(_, (_, score, _))| *score > 0)
        .map(|(index, (_, score, _))| (index, *score))
        .collect::<Vec<_>>();
    ranked.sort_by(|(left_index, left_score), (right_index, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| right_index.cmp(left_index))
    });

    let mut selected = BTreeSet::new();
    let mut selected_tokens = 0_u64;
    for (match_index, _) in ranked {
        let start = match_index.saturating_sub(2);
        let end = match_index.saturating_add(2).min(entries.len() - 1);
        for index in start..=end {
            if selected.contains(&index) {
                continue;
            }
            let item_tokens = entries[index].2;
            if selected_tokens.saturating_add(item_tokens) > token_budget {
                continue;
            }
            selected.insert(index);
            selected_tokens = selected_tokens.saturating_add(item_tokens);
        }
    }

    // Lexical matching is a shortlist, not a truth oracle. When it finds
    // nothing (or leaves room), backfill recent branch context so the Sideband
    // can still resolve paraphrases semantically.
    for index in (0..entries.len()).rev() {
        if selected.contains(&index) {
            continue;
        }
        let item_tokens = entries[index].2;
        if selected_tokens.saturating_add(item_tokens) > token_budget {
            continue;
        }
        selected.insert(index);
        selected_tokens = selected_tokens.saturating_add(item_tokens);
    }

    selected
        .into_iter()
        .map(|index| entries[index].0.clone())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_archive_item(index: usize, item: &ConversationItem) -> Option<String> {
    if matches!(item, ConversationItem::Reasoning(_)) {
        return None;
    }
    let (role, mut content) = match item {
        ConversationItem::System(_) => ("system", item.text_content()),
        ConversationItem::User(_) => ("user", item.text_content()),
        ConversationItem::Assistant(assistant) => {
            let mut content = assistant.content.to_string();
            for call in &assistant.tool_calls {
                content.push_str(&format!(
                    "\n[tool call name={} arguments={}]",
                    call.name, call.arguments
                ));
            }
            ("assistant", content)
        }
        ConversationItem::ToolResult(result) => (
            "tool",
            format!("call_id={}\n{}", result.tool_call_id, result.content),
        ),
        ConversationItem::BackendToolCall(_) => ("backend_tool", item.text_content()),
        ConversationItem::Reasoning(_) => unreachable!(),
    };
    if content.trim().is_empty() {
        return None;
    }
    content = tools::util::truncate::truncate_middle(&content, MAX_ARCHIVE_ITEM_CHARS);
    Some(format!("[item {index} role={role}]\n{content}"))
}

fn recall_terms(query: &str) -> Vec<String> {
    let mut terms = BTreeSet::new();
    for token in query
        .to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| token.chars().count() >= 2)
    {
        terms.insert(token.to_owned());
        let chars = token.chars().collect::<Vec<_>>();
        if chars.iter().any(|character| !character.is_ascii()) && chars.len() > 2 {
            for pair in chars.windows(2) {
                terms.insert(pair.iter().collect());
            }
        }
    }
    terms.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recall_selection_prefers_matching_old_context_and_keeps_neighbors() {
        let transcript = vec![
            ConversationItem::user("database migration discussion"),
            ConversationItem::assistant("Use a shadow table and swap atomically."),
            ConversationItem::user("unrelated work"),
            ConversationItem::assistant("more unrelated work"),
            ConversationItem::user("latest turn"),
        ];
        let archive = select_recall_archive(transcript, "database migration", 35);

        assert!(archive.contains("database migration discussion"));
        assert!(archive.contains("shadow table"));
    }

    #[test]
    fn chinese_query_terms_include_bigrams_for_retrieval() {
        let terms = recall_terms("回忆数据库迁移方案");
        assert!(terms.contains(&"数据".to_string()));
        assert!(terms.contains(&"迁移".to_string()));
    }

    #[test]
    fn output_projection_drops_private_reasoning() {
        let archive = select_recall_archive(
            vec![
                ConversationItem::user("visible fact"),
                ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
                    "private chain of thought",
                )),
            ],
            "visible",
            1_000,
        );
        assert!(archive.contains("visible fact"));
        assert!(!archive.contains("private chain of thought"));
    }

    #[test]
    fn budgets_bound_both_sideband_input_and_parent_result() {
        let output = context_recall_output_budget(32_000, 20_000, 200).unwrap();
        assert_eq!(output, MAX_RECALL_OUTPUT_TOKENS as u32);

        let archive = context_recall_archive_budget(32_000, 1_000, u64::from(output)).unwrap();
        let provider_reserve = 32_000_u64.saturating_div(20).clamp(1_024, 8_192);
        assert!(archive + 1_000 + u64::from(output) + provider_reserve <= 32_000);
    }

    #[test]
    fn recall_fails_closed_when_parent_has_no_safe_return_headroom() {
        assert!(context_recall_output_budget(8_000, 7_000, 100).is_none());
        assert!(context_recall_archive_budget(4_000, 1_500, 1_000).is_none());
    }
}
