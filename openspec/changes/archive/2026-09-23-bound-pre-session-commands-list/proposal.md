## Why

Pre-session `grow/commands/list` performs plugin, skill, and workflow filesystem discovery on an asynchronous extension request. A slow filesystem can occupy a Tokio runtime worker and delay unrelated requests.

## What Changes

- Run the complete pre-session non-chat catalog composition in the shared single-permit blocking discovery worker with a five-second deadline, including capacity wait.
- Preserve the chat catalog and live-session cached catalog early-return paths.
- Return timeout and worker failures as RPC errors; keep an in-flight worker's permit until it exits.

## Capabilities

### New Capabilities

None

### Modified Capabilities

- `client-surfaces`: Bound pre-session command catalog discovery and define timeout/failure behavior.

## Impact

Changes `crates/codegen/shell/src/extensions/session_admin.rs` and reuses the worker in `extensions/skills.rs`. No response shape, discovery rules, or disk state is changed beyond the existing folder-trust resolution semantics.
