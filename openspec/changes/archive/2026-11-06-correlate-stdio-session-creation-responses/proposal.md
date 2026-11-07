# Correlate stdio session creation responses

## Why

The stdio replay owner keeps only one pending session/new and consumes it for any response containing result.sessionId. Overlapping creations or a load response can assign the wrong cwd/MCP declaration to a session, while an error leaves stale pending state (PB-02).

## What Changes

Track each pending creation by its exact JSON-RPC request ID, settle only a matching response, and remove the matching entry on error. Preserve confirmed-session replay order and existing load/close behavior. Verify interleaved, reversed and numeric/string IDs through the actual forward/incoming-cache/reconnect chain.
