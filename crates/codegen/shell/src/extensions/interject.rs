//! `grow/steer` extension handler.
//!
//! Queues a mid-turn interjection into the active session's pending
//! interjection buffer.  The session actor drains it at the next safe
//! point in `process_conversation_turn`.

use acp_transport::protocol as acp;

use super::{ExtResult, parse_params};
use crate::agent::MvpAgent;
use crate::session::SessionCommand;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InterjectRequest {
    session_id: String,
    expected_turn_id: String,
    #[serde(default)]
    interjection_id: Option<String>,
    content: Vec<acp::ContentBlock>,
}

/// Split the one canonical content array into model text and image blocks.
fn split_content(
    content: Vec<acp::ContentBlock>,
) -> Result<(String, Vec<acp::ImageContent>), &'static str> {
    let mut texts = content.iter().filter_map(|block| match block {
        acp::ContentBlock::Text(text) if !text.text.trim().is_empty() => Some(text.text.clone()),
        _ => None,
    });
    let text = texts
        .next()
        .ok_or("steer content requires one text block")?;
    if texts.next().is_some() {
        return Err("steer content accepts exactly one non-empty text block");
    }
    Ok((text, crate::session::image_blocks(content)))
}

/// Handle `grow/steer` — append input to one identified regular turn.
pub async fn handle(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    let req: InterjectRequest = parse_params(args)?;
    let sid: acp::SessionId = req.session_id.clone().into();
    // Load-race-tolerant: an interjection racing a reconnect-replayed
    // `session/load` (leader restart) waits for the load instead of failing.
    let session_handle = agent.session_handle_waiting_for_load(&sid).await;
    let Some(session) = session_handle else {
        return Err(
            acp::Error::invalid_params().data(format!("session not found: {}", req.session_id))
        );
    };

    let (text, images) =
        split_content(req.content).map_err(|message| acp::Error::invalid_params().data(message))?;
    let (respond_to, response) = tokio::sync::oneshot::channel();
    session
        .cmd_tx
        .send(SessionCommand::SteerTurn {
            expected_turn_id: req.expected_turn_id,
            text,
            id: req.interjection_id,
            images,
            respond_to,
        })
        .map_err(|_| acp::Error::internal_error().data("session command channel closed"))?;
    response
        .await
        .map_err(|_| acp::Error::internal_error().data("session failed to acknowledge steer"))?
        .map_err(|message| acp::Error::invalid_params().data(message))?;

    super::to_ext_response(Ok(serde_json::json!({
        "status": "queued",
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removed_text_only_wire_shape_is_rejected() {
        let req = serde_json::from_value::<InterjectRequest>(serde_json::json!({
            "sessionId": "s1",
            "expectedTurnId": "t1",
            "text": "steer left",
            "interjectionId": "i1",
        }));
        assert!(req.is_err());
    }

    /// `content` with text + image blocks parses; the images are extracted
    /// and the Text block (the client's rewritten, path-stripped text)
    /// overrides the raw `text` param.
    #[test]
    fn parse_with_content_extracts_images_and_prefers_block_text() {
        let req: InterjectRequest = serde_json::from_value(serde_json::json!({
            "sessionId": "s1",
            "expectedTurnId": "t1",
            "content": [
                { "type": "text", "text": "look at [Image #1]" },
                { "type": "image", "data": "aGVsbG8=", "mimeType": "image/png" },
            ],
        }))
        .expect("content params must parse");
        let (text, images) = split_content(req.content).unwrap();
        assert_eq!(text, "look at [Image #1]");
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].mime_type, "image/png");
        assert_eq!(images[0].data, "aGVsbG8=");
    }

    /// Garbage `content` fails the whole parse (strict, like other params)
    /// instead of silently dropping attachments.
    #[test]
    fn parse_with_garbage_content_is_an_error() {
        let result: Result<InterjectRequest, _> = serde_json::from_value(serde_json::json!({
            "sessionId": "s1",
            "content": "not an array",
        }));
        assert!(result.is_err(), "garbage content must be rejected");
    }
}
