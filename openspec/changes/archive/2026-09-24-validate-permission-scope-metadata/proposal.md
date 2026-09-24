# Validate permission scope metadata against the request

## Why

Permission response metadata is currently accepted when it has the expected shape, even when its MCP tool/server or Bash command scope does not match the access being approved. The Pager also uses request option metadata to enable MCP scope selection without checking it against the request's tool identity.

## What Changes

- Validate MCP and Bash response scope metadata against the current access before creating a remembered permission outcome.
- Have Pager expose MCP scope selection only when the option metadata agrees with the permission request's tool identity.

## Capabilities

### Modified Capabilities

- `tool-authorization`: remembered permission scopes must remain within the current access request.

## Impact

Permission response mapping in workspace, Pager permission request presentation, and focused regression tests.
