//! Host-owned completion handshake. A provider terminator closes a response,
//! not the user's task. Text and punctuation never grant completion.

use super::*;

pub(super) const FINISH_TURN_TOOL: &str = "FinishTurn";
const MAX_COMPLETION_VIOLATIONS: u8 = 3;
const COMPLETION_REMINDER: &str = "Your last response ended without a valid FinishTurn declaration. \
    The current turn is still active. Continue the next authorized action using the available tools. \
    If the request is fulfilled, or progress requires user input or an external background result, \
    give the user the final answer or precise waiting reason and call FinishTurn alone. \
    An action preamble is not a completed task. Do not repeat actions already executed.";

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CompletionIntent {
    Completed,
    WaitingForUser,
    WaitingForBackground,
}

impl CompletionIntent {
    pub(crate) fn terminal_kind(self) -> &'static str {
        match self {
            Self::Completed => "explicit_completion",
            Self::WaitingForUser => "waiting_for_user",
            Self::WaitingForBackground => "waiting_for_background",
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FinishTurn {
    status: CompletionIntent,
    reason: String,
}

pub(super) fn finish_turn_tool() -> ToolSpec {
    ToolSpec {
        name: FINISH_TURN_TOOL.into(),
        description: Some(
            "Required to finish this turn. First write the final answer or a precise question/waiting \
             reason to the user as assistant text, then call this tool alone, exactly once, in that \
             same response. Use completed only after fulfilling the request and checking the result. \
             Use waiting_for_user only for missing input or authorization; use waiting_for_background \
             only when an external result is required and no useful authorized work remains. \
             If work remains, execute it with the available tools instead of ending with an action \
             preamble. Never combine FinishTurn with other tools. This ends only the current turn; \
             it does not complete or pause a Goal. Structured-output requests use their own contract."
                .into(),
        ),
        parameters: json!({
            "type": "object",
            "properties": {
                "status": {"type": "string", "enum": [
                    "completed", "waiting_for_user", "waiting_for_background"
                ]},
                "reason": {"type": "string", "minLength": 1,
                    "description": "What was delivered and verified, or the exact external dependency."}
            },
            "required": ["status", "reason"],
            "additionalProperties": false
        }),
    }
}

fn parse_finish_turn(arguments: &str, has_answer: bool) -> Result<CompletionIntent, String> {
    let declaration: FinishTurn = serde_json::from_str(arguments)
        .map_err(|error| format!("Invalid FinishTurn arguments: {error}"))?;
    if declaration.reason.trim().is_empty() {
        return Err("FinishTurn requires a nonempty reason.".into());
    }
    if !has_answer {
        return Err(
            "Write the final answer or waiting question as assistant text in the same response."
                .into(),
        );
    }
    Ok(declaration.status)
}

impl SessionActor {
    /// Resolve only this response's declaration. It cannot be inherited by
    /// later steering, Stop feedback, or the next user turn.
    pub(super) async fn accept_completion_intent(
        &self,
        tool_calls: &mut Vec<sampling_types::conversation::ToolCall>,
        has_answer: bool,
    ) -> Result<Option<CompletionIntent>, acp::Error> {
        let single_call = tool_calls.len() == 1;
        let mut intent = None;
        for call in tool_calls
            .iter()
            .filter(|call| call.name == FINISH_TURN_TOOL)
        {
            let validated = if single_call {
                parse_finish_turn(&call.arguments, has_answer)
            } else {
                Err("Call FinishTurn alone, exactly once, after all other tools finish.".into())
            };
            let result = match validated {
                Ok(value) => {
                    intent = Some(value);
                    format!(
                        "Turn completion declaration accepted: {}.",
                        value.terminal_kind()
                    )
                }
                Err(error) => error,
            };
            self.chat_state_handle
                .push_tool_result_durably(ConversationItem::tool_result(
                    call.id.to_string(),
                    result,
                ))
                .await
                .map_err(|error| {
                    crate::session::commands::fatal_turn_boundary_error(
                        "turn completion declaration",
                        error.to_string(),
                    )
                })?;
        }
        tool_calls.retain(|call| call.name != FINISH_TURN_TOOL);
        Ok(intent)
    }

    pub(super) async fn recover_missing_completion(
        &self,
        consecutive_violations: &mut u8,
    ) -> Result<(), acp::Error> {
        *consecutive_violations += 1;
        if *consecutive_violations >= MAX_COMPLETION_VIOLATIONS {
            return Err(acp::Error::internal_error().data(json!({
                "error_kind": "turn_completion_protocol_failed",
                "message": "Turn completion protocol failed: three consecutive responses contained neither \
                 a valid FinishTurn declaration nor an action tool call. The task was not marked complete.",
            })));
        }
        self.chat_state_handle
            .push_user_message_durably(ConversationItem::auto_recovery(COMPLETION_REMINDER))
            .await
            .map_err(|error| {
                crate::session::commands::fatal_turn_boundary_error(
                    "turn completion recovery",
                    error.to_string(),
                )
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations_are_strict_and_require_visible_answers() {
        for status in ["completed", "waiting_for_user", "waiting_for_background"] {
            let args = json!({"status": status, "reason": "verified or waiting"}).to_string();
            assert!(parse_finish_turn(&args, true).is_ok());
            assert!(parse_finish_turn(&args, false).is_err());
        }
        for args in [
            r#"{"status":"completed","reason":"  "}"#,
            r#"{"status":"continue","reason":"unfinished"}"#,
            r#"{"status":"completed"}"#,
            r#"{"status":"completed","reason":"done","extra":true}"#,
            r#"{"status":"completed","reason":"done""#,
        ] {
            assert!(parse_finish_turn(args, true).is_err(), "{args}");
        }
    }
}
