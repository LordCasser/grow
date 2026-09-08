//! Formatting functions for AskUserQuestion tool results.
//!
//! Each function produces the **exact** model-visible string for one of the
//! four user-action paths.
//!
//! The tests below pin the exact output strings and serve as the
//! source-of-truth specification.

use std::collections::HashMap;

use indexmap::IndexMap;

use super::Question;
use super::types::QuestionAnnotation;

// ── Path D: Cancel ──────────────────────────────────────────────────────

/// Tool result text when the user cancels / dismisses the question UI.
///
/// Cancel is a normal user decision, not a tool failure, so this is a
/// purpose-built message rather than a generic permission-denial string.
pub const CANCEL_TEXT: &str = "User declined to answer the questions. Continue with the task using your best judgment, or ask different questions.";

// ── Path A: Accepted ────────────────────────────────────────────────────

/// Format the tool result for Path A (user accepted and submitted answers).
///
/// Produces the accepted-answers tool result:
///
/// ```text
/// User has answered your questions: "<q>"="<label>" ..., "<q>"="<label>" .... You can now continue with the user's answers in mind.
/// ```
///
/// Rules:
/// - Only answered questions appear (unanswered are omitted by the caller).
/// - Multi-select: each selected label is its own `Vec` element on the
///   wire; this function joins them with `, ` at format time.
/// - Freeform-only: a single-element vec containing `"Other"`, free text
///   in `annotations[q].notes`.
/// - Preview is appended only when present in annotations.
/// - Notes are appended only when present in annotations.
/// - Questions/labels are interpolated raw (no escaping).
pub fn format_accepted_tool_result(
    answers: &IndexMap<String, Vec<String>>,
    annotations: &Option<HashMap<String, QuestionAnnotation>>,
) -> String {
    let entries: Vec<String> = answers
        .iter()
        .map(|(question_text, selected_labels)| {
            let selected_label = selected_labels.join(", ");
            let mut parts = vec![format!("\"{}\"=\"{}\"", question_text, selected_label)];

            if let Some(anns) = annotations
                && let Some(ann) = anns.get(question_text)
            {
                if let Some(ref preview) = ann.preview {
                    parts.push(format!("selected preview:\n{}", preview));
                }
                if let Some(ref notes) = ann.notes {
                    parts.push(format!("user notes: {}", notes));
                }
            }

            parts.join(" ")
        })
        .collect();

    format!(
        "User has answered your questions: {}. You can now continue with the user's answers in mind.",
        entries.join(", ")
    )
}

// ── Path B: Chat about this (plan mode) ─────────────────────────────────

/// Format the tool result for Path B ("Chat about this" / respond-to-agent).
///
/// Iterates ALL original questions. Answered questions show their label;
/// unanswered questions show "(No answer provided)".
///
/// Whitespace is intentional:
/// - Lines 2-4 and "Questions asked:" have 4-space indentation.
/// - Question bullets have no indentation.
/// - Answer lines have 2-space indentation.
pub fn format_chat_about_this(
    questions: &[Question],
    partial_answers: &HashMap<String, String>,
) -> String {
    let question_lines: Vec<String> = questions
        .iter()
        .map(|q| {
            if let Some(answer) = partial_answers.get(&q.question) {
                format!("- \"{}\"\n  Answer: {}", q.question, answer)
            } else {
                format!("- \"{}\"\n  (No answer provided)", q.question)
            }
        })
        .collect();

    format!(
        "The user wants to clarify these questions.\n\
         \x20\x20\x20\x20This means they may have additional information, context or questions for you.\n\
         \x20\x20\x20\x20Take their response into account and then reformulate the questions if appropriate.\n\
         \x20\x20\x20\x20Start by asking them what they would like to clarify.\n\
         \n\
         \x20\x20\x20\x20Questions asked:\n\
         {}",
        question_lines.join("\n")
    )
}

// ── Path C: Skip interview (plan mode) ──────────────────────────────────

