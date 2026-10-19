# Change: Harden live MCP catalog lifecycle

## Why

Live ACP sessions can refresh their MCP catalog when configuration changes. The existing `extension-runtime` contract covers additions, modifications, idempotent refresh, cwd scoping, client precedence, and watcher shutdown, but does not define removal/replacement behavior, stale asynchronous connection results, or recovery from a cold session restart. These lifecycle edges need explicit adversarial scenarios so a refreshed list cannot disagree with the callable tools or resurrect an obsolete server.

## What Changes

- Define removal and replacement as transitions of the effective MCP catalog, including tool visibility and actual invocation behavior.
- Prevent an older initialize/reconnect result from restoring a removed or superseded server.
- Define session isolation and cold recovery against current applicable configuration, while preserving existing client-source merge semantics and restarting applicable project watching.

## Capabilities

### Modified Capabilities

- `extension-runtime`: MCP catalog removal, replacement, stale connection results, session isolation, and cold recovery.

## Scope

This change adds lifecycle requirements to the existing live catalog refresh contract. It does not change configuration merge keys, trust filtering, client precedence, transport recovery policy, or permission boundaries. Implementation evidence, explicit boundaries, and separate adjacent policy/identity debt are recorded in audit.md; verification results are recorded before task completion.
