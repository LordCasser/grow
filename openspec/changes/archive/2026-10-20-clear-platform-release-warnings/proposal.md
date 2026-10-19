# Clear platform-specific release warnings

## Why

The first official 2.4.1 candidate exposed Windows-only first-party warnings absent from the macOS default build. Publication is cancelled before any public release so these can be corrected and checked on the replacement source.

## What Changes

Narrow imports, mutable bindings and Unix-only helpers to the platforms that use them; preserve resource ownership and Linux clipboard fallback semantics. This is non-behavioral maintenance (skip_specs), not a new product contract. Keep MCP and vendor dependency upgrades outside this change.
