// Re-export from chat-state — canonical definition lives there.
pub use chat_state::compaction_utils::extract_user_query;

/// Wrap a direct user prompt in its model-facing boundary.
pub fn user_query(user_message: String) -> String {
    format!("<user_query>\n{user_message}\n</user_query>")
}
