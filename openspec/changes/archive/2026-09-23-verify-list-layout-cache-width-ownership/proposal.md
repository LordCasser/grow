# Change: Verify list layout cache width ownership

## Why

The Pager backlog asks whether cached list item heights can survive a width change or content rebuild. Trace the width key, resize and scrollbar paths, and every production owner of variable-height list state.

## What Changes

- Record how `ListPaneState` invalidates or rebuilds variable-height caches across width changes and owner content updates.
- Remove only the resolved cached-width clause from the geometry backlog.

This is an audit and backlog maintenance change. It changes no runtime behavior or accepted contract, so `skip_specs: true` is intentional.
