//! SearchReplace tool implementation.
//!
//! This tool performs exact string replacements in files with support for:
//! - Exact string replacement (find/replace)
//! - New file creation (when `old_string` is empty)
//! - Replace all mode (`replace_all: true`)
//! - Read-before-edit validation (non-concise mode)
//! - External modification detection

use crate::types::output::SearchReplaceEditDetail;

// ============================================================================
// Shared string-edit helpers
// ============================================================================

/// Render a snippet of the file with line numbers around the edit.
pub(crate) fn render_snippet(
    new_text: &str,
    new_string: &str,
    start_pos: usize,
    context_size: usize,
) -> (String, String, String) {
    let LineRange {
        start_line,
        end_line,
    } = compute_line_range(new_text, start_pos, new_string);
    let total_lines_count = new_text.split_inclusive('\n').count();
    let lines = new_text.split_inclusive('\n').collect::<Vec<_>>();

    let snippet_start = start_line.saturating_sub(context_size);
    let snippet_end = (end_line + context_size).min(total_lines_count.saturating_sub(1));

    let before_context = if snippet_start < start_line {
        lines[snippet_start..start_line].join("")
    } else {
        String::new()
    };

    let after_context = if end_line < snippet_end {
        lines[(end_line + 1)..=snippet_end].join("")
    } else {
        String::new()
    };

    let snippet = lines
        .iter()
        .enumerate()
        .map(|(line_num, line)| format!("{}→{}", line_num + 1, line))
        .skip(snippet_start)
        .take(snippet_end - snippet_start + 1)
        .collect::<String>();

    (snippet, before_context, after_context)
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
/// 0-based inclusive start and end line indices
pub(crate) struct LineRange {
    pub start_line: usize,
    pub end_line: usize,
}

/// Compute the line range of the inserted text in the text
pub(crate) fn compute_line_range(text: &str, start_pos: usize, inserted_text: &str) -> LineRange {
    let start_line = text[..start_pos].matches('\n').count();
    let lines_in_inserted = inserted_text.split_inclusive('\n').count().max(1);
    let end_line = start_line + lines_in_inserted - 1;
    LineRange {
        start_line,
        end_line,
    }
}

/// Normalize CRLF only for matching while retaining a byte-offset map into
/// the original UTF-8 string. This lets callers preserve mixed line endings
/// when they write the replacement back.
pub(crate) fn normalize_line_endings_with_map(text: &str) -> (String, Vec<usize>) {
    let bytes = text.as_bytes();
    let mut normalized = String::with_capacity(text.len());
    let mut offsets = vec![0];
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
            normalized.push('\n');
            index += 2;
            offsets.push(index);
            continue;
        }
        let character = text[index..]
            .chars()
            .next()
            .expect("UTF-8 byte offset must begin a character");
        let width = character.len_utf8();
        normalized.push(character);
        for step in 1..=width {
            offsets.push(index + step);
        }
        index += width;
    }
    (normalized, offsets)
}

/// Replace disjoint byte ranges in the original text and return replacement
/// positions in the resulting text for diff metadata.
pub(crate) fn replace_at_ranges(
    text: &str,
    ranges: &[(usize, usize)],
    new_string: &str,
) -> (String, Vec<usize>) {
    let mut result = String::with_capacity(text.len());
    let mut positions = Vec::with_capacity(ranges.len());
    let mut last_end = 0;
    for &(start, end) in ranges {
        result.push_str(&text[last_end..start]);
        positions.push(result.len());
        result.push_str(new_string);
        last_end = end;
    }
    result.push_str(&text[last_end..]);
    (result, positions)
}

/// Pick the line-ending convention nearest to a replacement range.
pub(crate) fn line_ending_for_range(text: &str, start: usize, end: usize) -> &'static str {
    let bytes = text.as_bytes();
    let ending_at = |index: usize| {
        (bytes.get(index) == Some(&b'\n')).then(|| {
            if index > 0 && bytes[index - 1] == b'\r' {
                "\r\n"
            } else {
                "\n"
            }
        })
    };
    (start..end)
        .find_map(ending_at)
        .or_else(|| (end..bytes.len()).find_map(ending_at))
        .or_else(|| (0..start).rev().find_map(ending_at))
        .unwrap_or("\n")
}

