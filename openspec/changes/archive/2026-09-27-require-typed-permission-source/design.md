## Context

Shell's `tool/preparation.rs` constructs `PermissionRequestSource::Child` or `Primary` explicitly and calls `request_with_context`. The old convenience wrappers have no production callsite, but remain public and use optional `subagent_type` as an authorization discriminator.

## Decision

Compile the wrappers only for the workspace unit tests. In those helpers, reject partial identities, so test fixtures cannot accidentally exercise Primary when they intended Child. Remove the default Primary value from the request context and source types. Runtime has one permission request API, `request_with_context`, and source is a required enum value at construction. This removes ambiguity without adding a new entity or compatibility API.

## Verification

Search production callsites; run the workspace permission suite and Shell authorization tests; validate OpenSpec.
