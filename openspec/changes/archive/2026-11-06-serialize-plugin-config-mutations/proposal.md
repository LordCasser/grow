# Serialize plugin configuration mutations

## Why

Plugin path, enable/disable and CTA dismissal writers bypass the existing settings write lock and atomic publication. Some turn hard read errors into an empty config, risking lost user configuration (AC-01).

## What Changes

Move these raw TOML mutations onto the existing SAVE_LOCK and sibling atomic writer, propagate hard reads and parse errors, and adapt Shell/Pager callers to await the same transaction. The synchronous CTA mouse handler emits an existing-style async persistence effect. Preserve unknown fields, authored environment references, plugin policies and current caller error presentation. Installation/registry transaction errors (AC-08) remain separate.
