//! Model-free tool-result pruning for the **stored session**.
//!
//! This module is the deterministic, non-LLM half of session load shedding.
//! [`prune_tool_result_content`] trims one tool-result text to a token budget
//! by keeping a head prefix and a tail suffix and replacing the middle with a
//! marker. [`plan_tool_result_pruning`] walks a session oldest-first and picks
//! the oversized tool results worth trimming. Pure functions only: no async,
//! no serde, no I/O, no model call.
//!
//! # Budget and byte accounting
//!
//! Budgets are tokens; internally `max_bytes = budget_tokens * 4`, the same
//! conversion used by the range-summary budget estimator. The marker counts
//! toward the budget and its room is reserved **first**: the head gets
//! 1/2 and the tail 1/4 of `max_bytes`, the remaining 1/4 is the marker's
//! room. The returned string never exceeds `max_bytes`.
//!
use crate::token::ItemTokenCounter;

/// Minimal read seam needed by the model-free pruning planner.
pub trait ToolResultItem {
    fn is_tool_result(&self) -> bool;
}

/// Head share of the byte budget: 1/2 (see module docs).
const HEAD_BUDGET_NUM: usize = 1;
const HEAD_BUDGET_DEN: usize = 2;
/// Tail share of the byte budget: 1/4 (see module docs).
const TAIL_BUDGET_NUM: usize = 1;
const TAIL_BUDGET_DEN: usize = 4;

/// Trim `text` to a token budget, keeping a head prefix and a tail suffix
/// with `marker` inserted between them.
///
/// Returns `None` when `text` already fits (`text.len() <= budget_tokens * 4`),
/// i.e. no pruning is needed.
///
/// The byte budget is `budget_tokens * 4`. The marker counts toward the
/// budget and is reserved first:
/// head gets 1/2 and tail 1/4 of `max_bytes`; the remaining 1/4 is the
/// marker's room. A marker longer than its room is itself clipped (on a UTF-8
/// char boundary) so the result still never exceeds `max_bytes`. All cuts
/// land on UTF-8 char boundaries.
pub fn prune_tool_result_content(text: &str, budget_tokens: u32, marker: &str) -> Option<String> {
    let max_bytes = (budget_tokens as usize).saturating_mul(4);
    if text.len() <= max_bytes {
        return None;
    }

    // Fixed shares of the budget; the remainder is reserved for the marker.
    let head_budget = max_bytes * HEAD_BUDGET_NUM / HEAD_BUDGET_DEN;
    let tail_budget = max_bytes * TAIL_BUDGET_NUM / TAIL_BUDGET_DEN;
    let marker_room = max_bytes - head_budget - tail_budget;

    let head_end = floor_char_boundary(text, head_budget);
    let tail_start = ceil_char_boundary(text, text.len() - tail_budget);
    let marker_fit = &marker[..floor_char_boundary(marker, marker_room)];

    let mut out = String::with_capacity(max_bytes);
    out.push_str(&text[..head_end]);
    out.push_str(marker_fit);
    out.push_str(&text[tail_start..]);
    debug_assert!(out.len() <= max_bytes);
    Some(out)
}

/// Largest char boundary `<= idx`, clamped to `s.len()`.
fn floor_char_boundary(s: &str, idx: usize) -> usize {
    let mut end = idx.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// Smallest char boundary `>= idx`, clamped to `s.len()`.
fn ceil_char_boundary(s: &str, idx: usize) -> usize {
    let mut start = idx.min(s.len());
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    start
}

/// One item selected for tool-result pruning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PruneItem {
    /// Index of the item in the input slice (ascending = oldest first).
    pub index: usize,
    /// Trusted token count before pruning.
    pub tokens_before: u32,
    /// Per-item budget applied when pruning this item.
    pub budget_tokens: u32,
    /// Conservative savings estimate: `tokens_before - budget_tokens`
    /// (assumes the pruned item lands exactly at its budget).
    pub estimated_savings: u32,
}

/// Which tool results to prune, oldest first. An empty plan prunes nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrunePlan {
    pub items: Vec<PruneItem>,
}

impl PrunePlan {
    /// Whether the plan prunes nothing.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Sum of [`PruneItem::estimated_savings`] across the plan (saturating).
    pub fn total_estimated_savings(&self) -> u32 {
        self.items
            .iter()
            .map(|item| item.estimated_savings)
            .fold(0u32, u32::saturating_add)
    }
}