/// Convert a replacement's line endings to the convention of the matched
/// region, without changing a replacement that has no line breaks.
pub(crate) fn normalize_replacement_line_endings(replacement: &str, line_ending: &str) -> String {
    if !replacement.contains('\n') {
        return replacement.to_owned();
    }
    let normalized = replacement.replace("\r\n", "\n");
    if line_ending == "\r\n" {
        normalized.replace('\n', "\r\n")
    } else {
        normalized
    }
}

/// Build edit details for each replacement.
pub(crate) fn build_edit_details(
    new_text: &str,
    old_string: &str,
    new_string: &str,
    new_positions: &[usize],
    context_lines: usize,
) -> Vec<SearchReplaceEditDetail> {
    let mut details: Vec<SearchReplaceEditDetail> = Vec::with_capacity(new_positions.len());
    for &start_pos in new_positions {
        let (_snippet, context_before, context_after) =
            render_snippet(new_text, new_string, start_pos, context_lines);
        let line_range_new = compute_line_range(new_text, start_pos, new_string);
        // Extract the leading text on the line before the match starts.
        // This is the text between the last '\n' before start_pos and start_pos itself.
        let line_start = new_text[..start_pos]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let line_prefix = new_text[line_start..start_pos].to_owned();

        details.push(SearchReplaceEditDetail {
            old_string: old_string.to_owned(),
            old_line: line_range_new.start_line + 1,
            new_string: new_string.to_owned(),
            new_line: line_range_new.start_line + 1,
            context_before,
            context_after,
            line_prefix,
        });
    }
    details
}

// ============================================================================
// Normalized (confusable-aware) matching helpers
// ============================================================================

/// A single match found via confusable-normalized comparison, expressed in
/// the original text's byte coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalizedMatch {
    /// Byte offset in the original text where the match starts.
    pub original_start: usize,
    /// Length in bytes of the matched region in the original text.
    /// May differ from the search pattern's byte length because confusable
    /// characters have different UTF-8 widths than their ASCII equivalents.
    pub original_len: usize,
}

/// Result of normalized match search — distinguishes "no match exists" from
/// "matches exist but are unsafe to apply."
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NormalizedMatchResult {
    /// No normalized match exists in the text.
    NoMatch,
    /// One or more valid, non-overlapping matches were found.
    Matches(Vec<NormalizedMatch>),
    /// Normalized matching found candidates but they are ambiguous or unsafe
    /// (overlapping remapped spans, or partial-expansion matches that don't
    /// roundtrip correctly).  The caller should treat this as an explicit
    /// ambiguity error, NOT as "string not found."
    Ambiguous,
}

