/// Wrap a user message in the canonical `<user_query>` envelope.
pub fn user_query(user_message: &str) -> String {
    format!(
        r#"<user_query>
{user_message}
</user_query>"#
    )
}

/// Wrap interjection text as a synthetic user message with a mid-turn note.
/// No deferral instruction: the model decides how to weigh it against
/// in-flight work. Preserve admitted text; request admission owns context limits.
pub fn format_interjection(text: String) -> String {
    format!(
        "The user sent a message while you were working:\n{}",
        user_query(&text)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_in_user_query_with_midturn_note() {
        let out = format_interjection("stop and fix the test first".into());
        assert!(out.starts_with("The user sent a message while you were working:\n<user_query>\n"));
        assert!(out.ends_with("\n</user_query>"));
        assert!(out.contains("stop and fix the test first"));
    }

    #[test]
    fn preserves_long_utf8_text_and_trailing_guidance() {
        let text = format!(
            "{}\nMIDDLE_GUIDANCE\n{}\nTAIL_GUIDANCE",
            "设计🎉".repeat(3_000),
            "é".repeat(25_000)
        );
        let out = format_interjection(text.clone());
        assert!(out.contains("MIDDLE_GUIDANCE"));
        assert!(out.ends_with("TAIL_GUIDANCE\n</user_query>"));
        assert_eq!(
            out,
            format!(
                "The user sent a message while you were working:\n{}",
                user_query(&text)
            )
        );
    }

    #[test]
    fn short_text_untouched() {
        let out = format_interjection("hi".into());
        assert!(!out.contains("[truncated]"));
    }
}
