//! Terminal-usage projection for `streaming-messages-json`: reshaping the turn's
//! aggregate ledger into `result.usage` (`message.usage` shape) and the per-model
//! `modelUsage` map. Kept apart so the token/cost/model math is self-contained.

use serde_json::{Value, json};

use crate::headless::attach_result_usage;
use crate::headless::reducer::to_line;

use super::MessagesReducer;
use super::wire::{MessageUsage, ModelUsage};

/// The reshaped terminal usage: `message.usage`, `modelUsage`, turn count, cost, and API duration.
pub(super) struct ResultUsage {
    pub(super) usage: MessageUsage,
    pub(super) model_usage: Value,
    pub(super) num_turns: u64,
    pub(super) total_cost_usd: f64,
    pub(super) duration_api_ms: u64,
}

impl MessagesReducer {
    /// The Messages `result` usage, reshaped from the shell's projection into the `message.usage` shape.
    pub(super) fn messages_result_usage(&self, end_usage: Option<&Value>) -> ResultUsage {
        let mut scratch = json!({});
        if let Some(u) = end_usage {
            attach_result_usage(&mut scratch, u);
        }
        let field = |obj: Option<&Value>, key: &str| {
            obj.and_then(|o| o.get(key))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        };
        let u = scratch.get("usage");
        let usage_is_incomplete = scratch
            .get("usage_is_incomplete")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if end_usage.is_none() {
            tracing::warn!(
                "streaming-messages-json: no aggregate usage ledger at turn end; \
                 `result.usage` input/cache counts remain unknown and output \
                 falls back to zero"
            );
        } else if usage_is_incomplete {
            tracing::warn!(
                "streaming-messages-json: usage is incomplete; `result.usage` token \
                 counts may be unknown or under-count"
            );
        }
        let usage = MessageUsage {
            input_tokens: u
                .and_then(|usage| usage.get("input_tokens"))
                .and_then(Value::as_u64),
            full_input_tokens: u
                .and_then(|usage| usage.get("full_input_tokens"))
                .and_then(Value::as_u64),
            output_tokens: field(u, "output_tokens"),
            cache_read_input_tokens: u
                .and_then(|usage| usage.get("cache_read_input_tokens"))
                .and_then(Value::as_u64),
            cache_creation_input_tokens: u
                .and_then(|usage| usage.get("cache_creation_input_tokens"))
                .and_then(Value::as_u64),
            cache_read_known_input_tokens: field(u, "cache_read_known_input_tokens"),
            cache_read_unknown_calls: field(u, "cache_read_unknown_calls"),
            cache_write_unknown_calls: field(u, "cache_write_unknown_calls"),
        };
        let num_turns = scratch
            .get("num_turns")
            .and_then(Value::as_u64)
            .unwrap_or(self.completed_responses);
        let total_cost_usd = scratch
            .get("total_cost_usd")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        // `apiDurationMs` is dropped by the projection, so read it from `end_usage`.
        let duration_api_ms = end_usage.map_or(0, |u| field(Some(u), "apiDurationMs"));
        let model_usage = messages_model_usage(
            scratch.get("modelUsage"),
            self.session.as_ref().and_then(|s| s.model.as_deref()),
            self.session.as_ref().and_then(|s| s.context_window),
        );
        ResultUsage {
            usage,
            model_usage,
            num_turns,
            total_cost_usd,
            duration_api_ms,
        }
    }
}

/// Map the ledger's per-model rows into `ModelUsage` entries; `context_window`
/// goes to `current_model` only. `{}` when there is no breakdown.
pub(super) fn messages_model_usage(
    rows: Option<&Value>,
    current_model: Option<&str>,
    context_window: Option<u64>,
) -> Value {
    let Some(Value::Object(map)) = rows else {
        return json!({});
    };
    let out: serde_json::Map<String, Value> = map
        .iter()
        .map(|(model, row)| {
            let n = |k: &str| row.get(k).and_then(Value::as_u64).unwrap_or(0);
            let is_current = Some(model.as_str()) == current_model;
            (
                model.clone(),
                to_line(&ModelUsage {
                    input_tokens: row.get("inputTokens").and_then(Value::as_u64),
                    full_input_tokens: n("fullInputTokens"),
                    output_tokens: n("outputTokens"),
                    cache_read_input_tokens: n("cacheReadInputTokens"),
                    cache_creation_input_tokens: n("cacheCreationInputTokens"),
                    cache_read_known_input_tokens: n("cacheReadKnownInputTokens"),
                    cache_read_unknown_calls: n("cacheReadUnknownCalls"),
                    cache_write_unknown_calls: n("cacheWriteUnknownCalls"),
                    cost_usd: row.get("costUSD").and_then(Value::as_f64).unwrap_or(0.0),
                    context_window: if is_current { context_window } else { None },
                }),
            )
        })
        .collect();
    Value::Object(out)
}