/// Find match positions using confusable-normalized comparison and remap
/// them back to the original text's byte coordinates.
///
/// Algorithm:
/// 1. Build `(normalized_text, offset_map)` via [`build_offset_map`].
/// 2. Normalize the search pattern the same way.
/// 3. Find all non-overlapping matches in `normalized_text`.
/// 4. Remap each normalized `[start..end]` span back to original bytes
///    via `offset_map`.
/// 5. **Roundtrip validation:** For each candidate, verify that
///    `normalize_confusables(&text[orig_start..orig_end]) == norm_pattern`.
///    This rejects partial-expansion matches (e.g., pattern `-` matching
///    inside em-dash `—` which normalizes to `--`).
/// 6. Reject overlapping validated spans (fail closed → `Ambiguous`).
///
/// Returns [`NormalizedMatchResult`] to distinguish no-match from ambiguity.
pub(crate) fn find_normalized_match_positions(text: &str, pattern: &str) -> NormalizedMatchResult {
    use crate::util::unicode_confusables::{build_offset_map, normalize_confusables};

    let (norm_text, offset_map) = build_offset_map(text);
    let norm_pattern = normalize_confusables(pattern);

    if norm_pattern.is_empty() {
        return NormalizedMatchResult::NoMatch;
    }

    // Collect all non-overlapping matches in normalized space and validate
    // each candidate via roundtrip check.
    let mut validated = Vec::new();
    let mut had_rejected_candidates = false;

    for (norm_start, _) in norm_text.match_indices(&norm_pattern) {
        let norm_end = norm_start + norm_pattern.len();
        let orig_start = offset_map[norm_start];
        let orig_end = offset_map[norm_end];

        // Reject zero-length or inverted spans.
        if orig_end <= orig_start {
            had_rejected_candidates = true;
            continue;
        }

        let orig_slice = &text[orig_start..orig_end];

        // Roundtrip validation: the normalized original slice must exactly
        // equal the normalized pattern.  This catches partial-expansion
        // matches (e.g., pattern "-" matching inside "—" → "--").
        if normalize_confusables(orig_slice) != norm_pattern {
            had_rejected_candidates = true;
            continue;
        }

        validated.push(NormalizedMatch {
            original_start: orig_start,
            original_len: orig_end - orig_start,
        });
    }

    if validated.is_empty() {
        // If we had candidates but all were rejected, that's ambiguity.
        // If we had zero candidates at all, that's a genuine no-match.
        return if had_rejected_candidates {
            NormalizedMatchResult::Ambiguous
        } else {
            NormalizedMatchResult::NoMatch
        };
    }

    // Reject overlapping remapped spans (fail closed on ambiguity).
    for window in validated.windows(2) {
        let end_of_prev = window[0].original_start + window[0].original_len;
        if end_of_prev > window[1].original_start {
            return NormalizedMatchResult::Ambiguous;
        }
    }

    NormalizedMatchResult::Matches(validated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_snippet_middle_of_file_contexts() {
        let new_text = "one\ntwo NEW here\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n";
        let inserted = "NEW here";
        let start_pos = new_text.find(inserted).unwrap();

        let (snippet, before_context, after_context) =
            render_snippet(new_text, inserted, start_pos, 3);

        let expected_snippet = "1→one\n2→two NEW here\n3→three\n4→four\n5→five\n";
        assert_eq!(snippet, expected_snippet);
        assert_eq!(before_context, "one\n");
        assert_eq!(after_context, "three\nfour\nfive\n");

        let line_range = compute_line_range(new_text, start_pos, inserted);
        assert_eq!(
            line_range,
            LineRange {
                start_line: 1,
                end_line: 1
            }
        );
    }

    #[test]
    fn render_snippet_start_of_file_contexts() {
        let new_text = "AA1\nAA2\nb\nc\nd\ne\nf\n";
        let inserted = "AA1\nAA2\n";
        let start_pos = 0;

        let (snippet, before_context, after_context) =
            render_snippet(new_text, inserted, start_pos, 3);

        let expected_snippet = "1→AA1\n2→AA2\n3→b\n4→c\n5→d\n";
        assert_eq!(snippet, expected_snippet);
        assert_eq!(before_context, "");
        assert_eq!(after_context, "b\nc\nd\n");

        let line_range = compute_line_range(new_text, start_pos, inserted);
        assert_eq!(
            line_range,
            LineRange {
                start_line: 0,
                end_line: 1
            }
        );
    }

    #[test]
    fn render_snippet_end_of_file_contexts() {
        let new_text = "a\nb\nc\nd\ne\nNEW\n";
        let inserted = "NEW\n";
        let start_pos = new_text.find(inserted).unwrap();

        let (snippet, before_context, after_context) =
            render_snippet(new_text, inserted, start_pos, 3);

        let expected_snippet = "3→c\n4→d\n5→e\n6→NEW\n";
        assert_eq!(snippet, expected_snippet);
        assert_eq!(before_context, "c\nd\ne\n");
        assert_eq!(after_context, "");

        let line_range = compute_line_range(new_text, start_pos, inserted);
        assert_eq!(
            line_range,
            LineRange {
                start_line: 5,
                end_line: 5
            }
        );
    }

    #[test]
    fn render_snippet_multiline_insertion_middle() {
        let new_text = "1\n2\nAAA\nBBB\nCCC\n3\n4\n5\n6\n7\n";
        let inserted = "AAA\nBBB\nCCC\n";
        let start_pos = new_text.find(inserted).unwrap();

        let (snippet, before_context, after_context) =
            render_snippet(new_text, inserted, start_pos, 3);

        let expected_snippet = "1→1\n2→2\n3→AAA\n4→BBB\n5→CCC\n6→3\n7→4\n8→5\n";
        assert_eq!(snippet, expected_snippet);
        assert_eq!(before_context, "1\n2\n");
        assert_eq!(after_context, "3\n4\n5\n");

        let line_range = compute_line_range(new_text, start_pos, inserted);
        assert_eq!(
            line_range,
            LineRange {
                start_line: 2,
                end_line: 4
            }
        );
    }

    #[test]
    fn replacement_ranges_report_post_edit_offsets() {
        let text = "hello world, hello again";
        let ranges = [(0, 5), (13, 18)];
        let (new_text, new_positions) = replace_at_ranges(text, &ranges, "hi");
        assert_eq!(new_text, "hi world, hi again");
        assert_eq!(new_positions, vec![0, 10]);
    }

    // ── Normalized matching helpers ─────────────────────────────────────

    fn unwrap_matches(result: NormalizedMatchResult) -> Vec<NormalizedMatch> {
        match result {
            NormalizedMatchResult::Matches(m) => m,
            other => panic!("Expected Matches, got {:?}", other),
        }
    }

    #[test]
    fn normalized_match_smart_quotes() {
        let text = "say \u{201C}hello\u{201D} world";
        let matches = unwrap_matches(find_normalized_match_positions(text, "\"hello\""));
        assert_eq!(matches.len(), 1);
        let m = &matches[0];
        assert_eq!(
            &text[m.original_start..m.original_start + m.original_len],
            "\u{201C}hello\u{201D}"
        );
    }

    #[test]
    fn normalized_match_em_dash() {
        let text = "foo\u{2014}bar";
        let matches = unwrap_matches(find_normalized_match_positions(text, "foo--bar"));
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn normalized_match_nbsp() {
        let text = "hello\u{00A0}world";
        let matches = unwrap_matches(find_normalized_match_positions(text, "hello world"));
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn normalized_match_ellipsis() {
        let text = "wait\u{2026}";
        let matches = unwrap_matches(find_normalized_match_positions(text, "wait..."));
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn normalized_match_returns_no_match_for_unrelated() {
        assert_eq!(
            find_normalized_match_positions("hello world", "xyz"),
            NormalizedMatchResult::NoMatch
        );
    }

    #[test]
    fn normalized_match_pure_ascii_still_works() {
        let matches = unwrap_matches(find_normalized_match_positions("hello world", "hello"));
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].original_start, 0);
        assert_eq!(matches[0].original_len, 5);
    }

    #[test]
    fn normalized_match_multiple_occurrences() {
        let text = "\u{201C}a\u{201D} and \u{201C}b\u{201D}";
        let matches = unwrap_matches(find_normalized_match_positions(text, "\"a\""));
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn normalized_match_empty_pattern_returns_no_match() {
        assert_eq!(
            find_normalized_match_positions("hello", ""),
            NormalizedMatchResult::NoMatch
        );
    }

    // ── Partial-expansion rejection ─────────────────────────────────────

    #[test]
    fn partial_expansion_dash_inside_em_dash_rejected() {
        let text = "\u{2014}";
        assert_eq!(
            find_normalized_match_positions(text, "-"),
            NormalizedMatchResult::Ambiguous,
            "partial match inside em-dash must be Ambiguous"
        );
    }

    #[test]
    fn partial_expansion_dot_inside_ellipsis_rejected() {
        let text = "\u{2026}";
        assert_eq!(
            find_normalized_match_positions(text, "."),
            NormalizedMatchResult::Ambiguous,
            "partial match inside ellipsis must be Ambiguous"
        );
    }

    #[test]
    fn partial_expansion_double_dot_inside_ellipsis_rejected() {
        let text = "\u{2026}";
        assert_eq!(
            find_normalized_match_positions(text, ".."),
            NormalizedMatchResult::Ambiguous,
        );
    }

    #[test]
    fn full_expansion_em_dash_accepted() {
        let text = "a\u{2014}b";
        let matches = unwrap_matches(find_normalized_match_positions(text, "--"));
        assert_eq!(matches.len(), 1);
        assert_eq!(
            &text[matches[0].original_start..matches[0].original_start + matches[0].original_len],
            "\u{2014}"
        );
    }

    #[test]
    fn full_expansion_ellipsis_accepted() {
        let text = "a\u{2026}b";
        let matches = unwrap_matches(find_normalized_match_positions(text, "..."));
        assert_eq!(matches.len(), 1);
    }
}
