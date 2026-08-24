//! Pure request/response boundary for session-title generation.

use crate::sampling::{ConversationItem, ConversationRequest, JsonOutputFormat};
use crate::session::helpers::text::floor_char_boundary;

/// Upper bound on the user text that feeds title generation; titles only need
/// the opening, and this keeps the request well under the model prompt limit.
const TITLE_SOURCE_MAX_BYTES: usize = 8_000;

pub(crate) const SESSION_TITLE_PROMPT: &str = r#"You are tasked with generating the session title. The user is asking almost always software engineering related questions on their codebase.
We describe the session title below
# Session Title
A short and distinctive 5-10 word descriptive title for the session. Super info dense, no filler.

You will be given the user query below encapsulated in <user_query></user_query>.

Return exactly one JSON object matching this shape and nothing else:
{"session_title":"your title"}"#;

pub(crate) const SESSION_TITLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionTitle {
    session_title: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SessionTitleResponseError {
    #[error("session_title output is invalid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("session_title is empty")]
    EmptyTitle,
    #[error("session_title exceeds 160 characters")]
    TitleTooLong,
}

/// Remove `<system-reminder>…</system-reminder>` blocks from `text` — they are
/// system-injected context (e.g. the `/goal` setup reminder), not the user's
/// words, so they must not drive the session title.
fn strip_system_reminder_blocks(text: &str) -> String {
    const OPEN: &str = "<system-reminder>";
    const CLOSE: &str = "</system-reminder>";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + OPEN.len()..];
        // An unterminated reminder drops the remainder — it is system text.
        let Some(end) = after_open.find(CLOSE) else {
            return out.trim().to_string();
        };
        rest = &after_open[end + CLOSE.len()..];
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// Text the session title is derived from: strip system reminders and skill XML
/// markup, then cap to the first few KB. Stripping runs before the cap so a
/// leading reminder larger than the cap is still removed.
pub(crate) fn title_source_text(user_message: &str) -> String {
    let without_reminders = strip_system_reminder_blocks(user_message);
    let base = if without_reminders.is_empty() {
        user_message
    } else {
        &without_reminders
    };
    let mut display = tools::implementations::skills::skill::extract_skill_display_text(base)
        .unwrap_or_else(|| base.to_string());
    display.truncate(floor_char_boundary(&display, TITLE_SOURCE_MAX_BYTES));
    display
}

pub(crate) fn title_fallback_from_user_text(user_message: &str) -> String {
    let text = title_source_text(user_message);
    let s = text
        .split_whitespace()
        .take(10)
        .collect::<Vec<_>>()
        .join(" ");
    if s.is_empty() {
        "New session".to_string()
    } else {
        s
    }
}

pub(crate) fn session_title_output_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["session_title"],
        "properties": {
            "session_title": {
                "type": "string",
                "description": "Final session title, just 5-10 word descriptive title for the session. Super info dense, no filler."
            }
        },
        "additionalProperties": false
    })
}

pub(crate) fn build_session_title_request(
    user_message: &str,
    model: &str,
    backend: sampling_types::ApiBackend,
) -> ConversationRequest {
    let clean_message = title_source_text(&user_message);
    let mut request = ConversationRequest::from_items(vec![
        ConversationItem::system(SESSION_TITLE_PROMPT),
        ConversationItem::user(format!(
            r#"<user_query>
{}
</user_query>"#,
            clean_message
        )),
    ])
    .with_model(model);
    request.json_output = Some(match backend {
        sampling_types::ApiBackend::ChatCompletions => JsonOutputFormat::JsonObject,
        sampling_types::ApiBackend::Responses | sampling_types::ApiBackend::Messages => {
            JsonOutputFormat::JsonSchema(session_title_output_schema())
        }
    });
    request
}

