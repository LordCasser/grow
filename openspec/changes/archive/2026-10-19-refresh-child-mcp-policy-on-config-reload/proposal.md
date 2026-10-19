# Refresh child MCP policy on config reload

## Why

The archived live auxiliary-policy change defines subagent policy refresh, and child actors already apply the `UpdateMcpServers` command as a local policy-only refresh. The internal config-reload handler currently fans out only to `agent.sessions`, so active child actors never receive that command.

## What Changes

- Include active child sessions in cwd-filtered internal MCP catalog reload fanout.
- Send child actors a policy-only refresh command without passing client catalog state.
- Preserve the child actor rule that refreshes trusted/global auxiliary policy while leaving inherited clients and frozen catalogs alone.

## Scope

This change closes the missing live fanout path. It does not change policy precedence, catalog ownership, or the archived auxiliary-policy behavior.
