//! `grow/session/usage` — lifetime session usage plus cold-resume segments.
//!
//! Projects the in-memory [`chat_state::UsageLedger`] (main-loop + folded
//! subagent spend). Partial costs are scrubbed (absence ≠ free).

use acp_transport::protocol as acp;
use serde::{Deserialize, Serialize};

use super::{ExtResult, parse_params, to_raw_response};
use crate::agent::MvpAgent;
use crate::extensions::notification::PromptUsage;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionUsageRequest {
    session_id: String,
}

/// Wire response for `grow/session/usage`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUsageResponse {
    pub usage: PromptUsage,
    pub segments: Vec<SessionUsageSegment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUsageSegment {
    pub index: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<i64>,
    pub usage: PromptUsage,
}

#[tracing::instrument(skip_all, fields(method = %args.method))]
pub async fn handle(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    match args.method.as_ref() {
        "grow/session/usage" => handle_session_usage(agent, args).await,
        _ => Err(acp::Error::method_not_found()),
    }
}

async fn handle_session_usage(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    let req: SessionUsageRequest = parse_params(args)?;
    let session_id = acp::SessionId::new(req.session_id.as_str());

    // Wait out in-flight session/load rather than racing reconnect to not-found.
    let Some(handle) = agent.session_handle_waiting_for_load(&session_id).await else {
        return Err(acp::Error::resource_not_found(Some(format!(
            "session not found: {}",
            req.session_id
        ))));
    };

    // Fail closed: a dead chat-state actor is an error, never a zero bill.
    let ledger = handle
        .chat_state_handle
        .try_get_session_usage()
        .await
        .map_err(|()| acp::Error::internal_error().data("failed to read session usage"))?;

    let segments = ledger
        .segments
        .iter()
        .enumerate()
        .map(|(index, segment)| SessionUsageSegment {
            index: index as u64,
            started_at_ms: segment.started_at_ms,
            usage: PromptUsage::from(segment),
        })
        .collect();
    to_raw_response(&SessionUsageResponse {
        usage: PromptUsage::from(&ledger),
        segments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat_state::UsageLedger;
    use sampling_types::TokenUsage;

    fn usage(prompt: u32, completion: u32) -> TokenUsage {
        TokenUsage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: 0,
            reasoning_tokens: 0,
            cached_prompt_tokens: 0,
            cache_creation_prompt_tokens: 0,
        }
    }

    #[test]
    fn response_serializes_ledger_as_prompt_usage_wire_shape() {
        let mut ledger = UsageLedger::default();
        ledger.record_main_loop_call("grow-build", &usage(100, 10), Some(50), Some(20_000_000));
        ledger.record_subagent(
            "child-1",
            &[(
                "grow-build".into(),
                chat_state::UsageTotals {
                    input_tokens: 30,
                    output_tokens: 5,
                    model_calls: 1,
                    cost_usd_ticks: Some(10_000_000),
                    ..Default::default()
                },
            )],
            false,
        );
        let v = serde_json::to_value(&SessionUsageResponse {
            usage: PromptUsage::from(&ledger),
            segments: ledger
                .segments
                .iter()
                .enumerate()
                .map(|(index, segment)| SessionUsageSegment {
                    index: index as u64,
                    started_at_ms: segment.started_at_ms,
                    usage: PromptUsage::from(segment),
                })
                .collect(),
        })
        .unwrap();
        assert_eq!(v["usage"]["inputTokens"], 130);
        assert_eq!(v["usage"]["outputTokens"], 15);
        assert_eq!(v["usage"]["numTurns"], 1);
        assert_eq!(v["usage"]["costUsdTicks"], 30_000_000);
        assert_eq!(v["usage"]["modelUsage"]["grow-build"]["inputTokens"], 130);
        assert!(v["usage"].get("agentUsage").is_none());
        assert_eq!(v["segments"][0]["usage"]["inputTokens"], 130);
        assert!(v["segments"][0]["usage"].get("agentUsage").is_none());
        let rt: SessionUsageResponse = serde_json::from_value(v).unwrap();
        assert_eq!(rt.usage.totals.cost_usd_ticks, Some(30_000_000));
    }

    #[test]
    fn response_scrubs_partial_costs() {
        let mut ledger = UsageLedger::default();
        ledger.record_main_loop_call("a", &usage(100, 10), None, Some(70));
        ledger.record_main_loop_call("a", &usage(50, 5), None, None);
        let v = serde_json::to_value(&SessionUsageResponse {
            usage: PromptUsage::from(&ledger),
            segments: ledger
                .segments
                .iter()
                .enumerate()
                .map(|(index, segment)| SessionUsageSegment {
                    index: index as u64,
                    started_at_ms: segment.started_at_ms,
                    usage: PromptUsage::from(segment),
                })
                .collect(),
        })
        .unwrap();
        assert_eq!(v["usage"]["costUsdTicks"], serde_json::Value::Null);
        assert_eq!(v["usage"]["costIsPartial"], true);
    }
}
