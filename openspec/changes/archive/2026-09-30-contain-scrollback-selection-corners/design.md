## Existing path

`ScrollbackState` computes per-entry height and `gap_after`. Two adjacent collapsed groupable entries may have `gap_after = 0`. `ScrollbackPane::render_content` builds a `SelectionBox` for the selected entry or run. `SelectionBox::render` always puts its top and bottom corners one row outside `inner_area`, unless clipped. The post-render pass can therefore overwrite an adjacent row's left and right gutters.

## Change

Derive whether each side has a free spacer from the cached entry layout at the selected range boundary. Pass that geometry to `SelectionBox`; where no spacer exists, put the corresponding corner on the first or last row of the selected area, replacing that row's vertical border. Keep the existing outboard placement when there is a spacer. Clipped edges still render the existing dashed continuation without corners.

The selection frame owns the corner placement. This avoids changing verb-group membership, dense spacing, scroll offsets, or the adjacent entry's content. `ScrollbackState` exposes the cached boundary spacing so selected and hovered frames use the same rule for singleton and multi-entry ranges. Hidden thinking rows are transparent when finding the preceding visible boundary.

Other `SelectionBox` callers need the same ownership check. Agent view layout compares each side pane with its neighboring nonempty pane rectangles and insets corners where no separator row exists. A scrollback frame at its viewport edge also insets that edge's corner, since the renderer cannot claim a spacer outside its own area. Sticky prompt headers use their reserved inter-header and header-content gaps; a header starting at the scrollback area's top edge insets its top corners.

## Verification

Render selected and hovered collapsed tool rows between adjacent dense rows into a `ratatui::Buffer` and assert that neighboring header rows receive no frame corners. Check a spaced row and clipped edges to keep the existing placement and continuation behavior. Check compact side-pane adjacency and sticky header top-edge geometry. Sweep other `SelectionBox` call sites for the same geometry issue. Run focused Pager tests, formatting checks, and strict OpenSpec validation before archiving.