/// Plan session-side tool-result pruning: oldest oversized tool results first.
///
/// Walks `items` by ascending index (oldest first). An item is a candidate
/// only when `is_tool_result()` and its token count exceeds
/// `item_budget_tokens`; every other item is left untouched. Each selected
/// item contributes `estimated_savings = tokens_before - item_budget_tokens`;
/// selection stops as soon as the projected post-prune total
/// (`sum(all items) - accumulated savings`) is `<= total_target_tokens`, so
/// no more items are picked than needed.
///
/// Safety fallbacks returning an empty plan (nothing gets pruned):
/// `total_target_tokens == 0`, `item_budget_tokens == 0`, an empty input, or
/// a total already `<= total_target_tokens`.
pub fn plan_tool_result_pruning<T: ToolResultItem>(
    items: &[T],
    counter: &dyn ItemTokenCounter<T>,
    item_budget_tokens: u32,
    total_target_tokens: u32,
) -> PrunePlan {
    let mut plan = PrunePlan::default();
    if item_budget_tokens == 0 || total_target_tokens == 0 {
        return plan;
    }

    // Materialize per-item counts once (counters may be per-host and costly).
    let counts: Vec<u32> = items
        .iter()
        .map(|item| counter.count_item_tokens(item))
        .collect();
    let total_tokens = counts.iter().copied().fold(0u32, u32::saturating_add);
    let mut total_saved = 0u32;

    for (index, (item, &tokens_before)) in items.iter().zip(&counts).enumerate() {
        // Enough projected savings to reach the total target → stop adding.
        if total_tokens.saturating_sub(total_saved) <= total_target_tokens {
            break;
        }
        // Only oversized tool results are candidates; everything else stays.
        if !item.is_tool_result() || tokens_before <= item_budget_tokens {
            continue;
        }
        let estimated_savings = tokens_before - item_budget_tokens;
        plan.items.push(PruneItem {
            index,
            tokens_before,
            budget_tokens: item_budget_tokens,
            estimated_savings,
        });
        total_saved = total_saved.saturating_add(estimated_savings);
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct MockItem {
        tool_result: bool,
        tokens: u32,
    }

    impl MockItem {
        fn tool(_text: &str, tokens: u32) -> Self {
            Self {
                tool_result: true,
                tokens,
            }
        }

        fn user(_text: &str, tokens: u32) -> Self {
            Self {
                tool_result: false,
                tokens,
            }
        }
    }

    impl ToolResultItem for MockItem {
        fn is_tool_result(&self) -> bool {
            self.tool_result
        }
    }

    /// Counts tokens straight from the mock's `tokens` field for exact math.
    struct TokenCounter;

    impl ItemTokenCounter<MockItem> for TokenCounter {
        fn count_item_tokens(&self, item: &MockItem) -> u32 {
            item.tokens
        }
    }

    #[test]
    fn within_budget_returns_none() {
        let under = "x".repeat(399);
        assert_eq!(prune_tool_result_content(&under, 100, "[pruned]"), None);
        // Exactly at the byte budget still fits.
        let at_budget = "x".repeat(400);
        assert_eq!(prune_tool_result_content(&at_budget, 100, "[pruned]"), None);
        // One byte over requires pruning.
        let over = "x".repeat(401);
        assert!(prune_tool_result_content(&over, 100, "[pruned]").is_some());
    }

    #[test]
    fn head_and_tail_kept_marker_replaces_middle_once() {
        let text = format!("{}{}{}", "H".repeat(300), "M".repeat(400), "T".repeat(300));
        let marker = "[pruned middle]";
        // budget 50 tokens → 200 bytes: head 100, tail 50, marker room 50.
        let pruned = prune_tool_result_content(&text, 50, marker).expect("must prune");
        assert_eq!(
            pruned,
            format!("{}{}{}", "H".repeat(100), marker, "T".repeat(50)),
        );
        assert!(pruned.len() <= 200);
        assert_eq!(
            pruned.matches(marker).count(),
            1,
            "marker must appear exactly once"
        );
        assert!(
            pruned.starts_with(&"H".repeat(100)),
            "head prefix must be kept"
        );
        assert!(
            pruned.ends_with(&"T".repeat(50)),
            "tail suffix must be kept"
        );
        assert!(!pruned.contains('M'), "middle content must be dropped");
    }

    #[test]
    fn multibyte_utf8_cuts_land_on_char_boundaries() {
        // 3-byte chars; head cut (200) and tail cut (100) both fall inside a
        // char → must walk back/forward to boundaries.
        let text = "你".repeat(500); // 1500 bytes
        let marker = "…pruned…";
        let pruned = prune_tool_result_content(&text, 100, marker).expect("must prune");
        assert!(pruned.len() <= 400);
        let (head_part, tail_part) = pruned.split_once(marker).expect("marker once");
        assert_eq!(head_part, "你".repeat(66)); // 198 bytes
        assert_eq!(tail_part, "你".repeat(33)); // 99 bytes
    }

    #[test]
    fn emoji_cuts_land_on_char_boundaries() {
        // 4-byte chars with an odd budget (101 tokens → 404 bytes) so both
        // the head cut (202) and the tail cut (101) split a char.
        let text = "🚀".repeat(300); // 1200 bytes
        let marker = "<cut>";
        let pruned = prune_tool_result_content(&text, 101, marker).expect("must prune");
        assert!(pruned.len() <= 404);
        assert_eq!(
            pruned,
            format!("{}{}{}", "🚀".repeat(50), marker, "🚀".repeat(25))
        );
        assert!(pruned.starts_with('🚀'));
        assert!(pruned.ends_with('🚀'));
    }

    #[test]
    fn marker_room_is_reserved_before_head_and_tail() {
        // budget 20 tokens → 80 bytes: head 40, tail 20, marker room 20.
        let text = format!("{}{}", "H".repeat(200), "T".repeat(200));
        let marker = "m".repeat(20); // exactly the reserved room
        let pruned = prune_tool_result_content(&text, 20, &marker).expect("must prune");
        assert_eq!(
            pruned,
            format!("{}{}{}", "H".repeat(40), marker, "T".repeat(20))
        );
        assert_eq!(
            pruned.len(),
            80,
            "head + full marker + tail must stay within budget"
        );
    }

    #[test]
    fn oversized_marker_is_clipped_to_its_room() {
        // Marker longer than its 1/4 room: clip the marker (char boundary)
        // instead of eating head/tail shares or exceeding the budget.
        let text = format!("{}{}", "H".repeat(200), "T".repeat(200));
        let marker = "m".repeat(50);
        let pruned = prune_tool_result_content(&text, 20, &marker).expect("must prune");
        assert_eq!(
            pruned,
            format!("{}{}{}", "H".repeat(40), "m".repeat(20), "T".repeat(20))
        );
        assert!(pruned.len() <= 80);
    }

    #[test]
    fn zero_budget_content_prunes_to_empty_string() {
        assert_eq!(
            prune_tool_result_content("abc", 0, "[pruned]"),
            Some(String::new())
        );
        assert_eq!(prune_tool_result_content("", 0, "[pruned]"), None);
    }

    #[test]
    fn plan_skips_non_tool_and_in_budget_items() {
        let items = vec![
            MockItem::user("u0", 500), // oversized non-tool → skip
            MockItem::tool("t1", 40),  // within budget → skip
            MockItem::tool("t2", 300), // eligible, saves 250
            MockItem::tool("t3", 300), // eligible, saves 250
        ];
        // total 1140; target 700 → after t2: 890 > 700 → after t3: 640 <= 700.
        let plan = plan_tool_result_pruning(&items, &TokenCounter, 50, 700);
        let idxs: Vec<usize> = plan.items.iter().map(|item| item.index).collect();
        assert_eq!(idxs, vec![2, 3], "only oversized tool results are selected");
        assert_eq!(plan.total_estimated_savings(), 500);
        assert!(
            plan.items
                .iter()
                .all(|item| item.tokens_before == 300 && item.budget_tokens == 50),
            "PruneItem fields must match the selected items"
        );
    }

    #[test]
    fn plan_oldest_first_and_stops_once_target_reached() {
        let items: Vec<MockItem> = (0..5)
            .map(|i| MockItem::tool(&format!("t{i}"), 100))
            .collect();
        // total 500; savings 90 per item; target 300 → 3 items suffice (230).
        let plan = plan_tool_result_pruning(&items, &TokenCounter, 10, 300);
        let idxs: Vec<usize> = plan.items.iter().map(|item| item.index).collect();
        assert_eq!(
            idxs,
            vec![0, 1, 2],
            "oldest first, no extra items past the target"
        );
        assert_eq!(plan.total_estimated_savings(), 270);
        assert!(
            plan.items.iter().all(|item| item.estimated_savings == 90),
            "estimated_savings = tokens_before - budget_tokens"
        );
    }

    #[test]
    fn plan_empty_when_total_already_at_or_under_target() {
        let items = vec![MockItem::tool("t0", 300)];
        assert!(plan_tool_result_pruning(&items, &TokenCounter, 50, 300).is_empty());
        assert!(plan_tool_result_pruning(&items, &TokenCounter, 50, 400).is_empty());
    }

    #[test]
    fn plan_zero_budgets_and_empty_input_return_empty() {
        let items = vec![MockItem::tool("t0", 300)];
        assert!(plan_tool_result_pruning(&items, &TokenCounter, 0, 100).is_empty());
        assert!(plan_tool_result_pruning(&items, &TokenCounter, 50, 0).is_empty());
        let empty: Vec<MockItem> = Vec::new();
        assert!(plan_tool_result_pruning(&empty, &TokenCounter, 50, 100).is_empty());
    }

    #[test]
    fn plan_empty_when_no_eligible_items() {
        let items = vec![
            MockItem::user("u0", 500), // oversized but not a tool result
            MockItem::tool("t1", 50),  // exactly at budget, not above
        ];
        let plan = plan_tool_result_pruning(&items, &TokenCounter, 50, 100);
        assert!(
            plan.is_empty(),
            "total is over target but nothing is eligible"
        );
    }
}
