# Refresh live MCP auxiliary policy

## Why

The live MCP catalog lifecycle now defines how server additions, removals, replacement, and stale connection work converge. A running session also captures per-server policy such as disabled tools and startup/tool timeouts. Without a refresh contract for those fields, changing policy can leave an existing session using stale settings even when its catalog has been refreshed.

## What Changes

- Define how `disabled_mcp_tools`, `startup_timeout_sec`, `tool_timeout_sec`, and `tool_timeouts` updates take effect in an existing session.
- Define removal of an override as restoration of the default policy, while retaining current metadata precedence.
- Require policy refresh to respect project trust and cwd applicability, and prevent stale asynchronous work from restoring old policy.
- Preserve the deadline already admitted for an in-flight call when policy changes.

## Capabilities

### Modified Capabilities

- `extension-runtime`: live session MCP policy refresh and its interaction with catalog lifecycle and in-flight calls.

## Scope

This change defines policy refresh for an existing session. It does not change transport ownership, catalog merge identity, or the existing metadata precedence rules.