/// Validate the model's native structured output and return its normalized title.
pub(crate) fn parse_session_title_output(
    raw_output: &str,
) -> Result<String, SessionTitleResponseError> {
    let parsed = serde_json::from_str::<SessionTitle>(raw_output.trim())?;
    let normalized = parsed
        .session_title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        return Err(SessionTitleResponseError::EmptyTitle);
    }
    if normalized.chars().count() > 160 {
        return Err(SessionTitleResponseError::TitleTooLong);
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::{
        SESSION_TITLE_PROMPT, SessionTitleResponseError, TITLE_SOURCE_MAX_BYTES,
        build_session_title_request, parse_session_title_output, strip_system_reminder_blocks,
        title_fallback_from_user_text, title_source_text,
    };

    #[test]
    fn title_request_uses_native_structured_output_without_tools() {
        for backend in [
            sampling_types::ApiBackend::ChatCompletions,
            sampling_types::ApiBackend::Responses,
            sampling_types::ApiBackend::Messages,
        ] {
            let expects_schema = matches!(
                backend,
                sampling_types::ApiBackend::Responses | sampling_types::ApiBackend::Messages
            );
            let request = build_session_title_request("fix the auth bug", "test-model", backend);
            assert!(request.tools.is_empty());
            assert!(request.tool_choice.is_none());
            if expects_schema {
                assert!(matches!(
                    request.json_output,
                    Some(sampling_types::JsonOutputFormat::JsonSchema(_))
                ));
            } else {
                assert!(matches!(
                    request.json_output,
                    Some(sampling_types::JsonOutputFormat::JsonObject)
                ));
            }
            assert_eq!(request.items[0].text_content(), SESSION_TITLE_PROMPT);
        }
    }

    #[test]
    fn title_output_is_normalized_and_strictly_validated() {
        assert_eq!(
            parse_session_title_output(r#"{"session_title":"  Fix   auth  bug "}"#).unwrap(),
            "Fix auth bug"
        );
        assert!(matches!(
            parse_session_title_output(r#"{"session_title":"ok","extra":true}"#),
            Err(SessionTitleResponseError::InvalidJson(_))
        ));
    }

    #[test]
    fn title_source_text_caps_oversized_input() {
        let big = "word ".repeat(10_000);
        let out = title_source_text(&big);
        assert!(!out.is_empty() && out.len() <= TITLE_SOURCE_MAX_BYTES);
    }

    #[test]
    fn title_source_text_cap_is_utf8_safe() {
        // 3-byte chars straddle the byte cap; must truncate on a boundary, not panic.
        let big = "あ".repeat(10_000);
        let out = title_source_text(&big);
        assert!(!out.is_empty() && out.len() <= TITLE_SOURCE_MAX_BYTES);
    }

    #[test]
    fn title_source_text_strips_leading_reminder_larger_than_cap() {
        // A leading reminder bigger than the cap must still be stripped, so the
        // title derives from the objective rather than reminder text.
        let reminder = "x".repeat(TITLE_SOURCE_MAX_BYTES * 2);
        let input =
            format!("<system-reminder>\n{reminder}\n</system-reminder>\n\nbuild a mario game");
        let out = title_source_text(&input);
        assert_eq!(out, "build a mario game");
    }

    #[test]
    fn strip_removes_goal_setup_reminder_leaving_objective() {
        let input = "<system-reminder>\nA goal has been set: do stuff\nlots of rules\nStart \
                     now.\n</system-reminder>\n\nbuild a mario platformer game";
        assert_eq!(
            strip_system_reminder_blocks(input),
            "build a mario platformer game"
        );
    }

    #[test]
    fn strip_handles_unterminated_reminder() {
        assert_eq!(
            strip_system_reminder_blocks("<system-reminder>\nrules with no close tag"),
            ""
        );
    }

    #[test]
    fn strip_no_reminder_is_identity() {
        assert_eq!(
            strip_system_reminder_blocks("fix the auth bug"),
            "fix the auth bug"
        );
    }

    /// Regression: a `/goal <objective>` first turn must title off the
    /// objective, not the injected `<system-reminder>` setup block.
    #[test]
    fn fallback_titles_off_goal_objective_not_reminder() {
        let input = "<system-reminder>\nA goal has been set: do stuff\nStart \
                     now.\n</system-reminder>\n\nbuild a mario platformer game in html";
        assert_eq!(
            title_fallback_from_user_text(input),
            "build a mario platformer game in html"
        );
    }

    #[test]
    fn fallback_trims_to_words() {
        assert_eq!(
            title_fallback_from_user_text(
                "one two three four five six seven eight nine ten eleven"
            ),
            "one two three four five six seven eight nine ten"
        );
    }

    #[test]
    fn fallback_new_session_when_whitespace_only() {
        assert_eq!(title_fallback_from_user_text("   \n\t"), "New session");
    }

    #[test]
    fn fallback_strips_skill_xml_with_args() {
        let input = "<command-name>implement</command-name>\n\
                      <command-message>/implement</command-message>\n\
                      <command-args>fix the rendering bug</command-args>";
        assert_eq!(
            title_fallback_from_user_text(input),
            "/implement fix the rendering bug",
        );
    }

    #[test]
    fn fallback_strips_skill_xml_no_args() {
        let input = "<command-name>deploy</command-name>\n\
                      <command-message>/deploy</command-message>";
        assert_eq!(title_fallback_from_user_text(input), "/deploy");
    }

    #[test]
    fn fallback_plain_text_unaffected() {
        assert_eq!(
            title_fallback_from_user_text("fix the auth bug in login.rs"),
            "fix the auth bug in login.rs",
        );
    }
}
