## Context

`render_with_sticky_headers` draws a sticky header only when the display setting is enabled and compact prompt mode is off. `entry_top_scroll_offset` and `scroll_to_entry_center` independently compute a sticky layout without applying those conditions.

## Decision

Centralize the effective sticky layout calculation in `ScrollbackState` so precise navigation and current-header geometry share the rendering gates. When disabled, return an empty sticky layout; when enabled, compute from the same relative prompt descriptors and viewport geometry.

## Consequences

Top and center navigation stop reserving invisible rows. Enabled sticky navigation retains its existing iterative convergence against the visible sticky header.
