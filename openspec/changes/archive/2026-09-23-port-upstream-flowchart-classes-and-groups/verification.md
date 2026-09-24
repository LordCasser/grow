# Verification

## Scope and baseline

- On macOS, the pre-change vendored parser treated `A & B` as one unrecognized node segment, did not parse `classDef` or trailing `:::class`, and chose shape openings in a fixed delimiter-type order. This is verified from the pre-change `parser.rs` implementation; a separate pre-change test binary was not run.
- The current render route remains `mermaid::PureRustEngine` → vendored parser/layout/SVG → bundled rasterizer; Pager's worker applies source and time limits and only publishes a completed PNG. The experimental dagre port remains disabled. The [upstream parser at 4247f66](https://raw.githubusercontent.com/xai-org/grok-build/4247f66/third_party/mermaid-to-svg/src/parser.rs) was used as a semantic reference, not copied wholesale: Grow retains its own layout fixes and adds a 4096-edge parser budget.
- No external filesystem or network I/O was added to the vendored engine. The new maps and vectors live for one parse. The budget checks `checked_mul` and cumulative `checked_add` before constructing grouped edges. Ordinary ungrouped edges retain their prior acceptance behavior.

## Scenario evidence

- Inline class nodes preserve diamond/rectangle shapes and full labels, including `/v1/items/{id}`; all three class colors appear in SVG. The light and dark PNG test observes exact fill, stroke, and text-color pixels.
- Explicit `style` wins regardless of class definition order. Unsupported `class`, `click`, `linkStyle`, `direction`, `accTitle`, and `accDescr` directives do not become nodes.
- Both class and direct style colors are XML-escaped in emitted node attributes; a quote cannot inject an `onload` attribute.
- `A & B --> C --> D & E` creates exactly four expected edges. A quoted label with both `=&` and whitespace-delimited ` & ` remains a literal label; a standalone group creates two nodes.
- 4096 grouped edges parse. A cumulative 4097-edge graph returns `ParseError` instead of a partial AST. The Pager worker reports `Failed` and writes no PNG for an over-budget source smaller than its source-size cap.

## Commands and results

| Command | Result |
| --- | --- |
| `cargo test --locked --offline -p mermaid-to-svg --lib -- --test-threads=1` | 85 passed, 0 failed. |
| `cargo test --locked --offline -p mermaid --test pure_engine -- --test-threads=1` | 8 passed, 0 failed. |
| `cargo test --locked --offline -p pager --lib app::agent_view::mermaid_worker::tests -- --test-threads=1` | 38 passed, 0 failed. The only diagnostic was the existing macOS linker `__eh_frame` warning. |

The Pager worker tests use the in-process test renderer; this host did not run a Linux production render child or validate its address-space limit. Parser failure, no-partial-PNG behavior, theme rasterization, and existing worker failure mapping were verified on macOS. Disk free space after the run was approximately 57 GiB.

After archiving, `openspec validate --all --strict --no-interactive` passed 16/16, `openspec validate --archived --strict --no-interactive` passed 425/425, and `git diff --check` passed. The developer guide now links the merged client-surfaces contract.
