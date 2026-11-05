//! Stable, bounded model guidance; the full responsibility lives in LocalPlan.

use serde_json::{Value, json};
use tool_types::ReplanChange;

const MAX_GUIDANCE_BYTES: usize = 16 * 1024;
const MAX_REQUIREMENT_SUMMARIES: usize = 4;
const REPLAN_PREFIX: &str = "The parent TODO changed. Pause work that depends on the previous objective, inspect the latest parent TODO with todo_read, and adjust only affected work. Ask the parent with ask_parent if the changed objective is unclear. Preserve valid completed work and acknowledge this exact change.";
const STOP_PREFIX: &str = "The parent TODO was removed or cancelled. This delegation is terminated. Stop advancing its objective now, preserve existing work, perform only necessary cleanup and settlement, then report completed work, remaining work, artifacts, and unresolved outcomes to the parent. Do not resume this delegation because the TODO is later reopened.";

pub(super) fn guidance_body(
    parent_session_id: &str,
    change: &ReplanChange,
) -> Result<String, String> {
    let prefix = if change.is_stop() {
        STOP_PREFIX
    } else {
        REPLAN_PREFIX
    };
    let requirements = change.requirements.as_deref().unwrap_or_default();
    let shown = requirements.len().min(MAX_REQUIREMENT_SUMMARIES);
    let mut details = json!({
        "parent_session_id": parent_session_id,
        "child_session_id": change.child_session_id,
        "link_id": change.link_id,
        "step_id": change.step_id,
        "change_id": change.id,
        "source_revision": change.source_revision,
        "directive": if change.is_stop() { "stop" } else { "replan" },
        "priority": change.priority,
        "status": change.status,
        "removed": change.removed,
        "content_summary": text_summary(&change.content, 0),
        "requirements_summary": {
            "total": requirements.len(),
            "items": requirements[..shown].iter().map(|text| text_summary(text, 0)).collect::<Vec<_>>(),
            "omitted_items": requirements.len() - shown,
        },
    });
    // Empty summaries include their maximum omitted-byte counts. Adding text
    // can only shrink those counters, so every escaped text byte fits this
    // actual serialized envelope budget, including the prefix and newline.
    let mut remaining = MAX_GUIDANCE_BYTES
        .checked_sub(prefix.len() + 1 + details.to_string().len())
        .ok_or_else(|| "Planning control identities exceed the notification budget".to_owned())?;
    let content_budget = if shown == 0 { remaining } else { remaining / 2 };
    details["content_summary"] = text_summary(&change.content, content_budget);
    remaining -= escaped_text_len(details["content_summary"]["text"].as_str().unwrap());
    for (index, requirement) in requirements[..shown].iter().enumerate() {
        let summary = text_summary(requirement, remaining / (shown - index));
        remaining -= escaped_text_len(summary["text"].as_str().unwrap());
        details["requirements_summary"]["items"][index] = summary;
    }
    let body = format!("{prefix}\n{details}");
    debug_assert!(body.len() <= MAX_GUIDANCE_BYTES);
    Ok(body)
}

fn text_summary(text: &str, escaped_budget: usize) -> Value {
    let mut end = 0;
    let mut used = 0;
    for character in text.chars() {
        let bytes = escaped_character_len(character);
        if used + bytes > escaped_budget {
            break;
        }
        used += bytes;
        end += character.len_utf8();
    }
    json!({ "text": &text[..end], "omitted_bytes": text.len() - end })
}

fn escaped_text_len(text: &str) -> usize {
    text.chars().map(escaped_character_len).sum()
}

fn escaped_character_len(character: char) -> usize {
    match character {
        '"' | '\\' | '\u{08}' | '\u{0c}' | '\n' | '\r' | '\t' => 2,
        '\u{00}'..='\u{1f}' => 6,
        _ => character.len_utf8(),
    }
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
