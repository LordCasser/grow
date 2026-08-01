//! Markdown parser policy shared by the renderer's parsing paths.
//!
//! `pulldown-cmark` treats both `~~text~~` and `~text~` as strikethrough when
//! GFM strikethrough is enabled. Grow intentionally accepts only the
//! double-tilde form because single tildes frequently appear in model output as
//! approximation markers.

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

fn parser_options() -> Options {
    Options::ENABLE_GFM
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_MATH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_TABLES
}

/// Offset event stream with single-tilde strikethrough demoted to literal text.
pub(crate) fn offset_events(text: &str) -> impl Iterator<Item = (Event<'_>, Range<usize>)> + '_ {
    DoubleTildeOnlyStrike {
        text,
        events: Parser::new_ext(text, parser_options()).into_offset_iter(),
    }
}

struct DoubleTildeOnlyStrike<'a, I> {
    text: &'a str,
    events: I,
}

fn is_double_tilde_strike(text: &str, range: &Range<usize>) -> bool {
    text.get(range.start..).is_some_and(|s| s.starts_with("~~"))
}

fn strike_delim_text<'a>(
    text: &'a str,
    range: &Range<usize>,
    opening: bool,
) -> (Event<'a>, Range<usize>) {
    let delim = if opening {
        let end = range.start + 1;
        debug_assert!(text.is_char_boundary(end) && end <= text.len());
        (range.start..end, &text[range.start..end])
    } else {
        let start = range.end - 1;
        debug_assert!(text.is_char_boundary(start) && start < text.len());
        (start..range.end, &text[start..range.end])
    };
    (Event::Text(delim.1.into()), delim.0)
}

impl<'a, I> Iterator for DoubleTildeOnlyStrike<'a, I>
where
    I: Iterator<Item = (Event<'a>, Range<usize>)>,
{
    type Item = (Event<'a>, Range<usize>);

    fn next(&mut self) -> Option<Self::Item> {
        let (event, range) = self.events.next()?;
        match &event {
            Event::Start(Tag::Strikethrough) if !is_double_tilde_strike(self.text, &range) => {
                Some(strike_delim_text(self.text, &range, true))
            }
            Event::End(TagEnd::Strikethrough) if !is_double_tilde_strike(self.text, &range) => {
                Some(strike_delim_text(self.text, &range, false))
            }
            _ => Some((event, range)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strike_start_end_counts(text: &str) -> (usize, usize) {
        let mut starts = 0;
        let mut ends = 0;
        for (event, _) in offset_events(text) {
            match event {
                Event::Start(Tag::Strikethrough) => starts += 1,
                Event::End(TagEnd::Strikethrough) => ends += 1,
                _ => {}
            }
        }
        (starts, ends)
    }

    #[test]
    fn single_tilde_pair_is_literal() {
        let text = "~single~";
        assert_eq!(strike_start_end_counts(text), (0, 0));
        let delimiters = offset_events(text)
            .filter_map(|(event, _)| match event {
                Event::Text(text) if text.as_ref() == "~" => Some(()),
                _ => None,
            })
            .count();
        assert_eq!(delimiters, 2);
    }

    #[test]
    fn double_tilde_pair_remains_strikethrough() {
        assert_eq!(strike_start_end_counts("~~deleted~~"), (1, 1));
    }

    #[test]
    fn mixed_forms_keep_only_double_tilde() {
        assert_eq!(
            strike_start_end_counts("keep ~~this~~ but not ~that~"),
            (1, 1)
        );
    }

    #[test]
    fn nested_double_inside_single_remains_balanced() {
        assert_eq!(strike_start_end_counts("~start ~~double~~ end~"), (1, 1));
    }
}
