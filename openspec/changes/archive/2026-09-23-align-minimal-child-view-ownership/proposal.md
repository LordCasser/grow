# Align Minimal rendering with the selected Agent view

## Why

Minimal delegates input to the selected child Agent, but its native commit, viewport, live region and overlays still read the root Agent. The user can therefore edit a child prompt while seeing the parent's conversation and controls. The existing child-aware export and transcript paths do not repair this interactive mismatch.

## What changes

- Resolve one selected root or child view for every Minimal frame, following the same permission precedence as input.
- On a view-owner change, clear the old visible screen and establish a new native commit frontier for the selected view. Previously printed terminal scrollback remains historical; the selected view's content is presented again in the new screen epoch.
- Route the commit, viewport, live region, plan, expansion and Minimal key decisions through that owner. A stale child key falls back to the root for both input and rendering.
- Verify parent/child content, switches, permission precedence and invalidated child views.

## Capabilities

- `client-surfaces`: Minimal selected-view rendering and native history ownership.
