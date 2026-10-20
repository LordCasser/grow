## Why

The exact Cargo runner buffered both invocations until completion and had no subprocess deadline. Compilation, enumeration and execution stalls looked alike; cancellation could leave descendants running. An inherited output pipe can also keep cleanup waiting after the direct process exits.

## What Changes

Report compile, enumeration and execution phases; stream output while retaining bounded diagnostics; enforce independently configurable finite positive deadlines. Use standard Unix process-group and Windows tree termination, reap the direct child, and bound reader shutdown too. Report cleanup uncertainty explicitly rather than hanging or claiming termination. Treat output read failures as failures, retaining exact-name, ignored opt-in, zero-execution and one-successful-test checks.

## Capabilities

### Modified Capabilities
- development-workflow: observable phase deadlines, bounded cleanup and explicit failure/uncertainty in the exact runner.

## Impact

The Python runner, focused tests and developer guidance. Existing overall job deadlines and Windows test discovery remain unchanged. No Rust test behavior, MCP, version or published artifact changes.
