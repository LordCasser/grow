# Unify session MCP server identity

## Why

Live catalog lifecycle rules depend on an unambiguous relationship between configured MCP servers, their clients, advertised tools, and direct calls. If a server's name identifies different endpoints in different paths, tools can be listed under one identity and dispatched to another, especially during duplicate configuration, cold recovery, or hot replacement.

## What Changes

- Define `name` as the single-session identity used consistently by the directory, client, and tool prefix.
- Define source precedence and deterministic handling of same-name entries across configuration layers and endpoints.
- Reject duplicate names within one client-provided array (pending implementation audit confirmation).
- Require explicit `name` and URL inputs to match the same authoritative current entry; they cannot redirect to another name by URL.
- Define routing guarantees across cold recovery and hot replacement.

## Capabilities

### Modified Capabilities

- `extension-runtime`: MCP server identity, catalog admission, and direct-call routing.

## Scope

This change specifies identity and routing invariants. It does not change transport behavior, policy precedence, or the established configuration-source precedence (`config` over `plugin`, with client precedence where currently defined).
