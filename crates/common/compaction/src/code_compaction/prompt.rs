//! Canonical whole-session summarization prompt.

/// Build the session-level summarization prompt (no chat history).
///
/// `user_context` is the optional `/compact <text>` user-provided context,
/// spliced inline into the structured prompt.
pub fn build_summary_prompt(user_context: Option<&str>) -> String {
    let user_context_section = match user_context {
        Some(context) => format!(
            "\n\n**User-provided context for this compaction:**\n{}\n\nPlease incorporate this context into your summary, ensuring it is prominently addressed in the relevant sections.\n\n",
            context
        ),
        None => String::new(),
    };

    include_str!("templates/summary_prompt.txt")
        .replace("{user_context_section}", &user_context_section)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_prompt_splices_context_section_inline() {
        let p = build_summary_prompt(Some("focus on auth"));
        assert!(p.contains("**User-provided context for this compaction:**\nfocus on auth"));
        assert!(p.contains("1. Primary Request and Intent"));
        assert!(p.contains("9. Optional Next Step"));
    }

    #[test]
    fn summary_prompt_without_context_has_no_context_header() {
        let p = build_summary_prompt(None);
        assert!(!p.contains("**User-provided context for this compaction:**"));
        assert!(p.contains("6. All User Messages"));
        // Current prompt: no separate analysis block, concise framing.
        assert!(p.contains("do NOT emit a separate analysis block"));
        assert!(p.contains("faithful, concise summary"));
    }
}
