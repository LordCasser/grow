## Existing paths

`coalesce_rapid_keys` walks one collected `TimedInputEvent` batch and synthesizes a Paste from a qualifying key run. It currently treats `Event::Paste` as a local separator, then resumes synthesis for later key runs. The event collector already preserves a completed Paste as its own event.

`PromptWidget::handle_key_inner` delegates file-search keys only when `FileSearchState::is_visible()` is true. `start_query` clears results immediately, so an active context can be temporarily invisible. `clear_context` already removes the drill prefix and fences stale search results.

## Change

Track whether the coalescer has passed a completed `Event::Paste`; after that point, pass subsequent events through unchanged for that batch. Key-only runs before a Paste retain unbracketed multiline and Windows path coalescing. Route Escape through the existing file-search dismissal path whenever an active context exists, even if no results are visible; other navigation and acceptance keys still require a visible dropdown.

No new state is persisted. The changes use the existing event boundary and file-search context.

## Verification

Run the Pager library test suite with the focused paste and file-search regressions included. Confirm ordinary pre-Paste coalescing, two bracketed Pastes, a post-Paste multiline key run, Escape during an empty query, and stale-result fencing.
