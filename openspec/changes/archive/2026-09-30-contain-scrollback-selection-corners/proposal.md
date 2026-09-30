## Why

Scrollback selection corners are drawn one row outside the selected entry. Adjacent collapsed tool rows can have no spacer, so a corner lands on the next or previous tool group's header. The screenshots show `└` beside the following `Searched` and `Ran` headers, making the selected range appear to include a neighbor. The same `SelectionBox` also frames hovered entries, side panes, and sticky headers; some of those callers likewise have no guaranteed spacer.

## What Changes

- Keep the existing compact tool-row spacing, but draw a selected or hovered corner inside its own entry when the adjacent spacer is absent.
- Preserve the current outside-corner placement when a spacer exists and the dashed continuation when the viewport clips the entry.
- Apply the same boundary rule to side pane chrome and sticky headers where neighboring content can touch the frame.
- Add focused buffer-level regression coverage for adjacent collapsed entries, compact side panes, sticky headers, and ordinary spaced entries.

This change affects Pager frame geometry. Tool grouping, content, and session facts remain unchanged.
