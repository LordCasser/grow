//! Bounded credential fragments used by request authentication state.
//! Fragments remain sensitive; truncation is not redaction.

/// Maximum retained tail length for sent credential state. Short credentials
/// remain complete and must never be treated as safe log data.
pub const SENT_BEARER_PREFIX_LEN: usize = 12;
/// Last [`SENT_BEARER_PREFIX_LEN`] characters of a bearer, char-boundary
/// safe (bearer strings are visible-ASCII per the header grammars, but a
/// resolver-supplied `String` has no such guarantee -- counting chars from
/// the end avoids a byte-index panic on non-ASCII input).
pub(crate) fn bearer_tail_fragment(s: &str) -> &str {
    match s.char_indices().rev().nth(SENT_BEARER_PREFIX_LEN - 1) {
        Some((i, _)) => &s[i..],
        None => s,
    }
}
