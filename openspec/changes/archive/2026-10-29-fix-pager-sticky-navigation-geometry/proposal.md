## Why

Pager renders no sticky prompt header when sticky headers are disabled or compact prompt mode is active, but precise entry-top and entry-center navigation still subtracts a computed sticky header. The target therefore lands below the position the user asked for.

## What Changes

- Make precise entry-top and center navigation use the same sticky-header enablement conditions as rendering.
- Preserve sticky-aware target placement when sticky headers are enabled.

## Capabilities

### Modified Capabilities

- `client-surfaces`: precise scrollback navigation aligns targets with the rendered content area.

## Impact

Pager scrollback navigation geometry and its regression tests only.