/// Format the tool result for Path C ("Skip interview and plan immediately").
///
/// Same per-question format as Path B, but different header and NO indentation.
pub fn format_skip_interview(
    questions: &[Question],
    partial_answers: &HashMap<String, String>,
) -> String {
    let question_lines: Vec<String> = questions
        .iter()
        .map(|q| {
            if let Some(answer) = partial_answers.get(&q.question) {
                format!("- \"{}\"\n  Answer: {}", q.question, answer)
            } else {
                format!("- \"{}\"\n  (No answer provided)", q.question)
            }
        })
        .collect();

    format!(
        "The user has indicated they have provided enough answers for the plan interview.\n\
         Stop asking clarifying questions and proceed to finish the plan with the information you have.\n\
         \n\
         Questions asked and answers provided:\n\
         {}",
        question_lines.join("\n")
    )
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::QuestionOption;
    use super::*;

    // -- Helpers --

    fn make_question(text: &str, labels: &[&str]) -> Question {
        Question {
            question: text.to_string(),
            options: labels
                .iter()
                .map(|l| QuestionOption {
                    label: l.to_string(),
                    description: format!("Description for {l}"),
                    preview: None,
                    id: None,
                })
                .collect(),
            multi_select: None,
            id: None,
        }
    }

    // ── Path A: format_accepted_tool_result ──────────────────────────────

    #[test]
    fn format_accepted_single_no_annotations() {
        let mut answers = IndexMap::new();
        answers.insert(
            "Which database?".to_string(),
            vec!["Redis (Recommended)".to_string()],
        );

        let result = format_accepted_tool_result(&answers, &None);
        assert_eq!(
            result,
            "User has answered your questions: \"Which database?\"=\"Redis (Recommended)\". You can now continue with the user's answers in mind."
        );
    }

    #[test]
    fn format_accepted_multiple_with_annotations() {
        let mut answers = IndexMap::new();
        answers.insert("Which database?".to_string(), vec!["Redis".to_string()]);
        answers.insert("Which framework?".to_string(), vec!["React".to_string()]);

        let mut anns = HashMap::new();
        anns.insert(
            "Which database?".to_string(),
            QuestionAnnotation {
                preview: Some("<div>redis preview</div>".to_string()),
                notes: None,
            },
        );
        anns.insert(
            "Which framework?".to_string(),
            QuestionAnnotation {
                preview: None,
                notes: Some("I prefer React hooks".to_string()),
            },
        );

        let result = format_accepted_tool_result(&answers, &Some(anns));
        assert_eq!(
            result,
            "User has answered your questions: \"Which database?\"=\"Redis\" selected preview:\n<div>redis preview</div>, \"Which framework?\"=\"React\" user notes: I prefer React hooks. You can now continue with the user's answers in mind."
        );
    }

    #[test]
    fn format_accepted_multi_select() {
        let mut answers = IndexMap::new();
        answers.insert(
            "Which features?".to_string(),
            vec!["Auth".to_string(), "Logging".to_string()],
        );

        let result = format_accepted_tool_result(&answers, &None);
        assert_eq!(
            result,
            "User has answered your questions: \"Which features?\"=\"Auth, Logging\". You can now continue with the user's answers in mind."
        );
    }

    #[test]
    fn format_accepted_freeform_only() {
        // Freeform-only: label is "Other", typed text in annotations.notes
        let mut answers = IndexMap::new();
        answers.insert("Which database?".to_string(), vec!["Other".to_string()]);

        let mut anns = HashMap::new();
        anns.insert(
            "Which database?".to_string(),
            QuestionAnnotation {
                preview: None,
                notes: Some("I want to use DynamoDB".to_string()),
            },
        );

        let result = format_accepted_tool_result(&answers, &Some(anns));
        assert_eq!(
            result,
            "User has answered your questions: \"Which database?\"=\"Other\" user notes: I want to use DynamoDB. You can now continue with the user's answers in mind."
        );
    }

    #[test]
    fn format_accepted_preview_and_notes() {
        let mut answers = IndexMap::new();
        answers.insert("Which layout?".to_string(), vec!["Grid".to_string()]);

        let mut anns = HashMap::new();
        anns.insert(
            "Which layout?".to_string(),
            QuestionAnnotation {
                preview: Some("<div class=\"grid\">...</div>".to_string()),
                notes: Some("Use CSS Grid for the main layout".to_string()),
            },
        );

        let result = format_accepted_tool_result(&answers, &Some(anns));
        assert_eq!(
            result,
            "User has answered your questions: \"Which layout?\"=\"Grid\" selected preview:\n<div class=\"grid\">...</div> user notes: Use CSS Grid for the main layout. You can now continue with the user's answers in mind."
        );
    }

    #[test]
    fn format_accepted_empty() {
        let answers = IndexMap::new();
        let result = format_accepted_tool_result(&answers, &None);
        assert_eq!(
            result,
            "User has answered your questions: . You can now continue with the user's answers in mind."
        );
    }

    #[test]
    fn format_accepted_partial() {
        // Only answered questions appear. Unanswered questions are omitted by the caller
        // (the answers IndexMap simply doesn't contain them).
        let mut answers = IndexMap::new();
        answers.insert("Which database?".to_string(), vec!["Redis".to_string()]);
        // "Which framework?" is unanswered => not in the map

        let result = format_accepted_tool_result(&answers, &None);
        assert_eq!(
            result,
            "User has answered your questions: \"Which database?\"=\"Redis\". You can now continue with the user's answers in mind."
        );
    }

    #[test]
    fn format_accepted_special_chars() {
        // Quotes and newlines in labels appear verbatim (no escaping)
        let mut answers = IndexMap::new();
        answers.insert(
            "Which \"option\"?".to_string(),
            vec!["Option with\nnewline".to_string()],
        );

        let result = format_accepted_tool_result(&answers, &None);
        assert_eq!(
            result,
            "User has answered your questions: \"Which \"option\"?\"=\"Option with\nnewline\". You can now continue with the user's answers in mind."
        );
    }

    // ── Path B: format_chat_about_this ───────────────────────────────────

    #[test]
    fn format_chat_about_this_mixed() {
        let questions = vec![
            make_question("Which database?", &["Redis", "Postgres"]),
            make_question("Which framework?", &["React", "Vue"]),
        ];

        let mut partial = HashMap::new();
        partial.insert("Which database?".to_string(), "Redis".to_string());

        let result = format_chat_about_this(&questions, &partial);
        let expected = "\
The user wants to clarify these questions.
    This means they may have additional information, context or questions for you.
    Take their response into account and then reformulate the questions if appropriate.
    Start by asking them what they would like to clarify.

    Questions asked:
- \"Which database?\"
  Answer: Redis
- \"Which framework?\"
  (No answer provided)";

        assert_eq!(result, expected);
    }

    #[test]
    fn format_chat_about_this_all_answered() {
        let questions = vec![make_question("Which database?", &["Redis"])];

        let mut partial = HashMap::new();
        partial.insert("Which database?".to_string(), "Redis".to_string());

        let result = format_chat_about_this(&questions, &partial);
        assert!(result.contains("Answer: Redis"));
        assert!(!result.contains("(No answer provided)"));
    }

    #[test]
    fn format_chat_about_this_none_answered() {
        let questions = vec![make_question("Q1?", &["A"]), make_question("Q2?", &["B"])];

        let result = format_chat_about_this(&questions, &HashMap::new());
        assert!(result.contains("- \"Q1?\"\n  (No answer provided)"));
        assert!(result.contains("- \"Q2?\"\n  (No answer provided)"));
    }

    // ── Path C: format_skip_interview ────────────────────────────────────

    #[test]
    fn format_skip_interview_all_answered() {
        let questions = vec![
            make_question("Which database?", &["Redis", "Postgres"]),
            make_question("Which framework?", &["React", "Vue"]),
        ];

        let mut partial = HashMap::new();
        partial.insert("Which database?".to_string(), "Redis".to_string());
        partial.insert("Which framework?".to_string(), "React".to_string());

        let result = format_skip_interview(&questions, &partial);
        let expected = "\
The user has indicated they have provided enough answers for the plan interview.
Stop asking clarifying questions and proceed to finish the plan with the information you have.

Questions asked and answers provided:
- \"Which database?\"
  Answer: Redis
- \"Which framework?\"
  Answer: React";

        assert_eq!(result, expected);
    }

    #[test]
    fn format_skip_interview_mixed() {
        let questions = vec![
            make_question("Which database?", &["Redis"]),
            make_question("Which framework?", &["React"]),
        ];

        let mut partial = HashMap::new();
        partial.insert("Which database?".to_string(), "Redis".to_string());

        let result = format_skip_interview(&questions, &partial);
        assert!(result.contains("Answer: Redis"));
        assert!(result.contains("- \"Which framework?\"\n  (No answer provided)"));
    }

    #[test]
    fn format_skip_interview_no_indentation() {
        // Path C has NO indentation on any header lines (unlike Path B)
        let questions = vec![make_question("Q?", &["A"])];
        let result = format_skip_interview(&questions, &HashMap::new());

        // First line has no leading spaces
        let first_line = result.lines().next().unwrap();
        assert!(!first_line.starts_with(' '));

        // Second line has no leading spaces
        let second_line = result.lines().nth(1).unwrap();
        assert!(!second_line.starts_with(' '));

        // "Questions asked" line has no leading spaces
        assert!(result.contains("\nQuestions asked and answers provided:\n"));
    }

    // ── Path D: CANCEL_TEXT ─────────────────────────────────────────────

    #[test]
    fn format_cancel() {
        assert_eq!(
            CANCEL_TEXT,
            "User declined to answer the questions. Continue with the task using your best judgment, or ask different questions."
        );
    }
}
