## Why

Prompt history query matching currently retains every hit and sorts the full corpus before dropping all but 100 results. This makes temporary match storage and sorting scale with the entire history even though the UI can only show 100 rows. Item preprocessing also lacks cooperative stop checks, so closing the overlay's daemon can leave it converting the complete pending history before cancellation is observed.

## What Changes

- Retain only the best 100 scored matches during the corpus scan, then order those results so the best match remains last in the rendered list.
- Check the daemon stop signal while converting history items to nucleo strings and abandon canceled preprocessing without publishing partial results.
- Keep cancellation cooperative between indivisible score, conversion, and highlight operations.

## Capabilities

### Modified Capabilities
- `client-surfaces`: bound query-match candidates to the visible top 100 and make item preprocessing cooperatively cancellable.

## Impact

`crates/codegen/pager/src/views/history_search.rs`, its focused unit tests, and the history-search resource-boundary item in `openspec/backlog.md`. No new dependencies.
