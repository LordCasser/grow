# Pager markdown renderer clone audit

## Finding

`StreamingMarkdownRenderer::clone` reconstructs a renderer from the normalized source and calls `render(None)` (`crates/codegen/markdown/src/streaming.rs:180-200`). `MarkdownContent` derives `Clone` over `RenderState`, which owns that renderer (`crates/codegen/pager/src/scrollback/blocks/markdown_content.rs:29-33,70-75`), so cloning a pending `MarkdownContent` can parse/render its accumulated source again.

## Caller evidence

- The only `ScrollbackEntry` clone found in Pager scrollback is `live.push(entry.clone())` in `crates/codegen/pager/src/scrollback/state/selection.rs:2966`, inside the `#[cfg(test)] mod tests` beginning at line 1187. It is a replay-equivalence test.
- The similarly named production clones in `crates/codegen/pager/src/app/agent_view/plan.rs:156,413` clone `plan_content: String`; `crates/codegen/pager/src/views/file_search/line_viewer.rs:826` clones `Option<String>`. They do not clone `MarkdownContent` or `StreamingMarkdownRenderer`.
- `ScrollbackSearchIndex::entries_arc` clones only an `Arc<[IndexedEntry]>` (`crates/codegen/pager/src/scrollback/search.rs:98-100`); its entries are searchable text, not MarkdownContent.
- Resume staging uses `fresh_continuation` to create an empty scrollback state (`crates/codegen/pager/src/scrollback/state/mod.rs:412-423`), then transfers entries through the existing append/move path rather than cloning a live history; it does not exercise renderer Clone.
- Existing markdown tests explicitly check pending clone semantics and independent mutation: `crates/codegen/pager/src/scrollback/blocks/markdown_content/replay_tests.rs:216-237` and `:177-208`.

## Disposition

The repeated parse is reachable when a caller explicitly clones a pending `MarkdownContent`, and costs work proportional to its accumulated source. The inspected Pager production resume/search paths do not clone it, and the actual clone tests preserve final source, links, deferred content, and clone independence. No production hot path or semantic defect justifies changing the renderer's clone representation in this bounded backlog closure; keep this as an audited optimization opportunity only if a production caller is introduced.
