# Change: Audit dialog and viewport Unicode geometry

## Why

The Pager backlog asks whether dialog sizing, viewport byte/display-column conversion, or header byte widths can cause reachable geometry defects. Trace production rendering and its existing regressions before treating these as open issues.

## What Changes

- Record the production render paths and test coverage for dialog Unicode sizing, input viewports, and header/label widths.
- Remove only the resolved dialog/viewport/header clause from the geometry backlog.

This is a source audit and backlog maintenance change. It changes no runtime behavior or accepted contract, so `skip_specs: true` is intentional.
