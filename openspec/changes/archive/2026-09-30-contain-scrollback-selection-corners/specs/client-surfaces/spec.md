## ADDED Requirements

### Requirement: TUI frame corners remain within their owned rows

Pager SHALL render a selected or hovered scrollback frame without placing its corner glyphs on the content row of an adjacent scrollback entry. A corner MAY use the spacer row outside the framed entry when one exists; if adjacent entries are densely packed, the corner SHALL use the framed entry's edge row. Viewport-clipped edges SHALL retain a continuation border instead of a corner.

For a one-row selection with dense neighbors on both sides, Pager SHALL use a single-row junction mark rather than overwrite a neighbor or arbitrarily drop one edge of the selection.

#### Scenario: Selected collapsed tool between dense groups

- **WHEN** a selected tool entry or selected group has no spacer before or after an adjacent collapsed tool group
- **THEN** its top and bottom selection corners stay on its own edge rows, and neither neighboring group header receives a selection corner.

#### Scenario: Hovered collapsed tool between dense groups

- **WHEN** a hovered tool entry or group has no spacer before or after an adjacent collapsed tool group
- **THEN** its hover frame stays within its own edge rows, and neither neighboring group header receives a hover corner.

#### Scenario: Selected entry has spacer rows

- **WHEN** the selected entry has a free row before and after it
- **THEN** its corners use those spacer rows as before.

#### Scenario: One-row selection between dense neighbors

- **WHEN** the selected entry occupies one row and neither adjacent entry leaves a spacer
- **THEN** its selection marks remain on that row, with neither neighbor row repainted.

#### Scenario: Selected entry crosses the viewport edge

- **WHEN** the selected entry is clipped at the top or bottom of the scrollback viewport
- **THEN** the clipped side shows its continuation border and no corner outside or inside the clipped edge.

#### Scenario: Focused side pane touches another pane

- **WHEN** a focused side pane has no separator row before or after an adjacent pane
- **THEN** its corresponding frame corners stay on the focused pane's edge row and do not repaint the adjacent pane.

#### Scenario: One-row focused pane retains its close control

- **WHEN** a focused side pane occupies one row and both frame corners must be inset
- **THEN** its close control remains visible and aligned with its hit area.

#### Scenario: Sticky header starts at the scrollback top edge

- **WHEN** a selected sticky prompt header begins on the scrollback area's first row
- **THEN** its top frame corners stay within that area rather than repainting the preceding pane.

#### Scenario: Scrollback frame touches its viewport boundary

- **WHEN** a selected or hovered scrollback entry ends exactly at the scrollback area's top or bottom boundary without clipping its own content
- **THEN** its corresponding frame corner remains inside the scrollback area.
